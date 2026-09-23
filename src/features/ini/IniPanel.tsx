// Canvas "Project Settings" tab (`FR-UI-3`): Form/Raw/Libraries/Templates/PIO Settings
// over one `IniDocument` (`FR-INI-1`).

import { useCallback, useState } from "react";
import type { LintReport } from "../../lib/bindings";
import { FormTab } from "./FormTab";
import { LibrariesTab } from "./LibrariesTab";
import { PioSettingsTab } from "./PioSettingsTab";
import { RawTab } from "./RawTab";
import { TemplatesTab } from "./TemplatesTab";
import { useIni } from "./useIni";

type Tab = "form" | "raw" | "libraries" | "templates" | "pio-settings";

export function IniPanel({ workspaceId }: { workspaceId: string }) {
  const { schema, document, loading, error, conflict, reload, applyEdits, writeRaw, lint } = useIni(workspaceId);
  const [tab, setTab] = useState<Tab>("form");
  const [lintReport, setLintReport] = useState<LintReport | null>(null);
  const [linting, setLinting] = useState(false);
  const [librariesBusy, setLibrariesBusy] = useState(false);

  const runLint = useCallback(async () => {
    setLinting(true);
    try {
      setLintReport(await lint());
    } finally {
      setLinting(false);
    }
  }, [lint]);

  return (
    <div className="flex h-full flex-col">
      <header className="flex items-center justify-between gap-2 border-b border-neutral-200 px-4 py-2 dark:border-neutral-800">
        <span className="text-sm font-medium">platformio.ini</span>
        <button type="button" onClick={() => void runLint()} disabled={linting} className="text-xs text-neutral-500 hover:underline dark:text-neutral-400">
          {linting ? "Linting…" : "Lint"}
        </button>
      </header>

      {conflict && (
        <div className="flex items-center justify-between gap-2 border-b border-amber-300 bg-amber-50 px-4 py-2 text-xs dark:border-amber-800 dark:bg-amber-950">
          <span>platformio.ini changed on disk outside the app.</span>
          <button type="button" onClick={() => void reload()} className="rounded border border-amber-400 px-2 py-0.5 text-[11px] hover:bg-amber-100 dark:hover:bg-amber-900">
            Reload
          </button>
        </div>
      )}
      {error && <p className="border-b border-neutral-200 px-4 py-1.5 text-xs text-red-500 dark:border-neutral-800">{error}</p>}

      {lintReport && (
        <div className="border-b border-neutral-200 px-4 py-2 text-xs dark:border-neutral-800">
          {lintReport.errors.length === 0 && lintReport.warnings.length === 0 && <p className="text-emerald-600 dark:text-emerald-400">No linting errors.</p>}
          {lintReport.errors.map((e, i) => (
            <p key={`e${i}`} className="text-red-500">
              {e.kind}: {e.message}
            </p>
          ))}
          {lintReport.warnings.map((w, i) => (
            <p key={`w${i}`} className="text-amber-600 dark:text-amber-400">
              {w}
            </p>
          ))}
        </div>
      )}

      <div className="flex gap-1 border-b border-neutral-200 px-4 pt-2 dark:border-neutral-800">
        {(["form", "raw", "libraries", "templates", "pio-settings"] as const).map((t) => (
          <button
            key={t}
            type="button"
            onClick={() => setTab(t)}
            className={`rounded-t px-2.5 py-1 text-xs font-medium capitalize ${
              tab === t ? "bg-neutral-100 dark:bg-neutral-800" : "text-neutral-500 dark:text-neutral-400"
            }`}
          >
            {t === "pio-settings" ? "PIO Settings" : t}
          </button>
        ))}
      </div>

      <div className="min-h-0 flex-1">
        {loading && <p className="px-4 py-2 text-xs text-neutral-500 dark:text-neutral-400">Loading…</p>}
        {!loading && document && schema && tab === "form" && <FormTab document={document} schema={schema} onApply={applyEdits} />}
        {!loading && document && tab === "raw" && <RawTab document={document} onSave={writeRaw} />}
        {!loading && tab === "libraries" && <LibrariesTab workspaceId={workspaceId} onBusyChange={setLibrariesBusy} />}
        {!loading && document && tab === "templates" && <TemplatesTab workspaceId={workspaceId} document={document} onApplied={() => void reload()} />}
        {!loading && tab === "pio-settings" && <PioSettingsTab />}
      </div>
      {librariesBusy && <p className="px-4 py-1 text-[11px] text-neutral-500 dark:text-neutral-400">Working…</p>}
    </div>
  );
}
