import { Hono } from "hono";
import { DaemonClient } from "../lib/daemon-client.js";

const ENGINE_SOCKET =
  process.env.HEBBS_ENGINE_SOCKET || "/data/daemon/daemon.sock";
const VERSION = process.env.npm_package_version || "0.1.0";

export function healthRoutes() {
  const app = new Hono();
  const daemon = new DaemonClient(ENGINE_SOCKET);

  app.get("/v1/health/live", (c) => {
    return c.json({ status: "ok", version: VERSION });
  });

  app.get("/v1/system/health", async (c) => {
    try {
      const resp = await daemon.send(
        { type: "ping" },
        undefined,
        5000
      );
      const engineOk = resp.status === "ok";
      return c.json({
        status: engineOk ? "ok" : "degraded",
        version: VERSION,
        engine: engineOk ? "healthy" : "unreachable",
      });
    } catch {
      return c.json(
        { status: "degraded", version: VERSION, engine: "unreachable" },
        503
      );
    }
  });

  return app;
}
