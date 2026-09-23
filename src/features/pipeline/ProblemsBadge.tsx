// Sidebar "Problems badge" (`FR-UI-2`) — clicking switches the canvas to the Problems tab.

import type { Defect } from "../../lib/bindings";

export function ProblemsBadge({ defects, onOpen }: { defects: Defect[]; onOpen: () => void }) {
  const errors = defects.filter((d) => d.severity === "error").length;
  const warnings = defects.filter((d) => d.severity === "warning").length;
  if (errors === 0 && warnings === 0) return null;

  return (
    <button
      type="button"
      onClick={onOpen}
      className="flex w-full items-center justify-between border-b border-neutral-200 p-3 text-left text-xs dark:border-neutral-800"
    >
      <span className="font-medium">Problems</span>
      <span className="flex gap-1.5">
        {errors > 0 && <span className="rounded-full bg-red-100 px-2 py-0.5 font-medium text-red-700 dark:bg-red-950 dark:text-red-400">{errors} error{errors === 1 ? "" : "s"}</span>}
        {warnings > 0 && (
          <span className="rounded-full bg-amber-100 px-2 py-0.5 font-medium text-amber-800 dark:bg-amber-950 dark:text-amber-300">
            {warnings} warning{warnings === 1 ? "" : "s"}
          </span>
        )}
      </span>
    </button>
  );
}
