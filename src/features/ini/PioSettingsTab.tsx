// `FR-INI-6`: wraps `pio settings get/set` — each row rendered from the name/value/
// description triple the CLI itself prints, with a per-setting reset action when a default
// value was actually observed (`core::pio::global_settings` — not every row's `[default]`
// bracket is populated; see that module's doc comment).

import { useCallback, useEffect, useState } from "react";
import type { AppError, PioSetting } from "../../lib/bindings";
import { renderAppError } from "../../lib/errors";
import { pioSettingsGet, pioSettingsSet } from "../../lib/ipc";

function isAppError(e: unknown): e is AppError {
  return typeof e === "object" && e !== null && "code" in e;
}
function describe(e: unknown): string {
  return isAppError(e) ? renderAppError(e).message : String(e);
}

export function PioSettingsTab() {
  const [settings, setSettings] = useState<PioSetting[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState<string | null>(null);

  const load = useCallback(async () => {
    setLoading(true);
    setError(null);
    try {
      setSettings(await pioSettingsGet());
    } catch (e) {
      setError(describe(e));
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => {
    void load();
  }, [load]);

  const set = useCallback(async (name: string, value: string) => {
    setBusy(name);
    setError(null);
    try {
      setSettings(await pioSettingsSet(name, value));
    } catch (e) {
      setError(describe(e));
    } finally {
      setBusy(null);
    }
  }, []);

  if (loading) return <p className="p-4 text-xs text-neutral-500 dark:text-neutral-400">Loading…</p>;

  return (
    <div className="h-full overflow-y-auto px-4 py-2">
      {error && <p className="pb-2 text-xs text-red-500">{error}</p>}
      {settings.map((s) => (
        <div key={s.name} className="border-b border-neutral-100 py-2 text-xs last:border-0 dark:border-neutral-900">
          <div className="flex items-center justify-between gap-2">
            <span className="font-mono font-medium">{s.name}</span>
            {s.defaultValue !== null && s.currentValue !== s.defaultValue && (
              <button
                type="button"
                onClick={() => void set(s.name, s.defaultValue as string)}
                disabled={busy === s.name}
                className="shrink-0 rounded border border-neutral-300 px-2 py-0.5 text-[11px] hover:bg-neutral-100 disabled:opacity-40 dark:border-neutral-700 dark:hover:bg-neutral-800"
              >
                Reset to default
              </button>
            )}
          </div>
          <p className="mt-0.5 text-neutral-500 dark:text-neutral-400">{s.description}</p>
          <input
            defaultValue={s.currentValue}
            onKeyDown={(e) => {
              if (e.key === "Enter") void set(s.name, (e.target as HTMLInputElement).value);
            }}
            onBlur={(e) => {
              if (e.target.value !== s.currentValue) void set(s.name, e.target.value);
            }}
            disabled={busy === s.name}
            className="mt-1 w-full rounded border border-neutral-300 bg-transparent px-2 py-1 text-xs disabled:opacity-40 dark:border-neutral-700"
          />
        </div>
      ))}
    </div>
  );
}
