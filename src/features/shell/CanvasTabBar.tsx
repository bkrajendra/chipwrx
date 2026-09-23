// `FR-UI-3`: "Main canvas is tabbed: Chat (default), Logs, Changes, Problems, Monitor,
// Project Settings."

import { strings } from "../../lib/strings";

export type CanvasTab = "chat" | "logs" | "changes" | "problems" | "monitor" | "project-settings";

const TAB_ORDER: CanvasTab[] = ["chat", "logs", "changes", "problems", "monitor", "project-settings"];

const TAB_LABEL: Record<CanvasTab, string> = {
  chat: strings.tabs.chat,
  logs: strings.tabs.logs,
  changes: strings.tabs.changes,
  problems: strings.tabs.problems,
  monitor: strings.tabs.monitor,
  "project-settings": strings.tabs.projectSettings,
};

export function CanvasTabBar({
  active,
  onChange,
  problemCount,
}: {
  active: CanvasTab;
  onChange: (tab: CanvasTab) => void;
  problemCount: number;
}) {
  return (
    <div role="tablist" aria-label="Workspace" className="flex gap-1 border-b border-neutral-200 px-4 pt-2 dark:border-neutral-800">
      {TAB_ORDER.map((t) => (
        <button
          key={t}
          type="button"
          role="tab"
          aria-selected={active === t}
          onClick={() => onChange(t)}
          className={`rounded-t px-3 py-1.5 text-xs font-medium ${active === t ? "bg-neutral-100 dark:bg-neutral-800" : "text-neutral-500 hover:text-neutral-800 dark:text-neutral-400 dark:hover:text-neutral-200"}`}
        >
          {TAB_LABEL[t]}
          {t === "problems" && problemCount > 0 ? ` (${problemCount})` : ""}
        </button>
      ))}
    </div>
  );
}
