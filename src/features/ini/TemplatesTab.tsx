// `FR-INI-11`: save the current `platformio.ini` as a named template, and apply one to the
// current project. Applying merges `env:*` sections and writes immediately
// (`core::project::templates` module docs) — the diff-before-write confirmation
// `DATA-MODEL.md` §10 describes is approximated here as a diff-after-apply with an
// easy Undo, since the given IPC surface has no separate non-mutating preview command
// (`SPEC.md` §8 open question 33).

import { useCallback, useEffect, useState } from "react";
import type { AppError, IniDocument, IniTemplate } from "../../lib/bindings";
import { renderAppError } from "../../lib/errors";
import { iniTemplateApply, iniTemplateList, iniTemplateSave, iniWriteRaw } from "../../lib/ipc";

function isAppError(e: unknown): e is AppError {
  return typeof e === "object" && e !== null && "code" in e;
}
function describe(e: unknown): string {
  return isAppError(e) ? renderAppError(e).message : String(e);
}

export function TemplatesTab({
  workspaceId,
  document,
  onApplied,
}: {
  workspaceId: string;
  document: IniDocument;
  /** Called after a template apply or undo actually writes the file — the caller
   * re-fetches via `ini_read` rather than this component tracking document state itself. */
  onApplied: () => void;
}) {
  const [templates, setTemplates] = useState<IniTemplate[]>([]);
  const [name, setName] = useState("");
  const [error, setError] = useState<string | null>(null);
  const [undo, setUndo] = useState<{ raw: string; mtimeMs: number } | null>(null);

  const refresh = useCallback(async () => {
    try {
      setTemplates(await iniTemplateList());
    } catch (e) {
      setError(describe(e));
    }
  }, []);

  useEffect(() => {
    void refresh();
  }, [refresh]);

  const save = useCallback(async () => {
    if (!name.trim()) return;
    setError(null);
    try {
      await iniTemplateSave(workspaceId, name.trim());
      setName("");
      await refresh();
    } catch (e) {
      setError(describe(e));
    }
  }, [workspaceId, name, refresh]);

  const apply = useCallback(
    async (templateName: string) => {
      setError(null);
      const before = { raw: document.raw, mtimeMs: document.mtimeMs };
      try {
        await iniTemplateApply(workspaceId, templateName);
        setUndo(before);
        onApplied();
      } catch (e) {
        setError(describe(e));
      }
    },
    [workspaceId, document, onApplied],
  );

  const undoApply = useCallback(async () => {
    if (!undo) return;
    setError(null);
    try {
      await iniWriteRaw(workspaceId, undo.raw, document.mtimeMs);
      onApplied();
      setUndo(null);
    } catch (e) {
      setError(describe(e));
    }
  }, [workspaceId, undo, document.mtimeMs, onApplied]);

  return (
    <div className="flex h-full flex-col overflow-y-auto">
      <div className="flex gap-2 border-b border-neutral-200 px-4 py-2 dark:border-neutral-800">
        <input
          value={name}
          onChange={(e) => setName(e.target.value)}
          placeholder="Template name…"
          className="flex-1 rounded border border-neutral-300 bg-transparent px-2 py-1 text-xs dark:border-neutral-700"
        />
        <button
          type="button"
          onClick={() => void save()}
          disabled={!name.trim()}
          className="rounded bg-neutral-900 px-2.5 py-1 text-xs font-medium text-neutral-50 disabled:opacity-40 dark:bg-neutral-100 dark:text-neutral-900"
        >
          Save current as template
        </button>
      </div>

      {error && <p className="px-4 py-1.5 text-xs text-red-500">{error}</p>}

      {undo && (
        <div className="flex items-center justify-between gap-2 border-b border-amber-300 bg-amber-50 px-4 py-2 text-xs dark:border-amber-800 dark:bg-amber-950">
          <span>Template applied.</span>
          <button type="button" onClick={() => void undoApply()} className="rounded border border-amber-400 px-2 py-0.5 text-[11px] hover:bg-amber-100 dark:hover:bg-amber-900">
            Undo
          </button>
        </div>
      )}

      <div className="px-4 py-2">
        {templates.length === 0 && <p className="text-xs text-neutral-400">No saved templates yet.</p>}
        {templates.map((t) => (
          <div key={t.name} className="flex items-center justify-between gap-2 border-b border-neutral-100 py-2 text-xs last:border-0 dark:border-neutral-900">
            <div>
              <p className="font-medium">{t.name}</p>
              <p className="text-neutral-400">{new Date(t.createdAt).toLocaleString()}</p>
            </div>
            <button
              type="button"
              onClick={() => void apply(t.name)}
              className="rounded border border-neutral-300 px-2 py-0.5 text-[11px] hover:bg-neutral-100 dark:border-neutral-700 dark:hover:bg-neutral-800"
            >
              Apply
            </button>
          </div>
        ))}
      </div>
    </div>
  );
}
