import { Hono } from "hono";
import { eq } from "drizzle-orm";
import { db, schema } from "../db/index.js";
import { hashPassword } from "./auth.js";
import type { SessionInfo } from "./auth.js";

export function accountRoutes() {
  const app = new Hono();

  // List accounts (admin only)
  app.get("/v1/accounts", async (c) => {
    const session = c.get("session" as never) as SessionInfo;
    if (session.role !== "admin") {
      return c.json({ error: "Admin access required" }, 403);
    }

    const rows = db
      .select({
        id: schema.accounts.id,
        email: schema.accounts.email,
        role: schema.accounts.role,
        createdAt: schema.accounts.createdAt,
      })
      .from(schema.accounts)
      .all();

    // Attach workspace assignments
    const accounts = rows.map((account) => {
      const assignments = db
        .select({
          workspaceId: schema.accountWorkspaces.workspaceId,
          slug: schema.workspaces.slug,
          name: schema.workspaces.name,
        })
        .from(schema.accountWorkspaces)
        .innerJoin(
          schema.workspaces,
          eq(schema.accountWorkspaces.workspaceId, schema.workspaces.id)
        )
        .where(eq(schema.accountWorkspaces.accountId, account.id))
        .all();

      return { ...account, workspaces: assignments };
    });

    return c.json({ accounts });
  });

  // Create account (admin only)
  app.post("/v1/accounts", async (c) => {
    const session = c.get("session" as never) as SessionInfo;
    if (session.role !== "admin") {
      return c.json({ error: "Admin access required" }, 403);
    }

    const body = await c.req.json();
    const { email, password, role, workspace_ids } = body;

    if (!email || !password) {
      return c.json({ error: "Email and password required" }, 400);
    }

    // Check for existing account
    const existing = db
      .select()
      .from(schema.accounts)
      .where(eq(schema.accounts.email, email))
      .limit(1)
      .all();

    if (existing.length > 0) {
      return c.json({ error: "Account already exists" }, 409);
    }

    const passwordHash = hashPassword(password);
    const account = db
      .insert(schema.accounts)
      .values({
        email,
        passwordHash,
        role: role || "developer",
        createdAt: new Date().toISOString(),
      })
      .returning()
      .get();

    // Assign workspaces
    if (workspace_ids && Array.isArray(workspace_ids)) {
      for (const wsId of workspace_ids) {
        db.insert(schema.accountWorkspaces)
          .values({ accountId: account.id, workspaceId: wsId })
          .run();
      }
    }

    return c.json(
      {
        account: {
          id: account.id,
          email: account.email,
          role: account.role,
          createdAt: account.createdAt,
        },
      },
      201
    );
  });

  // Update account (admin only)
  app.put("/v1/accounts/:id", async (c) => {
    const session = c.get("session" as never) as SessionInfo;
    if (session.role !== "admin") {
      return c.json({ error: "Admin access required" }, 403);
    }

    const id = parseInt(c.req.param("id"), 10);
    const body = await c.req.json();

    const updates: Record<string, unknown> = {};
    if (body.role) updates.role = body.role;
    if (body.password) updates.passwordHash = hashPassword(body.password);

    if (Object.keys(updates).length > 0) {
      db.update(schema.accounts)
        .set(updates)
        .where(eq(schema.accounts.id, id))
        .run();
    }

    // Update workspace assignments if provided
    if (body.workspace_ids && Array.isArray(body.workspace_ids)) {
      db.delete(schema.accountWorkspaces)
        .where(eq(schema.accountWorkspaces.accountId, id))
        .run();

      for (const wsId of body.workspace_ids) {
        db.insert(schema.accountWorkspaces)
          .values({ accountId: id, workspaceId: wsId })
          .run();
      }
    }

    return c.json({ updated: true });
  });

  // Delete account (admin only)
  app.delete("/v1/accounts/:id", async (c) => {
    const session = c.get("session" as never) as SessionInfo;
    if (session.role !== "admin") {
      return c.json({ error: "Admin access required" }, 403);
    }

    const id = parseInt(c.req.param("id"), 10);

    // Prevent self-deletion
    if (id === session.accountId) {
      return c.json({ error: "Cannot delete your own account" }, 400);
    }

    db.delete(schema.accountWorkspaces)
      .where(eq(schema.accountWorkspaces.accountId, id))
      .run();

    db.delete(schema.sessions)
      .where(eq(schema.sessions.accountId, id))
      .run();

    db.delete(schema.accounts)
      .where(eq(schema.accounts.id, id))
      .run();

    return c.json({ deleted: true });
  });

  return app;
}
