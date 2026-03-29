import { sqliteTable, text, integer } from "drizzle-orm/sqlite-core";

export const apiKeys = sqliteTable("api_keys", {
  id: integer("id").primaryKey({ autoIncrement: true }),
  keyHash: text("key_hash").notNull().unique(),
  keyPrefix: text("key_prefix").notNull(),
  label: text("label").notNull().default("default"),
  role: text("role", { enum: ["admin", "workspace"] }).notNull().default("admin"),
  workspaceId: integer("workspace_id"),
  createdAt: text("created_at")
    .notNull()
    .$defaultFn(() => new Date().toISOString()),
  revokedAt: text("revoked_at"),
});

export const workspaces = sqliteTable("workspaces", {
  id: integer("id").primaryKey({ autoIncrement: true }),
  slug: text("slug").notNull().unique(),
  name: text("name").notNull(),
  vaultPath: text("vault_path").notNull(),
  createdAt: text("created_at")
    .notNull()
    .$defaultFn(() => new Date().toISOString()),
});

export const accounts = sqliteTable("accounts", {
  id: integer("id").primaryKey({ autoIncrement: true }),
  email: text("email").notNull().unique(),
  passwordHash: text("password_hash").notNull(),
  role: text("role", { enum: ["admin", "developer"] }).notNull().default("developer"),
  createdAt: text("created_at")
    .notNull()
    .$defaultFn(() => new Date().toISOString()),
});

export const accountWorkspaces = sqliteTable("account_workspaces", {
  id: integer("id").primaryKey({ autoIncrement: true }),
  accountId: integer("account_id").notNull(),
  workspaceId: integer("workspace_id").notNull(),
});

export const sessions = sqliteTable("sessions", {
  id: integer("id").primaryKey({ autoIncrement: true }),
  token: text("token").notNull().unique(),
  accountId: integer("account_id").notNull(),
  expiresAt: text("expires_at").notNull(),
  createdAt: text("created_at")
    .notNull()
    .$defaultFn(() => new Date().toISOString()),
});

export const onboarding = sqliteTable("onboarding", {
  id: integer("id").primaryKey({ autoIncrement: true }),
  completed: integer("completed").notNull().default(0),
  completedAt: text("completed_at"),
});
