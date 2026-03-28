import { mkdir, writeFile, chmod } from "node:fs/promises";
import { existsSync } from "node:fs";
import { join } from "node:path";

const WORKSPACES_DIR = process.env.HEBBS_WORKSPACES_DIR || "/data/workspaces";

export function workspaceVaultPath(slug: string): string {
  return join(WORKSPACES_DIR, slug);
}

async function mkdirWritable(path: string) {
  await mkdir(path, { recursive: true });
  await chmod(path, 0o777);
}

async function writeFileWritable(path: string, content: string) {
  await writeFile(path, content);
  await chmod(path, 0o666);
}

export async function initWorkspaceVault(slug: string): Promise<string> {
  const vaultPath = workspaceVaultPath(slug);
  const hebbsDir = join(vaultPath, ".hebbs");

  if (existsSync(hebbsDir)) {
    return vaultPath;
  }

  await mkdirWritable(vaultPath);
  await mkdirWritable(hebbsDir);

  const llmProvider = process.env.HEBBS_LLM_PROVIDER || "openai";
  const llmModel = process.env.HEBBS_LLM_MODEL || "gpt-4o-mini";
  const llmKey = process.env.HEBBS_LLM_API_KEY || "";

  const config = `[llm]
provider = "${llmProvider}"
model = "${llmModel}"
api_key = "${llmKey}"

[embedding]
provider = "openai"
model = "text-embedding-3-small"
dimensions = 1536
api_key = "${llmKey}"
`;

  await writeFileWritable(join(hebbsDir, "config.toml"), config);
  await writeFileWritable(join(hebbsDir, "manifest.json"), JSON.stringify({ files: {} }));
  await writeFileWritable(join(hebbsDir, "epoch"), "0");

  return vaultPath;
}

export function slugify(name: string): string {
  return name
    .toLowerCase()
    .replace(/[^a-z0-9]+/g, "-")
    .replace(/^-|-$/g, "");
}
