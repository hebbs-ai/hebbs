"use client";

import { useEffect, useState } from "react";
import { useAuth } from "@/lib/auth";
import { api } from "@/lib/api";
import { Nav } from "@/components/nav";
import { Card, Button, Input, Badge } from "@/components/ui";
import { Modal, ConfirmModal } from "@/components/modal";

interface Workspace {
  workspaceId: number;
  slug: string;
  name: string;
}

interface Account {
  id: number;
  email: string;
  role: string;
  createdAt: string;
  workspaces: Workspace[];
}

export default function Team() {
  const { account, loading } = useAuth();
  const [accounts, setAccounts] = useState<Account[]>([]);
  const [allWorkspaces, setAllWorkspaces] = useState<Array<{ id: number; slug: string; name: string }>>([]);
  const [showAdd, setShowAdd] = useState(false);
  const [editId, setEditId] = useState<number | null>(null);
  const [removeId, setRemoveId] = useState<number | null>(null);

  // Add form
  const [addEmail, setAddEmail] = useState("");
  const [addPassword, setAddPassword] = useState("");
  const [addRole, setAddRole] = useState("developer");
  const [addWsIds, setAddWsIds] = useState<number[]>([]);

  // Edit form
  const [editRole, setEditRole] = useState("");
  const [editWsIds, setEditWsIds] = useState<number[]>([]);

  const load = () => {
    api.get<{ accounts: Account[] }>("/v1/accounts").then((d) => setAccounts(d.accounts)).catch(() => {});
    api.get<{ workspaces: Array<{ id: number; slug: string; name: string }> }>("/v1/workspaces").then((d) => setAllWorkspaces(d.workspaces)).catch(() => {});
  };

  useEffect(() => {
    if (!loading && !account) window.location.href = "/login/";
    if (account?.role === "admin") load();
  }, [account, loading]);

  if (loading || !account) return null;
  if (account.role !== "admin") {
    return <><Nav /><main className="max-w-3xl mx-auto px-6 py-8"><p className="text-zinc-500">Admin access required.</p></main></>;
  }

  const addMember = async () => {
    await api.post("/v1/accounts", {
      email: addEmail,
      password: addPassword,
      role: addRole,
      workspace_ids: addWsIds,
    });
    setShowAdd(false);
    setAddEmail("");
    setAddPassword("");
    setAddRole("developer");
    setAddWsIds([]);
    load();
  };

  const startEdit = (a: Account) => {
    setEditId(a.id);
    setEditRole(a.role);
    setEditWsIds(a.workspaces.map((w) => w.workspaceId));
  };

  const saveEdit = async () => {
    if (editId === null) return;
    await api.put(`/v1/accounts/${editId}`, { role: editRole, workspace_ids: editWsIds });
    setEditId(null);
    load();
  };

  const removeMember = async () => {
    if (removeId === null) return;
    await api.del(`/v1/accounts/${removeId}`);
    load();
  };

  const toggleWs = (ids: number[], setIds: (v: number[]) => void, wsId: number) => {
    setIds(ids.includes(wsId) ? ids.filter((id) => id !== wsId) : [...ids, wsId]);
  };

  return (
    <>
      <Nav />
      <main className="max-w-3xl mx-auto px-6 py-8 space-y-6">
        <div className="flex items-center justify-between">
          <h1 className="text-xl font-semibold">Team</h1>
          <Button onClick={() => setShowAdd(true)}>+ Add Member</Button>
        </div>

        <Card>
          <table className="w-full text-sm">
            <thead>
              <tr className="text-zinc-500 text-left">
                <th className="pb-3">User</th>
                <th className="pb-3">Role</th>
                <th className="pb-3">Workspaces</th>
                <th className="pb-3"></th>
              </tr>
            </thead>
            <tbody>
              {accounts.map((a) => (
                <tr key={a.id} className="border-t border-[#1e1e2e]">
                  <td className="py-3 text-zinc-300">{a.email}</td>
                  <td className="py-3">
                    <Badge color={a.role === "admin" ? "amber" : "zinc"}>{a.role}</Badge>
                  </td>
                  <td className="py-3 text-zinc-400 text-xs">
                    {a.role === "admin" ? "All" : a.workspaces.map((w) => w.slug).join(", ") || "None"}
                  </td>
                  <td className="py-3 text-right">
                    {a.id !== account.id && (
                      <div className="flex gap-2 justify-end">
                        <button onClick={() => startEdit(a)} className="text-amber-500 hover:text-amber-400 text-xs">Edit</button>
                        <button onClick={() => setRemoveId(a.id)} className="text-red-400 hover:text-red-300 text-xs">Remove</button>
                      </div>
                    )}
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        </Card>
      </main>

      {/* Add member modal */}
      <Modal open={showAdd} onClose={() => setShowAdd(false)} title="Add Team Member">
        <div className="space-y-4">
          <div>
            <label className="text-xs text-zinc-500 block mb-1">Email</label>
            <Input value={addEmail} onChange={(e) => setAddEmail(e.target.value)} />
          </div>
          <div>
            <label className="text-xs text-zinc-500 block mb-1">Password</label>
            <Input type="password" value={addPassword} onChange={(e) => setAddPassword(e.target.value)} />
          </div>
          <div>
            <label className="text-xs text-zinc-500 block mb-1">Role</label>
            <select
              value={addRole}
              onChange={(e) => setAddRole(e.target.value)}
              className="w-full bg-[#1a1a2e] border border-[#1e1e2e] rounded-lg px-3 py-2 text-sm text-zinc-200"
            >
              <option value="developer">Developer</option>
              <option value="admin">Admin</option>
            </select>
          </div>
          <div>
            <label className="text-xs text-zinc-500 block mb-1">Workspaces</label>
            <div className="space-y-2">
              {allWorkspaces.map((ws) => (
                <label key={ws.id} className="flex items-center gap-2 text-sm text-zinc-300">
                  <input
                    type="checkbox"
                    checked={addWsIds.includes(ws.id)}
                    onChange={() => toggleWs(addWsIds, setAddWsIds, ws.id)}
                    className="accent-amber-500"
                  />
                  {ws.name}
                </label>
              ))}
            </div>
          </div>
          <div className="flex justify-end gap-3">
            <Button variant="secondary" onClick={() => setShowAdd(false)}>Cancel</Button>
            <Button onClick={addMember}>Add</Button>
          </div>
        </div>
      </Modal>

      {/* Edit member modal */}
      <Modal open={editId !== null} onClose={() => setEditId(null)} title="Edit Team Member">
        <div className="space-y-4">
          <div>
            <label className="text-xs text-zinc-500 block mb-1">Role</label>
            <select
              value={editRole}
              onChange={(e) => setEditRole(e.target.value)}
              className="w-full bg-[#1a1a2e] border border-[#1e1e2e] rounded-lg px-3 py-2 text-sm text-zinc-200"
            >
              <option value="developer">Developer</option>
              <option value="admin">Admin</option>
            </select>
          </div>
          <div>
            <label className="text-xs text-zinc-500 block mb-1">Workspaces</label>
            <div className="space-y-2">
              {allWorkspaces.map((ws) => (
                <label key={ws.id} className="flex items-center gap-2 text-sm text-zinc-300">
                  <input
                    type="checkbox"
                    checked={editWsIds.includes(ws.id)}
                    onChange={() => toggleWs(editWsIds, setEditWsIds, ws.id)}
                    className="accent-amber-500"
                  />
                  {ws.name}
                </label>
              ))}
            </div>
          </div>
          <div className="flex justify-end gap-3">
            <Button variant="secondary" onClick={() => setEditId(null)}>Cancel</Button>
            <Button onClick={saveEdit}>Save</Button>
          </div>
        </div>
      </Modal>

      {/* Remove confirmation */}
      <ConfirmModal
        open={removeId !== null}
        onClose={() => setRemoveId(null)}
        onConfirm={removeMember}
        title="Remove Team Member"
        message="This will delete the account and revoke all their sessions. They will lose access immediately."
        confirmLabel="Remove"
        danger
      />
    </>
  );
}
