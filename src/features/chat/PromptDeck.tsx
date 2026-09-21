import { useState, type KeyboardEvent } from "react";

interface PromptDeckProps {
  running: boolean;
  /** Previously submitted prompts, oldest first — `ArrowUp` in an empty box cycles through
   * them (per-project history, `ROADMAP.md` M3: "per-project history"). */
  history: string[];
  onSend: (text: string) => void;
  onStop: () => void;
}

export function PromptDeck({ running, history, onSend, onStop }: PromptDeckProps) {
  const [text, setText] = useState("");
  const [historyCursor, setHistoryCursor] = useState<number | null>(null);

  const submit = () => {
    const trimmed = text.trim();
    if (!trimmed || running) return;
    onSend(trimmed);
    setText("");
    setHistoryCursor(null);
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
    }
  };

  return (
    <div className="border-t border-neutral-200 p-3 dark:border-neutral-800">
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
        className="w-full resize-none rounded border border-neutral-300 bg-transparent p-2 text-sm disabled:opacity-60 dark:border-neutral-700"
      />
      <div className="mt-2 flex justify-end gap-2">
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
