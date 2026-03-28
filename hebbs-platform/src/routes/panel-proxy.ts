import { Hono } from "hono";

const ENGINE_URL = process.env.HEBBS_ENGINE_URL || "http://engine:6381";

export function panelProxyRoutes() {
  const app = new Hono();

  // Proxy Memory Palace UI and panel API to engine (no auth required for panel)
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

  // Proxy static assets (Memory Palace UI)
  app.get("/static/*", async (c) => {
    const url = new URL(c.req.url);
    const target = `${ENGINE_URL}${url.pathname}`;
    const resp = await fetch(target);
    return new Response(resp.body, {
      status: resp.status,
      headers: resp.headers,
    });
  });

  // Proxy root to Memory Palace
  app.get("/", async (c) => {
    const resp = await fetch(`${ENGINE_URL}/`);
    return new Response(resp.body, {
      status: resp.status,
      headers: resp.headers,
    });
  });

  return app;
}
