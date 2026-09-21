import { lazy, Suspense, useCallback, useEffect, useState } from "react";
import type { AppError, FileChange, FileDiff } from "../../lib/bindings";
import { renderAppError } from "../../lib/errors";
import { changesDiff, changesForTurn, changesRevertFile, changesRevertTurn } from "../../lib/ipc";
import { diffIni } from "./iniDiff";

// CodeMirror is a meaningful chunk of the bundle and is only needed once a diff is
// actually opened — loaded on demand rather than on every app start.
const DiffViewer = lazy(() => import("./DiffViewer").then((m) => ({ default: m.DiffViewer })));

function isAppError(e: unknown): e is AppError {
  return typeof e === "object" && e !== null && "code" in e;
}

function describe(e: unknown): string {
  return isAppError(e) ? renderAppError(e).message : String(e);
}

const STATUS_LABEL: Record<FileChange["status"], string> = {
  added: "A",
  modified: "M",
  deleted: "D",
  renamed: "R",
};

const STATUS_COLOR: Record<FileChange["status"], string> = {
  added: "text-emerald-600 dark:text-emerald-400",
  modified: "text-amber-600 dark:text-amber-400",
  deleted: "text-red-600 dark:text-red-400",
  renamed: "text-blue-600 dark:text-blue-400",
};

function IniSummary({ before, after }: { before: string; after: string }) {
  const changes = diffIni(before, after);
  if (changes.length === 0) return null;
  return (
    <div className="mb-3 rounded border border-amber-400 bg-amber-50 p-2.5 text-xs dark:border-amber-700 dark:bg-amber-950">
      <p className="mb-1.5 font-medium">
        platformio.ini changed — this can affect the board, upload protocol, or flash layout (<code>FR-SAFE-5</code>):
      </p>
      <ul className="space-y-0.5 font-mono">
        {changes.map((c) => (
          <li key={`${c.section}.${c.key}`}>
            [{c.section}] {c.key}: {c.before ?? <em>(unset)</em>} → {c.after ?? <em>(removed)</em>}
          </li>
        ))}
      </ul>
    </div>
  );
}

export function ChangesPanel({ workspaceId, turnId, onClose }: { workspaceId: string; turnId: string; onClose: () => void }) {
  const [changes, setChanges] = useState<FileChange[] | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [selected, setSelected] = useState<string | null>(null);
  const [diff, setDiff] = useState<FileDiff | null>(null);
  const [busy, setBusy] = useState(false);

  const reload = useCallback(async () => {
    setError(null);
    try {
      setChanges(await changesForTurn(workspaceId, turnId));
    } catch (e) {
      setError(describe(e));
    }
  }, [workspaceId, turnId]);

  useEffect(() => {
    void reload();
  }, [reload]);

  useEffect(() => {
    if (!selected) {
      setDiff(null);
      return;
    }
    let cancelled = false;
    void changesDiff(workspaceId, turnId, selected).then(
      (d) => {
        if (!cancelled) setDiff(d);
      },
      (e: unknown) => {
        if (!cancelled) setError(describe(e));
      },
    );
    return () => {
      cancelled = true;
    };
  }, [workspaceId, turnId, selected]);

  const revertOne = async (path: string) => {
    setBusy(true);
    setError(null);
    try {
      await changesRevertFile(workspaceId, turnId, path);
      if (selected === path) setSelected(null);
      await reload();
    } catch (e) {
      setError(describe(e));
    } finally {
      setBusy(false);
    }
  };

  const revertAll = async () => {
    setBusy(true);
    setError(null);
    try {
      await changesRevertTurn(workspaceId, turnId);
      setSelected(null);
      await reload();
    } catch (e) {
      setError(describe(e));
    } finally {
      setBusy(false);
    }
  };

  const hasOutside = changes?.some((c) => c.outsideExpectedDirs) ?? false;

  return (
    <div className="flex h-full flex-col">
      <header className="flex items-center justify-between border-b border-neutral-200 px-4 py-2 dark:border-neutral-800">
        <span className="text-sm font-medium">Changes{changes ? ` (${changes.length})` : ""}</span>
        <div className="flex gap-2">
          <button
            type="button"
            onClick={() => void revertAll()}
            disabled={busy || !changes || changes.length === 0}
            className="rounded border border-neutral-300 px-2.5 py-1 text-xs font-medium hover:bg-neutral-100 disabled:opacity-40 dark:border-neutral-700 dark:hover:bg-neutral-800"
          >
            Revert all
          </button>
          <button type="button" onClick={onClose} className="text-xs text-neutral-500 hover:underline dark:text-neutral-400">
            Close
          </button>
        </div>
      </header>

      {hasOutside && (
        <p className="border-b border-amber-400 bg-amber-50 px-4 py-2 text-xs dark:border-amber-700 dark:bg-amber-950">
          This turn wrote outside <code>src/</code>, <code>include/</code>, <code>lib/</code>, <code>test/</code>,{" "}
          <code>data/</code>, and <code>platformio.ini</code> — review those files carefully (<code>FR-SAFE-4</code>).
        </p>
      )}
      {error && <p className="border-b border-neutral-200 px-4 py-2 text-xs text-red-500 dark:border-neutral-800">{error}</p>}

      <div className="flex flex-1 overflow-hidden">
        <div className="w-64 shrink-0 overflow-y-auto border-r border-neutral-200 dark:border-neutral-800">
          {changes === null && <p className="p-3 text-xs text-neutral-500 dark:text-neutral-400">Loading…</p>}
          {changes?.length === 0 && <p className="p-3 text-xs text-neutral-500 dark:text-neutral-400">No changes.</p>}
          {changes?.map((c) => (
            <div
              key={c.path}
              className={`flex items-center justify-between gap-1 border-b border-neutral-100 px-2 py-1.5 text-xs last:border-0 dark:border-neutral-900 ${
                selected === c.path ? "bg-neutral-100 dark:bg-neutral-800" : ""
              }`}
            >
              <button type="button" onClick={() => setSelected(c.path)} className="flex min-w-0 flex-1 items-center gap-1.5 text-left">
                <span className={`w-3 shrink-0 font-mono font-bold ${STATUS_COLOR[c.status]}`}>{STATUS_LABEL[c.status]}</span>
                <span className="truncate" title={c.path}>
                  {c.path}
                </span>
                {c.outsideExpectedDirs && (
                  <span className="shrink-0 text-amber-500" title="Outside the expected source directories">
                    ⚠
                  </span>
                )}
              </button>
              <span className="shrink-0 font-mono text-[10px] text-neutral-400">
                +{c.additions} -{c.deletions}
              </span>
              <button
                type="button"
                onClick={() => void revertOne(c.path)}
                disabled={busy}
                className="shrink-0 text-neutral-400 hover:text-neutral-700 disabled:opacity-40 dark:hover:text-neutral-200"
                title={`Revert ${c.path}`}
              >
                ↺
              </button>
            </div>
          ))}
        </div>

        <div className="flex-1 overflow-y-auto p-3">
          {!selected && <p className="text-xs text-neutral-500 dark:text-neutral-400">Select a file to view its diff.</p>}
          {selected && !diff && <p className="text-xs text-neutral-500 dark:text-neutral-400">Loading diff…</p>}
          {selected && diff && (
            <>
              {selected === "platformio.ini" && diff.before !== null && diff.after !== null && (
                <IniSummary before={diff.before} after={diff.after} />
              )}
              <Suspense fallback={<p className="text-xs text-neutral-500 dark:text-neutral-400">Loading diff viewer…</p>}>
                <DiffViewer before={diff.before ?? ""} after={diff.after ?? ""} filename={selected} />
              </Suspense>
            </>
          )}
        </div>
      </div>
    </div>
  );
}
