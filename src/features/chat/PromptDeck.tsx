import { open as openDialog } from "@tauri-apps/plugin-dialog";
import { useState, type KeyboardEvent } from "react";
import { attachmentAdd } from "../../lib/ipc";

interface PromptDeckProps {
  workspaceId: string;
  running: boolean;
  /** Previously submitted prompts, oldest first — `ArrowUp` in an empty box cycles through
   * them (per-project history, `ROADMAP.md` M3: "per-project history"). */
  history: string[];
  onSend: (text: string, attachments: string[]) => void;
  onStop: () => void;
}

/** `FR-CHAT-9`: a workspace-relative attachment path plus a short display name for the chip. */
interface Attachment {
  relativePath: string;
  name: string;
}

export function PromptDeck({ workspaceId, running, history, onSend, onStop }: PromptDeckProps) {
  const [text, setText] = useState("");
  const [historyCursor, setHistoryCursor] = useState<number | null>(null);
  const [attachments, setAttachments] = useState<Attachment[]>([]);
  const [attaching, setAttaching] = useState(false);

  const submit = () => {
    const trimmed = text.trim();
    if (!trimmed || running) return;
    onSend(
      trimmed,
      attachments.map((a) => a.relativePath),
    );
    setText("");
    setHistoryCursor(null);
    setAttachments([]);
  };

  const addAttachment = async () => {
    const path = await openDialog({ multiple: false });
    if (typeof path !== "string") return;
    setAttaching(true);
    try {
      const relativePath = await attachmentAdd(workspaceId, path);
      const name = relativePath.split("/").pop() ?? relativePath;
      setAttachments((prev) => [...prev, { relativePath, name }]);
    } finally {
      setAttaching(false);
    }
  };

  const onKeyDown = (e: KeyboardEvent<HTMLTextAreaElement>) => {
    if ((e.metaKey || e.ctrlKey) && e.key === "Enter") {
      e.preventDefault();
      submit();
      return;
    }
    if (e.key === "ArrowUp" && text === "" && history.length > 0) {
      e.preventDefault();
      const next = historyCursor === null ? history.length - 1 : Math.max(0, historyCursor - 1);
      setHistoryCursor(next);
      setText(history[next] ?? "");
      return;
    }
    if (e.key === "ArrowDown" && historyCursor !== null) {
      e.preventDefault();
      const next = historyCursor + 1;
      if (next >= history.length) {
        setHistoryCursor(null);
        setText("");
      } else {
        setHistoryCursor(next);
        setText(history[next] ?? "");
      }
    }
  };

  return (
    <div className="border-t border-neutral-200 p-3 dark:border-neutral-800">
      {attachments.length > 0 && (
        <div className="mb-2 flex flex-wrap gap-1.5">
          {attachments.map((a) => (
            <span key={a.relativePath} className="flex items-center gap-1 rounded-full bg-neutral-100 px-2 py-0.5 text-[11px] dark:bg-neutral-800">
              {a.name}
              <button
                type="button"
                onClick={() => setAttachments((prev) => prev.filter((x) => x.relativePath !== a.relativePath))}
                aria-label={`Remove ${a.name}`}
                className="text-neutral-500 hover:text-neutral-800 dark:text-neutral-400 dark:hover:text-neutral-100"
              >
                ×
              </button>
            </span>
          ))}
        </div>
      )}
      <textarea
        value={text}
        onChange={(e) => {
          setText(e.target.value);
          setHistoryCursor(null);
        }}
        onKeyDown={onKeyDown}
        disabled={running}
        rows={3}
        placeholder={running ? "Claude is working…" : "Describe what you want (Cmd/Ctrl+Enter to send)"}
        title={running ? "Disabled while a turn is in flight" : undefined}
        aria-label="Prompt"
        aria-disabled={running}
        className="w-full resize-none rounded border border-neutral-300 bg-transparent p-2 text-sm disabled:opacity-60 dark:border-neutral-700"
      />
      <div className="mt-2 flex items-center justify-between gap-2">
        <button
          type="button"
          onClick={() => void addAttachment()}
          disabled={running || attaching}
          title="Attach a datasheet, photo, or log file"
          className="rounded border border-neutral-300 px-2.5 py-1 text-xs font-medium hover:bg-neutral-100 disabled:opacity-40 dark:border-neutral-700 dark:hover:bg-neutral-800"
        >
          {attaching ? "Attaching…" : "Attach"}
        </button>
        {running ? (
          <button
            type="button"
            onClick={onStop}
            className="rounded border border-red-400 px-3 py-1 text-xs font-medium text-red-600 hover:bg-red-50 dark:hover:bg-red-950"
          >
            Stop
          </button>
        ) : (
          <button
            type="button"
            onClick={submit}
            disabled={!text.trim()}
            className="rounded bg-neutral-900 px-3 py-1 text-xs font-medium text-neutral-50 disabled:opacity-40 dark:bg-neutral-100 dark:text-neutral-900"
          >
            Send
          </button>
        )}
      </div>
    </div>
  );
}
