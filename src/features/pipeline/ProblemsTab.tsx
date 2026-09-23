// Canvas "Problems" tab (`FR-UI-3`, `FR-BUILD-4/9`): structured defects from build/check.

import type { Defect } from "../../lib/bindings";
import type { LogLineView } from "./usePipeline";
import { composeFixPrompt } from "./fixPrompt";

const SEVERITY_COLOR: Record<Defect["severity"], string> = {
  error: "text-red-600 dark:text-red-400",
  warning: "text-amber-600 dark:text-amber-400",
  note: "text-neutral-500 dark:text-neutral-400",
};

// `NFR-A2`: status conveyed by icon + text, never colour alone — each row's severity is
// also spelled out as a word, not just a colour.
const SEVERITY_LABEL: Record<Defect["severity"], string> = {
  error: "Error",
  warning: "Warning",
  note: "Note",
};

export function ProblemsTab({
  defects,
  lines,
  running,
  onAskClaudeToFix,
}: {
  defects: Defect[];
  lines: LogLineView[];
  running: boolean;
  onAskClaudeToFix: (prompt: string) => void;
}) {
  const errorCount = defects.filter((d) => d.severity === "error").length;

  return (
    <div className="h-full overflow-y-auto px-4 py-2">
      {!running && errorCount > 0 && (
        <button
          type="button"
          onClick={() => onAskClaudeToFix(composeFixPrompt(defects, lines))}
          className="mb-2 rounded bg-red-600 px-2.5 py-1.5 text-xs font-medium text-white hover:bg-red-700"
        >
          Ask Claude to fix these {errorCount} error{errorCount === 1 ? "" : "s"}
        </button>
      )}
      {defects.length === 0 && <p className="text-xs text-neutral-500 dark:text-neutral-400">No problems.</p>}
      {defects.map((d, i) => (
        <div key={i} className="border-b border-neutral-100 py-1.5 text-xs last:border-0 dark:border-neutral-900">
          <span className={`font-mono font-medium ${SEVERITY_COLOR[d.severity]}`}>
            [{SEVERITY_LABEL[d.severity]}] {d.file}:{d.line}
            {d.column ? `:${d.column}` : ""}
          </span>
          <span className="ml-1.5 text-neutral-700 dark:text-neutral-300">{d.message}</span>
        </div>
      ))}
    </div>
  );
}
