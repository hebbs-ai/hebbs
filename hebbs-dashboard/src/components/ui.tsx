"use client";

import type { InputHTMLAttributes, ButtonHTMLAttributes, ReactNode } from "react";

export function Input(props: InputHTMLAttributes<HTMLInputElement>) {
  return (
    <input
      {...props}
      className={`w-full bg-[#1a1a2e] border border-[#1e1e2e] rounded-lg px-3 py-2 text-sm text-zinc-200 placeholder-zinc-600 focus:outline-none focus:border-amber-500/50 ${props.className || ""}`}
    />
  );
}

export function Select({
  children,
  ...props
}: InputHTMLAttributes<HTMLSelectElement> & { children: ReactNode }) {
  return (
    <select
      {...(props as React.SelectHTMLAttributes<HTMLSelectElement>)}
      className="w-full bg-[#1a1a2e] border border-[#1e1e2e] rounded-lg px-3 py-2 text-sm text-zinc-200 focus:outline-none focus:border-amber-500/50"
    >
      {children}
    </select>
  );
}

export function Button({
  variant = "primary",
  children,
  ...props
}: ButtonHTMLAttributes<HTMLButtonElement> & {
  variant?: "primary" | "secondary" | "danger";
  children: ReactNode;
}) {
  const cls = {
    primary: "bg-amber-600 hover:bg-amber-700 text-white",
    secondary: "bg-[#1a1a2e] hover:bg-[#1e1e2e] text-zinc-300",
    danger: "bg-red-600/20 hover:bg-red-600/30 text-red-400",
  }[variant];

  return (
    <button
      {...props}
      className={`px-4 py-2 text-sm rounded-lg disabled:opacity-50 ${cls} ${props.className || ""}`}
    >
      {children}
    </button>
  );
}

export function Card({ children, className = "" }: { children: ReactNode; className?: string }) {
  return (
    <div className={`bg-[#14141f] border border-[#1e1e2e] rounded-xl p-6 ${className}`}>
      {children}
    </div>
  );
}

export function StatBox({ label, value }: { label: string; value: string | number }) {
  return (
    <div className="bg-[#14141f] border border-[#1e1e2e] rounded-xl p-4 text-center">
      <div className="text-2xl font-bold text-zinc-200">{value}</div>
      <div className="text-xs text-zinc-500 mt-1">{label}</div>
    </div>
  );
}

export function Badge({ children, color = "zinc" }: { children: ReactNode; color?: "green" | "amber" | "red" | "zinc" }) {
  const cls = {
    green: "bg-green-500/20 text-green-400",
    amber: "bg-amber-500/20 text-amber-400",
    red: "bg-red-500/20 text-red-400",
    zinc: "bg-zinc-700/40 text-zinc-400",
  }[color];

  return (
    <span className={`inline-block px-2 py-0.5 rounded text-xs ${cls}`}>{children}</span>
  );
}

export function CopyButton({ text }: { text: string }) {
  const copy = () => {
    navigator.clipboard.writeText(text);
  };
  return (
    <button
      onClick={copy}
      className="text-zinc-500 hover:text-amber-500 text-xs ml-2"
      title="Copy"
    >
      Copy
    </button>
  );
}
