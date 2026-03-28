import { Hono } from "hono";
import { eq, isNull } from "drizzle-orm";
import { db, schema } from "../db/index.js";
import { generateApiKey, hashKey, keyPrefix } from "../lib/crypto.js";
import type { AuthInfo } from "../lib/auth.js";

export function keyRoutes() {
  const app = new Hono();

  // List API keys (admin only)
  app.get("/v1/keys", async (c) => {
    const auth = c.get("auth" as never) as AuthInfo;
    if (auth.role !== "admin") {
      return c.json({ error: "Admin access required" }, 403);
    }

    const keys = db
      .select({
        id: schema.apiKeys.id,
        prefix: schema.apiKeys.keyPrefix,
        label: schema.apiKeys.label,
        role: schema.apiKeys.role,
        workspaceId: schema.apiKeys.workspaceId,
        createdAt: schema.apiKeys.createdAt,
        revokedAt: schema.apiKeys.revokedAt,
      })
      .from(schema.apiKeys)
      .all();

    return c.json({ keys });
  });

  // Create API key (admin only)
  app.post("/v1/keys", async (c) => {
    const auth = c.get("auth" as never) as AuthInfo;
    if (auth.role !== "admin") {
      return c.json({ error: "Admin access required" }, 403);
    }

    const body = await c.req.json();
    const label = body.label || "default";
    const role = body.role || "workspace";

    const rawKey = generateApiKey(role as "admin" | "workspace");
    const hash = hashKey(rawKey);
    const prefix = keyPrefix(rawKey);

    db.insert(schema.apiKeys)
      .values({
        keyHash: hash,
        keyPrefix: prefix,
        label,
        role,
        workspaceId: body.workspace_id ?? null,
        createdAt: new Date().toISOString(),
      })
      .run();

    return c.json({ api_key: rawKey, prefix, label, role }, 201);
  });

  // Revoke API key (admin only)
  app.delete("/v1/keys/:id", async (c) => {
    const auth = c.get("auth" as never) as AuthInfo;
    if (auth.role !== "admin") {
      return c.json({ error: "Admin access required" }, 403);
    }

    const id = parseInt(c.req.param("id"), 10);
    db.update(schema.apiKeys)
      .set({ revokedAt: new Date().toISOString() })
      .where(eq(schema.apiKeys.id, id))
      .run();

    return c.json({ revoked: true });
  });

  return app;
}
