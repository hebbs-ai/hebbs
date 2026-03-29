"use client";

import { useEffect, useState } from "react";
import { usePathname } from "next/navigation";
import Link from "next/link";
import { useAuth } from "@/lib/auth";
import { api } from "@/lib/api";
import { Nav } from "@/components/nav";
import { Card, Button, Badge, StatBox, Input, CopyButton } from "@/components/ui";
import { Modal, ConfirmModal } from "@/components/modal";

// Parse /workspaces/SLUG/TAB from pathname
function parseRoute(pathname: string) {
  const parts = pathname.replace(/^\/workspaces\/?/, "").replace(/\/$/, "").split("/");
  return { slug: parts[0] || "", tab: parts[1] || "overview" };
}

export default function WorkspacePage() {
  const pathname = usePathname();
  const { slug, tab } = parseRoute(pathname);
  const { account, loading } = useAuth();

  useEffect(() => {
    if (!loading && !account) window.location.href = "/login/";
  }, [account, loading]);

  if (loading || !account || !slug) return null;

  const tabs = [
    { id: "overview", label: "Overview" },
    { id: "files", label: "Files" },
    { id: "search", label: "Search" },
    { id: "entities", label: "Entities" },
    { id: "keys", label: "Keys" },
    { id: "palace", label: "Memory Palace" },
  ];

  return (
    <>
      <Nav />
      <main className="max-w-5xl mx-auto px-6 py-8">
        <div className="flex items-center gap-2 mb-1">
          <Link href="/" className="text-zinc-500 hover:text-zinc-300 text-sm">&larr; Workspaces</Link>
        </div>
        <h1 className="text-xl font-semibold mb-4">{slug}</h1>
        <div className="flex gap-1 border-b border-[#1e1e2e] mb-6">
          {tabs.map((t) => (
            <Link
              key={t.id}
              href={`/workspaces/${slug}/${t.id === "overview" ? "" : t.id + "/"}`}
              className={`px-4 py-2 text-sm border-b-2 ${
                tab === t.id
                  ? "border-amber-500 text-amber-500"
                  : "border-transparent text-zinc-500 hover:text-zinc-300"
              }`}
            >
              {t.label}
            </Link>
          ))}
        </div>

        {tab === "overview" && <OverviewTab slug={slug} />}
        {tab === "files" && <FilesTab slug={slug} />}
        {tab === "search" && <SearchTab slug={slug} />}
        {tab === "entities" && <EntitiesTab slug={slug} />}
        {tab === "keys" && <KeysTab slug={slug} />}
        {tab === "palace" && <PalaceTab slug={slug} />}
      </main>
    </>
  );
}

// ── Overview Tab ──

function OverviewTab({ slug }: { slug: string }) {
  const { account } = useAuth();
  const [stats, setStats] = useState<Record<string, number | string>>({});
  const [showDelete, setShowDelete] = useState(false);

  useEffect(() => {
    api.get<{ stats: Record<string, number | string> }>(`/v1/workspaces/${slug}/stats`).then((d) => setStats(d.stats)).catch(() => {});
  }, [slug]);

  const deleteWorkspace = async () => {
    await api.del(`/v1/workspaces/${slug}`);
    window.location.href = "/";
  };

  return (
    <div className="space-y-6">
      <div className="grid grid-cols-4 gap-4">
        <StatBox label="Memories" value={stats.memories ?? 0} />
        <StatBox label="Files" value={stats.files ?? 0} />
        <StatBox label="Entities" value={stats.entities ?? 0} />
        <StatBox label="Edges" value={stats.edges ?? 0} />
      </div>

      <Card>
        <div className="space-y-2 text-sm">
          <div className="flex items-center gap-2">
            <span className="w-2 h-2 rounded-full bg-green-500" />
            <span className="text-zinc-300">Indexing: {String(stats.indexing_status || "idle")}</span>
          </div>
        </div>
      </Card>

      <Link href={`/workspaces/${slug}/palace/`}>
        <Button>Open Memory Palace &rarr;</Button>
      </Link>

      {account?.role === "admin" && (
        <div className="pt-8 border-t border-[#1e1e2e]">
          <Button variant="danger" onClick={() => setShowDelete(true)}>Delete Workspace</Button>
        </div>
      )}

      <ConfirmModal
        open={showDelete}
        onClose={() => setShowDelete(false)}
        onConfirm={deleteWorkspace}
        title="Delete Workspace"
        message={`This will permanently delete "${slug}" and all its data. This cannot be undone.`}
        confirmLabel="Delete"
        danger
      />
    </div>
  );
}

// ── Files Tab ──

function FilesTab({ slug }: { slug: string }) {
  const [files, setFiles] = useState<Array<{ path: string; sections?: number; status?: string }>>([]);
  const [uploading, setUploading] = useState(false);

  useEffect(() => {
    api.get<{ files: Array<{ path: string }> }>(`/v1/workspaces/${slug}/files`).then((d) => setFiles(d.files || [])).catch(() => {});
  }, [slug]);

  const handleUpload = async (e: React.ChangeEvent<HTMLInputElement>) => {
    const fileList = e.target.files;
    if (!fileList?.length) return;
    setUploading(true);
    const fd = new FormData();
    for (const f of Array.from(fileList)) fd.append("files", f);
    try {
      await api.upload(`/v1/upload`, fd);
      api.get<{ files: Array<{ path: string }> }>(`/v1/workspaces/${slug}/files`).then((d) => setFiles(d.files || [])).catch(() => {});
    } finally {
      setUploading(false);
    }
  };

  return (
    <div className="space-y-6">
      <div className="flex items-center justify-between">
        <h2 className="text-sm text-zinc-400">Files ({files.length})</h2>
        <label className="cursor-pointer">
          <Button disabled={uploading}>{uploading ? "Uploading..." : "Upload Files"}</Button>
          <input type="file" multiple accept=".md,.txt,.pdf" className="hidden" onChange={handleUpload} />
        </label>
      </div>

      <Card>
        {files.length > 0 ? (
          <table className="w-full text-sm">
            <thead>
              <tr className="text-zinc-500 text-left">
                <th className="pb-3">File</th>
                <th className="pb-3">Status</th>
              </tr>
            </thead>
            <tbody className="text-zinc-300">
              {files.map((f, i) => (
                <tr key={i} className="border-t border-[#1e1e2e]">
                  <td className="py-2 font-mono text-xs">{f.path}</td>
                  <td className="py-2"><Badge color="green">Indexed</Badge></td>
                </tr>
              ))}
            </tbody>
          </table>
        ) : (
          <p className="text-zinc-500 text-sm text-center py-8">No files indexed. Upload files to get started.</p>
        )}
      </Card>

      <Card className="border-dashed text-center text-zinc-500 text-sm py-8">
        Drop files here or click Upload Files. Supports: .md, .txt, .pdf
      </Card>
    </div>
  );
}

// ── Search Tab ──

function SearchTab({ slug }: { slug: string }) {
  const [query, setQuery] = useState("");
  const [results, setResults] = useState<Array<Record<string, unknown>>>([]);
  const [latency, setLatency] = useState<number | null>(null);
  const [searching, setSearching] = useState(false);
  const [indexingMsg, setIndexingMsg] = useState<string | null>(null);
  const [topK, setTopK] = useState(10);

  const search = async () => {
    if (!query.trim()) return;
    setSearching(true);
    setIndexingMsg(null);
    const start = Date.now();
    try {
      const res = await api.post<{ results: Array<Record<string, unknown>>; indexing_pct?: number }>(`/v1/workspaces/${slug}/recall`, {
        cue: query,
        top_k: topK,
      });
      setLatency(Date.now() - start);
      setResults(res.results || []);
      if (res.indexing_pct !== undefined && res.indexing_pct < 100) {
        setIndexingMsg(`Workspace is still indexing (${res.indexing_pct}% complete). Results may be partial.`);
      }
    } catch (err: unknown) {
      const msg = err instanceof Error ? err.message : "";
      if (msg.includes("not initialized") || msg.includes("index")) {
        setIndexingMsg("This workspace is still being indexed. Try again in a moment.");
        setResults([]);
      }
    } finally {
      setSearching(false);
    }
  };

  return (
    <div className="space-y-6">
      <form onSubmit={(e) => { e.preventDefault(); search(); }} className="flex gap-2">
        <Input
          value={query}
          onChange={(e) => setQuery(e.target.value)}
          placeholder="Search memories..."
          className="flex-1"
        />
        <Input
          type="number"
          value={topK}
          onChange={(e) => setTopK(parseInt(e.target.value) || 10)}
          className="w-20"
          min={1}
          max={100}
        />
        <Button disabled={searching}>{searching ? "..." : "Go"}</Button>
      </form>

      {indexingMsg && (
        <div className="bg-amber-500/10 text-amber-400 rounded-lg p-3 text-sm">{indexingMsg}</div>
      )}

      {latency !== null && (
        <p className="text-xs text-zinc-500">{results.length} results ({latency}ms)</p>
      )}

      <div className="space-y-3">
        {results.map((r, i) => (
          <Card key={i}>
            <div className="flex items-start justify-between">
              <div className="flex-1">
                <p className="text-sm text-zinc-300">{String(r.content)}</p>
                <div className="flex gap-4 mt-2 text-xs text-zinc-500">
                  {r.score !== undefined && <span>score: {Number(r.score).toFixed(3)}</span>}
                  {r.importance !== undefined && <span>importance: {Number(r.importance).toFixed(2)}</span>}
                  {r.file_path ? <span>source: {String(r.file_path)}</span> : null}
                </div>
              </div>
              <span className="text-xs text-zinc-600 ml-4">{i + 1}</span>
            </div>
          </Card>
        ))}
      </div>
    </div>
  );
}

// ── Entities Tab ──

function EntitiesTab({ slug }: { slug: string }) {
  const [entities, setEntities] = useState<Array<{ entity_id: string; memory_count: number }>>([]);
  const [selected, setSelected] = useState<string | null>(null);
  const [entityMemories, setEntityMemories] = useState<Array<Record<string, unknown>>>([]);

  useEffect(() => {
    api.get<{ entities: Array<{ entity_id: string; memory_count: number }> }>(`/v1/workspaces/${slug}/entities`).then((d) => setEntities(d.entities || [])).catch(() => {});
  }, [slug]);

  const viewEntity = async (entityId: string) => {
    setSelected(entityId);
    const res = await api.post<{ results: Array<Record<string, unknown>> }>(`/v1/workspaces/${slug}/recall`, {
      cue: entityId,
      entity_id: entityId,
      top_k: 50,
    });
    setEntityMemories(res.results || []);
  };

  return (
    <div className="space-y-6">
      <h2 className="text-sm text-zinc-400">Entities ({entities.length})</h2>

      {entities.length > 0 ? (
        <Card>
          <table className="w-full text-sm">
            <thead>
              <tr className="text-zinc-500 text-left">
                <th className="pb-3">Entity</th>
                <th className="pb-3">Memories</th>
                <th className="pb-3"></th>
              </tr>
            </thead>
            <tbody className="text-zinc-300">
              {entities.map((e) => (
                <tr key={e.entity_id} className="border-t border-[#1e1e2e]">
                  <td className="py-2 font-mono text-xs">{e.entity_id}</td>
                  <td className="py-2">{e.memory_count}</td>
                  <td className="py-2">
                    <button onClick={() => viewEntity(e.entity_id)} className="text-amber-500 hover:text-amber-400 text-xs">
                      View
                    </button>
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        </Card>
      ) : (
        <Card className="text-center text-zinc-500 text-sm py-8">
          No entities found. Entities are created when memories include an entity_id.
        </Card>
      )}

      {/* Entity detail modal */}
      <Modal open={!!selected} onClose={() => setSelected(null)} title={`Entity: ${selected}`}>
        <div className="space-y-3 max-h-96 overflow-y-auto">
          {entityMemories.length === 0 && <p className="text-zinc-500 text-sm">No memories found.</p>}
          {entityMemories.map((m, i) => (
            <div key={i} className="bg-[#1a1a2e] rounded-lg p-3 text-sm text-zinc-300">
              {String(m.content)}
              <div className="text-xs text-zinc-500 mt-1">
                importance: {Number(m.importance || 0).toFixed(2)}
              </div>
            </div>
          ))}
        </div>
      </Modal>
    </div>
  );
}

// ── Keys Tab ──

function KeysTab({ slug }: { slug: string }) {
  const [keys, setKeys] = useState<Array<Record<string, unknown>>>([]);
  const [showCreate, setShowCreate] = useState(false);
  const [newLabel, setNewLabel] = useState("");
  const [createdKey, setCreatedKey] = useState<string | null>(null);
  const [revokeId, setRevokeId] = useState<number | null>(null);

  useEffect(() => {
    api.get<{ keys: Array<Record<string, unknown>> }>("/v1/keys").then((d) => setKeys(d.keys)).catch(() => {});
  }, [slug]);

  const createKey = async () => {
    const res = await api.post<{ api_key: string }>("/v1/keys", { label: newLabel || `${slug}-key`, role: "workspace" });
    setCreatedKey(res.api_key);
    setNewLabel("");
    api.get<{ keys: Array<Record<string, unknown>> }>("/v1/keys").then((d) => setKeys(d.keys)).catch(() => {});
  };

  const revokeKey = async () => {
    if (revokeId === null) return;
    await api.del(`/v1/keys/${revokeId}`);
    setKeys((prev) => prev.filter((k) => k.id !== revokeId));
  };

  const endpoint = typeof window !== "undefined" ? window.location.origin : "";

  return (
    <div className="space-y-6">
      <div className="flex items-center justify-between">
        <h2 className="text-sm text-zinc-400">API Keys</h2>
        <Button onClick={() => setShowCreate(true)}>+ New Key</Button>
      </div>

      <Card>
        {keys.length > 0 ? (
          <div className="space-y-3">
            {keys.filter((k) => !k.revokedAt).map((k) => (
              <div key={Number(k.id)} className="flex items-center justify-between py-2 border-b border-[#1e1e2e] last:border-0">
                <div>
                  <span className="font-mono text-xs text-zinc-300">{String(k.prefix)}</span>
                  <span className="text-zinc-500 text-xs ml-3">{String(k.label)}</span>
                  <span className="ml-2"><Badge color="zinc">{String(k.role)}</Badge></span>
                </div>
                <Button variant="danger" onClick={() => setRevokeId(Number(k.id))}>Revoke</Button>
              </div>
            ))}
          </div>
        ) : (
          <p className="text-zinc-500 text-sm text-center py-4">No API keys.</p>
        )}
      </Card>

      <Card>
        <div className="text-sm text-zinc-400 mb-2">Endpoint</div>
        <div className="bg-[#1a1a2e] rounded-lg p-3 font-mono text-sm text-zinc-300 flex items-center justify-between">
          {endpoint}
          <CopyButton text={endpoint} />
        </div>
      </Card>

      <Card>
        <div className="text-sm text-zinc-400 mb-2">Quick Start</div>
        <pre className="bg-[#1a1a2e] rounded-lg p-3 text-xs text-zinc-400 overflow-x-auto">
{`curl -X POST ${endpoint}/v1/recall \\
  -H "Authorization: Bearer hb_live_sk_..." \\
  -H "Content-Type: application/json" \\
  -d '{"cue": "your query"}'`}
        </pre>
      </Card>

      {/* Create key modal */}
      <Modal open={showCreate && !createdKey} onClose={() => setShowCreate(false)} title="Create API Key">
        <div className="space-y-4">
          <div>
            <label className="text-sm text-zinc-400 block mb-1">Label</label>
            <Input value={newLabel} onChange={(e) => setNewLabel(e.target.value)} placeholder="e.g. ci-pipeline" />
          </div>
          <div className="flex justify-end gap-3">
            <Button variant="secondary" onClick={() => setShowCreate(false)}>Cancel</Button>
            <Button onClick={createKey}>Create</Button>
          </div>
        </div>
      </Modal>

      {/* Show created key */}
      <Modal open={!!createdKey} onClose={() => { setCreatedKey(null); setShowCreate(false); }} title="API Key Created">
        <div className="space-y-4">
          <p className="text-sm text-zinc-400">Save this key now. It won&apos;t be shown again.</p>
          <div className="bg-[#1a1a2e] rounded-lg p-3 font-mono text-sm text-amber-400 break-all">
            {createdKey}
          </div>
          <div className="flex justify-end">
            <Button onClick={() => navigator.clipboard.writeText(createdKey || "")}>Copy Key</Button>
          </div>
        </div>
      </Modal>

      {/* Revoke confirmation */}
      <ConfirmModal
        open={revokeId !== null}
        onClose={() => setRevokeId(null)}
        onConfirm={revokeKey}
        title="Revoke API Key"
        message="This will immediately invalidate the key. Any integrations using it will stop working."
        confirmLabel="Revoke"
        danger
      />
    </div>
  );
}

// ── Palace Tab ──

function PalaceTab({ slug }: { slug: string }) {
  const [graphData, setGraphData] = useState<{ nodes: Array<Record<string, unknown>>; edges: Array<Record<string, unknown>> } | null>(null);
  const [selected, setSelected] = useState<Record<string, unknown> | null>(null);

  useEffect(() => {
    fetch(`/api/panel/ws/${slug}/graph`)
      .then((r) => r.json())
      .then(setGraphData)
      .catch(() => {});
  }, [slug]);

  return (
    <div className="space-y-6">
      <Card className="min-h-[400px] flex items-center justify-center">
        {graphData ? (
          <div className="text-center">
            <div className="text-6xl mb-4 opacity-30">🧠</div>
            <p className="text-zinc-400 text-sm">{graphData.nodes.length} nodes, {graphData.edges.length} edges</p>
            <p className="text-zinc-500 text-xs mt-2">Interactive graph visualization (coming soon)</p>
            <p className="text-zinc-600 text-xs mt-1">For now, use the engine&apos;s built-in Memory Palace</p>
          </div>
        ) : (
          <p className="text-zinc-500 text-sm">Loading graph data...</p>
        )}
      </Card>

      <a
        href={`/api/panel/graph?vault=/data/workspaces/${slug}`}
        target="_blank"
        rel="noopener noreferrer"
      >
        <Button variant="secondary">Open Raw Graph Data</Button>
      </a>
    </div>
  );
}
