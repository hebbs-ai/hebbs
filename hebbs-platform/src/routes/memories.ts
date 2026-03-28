import { Hono } from "hono";
import { DaemonClient } from "../lib/daemon-client.js";
import type { AuthInfo } from "../lib/auth.js";

const ENGINE_SOCKET =
  process.env.HEBBS_ENGINE_SOCKET || "/data/daemon/daemon.sock";

export function memoryRoutes() {
  const app = new Hono();
  const daemon = new DaemonClient(ENGINE_SOCKET);

  // Remember
  app.post("/v1/memories", async (c) => {
    const auth = c.get("auth" as never) as AuthInfo;
    const body = await c.req.json();
    const resp = await daemon.send(
      {
        type: "remember",
        content: body.content,
        importance: body.importance,
        entity_id: body.entity_id,
        context: body.context ? JSON.stringify(body.context) : undefined,
      },
      auth.vaultPath
    );

    if (resp.status === "error") {
      return c.json({ error: resp.error }, 500);
    }

    return c.json(resp.data, 201);
  });

  // Recall
  app.post("/v1/recall", async (c) => {
    const auth = c.get("auth" as never) as AuthInfo;
    const body = await c.req.json();
    const resp = await daemon.send(
      {
        type: "recall",
        cue: body.cue,
        top_k: body.top_k ?? 10,
        entity_id: body.entity_id,
        strategy: body.strategy,
      },
      auth.vaultPath
    );

    if (resp.status === "error") {
      return c.json({ error: resp.error }, 500);
    }

    return c.json(resp.data);
  });

  // Prime
  app.post("/v1/prime", async (c) => {
    const auth = c.get("auth" as never) as AuthInfo;
    const body = await c.req.json();
    const resp = await daemon.send(
      {
        type: "prime",
        entity_id: body.entity_id,
        max_memories: body.max_memories,
        similarity_cue: body.similarity_cue,
      },
      auth.vaultPath
    );

    if (resp.status === "error") {
      return c.json({ error: resp.error }, 500);
    }

    return c.json(resp.data);
  });

  // Forget
  app.post("/v1/forget", async (c) => {
    const auth = c.get("auth" as never) as AuthInfo;
    const body = await c.req.json();
    const resp = await daemon.send(
      {
        type: "forget",
        ids: body.ids,
        entity_id: body.entity_id,
      },
      auth.vaultPath
    );

    if (resp.status === "error") {
      return c.json({ error: resp.error }, 500);
    }

    return c.json(resp.data);
  });

  // Get memory by ID
  app.get("/v1/memories/:id", async (c) => {
    const auth = c.get("auth" as never) as AuthInfo;
    const id = c.req.param("id");
    const resp = await daemon.send(
      { type: "get", id },
      auth.vaultPath
    );

    if (resp.status === "error") {
      return c.json({ error: resp.error }, 404);
    }

    return c.json(resp.data);
  });

  // Entities
  app.get("/v1/entities", async (c) => {
    const auth = c.get("auth" as never) as AuthInfo;
    const resp = await daemon.send(
      { type: "export", limit: 10000 },
      auth.vaultPath
    );

    if (resp.status === "error") {
      return c.json({ error: resp.error }, 500);
    }

    const data = resp.data as { memories?: Array<{ entity_id?: string }> };
    const entities = [
      ...new Set(
        (data.memories || [])
          .map((m) => m.entity_id)
          .filter(Boolean)
      ),
    ];
    return c.json({ entities });
  });

  // Insights
  app.get("/v1/insights", async (c) => {
    const auth = c.get("auth" as never) as AuthInfo;
    const entityId = c.req.query("entity_id");
    const resp = await daemon.send(
      {
        type: "insights",
        entity_id: entityId,
      },
      auth.vaultPath
    );

    if (resp.status === "error") {
      return c.json({ error: resp.error }, 500);
    }

    return c.json(resp.data);
  });

  return app;
}
