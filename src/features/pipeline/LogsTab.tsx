// Canvas "Logs" tab (`FR-UI-3`): ANSI terminal block for pio/claude/installer output.

import { lazy, Suspense } from "react";
import type { LogLineView } from "./usePipeline";

const LogsPane = lazy(() => import("./LogsPane").then((m) => ({ default: m.LogsPane })));

export function LogsTab({ lines }: { lines: LogLineView[] }) {
  return (
    <Suspense fallback={<p className="px-4 py-2 text-xs text-neutral-500 dark:text-neutral-400">Loading terminal…</p>}>
      <LogsPane lines={lines} />
    </Suspense>
  );
}
