import { Hono } from "hono";
import { serve } from "@hono/node-server";
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

// Health routes (no auth)
app.route("/", healthRoutes());

// Panel proxy (no auth, serves Memory Palace UI)
app.route("/", panelProxyRoutes());

// Auth middleware for /v1/* (except public endpoints)
app.use("/v1/*", authMiddleware);

// Public API routes (auth handled internally)
app.route("/", authRoutes());
app.route("/", onboardingRoutes());

// Authenticated API routes
app.route("/", memoryRoutes());
app.route("/", uploadRoutes());
app.route("/", keyRoutes());
app.route("/", workspaceRoutes());
app.route("/", accountRoutes());
app.route("/", configRoutes());

console.log(`HEBBS Platform listening on port ${PORT}`);

serve({ fetch: app.fetch, port: PORT });
