import { Hono } from "hono";
import { readFile, writeFile } from "node:fs/promises";
import { join } from "node:path";
import type { SessionInfo } from "./auth.js";

const DEFAULT_VAULT = process.env.HEBBS_DEFAULT_VAULT || "/data/vault";

interface HebbsConfig {
  llm: { provider: string; model: string; api_key?: string };
  embedding: { provider: string; model: string; dimensions: number };
  decay: { enabled: boolean; half_life_days: number };
  reflection: { enabled: boolean; interval_hours: number };
  contradiction: { enabled: boolean };
  api: { max_concurrent_requests: number };
  deployment: {
    id: string;
    heartbeat_enabled: boolean;
    central_url: string;
  };
}

async function readVaultConfig(vaultPath: string): Promise<HebbsConfig> {
  const configPath = join(vaultPath, ".hebbs", "config.toml");
  let raw = "";
  try {
    raw = await readFile(configPath, "utf-8");
  } catch {
    // File doesn't exist, return defaults
  }

  // Simple TOML parser for known fields
  const get = (key: string): string | undefined => {
    const match = raw.match(new RegExp(`^${key}\\s*=\\s*"?([^"\\n]*)"?`, "m"));
    return match?.[1];
  };

  return {
    llm: {
      provider: get("provider") || process.env.HEBBS_LLM_PROVIDER || "openai",
      model: get("model") || process.env.HEBBS_LLM_MODEL || "gpt-4o-mini",
    },
    embedding: {
      provider: "openai",
      model: "text-embedding-3-small",
      dimensions: 1536,
    },
    decay: {
      enabled: true,
      half_life_days: 30,
    },
    reflection: {
      enabled: false,
      interval_hours: 1,
    },
    contradiction: {
      enabled: true,
    },
    api: {
      max_concurrent_requests: 10,
    },
    deployment: {
      id: process.env.HEBBS_DEPLOYMENT_ID || "",
      heartbeat_enabled: process.env.HEBBS_HEARTBEAT_ENABLED !== "false",
      central_url: process.env.HEBBS_CENTRAL_URL || "https://central.hebbs.ai",
    },
  };
}

export function configRoutes() {
  const app = new Hono();

  // Get config
  app.get("/v1/config", async (c) => {
    const session = c.get("session" as never) as SessionInfo | undefined;
    if (!session || session.role !== "admin") {
      return c.json({ error: "Admin access required" }, 403);
    }

    const config = await readVaultConfig(DEFAULT_VAULT);
    return c.json({ config });
  });

  // Update config
  app.put("/v1/config", async (c) => {
    const session = c.get("session" as never) as SessionInfo | undefined;
    if (!session || session.role !== "admin") {
      return c.json({ error: "Admin access required" }, 403);
    }

    const body = await c.req.json();
    const current = await readVaultConfig(DEFAULT_VAULT);

    // Merge updates
    const updated = { ...current, ...body };

    // Write back to config.toml
    const configPath = join(DEFAULT_VAULT, ".hebbs", "config.toml");
    const toml = `[llm]
provider = "${updated.llm?.provider || current.llm.provider}"
model = "${updated.llm?.model || current.llm.model}"
${updated.llm?.api_key ? `api_key = "${updated.llm.api_key}"` : ""}

[embedding]
provider = "${updated.embedding?.provider || current.embedding.provider}"
model = "${updated.embedding?.model || current.embedding.model}"
dimensions = ${updated.embedding?.dimensions || current.embedding.dimensions}

[decay]
half_life_days = ${updated.decay?.half_life_days ?? current.decay.half_life_days}

[api]
max_concurrent_requests = ${updated.api?.max_concurrent_requests ?? current.api.max_concurrent_requests}
`;

    await writeFile(configPath, toml);

    return c.json({ updated: true, config: updated });
  });

  return app;
}
