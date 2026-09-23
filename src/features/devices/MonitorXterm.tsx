// The monitor's terminal rendering — `FR-DEV-5`: "ANSI rendering, timestamps... regex
// filter." `ARCHITECTURE.md`'s stack choice: xterm.js, same as `pipeline/LogsPane`.

import "@xterm/xterm/css/xterm.css";
import { FitAddon } from "@xterm/addon-fit";
import { Terminal } from "@xterm/xterm";
import { useEffect, useRef } from "react";
import type { MonitorLine } from "./useMonitor";

function formatTs(ms: number): string {
  const d = new Date(ms);
  const pad = (n: number, w = 2) => n.toString().padStart(w, "0");
  return `${pad(d.getHours())}:${pad(d.getMinutes())}:${pad(d.getSeconds())}.${pad(d.getMilliseconds(), 3)}`;
}

export function MonitorXterm({
  lines,
  timestamps,
  filter,
  autoscroll,
  onSelectionChange,
}: {
  lines: MonitorLine[];
  timestamps: boolean;
  filter: RegExp | null;
  autoscroll: boolean;
  onSelectionChange: (selection: string) => void;
}) {
  const containerRef = useRef<HTMLDivElement>(null);
  const termRef = useRef<Terminal | null>(null);
  const writtenCount = useRef(0);
  const lastFilterKey = useRef("");

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
    lastFilterKey.current = "";

    const selectionDisposable = term.onSelectionChange(() => onSelectionChange(term.getSelection()));
    const resizeObserver = new ResizeObserver(() => fit.fit());
    resizeObserver.observe(containerRef.current);

    return () => {
      selectionDisposable.dispose();
      resizeObserver.disconnect();
      term.dispose();
      termRef.current = null;
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps -- onSelectionChange is stable per caller; re-subscribing on every render would drop selections
  }, []);

  useEffect(() => {
    const term = termRef.current;
    if (!term) return;
    const filterKey = `${timestamps}|${filter?.source ?? ""}`;
    const filterChanged = filterKey !== lastFilterKey.current;
    const bufferReset = lines.length < writtenCount.current;

    const render = (line: MonitorLine) => term.writeln(timestamps ? `[${formatTs(line.tsMs)}] ${line.text}` : line.text);

    if (filterChanged || bufferReset) {
      term.clear();
      lastFilterKey.current = filterKey;
      const all = filter ? lines.filter((l) => filter.test(l.text)) : lines;
      for (const line of all) render(line);
    } else {
      const fresh = lines.slice(writtenCount.current);
      const toRender = filter ? fresh.filter((l) => filter.test(l.text)) : fresh;
      for (const line of toRender) render(line);
    }
    writtenCount.current = lines.length;
    if (autoscroll) term.scrollToBottom();
  }, [lines, timestamps, filter, autoscroll]);

  return <div ref={containerRef} className="h-full w-full overflow-hidden" />;
}
