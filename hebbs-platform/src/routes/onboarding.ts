import { Hono } from "hono";
import { db, schema } from "../db/index.js";
import { hashPassword } from "./auth.js";
import { generateApiKey, hashKey, keyPrefix } from "../lib/crypto.js";
import { initWorkspaceVault, slugify } from "../lib/workspace.js";

export function onboardingRoutes() {
  const app = new Hono();

  // Check onboarding status (no auth required)
  app.get("/v1/onboarding/status", async (c) => {
    const rows = db.select().from(schema.onboarding).limit(1).all();
    const completed = rows.length > 0 && rows[0].completed === 1;
    return c.json({ completed });
  });

  // Complete onboarding (no auth required, only works once)
  app.post("/v1/onboarding", async (c) => {
    // Check if already completed
    const rows = db.select().from(schema.onboarding).limit(1).all();
    if (rows.length > 0 && rows[0].completed === 1) {
      return c.json({ error: "Onboarding already completed" }, 400);
    }

    const body = await c.req.json();
    const { email, password, workspace_name } = body;

    if (!email || !password || !workspace_name) {
      return c.json({ error: "email, password, and workspace_name required" }, 400);
    }

    // Create admin account
    const passwordHash = hashPassword(password);
    const account = db
      .insert(schema.accounts)
      .values({
        email,
        passwordHash,
        role: "admin",
        createdAt: new Date().toISOString(),
      })
      .returning()
      .get();

    // Create workspace
    const slug = slugify(workspace_name);
    const vaultPath = await initWorkspaceVault(slug);

    const workspace = db
      .insert(schema.workspaces)
      .values({
        slug,
        name: workspace_name,
        vaultPath,
        createdAt: new Date().toISOString(),
      })
      .returning()
      .get();

    // Assign workspace to admin
    db.insert(schema.accountWorkspaces)
      .values({ accountId: account.id, workspaceId: workspace.id })
      .run();

    // Generate workspace API key
    const rawKey = generateApiKey("workspace");
    const hash = hashKey(rawKey);
    const prefix = keyPrefix(rawKey);

    db.insert(schema.apiKeys)
      .values({
        keyHash: hash,
        keyPrefix: prefix,
        label: `${slug}-default`,
        role: "workspace",
        workspaceId: workspace.id,
        createdAt: new Date().toISOString(),
      })
      .run();

    // Mark onboarding complete
    if (rows.length === 0) {
      db.insert(schema.onboarding)
        .values({ completed: 1, completedAt: new Date().toISOString() })
        .run();
    } else {
      db.update(schema.onboarding)
        .set({ completed: 1, completedAt: new Date().toISOString() })
        .run();
    }

    return c.json({
      account: { id: account.id, email: account.email, role: account.role },
      workspace: { id: workspace.id, slug, name: workspace_name },
      api_key: rawKey,
    }, 201);
  });

  return app;
}
