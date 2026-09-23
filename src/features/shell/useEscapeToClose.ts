// `NFR-A1`: "no [keyboard traps]" — every modal-ish overlay in the app closes with
// `Escape`, wired once here rather than by each screen that pops one up.

import { useEffect } from "react";

export function useEscapeToClose(open: boolean, onClose: () => void) {
  useEffect(() => {
    if (!open) return;
    const onKeyDown = (e: KeyboardEvent) => {
      if (e.key === "Escape") {
        // Capture phase, stopped here: closes this overlay before the shell's own global
        // `Esc` (stop build/turn) handler sees the event, so Escape always dismisses the
        // topmost overlay first.
        e.stopPropagation();
        onClose();
      }
    };
    window.addEventListener("keydown", onKeyDown, true);
    return () => window.removeEventListener("keydown", onKeyDown, true);
  }, [open, onClose]);
}
