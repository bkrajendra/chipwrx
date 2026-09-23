// `FR-UI-7`: "Light and dark themes, following the OS by default." Backed by
// `GlobalSettings.appearance.theme` (`system | light | dark`). The single source of truth
// for whether the app is dark is the `.dark` class on `<html>` — Tailwind's `dark:` variant
// (`@custom-variant dark` in `index.css`) and every native-control color rule (the
// `select`/`option` backstop, also in `index.css`) both key off that same class, so there is
// exactly one thing to keep in sync, not two mechanisms that can disagree.

import { useCallback, useEffect, useState } from "react";
import type { ThemeSetting } from "../../lib/bindings";
import { settingsGetGlobal, settingsSetGlobal } from "../../lib/ipc";
import { patchSection } from "../../lib/settings";

function systemPrefersDark(): boolean {
  return window.matchMedia("(prefers-color-scheme: dark)").matches;
}

function resolveDark(theme: ThemeSetting): boolean {
  return theme === "dark" || (theme === "system" && systemPrefersDark());
}

function applyTheme(theme: ThemeSetting) {
  const dark = resolveDark(theme);
  document.documentElement.classList.toggle("dark", dark);
  document.documentElement.style.colorScheme = dark ? "dark" : "light";
}

export function useTheme() {
  const [theme, setThemeState] = useState<ThemeSetting>("system");

  useEffect(() => {
    let cancelled = false;
    void settingsGetGlobal().then((s) => {
      if (cancelled) return;
      setThemeState(s.appearance.theme);
      applyTheme(s.appearance.theme);
    });
    return () => {
      cancelled = true;
    };
  }, []);

  useEffect(() => {
    applyTheme(theme);
    if (theme !== "system") return;
    const mq = window.matchMedia("(prefers-color-scheme: dark)");
    const onChange = () => applyTheme("system");
    mq.addEventListener("change", onChange);
    return () => mq.removeEventListener("change", onChange);
  }, [theme]);

  const setTheme = useCallback(async (next: ThemeSetting) => {
    setThemeState(next);
    applyTheme(next);
    const settings = await settingsGetGlobal();
    await settingsSetGlobal(patchSection(settings, "appearance", { theme: next }));
  }, []);

  return { theme, setTheme };
}
