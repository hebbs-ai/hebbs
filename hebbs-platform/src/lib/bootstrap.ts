import { db, schema } from "../db/index.js";
import { generateApiKey, hashKey, keyPrefix } from "./crypto.js";

export function bootstrapAdminKey(): string | null {
  // Check if any admin key exists
  const existing = db
    .select()
    .from(schema.apiKeys)
    .limit(1)
    .all();

  if (existing.length > 0) return null;

  // First start: generate admin key
  const rawKey = generateApiKey("admin");
  const hash = hashKey(rawKey);
  const prefix = keyPrefix(rawKey);

  db.insert(schema.apiKeys)
    .values({
      keyHash: hash,
      keyPrefix: prefix,
      label: "bootstrap-admin",
      role: "admin",
      createdAt: new Date().toISOString(),
    })
    .run();

  return rawKey;
}
