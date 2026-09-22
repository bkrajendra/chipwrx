// ANSI-preserving build/upload output (`FR-BUILD-1`: "the app must not pass --no-ansi
// when it wants colour, and must render ANSI SGR sequences"). `ARCHITECTURE.md`'s stack
// choice: xterm.js — "do not hand-roll ANSI."

import "@xterm/xterm/css/xterm.css";
import { FitAddon } from "@xterm/addon-fit";
import { Terminal } from "@xterm/xterm";
import { useEffect, useRef } from "react";
import type { LogLineView } from "./usePipeline";

export function LogsPane({ lines }: { lines: LogLineView[] }) {
  const containerRef = useRef<HTMLDivElement>(null);
  const termRef = useRef<Terminal | null>(null);
  const writtenCount = useRef(0);

  useEffect(() => {
    if (!containerRef.current) return;
    const term = new Terminal({
      convertEol: true,
      fontSize: 12,
      disableStdin: true,
      theme: { background: "#0a0a0a" },
    });
    const fit = new FitAddon();
    term.loadAddon(fit);
    term.open(containerRef.current);
    fit.fit();
    termRef.current = term;
    writtenCount.current = 0;

    const resizeObserver = new ResizeObserver(() => fit.fit());
    resizeObserver.observe(containerRef.current);

    return () => {
      resizeObserver.disconnect();
      term.dispose();
      termRef.current = null;
    };
  }, []);

  useEffect(() => {
    const term = termRef.current;
    if (!term) return;
    if (lines.length < writtenCount.current) {
      // A new run started (`usePipeline` resets `lines` on `started`) — fresh terminal.
      term.clear();
      writtenCount.current = 0;
    }
    for (let i = writtenCount.current; i < lines.length; i++) {
      term.writeln(lines[i].text);
    }
    writtenCount.current = lines.length;
  }, [lines]);

  return <div ref={containerRef} className="h-full w-full overflow-hidden" />;
}
