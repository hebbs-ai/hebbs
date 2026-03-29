import { Hono } from "hono";
import { serve } from "@hono/node-server";
import { serveStatic } from "@hono/node-server/serve-static";
import { logger } from "hono/logger";

import { migrate } from "./db/index.js";
import { authMiddleware } from "./lib/auth.js";
import { bootstrapAdminKey } from "./lib/bootstrap.js";
import { healthRoutes } from "./routes/health.js";
import { memoryRoutes } from "./routes/memories.js";
import { uploadRoutes } from "./routes/upload.js";
import { keyRoutes } from "./routes/keys.js";
import { workspaceRoutes } from "./routes/workspaces.js";
import { authRoutes } from "./routes/auth.js";
import { accountRoutes } from "./routes/accounts.js";
import { onboardingRoutes } from "./routes/onboarding.js";
import { configRoutes } from "./routes/config.js";
import { panelProxyRoutes } from "./routes/panel-proxy.js";

const PORT = parseInt(process.env.PORT || "8080", 10);
const DASHBOARD_DIR = process.env.HEBBS_DASHBOARD_DIR || "/app/dashboard";

// Initialize database
migrate();

// Bootstrap admin key on first start
const adminKey = bootstrapAdminKey();
if (adminKey) {
  console.log("");
  console.log("=".repeat(60));
  console.log("  HEBBS Platform: First Start");
  console.log("  Bootstrap admin API key (save this, shown only once):");
  console.log("");
  console.log(`  ${adminKey}`);
  console.log("");
  console.log("=".repeat(60));
  console.log("");
}

const app = new Hono();

app.use("*", logger());

// API: Health routes (no auth)
app.route("/", healthRoutes());

// API: Panel proxy (no auth, proxies /api/panel/* to engine)
app.route("/", panelProxyRoutes());

// API: Auth middleware for /v1/* (except public endpoints)
app.use("/v1/*", authMiddleware);

// API: Public routes (auth handled internally)
app.route("/", authRoutes());
app.route("/", onboardingRoutes());

// API: Authenticated routes
app.route("/", memoryRoutes());
app.route("/", uploadRoutes());
app.route("/", keyRoutes());
app.route("/", workspaceRoutes());
app.route("/", accountRoutes());
app.route("/", configRoutes());

// Dashboard: Next.js static assets
app.use("/_next/*", serveStatic({ root: DASHBOARD_DIR }));

// Dashboard: static pages (login, onboarding, settings, team)
for (const page of ["login", "onboarding", "settings", "team"]) {
  app.get(`/${page}/*`, serveStatic({ root: DASHBOARD_DIR }));
  app.get(`/${page}`, serveStatic({ root: DASHBOARD_DIR }));
}

// Dashboard: workspace routes (SPA fallback to workspaces/index.html)
app.get("/workspaces/*", async (c) => {
  const fs = await import("node:fs/promises");
  const path = await import("node:path");
  const html = await fs.readFile(
    path.join(DASHBOARD_DIR, "workspaces", "index.html"),
    "utf-8"
  );
  return c.html(html);
});

// Dashboard: root
app.get("/", serveStatic({ root: DASHBOARD_DIR, path: "/index.html" }));

console.log(`HEBBS Platform listening on port ${PORT}`);

serve({ fetch: app.fetch, port: PORT });
