import { readFileSync, writeFileSync, mkdirSync, existsSync } from "node:fs";
import { join } from "node:path";
import { homedir } from "node:os";

const CONFIG_DIR = join(homedir(), ".hb");
const CONFIG_FILE = join(CONFIG_DIR, "config.json");

export interface HbConfig {
  endpoint: string;
  api_key: string;
  workspace?: string;
}

export function loadConfig(): HbConfig {
  // Environment overrides
  const envEndpoint = process.env.HEBBS_ENDPOINT;
  const envKey = process.env.HEBBS_API_KEY;
  const envWorkspace = process.env.HEBBS_WORKSPACE;

  let file: Partial<HbConfig> = {};
  if (existsSync(CONFIG_FILE)) {
    try {
      file = JSON.parse(readFileSync(CONFIG_FILE, "utf-8"));
    } catch {
      // corrupt config, ignore
    }
  }

  return {
    endpoint: envEndpoint || file.endpoint || "",
    api_key: envKey || file.api_key || "",
    workspace: envWorkspace || file.workspace,
  };
}

export function saveConfig(config: Partial<HbConfig>) {
  const current = loadConfig();
  const merged = { ...current, ...config };
  mkdirSync(CONFIG_DIR, { recursive: true });
  writeFileSync(CONFIG_FILE, JSON.stringify(merged, null, 2));
}
