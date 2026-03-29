import { Hono } from "hono";
import { eq } from "drizzle-orm";
import { db, schema } from "../db/index.js";

const ENGINE_URL = process.env.HEBBS_ENGINE_URL || "http://engine:6381";

// Track which vault the engine panel is currently showing
let currentPanelVault: string | null = null;

async function switchPanelVault(vaultPath: string): Promise<boolean> {
  if (currentPanelVault === vaultPath) return true;

  try {
    const resp = await fetch(`${ENGINE_URL}/api/panel/vaults/switch`, {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ path: vaultPath }),
    });
    if (resp.ok) {
      currentPanelVault = vaultPath;
      return true;
    }
  } catch {
    // Engine may not be ready
  }
  return false;
}

export function panelProxyRoutes() {
  const app = new Hono();

  // Workspace-scoped panel proxy: /api/panel/ws/:slug/*
  // Switches the engine panel to the workspace vault before proxying
  app.all("/api/panel/ws/:slug/*", async (c) => {
    const slug = c.req.param("slug");

    // Resolve workspace vault path
    const [ws] = db
      .select()
      .from(schema.workspaces)
      .where(eq(schema.workspaces.slug, slug))
      .limit(1)
      .all();

    if (!ws) {
      return c.json({ error: "Workspace not found" }, 404);
    }

    // Switch the engine panel to this workspace
    const switched = await switchPanelVault(ws.vaultPath);
    if (!switched) {
      return c.json({ error: "Failed to switch vault" }, 503);
    }

    // Strip /ws/:slug from the path and proxy to engine
    const url = new URL(c.req.url);
    const panelPath = url.pathname.replace(`/api/panel/ws/${slug}`, "/api/panel");
    const target = `${ENGINE_URL}${panelPath}${url.search}`;

    const resp = await fetch(target, {
      method: c.req.method,
      headers: c.req.raw.headers,
      body: c.req.method !== "GET" ? c.req.raw.body : undefined,
      // @ts-ignore: duplex needed for streaming body
      duplex: "half",
    });

    return new Response(resp.body, {
      status: resp.status,
      headers: resp.headers,
    });
  });

  // Direct panel proxy (no workspace scoping, uses current active vault)
  app.all("/api/panel/*", async (c) => {
    const url = new URL(c.req.url);
    const target = `${ENGINE_URL}${url.pathname}${url.search}`;

    const resp = await fetch(target, {
      method: c.req.method,
      headers: c.req.raw.headers,
      body: c.req.method !== "GET" ? c.req.raw.body : undefined,
      // @ts-ignore: duplex needed for streaming body
      duplex: "half",
    });

    return new Response(resp.body, {
      status: resp.status,
      headers: resp.headers,
    });
  });

  return app;
}
