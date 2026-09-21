import { openUrl } from "@tauri-apps/plugin-opener";
import type { DoctorReport, ProbeResult, RemediationKind } from "../../lib/bindings";
import { diagnosticsExport } from "../../lib/ipc";
import { PROBE_DEFS, probeSummary, probeTone, remediationLabel } from "./probeDisplay";
import { useDoctor } from "./useDoctor";

const TONE_DOT: Record<string, string> = {
  ok: "bg-emerald-500",
  warn: "bg-amber-500",
  bad: "bg-red-500",
  pending: "bg-neutral-400 animate-pulse",
};

// `Missing` doesn't carry a RemediationKind, so the "which installer" mapping lives here,
// keyed by probe row rather than by API shape.
const INSTALL_KIND_FOR_PROBE: Partial<Record<string, RemediationKind>> = {
  claudeBinary: "installClaude",
  pioBinary: "installPio",
};

function ProbeRow({
  label,
  result,
  onRemediate,
}: {
  label: string;
  result: ProbeResult;
  onRemediate: (() => void) | null;
}) {
  const tone = probeTone(result);
  const label_ = remediationLabel(result);

  return (
    <div className="flex items-center justify-between border-b border-neutral-200 py-2.5 last:border-0 dark:border-neutral-800">
      <div className="flex items-center gap-3">
        <span className={`h-2.5 w-2.5 rounded-full ${TONE_DOT[tone]}`} aria-hidden />
        <div>
          <div className="text-sm font-medium">{label}</div>
          <div className="text-xs text-neutral-500 dark:text-neutral-400">{probeSummary(result)}</div>
        </div>
      </div>
      {label_ && onRemediate && (
        <button
          type="button"
          onClick={onRemediate}
          className="rounded border border-neutral-300 px-2.5 py-1 text-xs font-medium hover:bg-neutral-100 dark:border-neutral-700 dark:hover:bg-neutral-800"
        >
          {label_}
        </button>
      )}
    </div>
  );
}

function reportField(report: DoctorReport, key: string): ProbeResult {
  return (report as unknown as Record<string, ProbeResult>)[key];
}

export function DoctorScreen() {
  const { report, loading, refresh, install, installLog, dismissInstallLog, authenticate } = useDoctor();

  const handleRemediate = (key: string, result: ProbeResult) => {
    if (result.status === "missing") {
      const kind = INSTALL_KIND_FOR_PROBE[key];
      if (kind) void install(kind);
      return;
    }
    if (result.status === "degraded" && result.remediation) {
      const r = result.remediation;
      switch (r.kind) {
        case "installClaude":
        case "installPio":
          void install(r.kind);
          break;
        case "authenticateClaude":
          void authenticate();
          break;
        case "openUrl":
          if (r.url) void openUrl(r.url);
          break;
        case "installUdevRules":
        case "showCommand":
          // Command preview is already shown inline in the row's summary text via the
          // remediation's own label; a copyable block belongs to the full Settings →
          // Toolchain screen (later milestone), not this compact Doctor list.
          break;
      }
    }
  };

  return (
    <div className="mx-auto max-w-xl px-4 py-6">
      <div className="mb-4 flex items-center justify-between">
        <h2 className="text-sm font-semibold">Doctor</h2>
        <div className="flex gap-2">
          <button
            type="button"
            onClick={() => void refresh(true)}
            disabled={loading}
            className="rounded border border-neutral-300 px-2.5 py-1 text-xs font-medium hover:bg-neutral-100 disabled:opacity-50 dark:border-neutral-700 dark:hover:bg-neutral-800"
          >
            {loading ? "Checking…" : "Re-check"}
          </button>
          <button
            type="button"
            onClick={() => void diagnosticsExport()}
            className="rounded border border-neutral-300 px-2.5 py-1 text-xs font-medium hover:bg-neutral-100 dark:border-neutral-700 dark:hover:bg-neutral-800"
          >
            Export diagnostics
          </button>
        </div>
      </div>

      {!report && loading && (
        <p className="text-sm text-neutral-500 dark:text-neutral-400">Running probes…</p>
      )}

      {report && (
        <div className="rounded border border-neutral-200 px-3 dark:border-neutral-800">
          {PROBE_DEFS.map(({ key, label }) => {
            const result = reportField(report, key);
            return (
              <ProbeRow
                key={key}
                label={label}
                result={result}
                onRemediate={() => handleRemediate(key, result)}
              />
            );
          })}
        </div>
      )}

      {installLog && (
        <div className="mt-4 rounded border border-neutral-200 p-3 dark:border-neutral-800">
          <div className="mb-2 flex items-center justify-between">
            <span className="text-xs font-medium">
              Installing {installLog.kind === "installClaude" ? "Claude Code" : "PlatformIO"}
            </span>
            {installLog.done && (
              <button
                type="button"
                onClick={dismissInstallLog}
                className="text-xs text-neutral-500 hover:underline dark:text-neutral-400"
              >
                Close
              </button>
            )}
          </div>
          <div className="max-h-48 overflow-y-auto rounded bg-neutral-950 p-2 font-mono text-xs text-neutral-100">
            {installLog.events.map((event, i) => {
              if (event.type === "started") return <div key={i}>$ {event.data.argv.join(" ")}</div>;
              if (event.type === "line") return <div key={i}>{event.data.text}</div>;
              if (event.type === "progress") return <div key={i} className="text-neutral-400">— {event.data.message}</div>;
              return (
                <div key={i} className={event.data.success ? "text-emerald-400" : "text-red-400"}>
                  {event.data.success ? "Done." : `Failed (exit ${event.data.exitCode}).`}
                </div>
              );
            })}
          </div>
        </div>
      )}
    </div>
  );
}
