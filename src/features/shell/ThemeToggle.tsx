// `FR-UI-7`: light/dark themes. `GlobalSettingsScreen`'s Appearance tab has the full
// three-way (system/light/dark) chooser, but that's a couple of clicks away — this is a
// one-click cycle button for the header, next to the other always-visible shell controls.

import { useTheme } from "./useTheme";

const ICON = { system: "🖥", light: "☀", dark: "🌙" } as const;
const NEXT = { system: "light", light: "dark", dark: "system" } as const;

export function ThemeToggle() {
  const { theme, setTheme } = useTheme();

  return (
    <button
      type="button"
      onClick={() => void setTheme(NEXT[theme])}
      title={`Theme: ${theme} (click to switch to ${NEXT[theme]})`}
      className="rounded border border-neutral-300 px-2.5 py-1 text-xs font-medium hover:bg-neutral-100 dark:border-neutral-700 dark:hover:bg-neutral-800"
    >
      {ICON[theme]}
    </button>
  );
}
