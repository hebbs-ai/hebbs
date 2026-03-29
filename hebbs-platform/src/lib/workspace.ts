import { mkdir, writeFile, chmod } from "node:fs/promises";
import { existsSync } from "node:fs";
import { join } from "node:path";
import { DaemonClient } from "./daemon-client.js";

const WORKSPACES_DIR = process.env.HEBBS_WORKSPACES_DIR || "/data/workspaces";
const ENGINE_SOCKET =
  process.env.HEBBS_ENGINE_SOCKET || "/data/daemon/daemon.sock";

export function workspaceVaultPath(slug: string): string {
  return join(WORKSPACES_DIR, slug);
}

async function mkdirWritable(path: string) {
  await mkdir(path, { recursive: true });
  await chmod(path, 0o777);
}

async function writeFileWritable(path: string, content: string) {
  await writeFile(path, content);
  await chmod(path, 0o666);
}

export async function initWorkspaceVault(slug: string): Promise<string> {
  const vaultPath = workspaceVaultPath(slug);
  const hebbsDir = join(vaultPath, ".hebbs");

  if (existsSync(hebbsDir)) {
    return vaultPath;
  }

  await mkdirWritable(vaultPath);
  await mkdirWritable(hebbsDir);

  const llmProvider = process.env.HEBBS_LLM_PROVIDER || "openai";
  const llmModel = process.env.HEBBS_LLM_MODEL || "gpt-4o-mini";
  const llmKey = process.env.HEBBS_LLM_API_KEY || "";

  const config = `[llm]
provider = "${llmProvider}"
model = "${llmModel}"
api_key = "${llmKey}"

[embedding]
provider = "openai"
model = "text-embedding-3-small"
dimensions = 1536
api_key = "${llmKey}"
`;

  await writeFileWritable(join(hebbsDir, "config.toml"), config);
  await writeFileWritable(
    join(hebbsDir, "manifest.json"),
    JSON.stringify({ version: 1, files: {} })
  );
  // Generate a ULID-like epoch (26-char timestamp-based ID)
  const ts = Date.now();
  const epoch = ts.toString(36).toUpperCase().padStart(10, "0") + "0000000000000000";
  await writeFileWritable(join(hebbsDir, "epoch"), epoch.slice(0, 26));

  // Warm the vault: send a ping-like command to force the daemon to open it
  // and create the RocksDB index directory
  try {
    const daemon = new DaemonClient(ENGINE_SOCKET);
    await daemon.send({ type: "status" }, vaultPath, 10000);
  } catch {
    // Daemon may not have opened it yet; that's fine, it'll open on first use
  }

  return vaultPath;
}

export function slugify(name: string): string {
  return name
    .toLowerCase()
    .replace(/[^a-z0-9]+/g, "-")
    .replace(/^-|-$/g, "");
}
