"use client";

import Link from "next/link";
import { useState } from "react";
import { useAuth } from "@/lib/auth";

export function Nav() {
  const { account, logout } = useAuth();
  const [menuOpen, setMenuOpen] = useState(false);

  if (!account) return null;

  return (
    <nav className="flex items-center justify-between border-b border-[#1e1e2e] bg-[#0a0a0f] px-6 py-3">
      <div className="flex items-center gap-6">
        <Link href="/" className="flex items-center gap-2 text-amber-500 font-bold text-lg">
          <span className="inline-block w-3 h-3 rounded-full bg-amber-500" />
          <span className="inline-block w-3 h-3 rounded-full bg-amber-500 -ml-2" />
          <span className="ml-1">HEBBS</span>
        </Link>
        <Link href="/" className="text-zinc-400 hover:text-zinc-200 text-sm">
          Workspaces
        </Link>
        {account.role === "admin" && (
          <>
            <Link href="/settings/" className="text-zinc-400 hover:text-zinc-200 text-sm">
              Settings
            </Link>
            <Link href="/team/" className="text-zinc-400 hover:text-zinc-200 text-sm">
              Team
            </Link>
          </>
        )}
      </div>
      <div className="relative">
        <button
          onClick={() => setMenuOpen(!menuOpen)}
          className="text-zinc-400 hover:text-zinc-200 text-sm flex items-center gap-1"
        >
          {account.email}
          <svg className="w-4 h-4" fill="none" stroke="currentColor" viewBox="0 0 24 24">
            <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={2} d="M19 9l-7 7-7-7" />
          </svg>
        </button>
        {menuOpen && (
          <div className="absolute right-0 mt-2 w-48 bg-[#14141f] border border-[#1e1e2e] rounded-lg shadow-lg z-50">
            <div className="px-4 py-2 text-xs text-zinc-500 border-b border-[#1e1e2e]">
              {account.role}
            </div>
            <button
              onClick={logout}
              className="w-full text-left px-4 py-2 text-sm text-zinc-400 hover:text-zinc-200 hover:bg-[#1e1e2e]"
            >
              Sign out
            </button>
          </div>
        )}
      </div>
    </nav>
  );
}
