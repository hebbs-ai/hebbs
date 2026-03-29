import { createHash, randomBytes } from "node:crypto";

const KEY_PREFIX = "hb_live_sk_";
const ADMIN_PREFIX = "hb_admin_sk_";

export function generateApiKey(role: "admin" | "workspace" = "workspace"): string {
  const prefix = role === "admin" ? ADMIN_PREFIX : KEY_PREFIX;
  const random = randomBytes(24).toString("base64url");
  return `${prefix}${random}`;
}

export function hashKey(key: string): string {
  return createHash("sha256").update(key).digest("hex");
}

export function keyPrefix(key: string): string {
  return key.slice(0, 16) + "...";
}
