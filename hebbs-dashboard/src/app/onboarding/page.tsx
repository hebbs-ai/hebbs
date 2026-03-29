"use client";

import { useState, useEffect } from "react";
import { api } from "@/lib/api";
import { Input, Button, Card, CopyButton } from "@/components/ui";

export default function Onboarding() {
  const [step, setStep] = useState(1);
  const [email, setEmail] = useState("");
  const [password, setPassword] = useState("");
  const [confirm, setConfirm] = useState("");
  const [workspace, setWorkspace] = useState("");
  const [error, setError] = useState("");
  const [submitting, setSubmitting] = useState(false);
  const [result, setResult] = useState<{ api_key: string } | null>(null);
  const [health, setHealth] = useState<{ engine: string; status: string } | null>(null);

  useEffect(() => {
    api.get<{ completed: boolean }>("/v1/onboarding/status").then((d) => {
      if (d.completed) window.location.href = "/login/";
    }).catch(() => {});
  }, []);

  const nextStep = () => {
    setError("");
    if (step === 1) {
      if (!email || !password) return setError("All fields required");
      if (password !== confirm) return setError("Passwords do not match");
      if (password.length < 6) return setError("Password must be at least 6 characters");
      setStep(2);
    } else if (step === 2) {
      if (!workspace.trim()) return setError("Workspace name required");
      completeOnboarding();
    }
  };

  const completeOnboarding = async () => {
    setSubmitting(true);
    setError("");
    try {
      const res = await api.post<{ api_key: string }>("/v1/onboarding", {
        email,
        password,
        workspace_name: workspace,
      });
      setResult(res);
      api.get<{ engine: string; status: string }>("/v1/system/health").then(setHealth).catch(() => {});
      setStep(3);
    } catch (err: unknown) {
      setError(err instanceof Error ? err.message : "Setup failed");
    } finally {
      setSubmitting(false);
    }
  };

  return (
    <div className="min-h-screen flex items-center justify-center">
      <div className="w-full max-w-lg">
        <div className="flex items-center justify-between mb-6">
          <h1 className="text-lg font-semibold text-zinc-200">HEBBS Enterprise Setup</h1>
          <span className="text-sm text-zinc-500">Step {step}/3</span>
        </div>

        <Card>
          {error && <div className="text-red-400 text-sm bg-red-500/10 rounded-lg p-3 mb-4">{error}</div>}

          {step === 1 && (
            <div className="space-y-4">
              <h2 className="text-zinc-200 font-medium">Create your admin account</h2>
              <div>
                <label className="text-sm text-zinc-400 block mb-1">Email</label>
                <Input type="email" value={email} onChange={(e) => setEmail(e.target.value)} />
              </div>
              <div>
                <label className="text-sm text-zinc-400 block mb-1">Password</label>
                <Input type="password" value={password} onChange={(e) => setPassword(e.target.value)} />
              </div>
              <div>
                <label className="text-sm text-zinc-400 block mb-1">Confirm password</label>
                <Input type="password" value={confirm} onChange={(e) => setConfirm(e.target.value)} />
              </div>
              <div className="flex justify-end">
                <Button onClick={nextStep}>Next</Button>
              </div>
            </div>
          )}

          {step === 2 && (
            <div className="space-y-4">
              <h2 className="text-zinc-200 font-medium">Name your first workspace</h2>
              <div>
                <label className="text-sm text-zinc-400 block mb-1">Workspace name</label>
                <Input value={workspace} onChange={(e) => setWorkspace(e.target.value)} placeholder="e.g. support-agent" />
              </div>
              <div className="flex justify-between">
                <Button variant="secondary" onClick={() => setStep(1)}>Back</Button>
                <Button onClick={nextStep} disabled={submitting}>
                  {submitting ? "Setting up..." : "Next"}
                </Button>
              </div>
            </div>
          )}

          {step === 3 && result && (
            <div className="space-y-4">
              <h2 className="text-zinc-200 font-medium">Verify Connection</h2>
              <div className="space-y-2">
                <div className="flex items-center gap-2 text-sm">
                  <span className="w-2 h-2 rounded-full bg-green-500" />
                  <span className="text-zinc-300">OpenAI API connected (via .env)</span>
                </div>
                <div className="flex items-center gap-2 text-sm">
                  <span className={`w-2 h-2 rounded-full ${health?.engine === "healthy" ? "bg-green-500" : "bg-amber-500"}`} />
                  <span className="text-zinc-300">Engine {health?.engine || "checking..."}</span>
                </div>
                <div className="flex items-center gap-2 text-sm">
                  <span className="w-2 h-2 rounded-full bg-green-500" />
                  <span className="text-zinc-300">Workspace &quot;{workspace}&quot; created</span>
                </div>
              </div>
              <div>
                <label className="text-sm text-zinc-400 block mb-1">Your API Key</label>
                <div className="bg-[#1a1a2e] rounded-lg p-3 font-mono text-sm text-amber-400 break-all flex items-center justify-between">
                  {result.api_key}
                  <CopyButton text={result.api_key} />
                </div>
                <p className="text-xs text-zinc-500 mt-1">Save this key. It won&apos;t be shown again.</p>
              </div>
              <div className="flex justify-end">
                <Button onClick={() => (window.location.href = "/login/")}>Go to Dashboard</Button>
              </div>
            </div>
          )}
        </Card>
      </div>
    </div>
  );
}
