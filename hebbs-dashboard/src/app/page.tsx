"use client";

import { useEffect, useState } from "react";
import Link from "next/link";
import { useAuth } from "@/lib/auth";
import { api } from "@/lib/api";
import { Nav } from "@/components/nav";
import { Card, Badge, Button, StatBox } from "@/components/ui";
import { Modal } from "@/components/modal";
import { Input } from "@/components/ui";

interface Workspace {
  id: number;
  slug: string;
  name: string;
  stats: { memories: number; files: number };
  createdAt: string;
}

interface Health {
  status: string;
  version: string;
  engine: string;
}

export default function Dashboard() {
  const { account, loading } = useAuth();
  const [workspaces, setWorkspaces] = useState<Workspace[]>([]);
  const [health, setHealth] = useState<Health | null>(null);
  const [showCreate, setShowCreate] = useState(false);
  const [newName, setNewName] = useState("");
  const [creating, setCreating] = useState(false);
  const [createdKey, setCreatedKey] = useState<string | null>(null);

  useEffect(() => {
    if (loading) return;
    if (!account) {
      window.location.href = "/login/";
      return;
    }
    api.get<{ workspaces: Workspace[] }>("/v1/workspaces").then((d) => setWorkspaces(d.workspaces)).catch(() => {});
    api.get<Health>("/v1/system/health").then(setHealth).catch(() => {});
  }, [account, loading]);

  if (loading) return <div className="flex items-center justify-center h-screen text-zinc-500">Loading...</div>;
  if (!account) return null;

  const createWorkspace = async () => {
    if (!newName.trim()) return;
    setCreating(true);
    try {
      const res = await api.post<{ workspace: Workspace; api_key: string }>("/v1/workspaces", { name: newName });
      setWorkspaces((prev) => [...prev, { ...res.workspace, stats: { memories: 0, files: 0 } }]);
      setCreatedKey(res.api_key);
      setNewName("");
    } finally {
      setCreating(false);
    }
  };

  return (
    <>
      <Nav />
      <main className="max-w-5xl mx-auto px-6 py-8">
        <div className="flex items-center justify-between mb-6">
          <h1 className="text-xl font-semibold">Workspaces</h1>
          {account.role === "admin" && (
            <Button onClick={() => setShowCreate(true)}>+ New Workspace</Button>
          )}
        </div>

        <div className="space-y-4 mb-10">
          {workspaces.map((ws) => (
            <Card key={ws.id} className="flex items-center justify-between">
              <div>
                <div className="flex items-center gap-3">
                  <span className="font-medium text-zinc-200">{ws.name}</span>
                  <Badge color="green">Ready</Badge>
                </div>
                <div className="text-sm text-zinc-500 mt-1">
                  Memories: {ws.stats.memories} &middot; Files: {ws.stats.files}
                </div>
              </div>
              <div className="flex gap-2">
                <Link href={`/workspaces/${ws.slug}/`}>
                  <Button variant="secondary">Open</Button>
                </Link>
                <Link href={`/workspaces/${ws.slug}/palace/`}>
                  <Button variant="secondary">Memory Palace</Button>
                </Link>
                <Link href={`/workspaces/${ws.slug}/keys/`}>
                  <Button variant="secondary">Keys</Button>
                </Link>
              </div>
            </Card>
          ))}
          {workspaces.length === 0 && (
            <Card className="text-center text-zinc-500 py-12">
              No workspaces yet. Create one to get started.
            </Card>
          )}
        </div>

        {health && (
          <>
            <h2 className="text-lg font-semibold mb-4">System</h2>
            <div className="grid grid-cols-3 gap-4">
              <Card>
                <div className="text-sm text-zinc-500">Engine</div>
                <div className="flex items-center gap-2 mt-1">
                  <span className={`w-2 h-2 rounded-full ${health.engine === "healthy" ? "bg-green-500" : "bg-red-500"}`} />
                  <span className="text-zinc-200 capitalize">{health.engine}</span>
                </div>
                <div className="text-xs text-zinc-600 mt-1">v{health.version}</div>
              </Card>
              <Card>
                <div className="text-sm text-zinc-500">OpenAI</div>
                <div className="flex items-center gap-2 mt-1">
                  <span className="w-2 h-2 rounded-full bg-green-500" />
                  <span className="text-zinc-200">Connected</span>
                </div>
                <div className="text-xs text-zinc-600 mt-1">gpt-4o-mini</div>
              </Card>
              <Card>
                <div className="text-sm text-zinc-500">Status</div>
                <div className="flex items-center gap-2 mt-1">
                  <span className={`w-2 h-2 rounded-full ${health.status === "ok" ? "bg-green-500" : "bg-amber-500"}`} />
                  <span className="text-zinc-200 capitalize">{health.status}</span>
                </div>
              </Card>
            </div>
          </>
        )}
      </main>

      {/* Create workspace modal */}
      <Modal open={showCreate && !createdKey} onClose={() => setShowCreate(false)} title="Create Workspace">
        <div className="space-y-4">
          <div>
            <label className="text-sm text-zinc-400 block mb-1">Name</label>
            <Input value={newName} onChange={(e) => setNewName(e.target.value)} placeholder="e.g. support-agent" />
          </div>
          <p className="text-xs text-zinc-500">A new API key will be generated for this workspace.</p>
          <div className="flex justify-end gap-3">
            <Button variant="secondary" onClick={() => setShowCreate(false)}>Cancel</Button>
            <Button onClick={createWorkspace} disabled={creating || !newName.trim()}>
              {creating ? "Creating..." : "Create"}
            </Button>
          </div>
        </div>
      </Modal>

      {/* Created success modal */}
      <Modal open={!!createdKey} onClose={() => { setCreatedKey(null); setShowCreate(false); }} title="Workspace Created">
        <div className="space-y-4">
          <p className="text-sm text-zinc-400">Your workspace API key (shown once, save it now):</p>
          <div className="bg-[#1a1a2e] rounded-lg p-3 font-mono text-sm text-amber-400 break-all">
            {createdKey}
          </div>
          <div className="flex justify-end">
            <Button onClick={() => { navigator.clipboard.writeText(createdKey || ""); }}>Copy Key</Button>
          </div>
        </div>
      </Modal>
    </>
  );
}
