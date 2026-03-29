"use client";

import type { ReactNode } from "react";

interface Props {
  open: boolean;
  onClose: () => void;
  title: string;
  children: ReactNode;
}

export function Modal({ open, onClose, title, children }: Props) {
  if (!open) return null;

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/60">
      <div className="bg-[#14141f] border border-[#1e1e2e] rounded-xl w-full max-w-md mx-4 shadow-2xl">
        <div className="flex items-center justify-between px-6 py-4 border-b border-[#1e1e2e]">
          <h2 className="text-zinc-200 font-semibold">{title}</h2>
          <button onClick={onClose} className="text-zinc-500 hover:text-zinc-300">
            <svg className="w-5 h-5" fill="none" stroke="currentColor" viewBox="0 0 24 24">
              <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={2} d="M6 18L18 6M6 6l12 12" />
            </svg>
          </button>
        </div>
        <div className="px-6 py-4">{children}</div>
      </div>
    </div>
  );
}

interface ConfirmProps {
  open: boolean;
  onClose: () => void;
  onConfirm: () => void;
  title: string;
  message: string;
  confirmLabel?: string;
  danger?: boolean;
}

export function ConfirmModal({
  open,
  onClose,
  onConfirm,
  title,
  message,
  confirmLabel = "Confirm",
  danger = false,
}: ConfirmProps) {
  return (
    <Modal open={open} onClose={onClose} title={title}>
      <p className="text-zinc-400 text-sm mb-6">{message}</p>
      <div className="flex justify-end gap-3">
        <button
          onClick={onClose}
          className="px-4 py-2 text-sm text-zinc-400 hover:text-zinc-200 bg-[#1a1a2e] rounded-lg"
        >
          Cancel
        </button>
        <button
          onClick={() => {
            onConfirm();
            onClose();
          }}
          className={`px-4 py-2 text-sm rounded-lg ${
            danger
              ? "bg-red-600 hover:bg-red-700 text-white"
              : "bg-amber-600 hover:bg-amber-700 text-white"
          }`}
        >
          {confirmLabel}
        </button>
      </div>
    </Modal>
  );
}
