import { Hono } from "hono";
import { eq } from "drizzle-orm";
import { db, schema } from "../db/index.js";
import { generateApiKey, hashKey, keyPrefix } from "../lib/crypto.js";
import { initWorkspaceVault, slugify, workspaceVaultPath } from "../lib/workspace.js";
import { DaemonClient } from "../lib/daemon-client.js";
import type { AuthInfo } from "../lib/auth.js";

const ENGINE_SOCKET =
  process.env.HEBBS_ENGINE_SOCKET || "/data/daemon/daemon.sock";

export function workspaceRoutes() {
  const app = new Hono();
  const daemon = new DaemonClient(ENGINE_SOCKET);

  // List workspaces (admin only)
  app.get("/v1/workspaces", async (c) => {
    const auth = c.get("auth" as never) as AuthInfo;
    if (auth.role !== "admin") {
      return c.json({ error: "Admin access required" }, 403);
    }

    const rows = db.select().from(schema.workspaces).all();

    const workspaces = await Promise.all(
      rows.map(async (ws) => {
        let stats = { memories: 0, files: 0 };
        try {
          const resp = await daemon.send({ type: "status" }, ws.vaultPath, 5000);
          if (resp.status === "ok" && resp.data) {
            const d = resp.data as Record<string, unknown>;
            stats = {
              memories: (d.total_memories as number) ?? 0,
              files: (d.total_files as number) ?? 0,
            };
          }
        } catch {
          // Vault may not be open yet
        }
        return { ...ws, stats };
      })
    );

    return c.json({ workspaces });
  });

  // Create workspace (admin only)
  app.post("/v1/workspaces", async (c) => {
    const auth = c.get("auth" as never) as AuthInfo;
    if (auth.role !== "admin") {
      return c.json({ error: "Admin access required" }, 403);
    }

    const body = await c.req.json();
    const name = body.name;
    if (!name) {
      return c.json({ error: "name is required" }, 400);
    }

    const slug = body.slug || slugify(name);

    // Check if slug already exists
    const existing = db
      .select()
      .from(schema.workspaces)
      .where(eq(schema.workspaces.slug, slug))
      .all();

    if (existing.length > 0) {
      return c.json({ error: `Workspace '${slug}' already exists` }, 409);
    }

    // Initialize vault on disk
    const vaultPath = await initWorkspaceVault(slug);

    // Insert workspace
    const result = db
      .insert(schema.workspaces)
      .values({
        slug,
        name,
        vaultPath,
        createdAt: new Date().toISOString(),
      })
      .returning()
      .get();

    // Generate a workspace-scoped API key
    const rawKey = generateApiKey("workspace");
    const hash = hashKey(rawKey);
    const prefix = keyPrefix(rawKey);

    db.insert(schema.apiKeys)
      .values({
        keyHash: hash,
        keyPrefix: prefix,
        label: `${slug}-default`,
        role: "workspace",
        workspaceId: result.id,
        createdAt: new Date().toISOString(),
      })
      .run();

    return c.json(
      {
        workspace: result,
        api_key: rawKey,
      },
      201
    );
  });

  // Get workspace by slug
  app.get("/v1/workspaces/:slug", async (c) => {
    const auth = c.get("auth" as never) as AuthInfo;
    const slug = c.req.param("slug");

    const [ws] = db
      .select()
      .from(schema.workspaces)
      .where(eq(schema.workspaces.slug, slug))
      .limit(1)
      .all();

    if (!ws) {
      return c.json({ error: "Workspace not found" }, 404);
    }

    // Workspace-scoped keys can only see their own workspace
    if (auth.role === "workspace" && auth.workspaceId !== ws.id) {
      return c.json({ error: "Access denied" }, 403);
    }

    let stats = { memories: 0, files: 0 };
    try {
      const resp = await daemon.send({ type: "status" }, ws.vaultPath, 5000);
      if (resp.status === "ok" && resp.data) {
        const d = resp.data as Record<string, unknown>;
        stats = {
          memories: (d.total_memories as number) ?? 0,
          files: (d.total_files as number) ?? 0,
        };
      }
    } catch {
      // Vault may not be open yet
    }

    return c.json({ workspace: { ...ws, stats } });
  });

  // Delete workspace (admin only)
  app.delete("/v1/workspaces/:slug", async (c) => {
    const auth = c.get("auth" as never) as AuthInfo;
    if (auth.role !== "admin") {
      return c.json({ error: "Admin access required" }, 403);
    }

    const slug = c.req.param("slug");

    const [ws] = db
      .select()
      .from(schema.workspaces)
      .where(eq(schema.workspaces.slug, slug))
      .limit(1)
      .all();

    if (!ws) {
      return c.json({ error: "Workspace not found" }, 404);
    }

    // Revoke all keys for this workspace
    db.update(schema.apiKeys)
      .set({ revokedAt: new Date().toISOString() })
      .where(eq(schema.apiKeys.workspaceId, ws.id))
      .run();

    // Delete workspace record
    db.delete(schema.workspaces)
      .where(eq(schema.workspaces.id, ws.id))
      .run();

    return c.json({ deleted: true, slug });
  });

  // Get workspace files
  app.get("/v1/workspaces/:slug/files", async (c) => {
    const auth = c.get("auth" as never) as AuthInfo;
    const slug = c.req.param("slug");

    const [ws] = db
      .select()
      .from(schema.workspaces)
      .where(eq(schema.workspaces.slug, slug))
      .limit(1)
      .all();

    if (!ws) {
      return c.json({ error: "Workspace not found" }, 404);
    }

    if (auth.role === "workspace" && auth.workspaceId !== ws.id) {
      return c.json({ error: "Access denied" }, 403);
    }

    try {
      const resp = await daemon.send({ type: "list", sections: true }, ws.vaultPath, 10000);
      if (resp.status === "ok" && resp.data) {
        return c.json(resp.data);
      }
      return c.json({ files: [] });
    } catch {
      return c.json({ files: [] });
    }
  });

  // Get workspace detailed stats
  app.get("/v1/workspaces/:slug/stats", async (c) => {
    const auth = c.get("auth" as never) as AuthInfo;
    const slug = c.req.param("slug");

    const [ws] = db
      .select()
      .from(schema.workspaces)
      .where(eq(schema.workspaces.slug, slug))
      .limit(1)
      .all();

    if (!ws) {
      return c.json({ error: "Workspace not found" }, 404);
    }

    if (auth.role === "workspace" && auth.workspaceId !== ws.id) {
      return c.json({ error: "Access denied" }, 403);
    }

    let stats: Record<string, unknown> = {
      memories: 0,
      files: 0,
      entities: 0,
      edges: 0,
      indexing_status: "unknown",
    };

    try {
      const statusResp = await daemon.send({ type: "status" }, ws.vaultPath, 5000);
      if (statusResp.status === "ok" && statusResp.data) {
        const d = statusResp.data as Record<string, unknown>;
        stats.memories = (d.total_memories as number) ?? 0;
        stats.files = (d.total_files as number) ?? 0;
        stats.indexing_status = d.indexing ? "indexing" : "idle";
      }
    } catch {
      // Vault may not be open
    }

    // Get entity count
    try {
      const exportResp = await daemon.send({ type: "export", limit: 10000 }, ws.vaultPath, 10000);
      if (exportResp.status === "ok" && exportResp.data) {
        const data = exportResp.data as { memories?: Array<{ entity_id?: string }> };
        const entitySet = new Set(
          (data.memories || []).map((m) => m.entity_id).filter(Boolean)
        );
        stats.entities = entitySet.size;
      }
    } catch {
      // OK
    }

    return c.json({ stats });
  });

  // ── Workspace-scoped data endpoints (for dashboard session auth) ──

  // Helper to resolve workspace and check access
  async function resolveWorkspace(c: { req: { param: (k: string) => string }; json: (d: unknown, s?: number) => Response; get: (k: never) => unknown }) {
    const slug = c.req.param("slug");
    const auth = c.get("auth" as never) as AuthInfo;

    const [ws] = db
      .select()
      .from(schema.workspaces)
      .where(eq(schema.workspaces.slug, slug))
      .limit(1)
      .all();

    if (!ws) return null;
    if (auth.role === "workspace" && auth.workspaceId !== ws.id) return null;
    return ws;
  }

  // Remember into workspace
  app.post("/v1/workspaces/:slug/memories", async (c) => {
    const ws = await resolveWorkspace(c);
    if (!ws) return c.json({ error: "Workspace not found or access denied" }, 404);

    const body = await c.req.json();
    const resp = await daemon.send(
      {
        type: "remember",
        content: body.content,
        importance: body.importance,
        entity_id: body.entity_id,
        context: body.context ? JSON.stringify(body.context) : undefined,
      },
      ws.vaultPath
    );

    if (resp.status === "error") return c.json({ error: resp.error }, 500);
    return c.json(resp.data, 201);
  });

  // Recall from workspace
  app.post("/v1/workspaces/:slug/recall", async (c) => {
    const ws = await resolveWorkspace(c);
    if (!ws) return c.json({ error: "Workspace not found or access denied" }, 404);

    const body = await c.req.json();
    const resp = await daemon.send(
      {
        type: "recall",
        cue: body.cue,
        top_k: body.top_k ?? 10,
        entity_id: body.entity_id,
        strategy: body.strategy,
      },
      ws.vaultPath
    );

    if (resp.status === "error") return c.json({ error: resp.error }, 500);
    return c.json(resp.data);
  });

  // Prime from workspace
  app.post("/v1/workspaces/:slug/prime", async (c) => {
    const ws = await resolveWorkspace(c);
    if (!ws) return c.json({ error: "Workspace not found or access denied" }, 404);

    const body = await c.req.json();
    const resp = await daemon.send(
      {
        type: "prime",
        entity_id: body.entity_id,
        max_memories: body.max_memories,
        similarity_cue: body.similarity_cue,
      },
      ws.vaultPath
    );

    if (resp.status === "error") return c.json({ error: resp.error }, 500);
    return c.json(resp.data);
  });

  // Forget from workspace
  app.post("/v1/workspaces/:slug/forget", async (c) => {
    const ws = await resolveWorkspace(c);
    if (!ws) return c.json({ error: "Workspace not found or access denied" }, 404);

    const body = await c.req.json();
    const resp = await daemon.send(
      {
        type: "forget",
        ids: body.ids,
        entity_id: body.entity_id,
      },
      ws.vaultPath
    );

    if (resp.status === "error") return c.json({ error: resp.error }, 500);
    return c.json(resp.data);
  });

  // Entities in workspace
  app.get("/v1/workspaces/:slug/entities", async (c) => {
    const ws = await resolveWorkspace(c);
    if (!ws) return c.json({ error: "Workspace not found or access denied" }, 404);

    // Use list command to get memories, then extract unique entity_ids
    const resp = await daemon.send(
      { type: "list", sections: false },
      ws.vaultPath,
      10000
    );

    if (resp.status === "error") return c.json({ entities: [] });

    const data = resp.data as { memories?: Array<{ entity_id?: string; content?: string }> };
    const entityMap = new Map<string, { count: number }>();
    for (const m of data.memories || []) {
      if (m.entity_id) {
        const entry = entityMap.get(m.entity_id) || { count: 0 };
        entry.count++;
        entityMap.set(m.entity_id, entry);
      }
    }

    const entities = Array.from(entityMap.entries()).map(([id, info]) => ({
      entity_id: id,
      memory_count: info.count,
    }));

    return c.json({ entities });
  });

  // Insights in workspace
  app.get("/v1/workspaces/:slug/insights", async (c) => {
    const ws = await resolveWorkspace(c);
    if (!ws) return c.json({ error: "Workspace not found or access denied" }, 404);

    const entityId = c.req.query("entity_id");
    const resp = await daemon.send(
      {
        type: "insights",
        entity_id: entityId,
      },
      ws.vaultPath
    );

    if (resp.status === "error") return c.json({ error: resp.error }, 500);
    return c.json(resp.data);
  });

  // Upload files to workspace
  app.post("/v1/workspaces/:slug/upload", async (c) => {
    const ws = await resolveWorkspace(c);
    if (!ws) return c.json({ error: "Workspace not found or access denied" }, 404);

    const { writeFile, mkdir } = await import("node:fs/promises");
    const { join, dirname } = await import("node:path");

    const body = await c.req.parseBody({ all: true });
    const rawFiles = Array.isArray(body["files"]) ? body["files"] : [body["files"]];

    const uploaded: string[] = [];
    for (const file of rawFiles) {
      if (!(file instanceof File)) continue;
      const targetPath = join(ws.vaultPath, file.name);
      await mkdir(dirname(targetPath), { recursive: true });
      const arrayBuffer = await file.arrayBuffer();
      await writeFile(targetPath, Buffer.from(arrayBuffer));
      uploaded.push(file.name);
    }

    if (uploaded.length === 0) {
      return c.json({ error: "No files uploaded" }, 400);
    }

    // Fire-and-forget: index command opens the vault and scans for new files
    daemon.send({ type: "index" }, ws.vaultPath, 5000).catch(() => {});

    return c.json({
      uploaded: uploaded.length,
      files: uploaded,
      message: "Files uploaded. Indexing triggered.",
    });
  });

  return app;
}
