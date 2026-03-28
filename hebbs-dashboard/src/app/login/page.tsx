"use client";

import { useState, useEffect } from "react";
import { useAuth } from "@/lib/auth";
import { api } from "@/lib/api";
import { Input, Button } from "@/components/ui";

export default function Login() {
  const { account, loading, login } = useAuth();
  const [email, setEmail] = useState("");
  const [password, setPassword] = useState("");
  const [error, setError] = useState("");
  const [submitting, setSubmitting] = useState(false);

  useEffect(() => {
    if (loading) return;
    if (account) {
      window.location.href = "/";
      return;
    }
    api.get<{ completed: boolean }>("/v1/onboarding/status").then((d) => {
      if (!d.completed) window.location.href = "/onboarding/";
    }).catch(() => {});
  }, [account, loading]);

  const handleSubmit = async (e: React.FormEvent) => {
    e.preventDefault();
    setError("");
    setSubmitting(true);
    try {
      await login(email, password);
      window.location.href = "/";
    } catch (err: unknown) {
      setError(err instanceof Error ? err.message : "Login failed");
    } finally {
      setSubmitting(false);
    }
  };

  if (loading) return null;

  return (
    <div className="min-h-screen flex items-center justify-center">
      <div className="w-full max-w-sm">
        <div className="text-center mb-8">
          <div className="flex justify-center gap-1 mb-2">
            <span className="w-4 h-4 rounded-full bg-amber-500" />
            <span className="w-4 h-4 rounded-full bg-amber-500 -ml-2" />
          </div>
          <h1 className="text-xl font-bold text-zinc-200">HEBBS</h1>
          <p className="text-sm text-zinc-500">Enterprise</p>
        </div>

        <form onSubmit={handleSubmit} className="bg-[#14141f] border border-[#1e1e2e] rounded-xl p-6 space-y-4">
          {error && <div className="text-red-400 text-sm bg-red-500/10 rounded-lg p-3">{error}</div>}
          <div>
            <label className="text-sm text-zinc-400 block mb-1">Email</label>
            <Input type="email" value={email} onChange={(e) => setEmail(e.target.value)} required />
          </div>
          <div>
            <label className="text-sm text-zinc-400 block mb-1">Password</label>
            <Input type="password" value={password} onChange={(e) => setPassword(e.target.value)} required />
          </div>
          <Button className="w-full" disabled={submitting}>
            {submitting ? "Signing in..." : "Sign In"}
          </Button>
        </form>
        <p className="text-center text-xs text-zinc-600 mt-4">Forgot password? Contact your admin.</p>
      </div>
    </div>
  );
}
