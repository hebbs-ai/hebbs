import { Hono } from "hono";
import { writeFile, mkdir } from "node:fs/promises";
import { join, dirname } from "node:path";
import { DaemonClient } from "../lib/daemon-client.js";
import type { AuthInfo } from "../lib/auth.js";

const ENGINE_SOCKET =
  process.env.HEBBS_ENGINE_SOCKET || "/data/daemon/daemon.sock";

export function uploadRoutes() {
  const app = new Hono();
  const daemon = new DaemonClient(ENGINE_SOCKET);

  app.post("/v1/upload", async (c) => {
    const auth = c.get("auth" as never) as AuthInfo;
    const body = await c.req.parseBody({ all: true });
    const files = Array.isArray(body["files"]) ? body["files"] : [body["files"]];

    const uploaded: string[] = [];

    for (const file of files) {
      if (!(file instanceof File)) continue;

      const targetPath = join(auth.vaultPath, file.name);
      await mkdir(dirname(targetPath), { recursive: true });

      const arrayBuffer = await file.arrayBuffer();
      await writeFile(targetPath, Buffer.from(arrayBuffer));
      uploaded.push(file.name);
    }

    if (uploaded.length === 0) {
      return c.json({ error: "No files uploaded" }, 400);
    }

    // Fire-and-forget: trigger indexing without waiting for completion
    daemon.send({ type: "index" }, auth.vaultPath, 5000).catch(() => {});

    return c.json({
      uploaded: uploaded.length,
      files: uploaded,
      message: "Files uploaded. Indexing triggered. Check status for progress.",
    });
  });

  return app;
}
