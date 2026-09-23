// `FR-INI-1..5`: loads the option schema (once) and the current `platformio.ini` document,
// and applies edits through `ini_apply`/`ini_write_raw` — both return `AppError::IniChangedOnDisk`
// when the file was modified outside the app since it was last read, which surfaces here as
// `conflict` so the UI can offer Reload/Overwrite (`IPC-CONTRACT.md` §7) instead of silently
// clobbering an edit made in an external editor.

import { useCallback, useEffect, useRef, useState } from "react";
import type { AppError, IniDocument, IniEdit, IniOptionSchema } from "../../lib/bindings";
import { renderAppError } from "../../lib/errors";
import { iniApply, iniLint, iniRead, iniSchema, iniWriteRaw } from "../../lib/ipc";

function isAppError(e: unknown): e is AppError {
  return typeof e === "object" && e !== null && "code" in e;
}

function describe(e: unknown): string {
  return isAppError(e) ? renderAppError(e).message : String(e);
}

// The schema rarely changes within a session (it's cached per PIO version on the backend
// too) — fetched once per app load and shared across every panel instance.
let schemaCache: Promise<IniOptionSchema[]> | null = null;
function loadSchemaOnce(): Promise<IniOptionSchema[]> {
  if (!schemaCache) schemaCache = iniSchema();
  return schemaCache;
}

export function useIni(workspaceId: string) {
  const [schema, setSchema] = useState<IniOptionSchema[] | null>(null);
  const [document, setDocument] = useState<IniDocument | null>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [conflict, setConflict] = useState(false);
  const documentRef = useRef<IniDocument | null>(null);
  documentRef.current = document;

  const load = useCallback(async () => {
    setLoading(true);
    setError(null);
    try {
      const doc = await iniRead(workspaceId);
      setDocument(doc);
      setConflict(false);
    } catch (e) {
      setError(describe(e));
    } finally {
      setLoading(false);
    }
  }, [workspaceId]);

  useEffect(() => {
    void loadSchemaOnce().then(setSchema);
  }, []);

  useEffect(() => {
    void load();
  }, [load]);

  const applyEdits = useCallback(
    async (edits: IniEdit[]) => {
      const current = documentRef.current;
      if (!current) return;
      setError(null);
      try {
        const doc = await iniApply(workspaceId, edits, current.mtimeMs);
        setDocument(doc);
        setConflict(false);
      } catch (e) {
        if (isAppError(e) && e.code === "INI_CHANGED_ON_DISK") {
          setConflict(true);
        } else {
          setError(describe(e));
        }
      }
    },
    [workspaceId],
  );

  const writeRaw = useCallback(
    async (raw: string) => {
      const current = documentRef.current;
      if (!current) return;
      setError(null);
      try {
        const doc = await iniWriteRaw(workspaceId, raw, current.mtimeMs);
        setDocument(doc);
        setConflict(false);
      } catch (e) {
        if (isAppError(e) && e.code === "INI_CHANGED_ON_DISK") {
          setConflict(true);
        } else {
          setError(describe(e));
        }
      }
    },
    [workspaceId],
  );

  const lint = useCallback(() => iniLint(workspaceId), [workspaceId]);

  return { schema, document, loading, error, conflict, reload: load, applyEdits, writeRaw, lint };
}
