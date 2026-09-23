// `FR-UI-8`: "Global keyboard shortcuts: Cmd/Ctrl+B build, Cmd/Ctrl+U upload, Cmd/Ctrl+M
// monitor, Cmd/Ctrl+K command palette, Cmd/Ctrl+, settings, Esc stop." Attached once at the
// shell root — modifier combinations are safe to intercept even while a text input has
// focus (they never collide with typing a literal character), so this doesn't special-case
// which element triggered the event the way a plain "B" hotkey would have to.

import { useEffect, useRef } from "react";

export interface ShellShortcutHandlers {
  onBuild: () => void;
  onUpload: () => void;
  onMonitor: () => void;
  onPalette: () => void;
  onSettings: () => void;
  onStop: () => void;
}

function isModifierHeld(e: KeyboardEvent): boolean {
  // `metaKey` on macOS (Cmd), `ctrlKey` everywhere else — `CLI-CONTRACT.md`-adjacent
  // convention already used for `PromptDeck`'s Cmd/Ctrl+Enter.
  return e.metaKey || e.ctrlKey;
}

export function useKeyboardShortcuts(handlers: ShellShortcutHandlers) {
  // Callers naturally pass a fresh object literal every render; keeping the latest handlers
  // in a ref means the single `window` listener attached below never needs to be torn down
  // and re-added just because a handler closure changed identity.
  const handlersRef = useRef(handlers);
  handlersRef.current = handlers;

  useEffect(() => {
    const onKeyDown = (e: KeyboardEvent) => {
      const h = handlersRef.current;
      if (e.key === "Escape") {
        h.onStop();
        return;
      }
      if (!isModifierHeld(e)) return;
      switch (e.key.toLowerCase()) {
        case "b":
          e.preventDefault();
          h.onBuild();
          break;
        case "u":
          e.preventDefault();
          h.onUpload();
          break;
        case "m":
          e.preventDefault();
          h.onMonitor();
          break;
        case "k":
          e.preventDefault();
          h.onPalette();
          break;
        case ",":
          e.preventDefault();
          h.onSettings();
          break;
        default:
          break;
      }
    };
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, []);
}
