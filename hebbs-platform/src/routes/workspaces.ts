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
            const s = d.stats as Record<string, number> | undefined;
            stats = {
              memories: s?.memory_count ?? 0,
              files: s?.file_count ?? 0,
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
        const s = d.stats as Record<string, number> | undefined;
        stats = {
          memories: s?.memory_count ?? 0,
          files: s?.file_count ?? 0,
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
        const s = d.stats as Record<string, number> | undefined;
        stats.memories = s?.memory_count ?? 0;
        stats.files = s?.file_count ?? 0;
        stats.indexing_status = d.indexing_status ?? "idle";
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

  return app;
}
