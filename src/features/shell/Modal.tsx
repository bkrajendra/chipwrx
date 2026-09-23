// A reusable modal shell: backdrop-click-to-close plus `Escape`-to-close (via
// `useEscapeToClose`) — used by both places `DoctorScreen` gets shown (from the Launcher
// and from a `Workspace`). `GlobalSettingsScreen`/`CommandPalette` have their own layout
// needs but share the same `useEscapeToClose` hook, so every overlay in the app closes
// with `Escape` the same way (`NFR-A1`: no keyboard traps).

import type { ReactNode } from "react";
import { useEscapeToClose } from "./useEscapeToClose";

export function Modal({
  open,
  onClose,
  label,
  children,
  maxWidthClassName = "max-w-xl",
}: {
  open: boolean;
  onClose: () => void;
  label: string;
  children: ReactNode;
  maxWidthClassName?: string;
}) {
  useEscapeToClose(open, onClose);

  if (!open) return null;

  return (
    <div role="dialog" aria-modal="true" aria-label={label} className="fixed inset-0 z-50 flex items-center justify-center bg-black/30" onClick={onClose}>
      <div
        className={`max-h-[80vh] w-full ${maxWidthClassName} overflow-y-auto rounded-lg border border-neutral-200 bg-white shadow-xl dark:border-neutral-700 dark:bg-neutral-900`}
        onClick={(e) => e.stopPropagation()}
      >
        {children}
      </div>
    </div>
  );
}
