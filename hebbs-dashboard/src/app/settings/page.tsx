"use client";

import { useEffect, useState } from "react";
import { useAuth } from "@/lib/auth";
import { api } from "@/lib/api";
import { Nav } from "@/components/nav";
import { Card, Button, Input } from "@/components/ui";

interface Config {
  llm: { provider: string; model: string };
  embedding: { provider: string; model: string; dimensions: number };
  decay: { enabled: boolean; half_life_days: number };
  reflection: { enabled: boolean; interval_hours: number };
  contradiction: { enabled: boolean };
  api: { max_concurrent_requests: number };
  deployment: { id: string; heartbeat_enabled: boolean; central_url: string };
}

export default function Settings() {
  const { account, loading } = useAuth();
  const [config, setConfig] = useState<Config | null>(null);
  const [saving, setSaving] = useState(false);
  const [saved, setSaved] = useState(false);
  const [testing, setTesting] = useState(false);
  const [testResult, setTestResult] = useState<string | null>(null);

  useEffect(() => {
    if (!loading && !account) window.location.href = "/login/";
    if (account?.role !== "admin") return;
    api.get<{ config: Config }>("/v1/config").then((d) => setConfig(d.config)).catch(() => {});
  }, [account, loading]);

  if (loading || !account) return null;
  if (account.role !== "admin") {
    return (
      <>
        <Nav />
        <main className="max-w-3xl mx-auto px-6 py-8">
          <p className="text-zinc-500">Admin access required.</p>
        </main>
      </>
    );
  }

  const save = async () => {
    if (!config) return;
    setSaving(true);
    setSaved(false);
    try {
      await api.put("/v1/config", config);
      setSaved(true);
      setTimeout(() => setSaved(false), 3000);
    } finally {
      setSaving(false);
    }
  };

  const testConnection = async () => {
    setTesting(true);
    setTestResult(null);
    try {
      const res = await api.get<{ engine: string }>("/v1/system/health");
      setTestResult(res.engine === "healthy" ? "Connected" : "Degraded");
    } catch {
      setTestResult("Failed");
    } finally {
      setTesting(false);
    }
  };

  const update = (path: string, value: unknown) => {
    if (!config) return;
    const parts = path.split(".");
    const next = JSON.parse(JSON.stringify(config));
    let obj = next as Record<string, Record<string, unknown>>;
    for (let i = 0; i < parts.length - 1; i++) {
      obj = obj[parts[i]] as Record<string, Record<string, unknown>>;
    }
    (obj as Record<string, unknown>)[parts[parts.length - 1]] = value;
    setConfig(next);
  };

  if (!config) return <><Nav /><main className="max-w-3xl mx-auto px-6 py-8 text-zinc-500">Loading...</main></>;

  return (
    <>
      <Nav />
      <main className="max-w-3xl mx-auto px-6 py-8 space-y-6">
        <h1 className="text-xl font-semibold">Settings</h1>

        <Card>
          <h2 className="text-sm font-medium text-zinc-300 mb-4">LLM Configuration</h2>
          <div className="grid grid-cols-2 gap-4">
            <div>
              <label className="text-xs text-zinc-500 block mb-1">Provider</label>
              <Input value={config.llm.provider} onChange={(e) => update("llm.provider", e.target.value)} />
            </div>
            <div>
              <label className="text-xs text-zinc-500 block mb-1">Model</label>
              <Input value={config.llm.model} onChange={(e) => update("llm.model", e.target.value)} />
            </div>
          </div>
          <div className="mt-3 flex items-center gap-3">
            <Button variant="secondary" onClick={testConnection} disabled={testing}>
              {testing ? "Testing..." : "Test Connection"}
            </Button>
            {testResult && (
              <span className={`text-sm ${testResult === "Connected" ? "text-green-400" : "text-red-400"}`}>
                {testResult}
              </span>
            )}
          </div>
        </Card>

        <Card>
          <h2 className="text-sm font-medium text-zinc-300 mb-4">Embeddings</h2>
          <div className="grid grid-cols-3 gap-4">
            <div>
              <label className="text-xs text-zinc-500 block mb-1">Provider</label>
              <Input value={config.embedding.provider} disabled />
            </div>
            <div>
              <label className="text-xs text-zinc-500 block mb-1">Model</label>
              <Input value={config.embedding.model} disabled />
            </div>
            <div>
              <label className="text-xs text-zinc-500 block mb-1">Dimensions</label>
              <Input value={config.embedding.dimensions} disabled />
            </div>
          </div>
        </Card>

        <Card>
          <h2 className="text-sm font-medium text-zinc-300 mb-4">Engine</h2>
          <div className="space-y-4">
            <div>
              <label className="text-xs text-zinc-500 block mb-1">
                Max concurrent requests
                <span className="text-zinc-600 ml-2">(Free tier: 2, Tier 2: 5, Tier 3+: 10-20, Enterprise: 20-50)</span>
              </label>
              <Input
                type="number"
                value={config.api.max_concurrent_requests}
                onChange={(e) => update("api.max_concurrent_requests", parseInt(e.target.value) || 10)}
                min={1}
                max={50}
              />
            </div>
            <div className="flex items-center gap-3">
              <input
                type="checkbox"
                checked={config.decay.enabled}
                onChange={(e) => update("decay.enabled", e.target.checked)}
                className="accent-amber-500"
              />
              <span className="text-sm text-zinc-300">Decay enabled</span>
              <Input
                type="number"
                value={config.decay.half_life_days}
                onChange={(e) => update("decay.half_life_days", parseInt(e.target.value) || 30)}
                className="w-20 ml-4"
                min={1}
              />
              <span className="text-xs text-zinc-500">days half-life</span>
            </div>
            <div className="flex items-center gap-3">
              <input
                type="checkbox"
                checked={config.contradiction.enabled}
                onChange={(e) => update("contradiction.enabled", e.target.checked)}
                className="accent-amber-500"
              />
              <span className="text-sm text-zinc-300">Contradiction detection</span>
            </div>
          </div>
        </Card>

        <Card>
          <h2 className="text-sm font-medium text-zinc-300 mb-4">Deployment</h2>
          <div className="space-y-3 text-sm">
            <div className="flex items-center justify-between">
              <span className="text-zinc-500">Deployment ID</span>
              <span className="text-zinc-300 font-mono text-xs">{config.deployment.id || "not set"}</span>
            </div>
            <div className="flex items-center justify-between">
              <span className="text-zinc-500">Central URL</span>
              <span className="text-zinc-300 font-mono text-xs">{config.deployment.central_url}</span>
            </div>
            <div className="flex items-center gap-3">
              <input
                type="checkbox"
                checked={config.deployment.heartbeat_enabled}
                onChange={(e) => update("deployment.heartbeat_enabled", e.target.checked)}
                className="accent-amber-500"
              />
              <span className="text-zinc-300">Heartbeat enabled</span>
            </div>
          </div>
        </Card>

        <div className="flex items-center gap-3">
          <Button onClick={save} disabled={saving}>{saving ? "Saving..." : "Save Changes"}</Button>
          {saved && <span className="text-green-400 text-sm">Saved</span>}
        </div>
      </main>
    </>
  );
}
