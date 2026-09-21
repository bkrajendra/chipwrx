// Read-only side-by-side diff (`FR-SAFE-2`) — `ARCHITECTURE.md`'s stack choice: "`diff` +
// a read-only CodeMirror 6 instance... A read-only viewer is not 'a code editor'." Both
// sides are hard-locked read-only (`EditorState.readOnly` + `EditorView.editable`); this
// app has no code editor anywhere, and this component must never become one.

import { basicSetup, EditorView } from "codemirror";
import { cpp } from "@codemirror/lang-cpp";
import { EditorState, type Extension } from "@codemirror/state";
import { MergeView } from "@codemirror/merge";
import { useEffect, useRef } from "react";

const CPP_EXT_RE = /\.(cpp|cc|cxx|c\+\+|h|hh|hpp|hxx|c|ino)$/i;

function extensionsFor(filename: string): Extension[] {
  const base: Extension[] = [basicSetup, EditorState.readOnly.of(true), EditorView.editable.of(false)];
  return CPP_EXT_RE.test(filename) ? [...base, cpp()] : base;
}

export function DiffViewer({ before, after, filename }: { before: string; after: string; filename: string }) {
  const containerRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    if (!containerRef.current) return;
    const extensions = extensionsFor(filename);
    const view = new MergeView({
      a: { doc: before, extensions },
      b: { doc: after, extensions },
      parent: containerRef.current,
      highlightChanges: true,
      gutter: true,
      // No `revertControls` — reverting goes through this app's own snapshot-backed
      // commands (`changesRevertFile`/`changesRevertTurn`), not CodeMirror's own
      // chunk-accept buttons.
    });
    return () => view.destroy();
  }, [before, after, filename]);

  return <div ref={containerRef} className="cm-diff-viewer overflow-auto rounded border border-neutral-200 text-xs dark:border-neutral-800" />;
}
