// Shared helpers for `GlobalSettingsPatch` (`IPC-CONTRACT.md` §9) — every field is
// `T | null`, not `T?`; `null` means "leave unchanged." `emptyPatch()` gives every caller a
// safe base to spread a single section onto, so `settings_set_global` never accidentally
// touches a section the caller didn't mean to change.

import type { GlobalSettings, GlobalSettingsPatch } from "./bindings";

export function emptyPatch(): GlobalSettingsPatch {
  return {
    toolchain: null,
    claude: null,
    pipeline: null,
    monitor: null,
    logs: null,
    editor: null,
    appearance: null,
    network: null,
    advanced: null,
    onboardingCompleted: null,
  };
}

/** The object-valued top-level sections — `schemaVersion`/`lastAppVersion`/
 * `onboardingCompleted` are primitives and aren't patchable through this helper. */
type PatchableSection = Exclude<keyof GlobalSettingsPatch, "onboardingCompleted">;

/** Patches exactly one top-level section, spreading its current value under `overrides`. */
export function patchSection<K extends PatchableSection>(
  settings: GlobalSettings,
  section: K,
  overrides: Partial<GlobalSettings[K]>,
): GlobalSettingsPatch {
  return { ...emptyPatch(), [section]: { ...settings[section], ...overrides } };
}
