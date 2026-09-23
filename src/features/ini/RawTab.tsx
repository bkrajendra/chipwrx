// `FR-INI-1`: the Raw tab — direct text editing of `platformio.ini`, two-way synced with
// the Form tab through the same `IniDocument`. A plain `<textarea>` rather than CodeMirror:
// this is the app's one documented, first-class ini text editor (`FR-INI-1`), distinct from
// "no code editor" (that constraint is about firmware *source* files — `CLAUDE.md`).
//
// Known limitation: browsers normalize a `<textarea>`'s line endings to `\n` on read, so a
// file using `\r\n` loses that distinction if edited here (`SPEC.md` §8 open question 34)
// — Form-tab edits go through `ini_apply`'s surgical patcher instead and are unaffected.

import { useEffect, useState } from "react";
import type { IniDocument } from "../../lib/bindings";

export function RawTab({ document, onSave }: { document: IniDocument; onSave: (raw: string) => void }) {
  const [text, setText] = useState(document.raw);
  const [dirty, setDirty] = useState(false);

  useEffect(() => {
    setText(document.raw);
    setDirty(false);
  }, [document.raw]);

  return (
    <div className="flex h-full flex-col">
      <textarea
        value={text}
        onChange={(e) => {
          setText(e.target.value);
          setDirty(true);
        }}
        spellCheck={false}
        className="min-h-0 flex-1 resize-none bg-transparent p-4 font-mono text-xs outline-none"
      />
      <div className="flex justify-end gap-2 border-t border-neutral-200 px-4 py-2 dark:border-neutral-800">
        <button
          type="button"
          disabled={!dirty}
          onClick={() => {
            setText(document.raw);
            setDirty(false);
          }}
          className="rounded border border-neutral-300 px-2.5 py-1 text-xs font-medium hover:bg-neutral-100 disabled:opacity-40 dark:border-neutral-700 dark:hover:bg-neutral-800"
        >
          Revert
        </button>
        <button
          type="button"
          disabled={!dirty}
          onClick={() => {
            onSave(text);
            setDirty(false);
          }}
          className="rounded bg-neutral-900 px-2.5 py-1 text-xs font-medium text-neutral-50 disabled:opacity-40 dark:bg-neutral-100 dark:text-neutral-900"
        >
          Save
        </button>
      </div>
    </div>
  );
}
