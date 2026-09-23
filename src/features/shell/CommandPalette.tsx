// `FR-UI-8`: Cmd/Ctrl+K command palette. A modal, keyboard-navigable list of actions —
// every entry here is also reachable some other way (a button, a tab, a shortcut); the
// palette exists so a keyboard-only user never has to hunt through the sidebar for one.

import { useEffect, useMemo, useRef, useState } from "react";
import { strings } from "../../lib/strings";
import { useEscapeToClose } from "./useEscapeToClose";

export interface PaletteCommand {
  id: string;
  label: string;
  hint?: string;
  run: () => void;
}

export function CommandPalette({ open, onClose, commands }: { open: boolean; onClose: () => void; commands: PaletteCommand[] }) {
  const [query, setQuery] = useState("");
  const [activeIndex, setActiveIndex] = useState(0);
  const inputRef = useRef<HTMLInputElement>(null);
  useEscapeToClose(open, onClose);

  const filtered = useMemo(() => {
    const q = query.trim().toLowerCase();
    if (!q) return commands;
    return commands.filter((c) => c.label.toLowerCase().includes(q));
  }, [commands, query]);

  useEffect(() => {
    if (open) {
      setQuery("");
      setActiveIndex(0);
      // Focus after the modal actually mounts, not before.
      requestAnimationFrame(() => inputRef.current?.focus());
    }
  }, [open]);

  useEffect(() => {
    setActiveIndex(0);
  }, [query]);

  if (!open) return null;

  const runActive = () => {
    const cmd = filtered[activeIndex];
    if (cmd) {
      onClose();
      cmd.run();
    }
  };

  return (
    <div
      role="dialog"
      aria-modal="true"
      aria-label="Command palette"
      className="fixed inset-0 z-50 flex items-start justify-center bg-black/30 pt-[15vh]"
      onClick={onClose}
    >
      <div
        className="w-full max-w-lg overflow-hidden rounded-lg border border-neutral-200 bg-white shadow-xl dark:border-neutral-700 dark:bg-neutral-900"
        onClick={(e) => e.stopPropagation()}
      >
        <input
          ref={inputRef}
          value={query}
          onChange={(e) => setQuery(e.target.value)}
          onKeyDown={(e) => {
            if (e.key === "Escape") {
              e.preventDefault();
              onClose();
            } else if (e.key === "ArrowDown") {
              e.preventDefault();
              setActiveIndex((i) => Math.min(i + 1, filtered.length - 1));
            } else if (e.key === "ArrowUp") {
              e.preventDefault();
              setActiveIndex((i) => Math.max(i - 1, 0));
            } else if (e.key === "Enter") {
              e.preventDefault();
              runActive();
            }
          }}
          placeholder={strings.palette.placeholder}
          aria-label={strings.palette.placeholder}
          className="w-full border-b border-neutral-200 bg-transparent px-4 py-3 text-sm outline-none dark:border-neutral-700"
        />
        <ul role="listbox" className="max-h-80 overflow-y-auto py-1">
          {filtered.length === 0 && <li className="px-4 py-3 text-xs text-neutral-500 dark:text-neutral-400">{strings.palette.noResults}</li>}
          {filtered.map((cmd, i) => (
            <li key={cmd.id}>
              <button
                type="button"
                role="option"
                aria-selected={i === activeIndex}
                onMouseEnter={() => setActiveIndex(i)}
                onClick={() => {
                  onClose();
                  cmd.run();
                }}
                className={`flex w-full items-center justify-between px-4 py-2 text-left text-sm ${
                  i === activeIndex ? "bg-neutral-100 dark:bg-neutral-800" : ""
                }`}
              >
                <span>{cmd.label}</span>
                {cmd.hint && <span className="text-xs text-neutral-500 dark:text-neutral-400">{cmd.hint}</span>}
              </button>
            </li>
          ))}
        </ul>
      </div>
    </div>
  );
}
