#!/usr/bin/env node

import { Command } from "commander";
import { loadConfig, saveConfig } from "./config.js";
import { api } from "./api.js";
import { readFileSync, readdirSync, statSync } from "node:fs";
import { join, relative } from "node:path";

const program = new Command();
program.name("hb").description("HEBBS remote client").version("0.1.0");

// ── Login ──

program
  .command("login")
  .description("Authenticate with a HEBBS server")
  .requiredOption("--endpoint <url>", "Server URL (e.g. http://host:8080)")
  .option("--api-key <key>", "API key (skips browser login)")
  .action(async (opts) => {
    const endpoint = opts.endpoint.replace(/\/$/, "");

    // Test connection
    try {
      const health = await fetch(`${endpoint}/v1/system/health`);
      if (!health.ok) throw new Error();
      console.log(`Connected to ${endpoint}`);
    } catch {
      console.error(`Cannot reach ${endpoint}`);
      process.exit(1);
    }

    saveConfig({ endpoint, api_key: opts.apiKey || "" });

    if (opts.apiKey) {
      console.log("API key saved. Ready.");
    } else {
      console.log("Endpoint saved. Set API key with: hb login --endpoint ... --api-key ...");
    }
  });

// ── Status ──

program
  .command("status")
  .description("Show workspace health and indexing status")
  .action(async () => {
    const config = loadConfig();
    try {
      const health = await api.get<Record<string, unknown>>("/v1/system/health");
      console.log(`Engine:  ${health.engine}`);
      console.log(`Version: ${health.version}`);
      console.log(`Status:  ${health.status}`);
    } catch (e: unknown) {
      console.error(e instanceof Error ? e.message : "Failed");
      process.exit(1);
    }
  });

// ── Recall ──

program
  .command("recall <query>")
  .description("Search memories")
  .option("-k, --top-k <n>", "Number of results", "10")
  .option("--entity-id <id>", "Scope to entity")
  .option("--strategy <s>", "Search strategy (similarity, temporal, causal, analogical)")
  .option("--format <f>", "Output format (text, json)", "text")
  .action(async (query, opts) => {
    try {
      const res = await api.post<{ results: Array<Record<string, unknown>>; count?: number }>("/v1/recall", {
        cue: query,
        top_k: parseInt(opts.topK),
        entity_id: opts.entityId,
        strategy: opts.strategy,
      });

      if (opts.format === "json") {
        console.log(JSON.stringify(res, null, 2));
        return;
      }

      const results = res.results || [];
      if (results.length === 0) {
        console.log("No results.");
        return;
      }

      for (let i = 0; i < results.length; i++) {
        const r = results[i];
        const score = Number(r.score || 0).toFixed(3);
        console.log(`  ${i + 1}. [${score}] ${r.content}`);
        if (r.file_path) console.log(`     source: ${r.file_path}`);
      }
    } catch (e: unknown) {
      console.error(e instanceof Error ? e.message : "Failed");
      process.exit(1);
    }
  });

// ── Remember ──

program
  .command("remember <content>")
  .description("Store a memory")
  .option("--entity-id <id>", "Entity ID")
  .option("--importance <n>", "Importance (0-1)", "0.5")
  .action(async (content, opts) => {
    try {
      const res = await api.post<Record<string, unknown>>("/v1/memories", {
        content,
        entity_id: opts.entityId,
        importance: parseFloat(opts.importance),
      });
      console.log(`Remembered: "${content}"`);
      if (res.memory_id) console.log(`  id: ${res.memory_id}`);
    } catch (e: unknown) {
      console.error(e instanceof Error ? e.message : "Failed");
      process.exit(1);
    }
  });

// ── Prime ──

program
  .command("prime <entity-id>")
  .description("Load all memories for an entity")
  .option("--max <n>", "Max memories", "30")
  .option("--format <f>", "Output format (text, json)", "text")
  .action(async (entityId, opts) => {
    try {
      const res = await api.post<{ results: Array<Record<string, unknown>> }>("/v1/prime", {
        entity_id: entityId,
        max_memories: parseInt(opts.max),
      });

      if (opts.format === "json") {
        console.log(JSON.stringify(res, null, 2));
        return;
      }

      const results = res.results || [];
      console.log(`${results.length} memories for "${entityId}":`);
      for (const r of results) {
        console.log(`  - ${r.content}`);
      }
    } catch (e: unknown) {
      console.error(e instanceof Error ? e.message : "Failed");
      process.exit(1);
    }
  });

// ── Forget ──

program
  .command("forget")
  .description("Delete memories")
  .option("--entity-id <id>", "Delete all memories for entity")
  .option("--ids <ids>", "Comma-separated memory IDs")
  .action(async (opts) => {
    if (!opts.entityId && !opts.ids) {
      console.error("Specify --entity-id or --ids");
      process.exit(1);
    }
    try {
      const body: Record<string, unknown> = {};
      if (opts.entityId) body.entity_id = opts.entityId;
      if (opts.ids) body.ids = opts.ids.split(",");
      const res = await api.post<Record<string, unknown>>("/v1/forget", body);
      console.log(`Forgotten: ${res.forgotten_count || 0} memories`);
    } catch (e: unknown) {
      console.error(e instanceof Error ? e.message : "Failed");
      process.exit(1);
    }
  });

// ── Push ──

program
  .command("push <path>")
  .description("Upload files to the server")
  .action(async (dirPath) => {
    try {
      const files = collectFiles(dirPath);
      if (files.length === 0) {
        console.log("No files found.");
        return;
      }

      console.log(`Pushing ${files.length} file(s)...`);

      const formData = new FormData();
      for (const f of files) {
        const content = readFileSync(f.absolute);
        const blob = new Blob([content]);
        formData.append("files", blob, f.relative);
      }

      const res = await api.post<{ uploaded: number }>("/v1/upload", undefined);
      // Use fetch directly for multipart
      const config = loadConfig();
      const resp = await fetch(`${config.endpoint}/v1/upload`, {
        method: "POST",
        headers: { Authorization: `Bearer ${config.api_key}` },
        body: formData,
      });
      const data = await resp.json() as Record<string, unknown>;
      console.log(`Uploaded: ${data.uploaded} file(s). Indexing triggered.`);
    } catch (e: unknown) {
      console.error(e instanceof Error ? e.message : "Failed");
      process.exit(1);
    }
  });

// ── Workspaces ──

const ws = program.command("workspaces").description("Manage workspaces");

ws.command("list").action(async () => {
  try {
    const res = await api.get<{ workspaces: Array<Record<string, unknown>> }>("/v1/workspaces");
    for (const w of res.workspaces) {
      const stats = w.stats as Record<string, number> | undefined;
      console.log(`  ${w.slug}  memories:${stats?.memories ?? 0}  files:${stats?.files ?? 0}`);
    }
  } catch (e: unknown) {
    console.error(e instanceof Error ? e.message : "Failed");
    process.exit(1);
  }
});

ws.command("create <name>").action(async (name) => {
  try {
    const res = await api.post<{ workspace: Record<string, unknown>; api_key: string }>("/v1/workspaces", { name });
    console.log(`Created: ${res.workspace.slug}`);
    console.log(`API key: ${res.api_key}`);
  } catch (e: unknown) {
    console.error(e instanceof Error ? e.message : "Failed");
    process.exit(1);
  }
});

ws.command("switch <slug>").action(async (slug) => {
  saveConfig({ workspace: slug });
  console.log(`Switched to workspace: ${slug}`);
});

// ── Keys ──

const keys = program.command("keys").description("Manage API keys");

keys.command("list").action(async () => {
  try {
    const res = await api.get<{ keys: Array<Record<string, unknown>> }>("/v1/keys");
    for (const k of res.keys) {
      if (k.revokedAt) continue;
      console.log(`  ${k.prefix}  ${k.label}  (${k.role})`);
    }
  } catch (e: unknown) {
    console.error(e instanceof Error ? e.message : "Failed");
    process.exit(1);
  }
});

keys.command("create [label]").action(async (label) => {
  try {
    const res = await api.post<{ api_key: string }>("/v1/keys", { label: label || "cli-key" });
    console.log(`API key: ${res.api_key}`);
  } catch (e: unknown) {
    console.error(e instanceof Error ? e.message : "Failed");
    process.exit(1);
  }
});

keys.command("revoke <id>").action(async (id) => {
  try {
    await api.del(`/v1/keys/${id}`);
    console.log("Key revoked.");
  } catch (e: unknown) {
    console.error(e instanceof Error ? e.message : "Failed");
    process.exit(1);
  }
});

// ── Dashboard ──

program
  .command("dashboard")
  .description("Open the dashboard in a browser")
  .action(async () => {
    const config = loadConfig();
    if (!config.endpoint) {
      console.error("Not logged in.");
      process.exit(1);
    }
    const url = config.endpoint;
    console.log(`Open: ${url}`);
    // Try to open browser
    const { exec } = await import("node:child_process");
    const cmd = process.platform === "darwin" ? "open" : process.platform === "win32" ? "start" : "xdg-open";
    exec(`${cmd} ${url}`);
  });

// ── Helpers ──

function collectFiles(dir: string): Array<{ absolute: string; relative: string }> {
  const results: Array<{ absolute: string; relative: string }> = [];
  const walk = (d: string) => {
    for (const entry of readdirSync(d)) {
      if (entry.startsWith(".")) continue;
      const full = join(d, entry);
      const stat = statSync(full);
      if (stat.isDirectory()) walk(full);
      else if (/\.(md|txt|pdf)$/i.test(entry)) {
        results.push({ absolute: full, relative: relative(dir, full) });
      }
    }
  };
  walk(dir);
  return results;
}

program.parse();
