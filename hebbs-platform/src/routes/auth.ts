import { Hono } from "hono";
import { createHash, randomBytes } from "node:crypto";
import { eq, and, gt } from "drizzle-orm";
import { db, schema } from "../db/index.js";

function hashPassword(password: string): string {
  const salt = randomBytes(16).toString("hex");
  const hash = createHash("sha256").update(salt + password).digest("hex");
  return `${salt}:${hash}`;
}

function verifyPassword(password: string, stored: string): boolean {
  const [salt, hash] = stored.split(":");
  const check = createHash("sha256").update(salt + password).digest("hex");
  return check === hash;
}

function generateSessionToken(): string {
  return randomBytes(32).toString("base64url");
}

export interface SessionInfo {
  accountId: number;
  email: string;
  role: "admin" | "developer";
}

export function authRoutes() {
  const app = new Hono();

  // Login
  app.post("/v1/auth/login", async (c) => {
    const body = await c.req.json();
    const { email, password } = body;

    if (!email || !password) {
      return c.json({ error: "Email and password required" }, 400);
    }

    const [account] = db
      .select()
      .from(schema.accounts)
      .where(eq(schema.accounts.email, email))
      .limit(1)
      .all();

    if (!account || !verifyPassword(password, account.passwordHash)) {
      return c.json({ error: "Invalid credentials" }, 401);
    }

    const token = generateSessionToken();
    const expiresAt = new Date(Date.now() + 7 * 24 * 60 * 60 * 1000).toISOString(); // 7 days

    db.insert(schema.sessions)
      .values({
        token,
        accountId: account.id,
        expiresAt,
        createdAt: new Date().toISOString(),
      })
      .run();

    return c.json({
      token,
      expires_at: expiresAt,
      account: {
        id: account.id,
        email: account.email,
        role: account.role,
      },
    });
  });

  // Logout
  app.post("/v1/auth/logout", async (c) => {
    const token = extractSessionToken(c);
    if (token) {
      db.delete(schema.sessions)
        .where(eq(schema.sessions.token, token))
        .run();
    }
    return c.json({ ok: true });
  });

  // Get current user
  app.get("/v1/auth/me", async (c) => {
    const session = c.get("session" as never) as SessionInfo | undefined;
    if (!session) {
      return c.json({ error: "Not authenticated" }, 401);
    }

    const [account] = db
      .select({
        id: schema.accounts.id,
        email: schema.accounts.email,
        role: schema.accounts.role,
        createdAt: schema.accounts.createdAt,
      })
      .from(schema.accounts)
      .where(eq(schema.accounts.id, session.accountId))
      .limit(1)
      .all();

    if (!account) {
      return c.json({ error: "Account not found" }, 404);
    }

    // Get assigned workspaces
    const assignments = db
      .select({ workspaceId: schema.accountWorkspaces.workspaceId })
      .from(schema.accountWorkspaces)
      .where(eq(schema.accountWorkspaces.accountId, account.id))
      .all();

    const workspaceIds = assignments.map((a) => a.workspaceId);

    return c.json({ account, workspaces: workspaceIds });
  });

  // Whoami: returns identity and workspace context of the authenticated key
  app.get("/v1/auth/whoami", async (c) => {
    const auth = c.get("auth" as never) as import("../lib/auth.js").AuthInfo | undefined;
    if (!auth) {
      return c.json({ error: "Not authenticated" }, 401);
    }

    const result: Record<string, unknown> = {
      key_id: auth.keyId,
      role: auth.role,
      workspace_id: auth.workspaceId,
      workspace_slug: null as string | null,
      workspace_name: null as string | null,
    };

    if (auth.workspaceId) {
      const [ws] = db
        .select({
          slug: schema.workspaces.slug,
          name: schema.workspaces.name,
        })
        .from(schema.workspaces)
        .where(eq(schema.workspaces.id, auth.workspaceId))
        .limit(1)
        .all();

      if (ws) {
        result.workspace_slug = ws.slug;
        result.workspace_name = ws.name;
      }
    }

    return c.json(result);
  });

  return app;
}

function extractSessionToken(c: { req: { header: (name: string) => string | undefined } }): string | null {
  const authHeader = c.req.header("Authorization");
  if (authHeader?.startsWith("Bearer ")) {
    return authHeader.slice(7);
  }
  return null;
}

export { hashPassword, verifyPassword };
