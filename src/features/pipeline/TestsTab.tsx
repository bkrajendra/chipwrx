// Canvas "Tests" tab (`FR-BUILD-10`, M9): `pio test --json-output`, rendered as a pass/fail
// list.

import type { TestCaseResult, TestStatus, TestSuite } from "../../lib/bindings";

const STATUS_COLOR: Record<TestStatus, string> = {
  passed: "text-emerald-600 dark:text-emerald-400",
  failed: "text-red-600 dark:text-red-400",
  errored: "text-red-600 dark:text-red-400",
  skipped: "text-neutral-500 dark:text-neutral-400",
};

// `NFR-A2`: status conveyed by icon + text, never colour alone.
const STATUS_LABEL: Record<TestStatus, string> = {
  passed: "PASS",
  failed: "FAIL",
  errored: "ERROR",
  skipped: "SKIP",
};

function CaseRow({ c }: { c: TestCaseResult }) {
  return (
    <div className="border-b border-neutral-100 py-1.5 pl-4 text-xs last:border-0 dark:border-neutral-900">
      <span className={`font-mono font-medium ${STATUS_COLOR[c.status]}`}>[{STATUS_LABEL[c.status]}]</span>
      <span className="ml-1.5 font-mono text-neutral-700 dark:text-neutral-300">{c.name}</span>
      {c.file && (
        <span className="ml-1.5 text-neutral-500 dark:text-neutral-400">
          {c.file}
          {c.line ? `:${c.line}` : ""}
        </span>
      )}
      {c.message && <p className="mt-0.5 whitespace-pre-wrap text-neutral-600 dark:text-neutral-400">{c.message}</p>}
    </div>
  );
}

function SuiteBlock({ s }: { s: TestSuite }) {
  const passed = s.cases.filter((c) => c.status === "passed").length;
  return (
    <div className="mb-3">
      <p className="text-xs font-medium">
        <span className={STATUS_COLOR[s.status]}>[{STATUS_LABEL[s.status]}]</span> {s.envName} · {s.testName} —{" "}
        {passed}/{s.cases.length} passed · {s.duration.toFixed(1)}s
      </p>
      {s.cases.map((c, i) => (
        <CaseRow key={i} c={c} />
      ))}
    </div>
  );
}

export function TestsTab({ suites, running, onRun }: { suites: TestSuite[]; running: boolean; onRun: () => void }) {
  return (
    <div className="h-full overflow-y-auto px-4 py-2">
      <button
        type="button"
        onClick={onRun}
        disabled={running}
        title="pio test --json-output"
        className="mb-2 rounded border border-neutral-300 px-2.5 py-1.5 text-xs font-medium hover:bg-neutral-100 disabled:opacity-40 dark:border-neutral-700 dark:hover:bg-neutral-800"
      >
        {running ? "Running tests…" : "Run tests"}
      </button>
      {suites.length === 0 && !running && <p className="text-xs text-neutral-500 dark:text-neutral-400">No test results yet.</p>}
      {suites.map((s, i) => (
        <SuiteBlock key={i} s={s} />
      ))}
    </div>
  );
}
