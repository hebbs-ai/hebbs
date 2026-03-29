import * as net from "node:net";

const MAX_MESSAGE_SIZE = 16 * 1024 * 1024; // 16 MiB
const DEFAULT_TIMEOUT = 60_000; // 60s for indexing operations

interface DaemonRequest {
  command: Record<string, unknown>;
  vault_path?: string;
  vault_paths?: string[];
  caller?: string;
}

interface DaemonResponse {
  status: "ok" | "error" | "progress";
  status_code?: string;
  data?: unknown;
  error?: string;
}

function encodeLengthPrefixed(data: unknown): Buffer {
  const json = Buffer.from(JSON.stringify(data), "utf-8");
  const header = Buffer.alloc(4);
  header.writeUInt32BE(json.length, 0);
  return Buffer.concat([header, json]);
}

function readLengthPrefixedMessages(
  buf: Buffer
): { messages: DaemonResponse[]; remaining: Buffer } {
  const messages: DaemonResponse[] = [];
  let offset = 0;

  while (offset + 4 <= buf.length) {
    const len = buf.readUInt32BE(offset);
    if (len > MAX_MESSAGE_SIZE) {
      throw new Error(`Message too large: ${len} bytes`);
    }
    if (offset + 4 + len > buf.length) break; // incomplete
    const payload = buf.subarray(offset + 4, offset + 4 + len);
    messages.push(JSON.parse(payload.toString("utf-8")));
    offset += 4 + len;
  }

  return { messages, remaining: buf.subarray(offset) };
}

export class DaemonClient {
  constructor(private socketPath: string) {}

  async send(
    command: Record<string, unknown>,
    vaultPath?: string,
    timeout = DEFAULT_TIMEOUT
  ): Promise<DaemonResponse> {
    const req: DaemonRequest = {
      command,
      vault_path: vaultPath,
      caller: "hebbs-platform",
    };

    return new Promise((resolve, reject) => {
      const socket = net.createConnection(this.socketPath);
      let buffer = Buffer.alloc(0);
      let settled = false;

      const timer = setTimeout(() => {
        if (!settled) {
          settled = true;
          socket.destroy();
          reject(new Error(`Daemon request timed out after ${timeout}ms`));
        }
      }, timeout);

      socket.on("connect", () => {
        socket.write(encodeLengthPrefixed(req));
      });

      socket.on("data", (chunk: Buffer) => {
        buffer = Buffer.concat([buffer, chunk]) as Buffer<ArrayBuffer>;
        const { messages, remaining } = readLengthPrefixedMessages(buffer);
        buffer = remaining as Buffer<ArrayBuffer>;

        for (const msg of messages) {
          if (msg.status === "progress") continue;
          if (!settled) {
            settled = true;
            clearTimeout(timer);
            socket.destroy();
            resolve(msg);
          }
        }
      });

      socket.on("error", (err) => {
        if (!settled) {
          settled = true;
          clearTimeout(timer);
          reject(err);
        }
      });

      socket.on("close", () => {
        if (!settled) {
          settled = true;
          clearTimeout(timer);
          reject(new Error("Daemon connection closed unexpectedly"));
        }
      });
    });
  }
}
