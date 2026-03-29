import type { Context, Next } from "hono";
import { eq, and, gt } from "drizzle-orm";
import { db, schema } from "../db/index.js";
import { hashKey } from "./crypto.js";
import type { SessionInfo } from "../routes/auth.js";

const DEFAULT_VAULT = process.env.HEBBS_DEFAULT_VAULT || "/data/vault";

export interface AuthInfo {
  keyId: number;
  role: "admin" | "workspace";
  workspaceId: number | null;
  vaultPath: string;
}

// API key auth for /v1/* data-plane endpoints
export async function authMiddleware(c: Context, next: Next) {
  const path = c.req.path;

  // Public endpoints
  if (
    path === "/v1/health/live" ||
    path === "/v1/system/health" ||
    path === "/v1/onboarding/status" ||
    path === "/v1/onboarding" ||
    path === "/v1/auth/login"
  ) {
    return next();
  }

  const authHeader = c.req.header("Authorization");
  if (!authHeader?.startsWith("Bearer ")) {
    return c.json({ error: "Missing or invalid Authorization header" }, 401);
  }

  const token = authHeader.slice(7);

  // Try session token first (for dashboard)
  const now = new Date().toISOString();
  const [session] = db
    .select({
      id: schema.sessions.id,
      accountId: schema.sessions.accountId,
    })
    .from(schema.sessions)
    .where(
      and(
        eq(schema.sessions.token, token),
        gt(schema.sessions.expiresAt, now)
      )
    )
    .limit(1)
    .all();

  if (session) {
    const [account] = db
      .select()
      .from(schema.accounts)
      .where(eq(schema.accounts.id, session.accountId))
      .limit(1)
      .all();

    if (account) {
      c.set("session", {
        accountId: account.id,
        email: account.email,
        role: account.role,
      } satisfies SessionInfo);

      // For session auth, set a default AuthInfo so data-plane routes work
      c.set("auth", {
        keyId: 0,
        role: account.role === "admin" ? "admin" : "workspace",
        workspaceId: null,
        vaultPath: DEFAULT_VAULT,
      } satisfies AuthInfo);

      return next();
    }
  }

  // Try API key auth
  const hash = hashKey(token);

  const [key] = db
    .select()
    .from(schema.apiKeys)
    .where(eq(schema.apiKeys.keyHash, hash))
    .limit(1)
    .all();

  if (!key || key.revokedAt) {
    return c.json({ error: "Invalid or revoked API key / session" }, 401);
  }

  // Resolve vault path from workspace
  let vaultPath = DEFAULT_VAULT;
  if (key.workspaceId) {
    const [ws] = db
      .select()
      .from(schema.workspaces)
      .where(eq(schema.workspaces.id, key.workspaceId))
      .limit(1)
      .all();

    if (ws) {
      vaultPath = ws.vaultPath;
    }
  }

  c.set("auth", {
    keyId: key.id,
    role: key.role,
    workspaceId: key.workspaceId,
    vaultPath,
  } satisfies AuthInfo);

  return next();
}
