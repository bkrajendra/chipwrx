// `Cmd/Ctrl+,` (`FR-UI-8`) opens this. Screen inventory: "Toolchain, Claude policy &
// model, PlatformIO global settings, Appearance, Editor, Advanced" — a modal rather than a
// canvas tab since it's workspace-independent (`GlobalSettings` has no per-project scope).

import { relaunch } from "@tauri-apps/plugin-process";
import { check as checkForUpdate, type Update } from "@tauri-apps/plugin-updater";
import { useEffect, useState } from "react";
import { useEscapeToClose } from "./useEscapeToClose";
import type { AppError, AppInfo, GlobalSettings, ThemeSetting, ToolchainTool } from "../../lib/bindings";
import { renderAppError } from "../../lib/errors";
import { appInfoGet, secretsClearApiKey, secretsHasApiKey, secretsSetApiKey, settingsGetGlobal, settingsSetGlobal, toolchainSetPath } from "../../lib/ipc";
import { patchSection } from "../../lib/settings";
import { strings } from "../../lib/strings";
import { PioSettingsTab } from "../ini/PioSettingsTab";
import { useTheme } from "./useTheme";

function isAppError(e: unknown): e is AppError {
  return typeof e === "object" && e !== null && "code" in e;
}
function describe(e: unknown): string {
  return isAppError(e) ? renderAppError(e).message : String(e);
}

type SettingsTab = "toolchain" | "claude" | "pio" | "appearance" | "editor" | "advanced" | "about";

const inputClass = "w-full rounded border border-neutral-300 bg-white px-2 py-1 text-xs text-neutral-900 dark:border-neutral-700 dark:bg-neutral-900 dark:text-neutral-100";

function ToolchainSection({ settings, onChanged }: { settings: GlobalSettings; onChanged: (s: GlobalSettings) => void }) {
  const [error, setError] = useState<string | null>(null);

  const setPath = async (tool: ToolchainTool, path: string) => {
    setError(null);
    try {
      await toolchainSetPath(tool, path);
      onChanged(await settingsGetGlobal());
    } catch (e) {
      setError(describe(e));
    }
  };

  const setPythonPath = async (path: string) => {
    onChanged(await settingsSetGlobal(patchSection(settings, "toolchain", { pythonPath: path || null })));
  };

  const setAutoProbe = async (value: boolean) => {
    onChanged(await settingsSetGlobal(patchSection(settings, "toolchain", { autoProbeOnFocus: value })));
  };

  return (
    <div className="space-y-3 text-xs">
      {error && <p className="text-red-500">{error}</p>}
      <label className="block">
        <span className="mb-1 block font-medium">Claude CLI path (empty = auto-resolve)</span>
        <input defaultValue={settings.toolchain.claudePath ?? ""} onBlur={(e) => void setPath("claude", e.target.value)} className={inputClass} />
      </label>
      <label className="block">
        <span className="mb-1 block font-medium">PlatformIO CLI path (empty = auto-resolve)</span>
        <input defaultValue={settings.toolchain.pioPath ?? ""} onBlur={(e) => void setPath("pio", e.target.value)} className={inputClass} />
      </label>
      <label className="block">
        <span className="mb-1 block font-medium">Python path (empty = auto-resolve)</span>
        <input defaultValue={settings.toolchain.pythonPath ?? ""} onBlur={(e) => void setPythonPath(e.target.value)} className={inputClass} />
      </label>
      <label className="flex items-center gap-2">
        <input type="checkbox" checked={settings.toolchain.autoProbeOnFocus} onChange={(e) => void setAutoProbe(e.target.checked)} />
        Re-probe Doctor automatically when the window regains focus
      </label>
    </div>
  );
}

/** `NFR-S1`: the optional `ANTHROPIC_API_KEY` — lives in the OS keychain, never in
 * `settings.json`. Only "set" vs. "not set" is ever fetched; the value itself is
 * write-only from this screen. */
function ApiKeySection() {
  const [hasKey, setHasKey] = useState<boolean | null>(null);
  const [draft, setDraft] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const refresh = () => void secretsHasApiKey().then(setHasKey);
  useEffect(refresh, []);

  const save = async () => {
    if (!draft.trim()) return;
    setBusy(true);
    setError(null);
    try {
      await secretsSetApiKey(draft.trim());
      setDraft("");
      refresh();
    } catch (e) {
      setError(describe(e));
    } finally {
      setBusy(false);
    }
  };

  const clear = async () => {
    setBusy(true);
    setError(null);
    try {
      await secretsClearApiKey();
      refresh();
    } catch (e) {
      setError(describe(e));
    } finally {
      setBusy(false);
    }
  };

  return (
    <div className="space-y-2 border-t border-neutral-200 pt-3 dark:border-neutral-800">
      <span className="block font-medium">Anthropic API key (optional)</span>
      <p className="text-neutral-500 dark:text-neutral-400">
        Stored in the OS keychain, never in a config file. When set, it's used for turns instead of subscription auth.
      </p>
      {error && <p className="text-red-500">{error}</p>}
      <p className="text-neutral-600 dark:text-neutral-400">{hasKey === null ? "Checking…" : hasKey ? "A key is set." : "No key set."}</p>
      <div className="flex gap-2">
        <input
          type="password"
          placeholder="sk-ant-…"
          value={draft}
          onChange={(e) => setDraft(e.target.value)}
          className={inputClass}
          autoComplete="off"
        />
        <button
          type="button"
          onClick={() => void save()}
          disabled={busy || !draft.trim()}
          className="shrink-0 rounded border border-neutral-300 px-2.5 py-1 font-medium hover:bg-neutral-100 disabled:opacity-40 dark:border-neutral-700 dark:hover:bg-neutral-800"
        >
          Save
        </button>
        {hasKey && (
          <button
            type="button"
            onClick={() => void clear()}
            disabled={busy}
            className="shrink-0 rounded border border-neutral-300 px-2.5 py-1 font-medium hover:bg-neutral-100 disabled:opacity-40 dark:border-neutral-700 dark:hover:bg-neutral-800"
          >
            Clear
          </button>
        )}
      </div>
    </div>
  );
}

function ClaudeSection({ settings, onChanged }: { settings: GlobalSettings; onChanged: (s: GlobalSettings) => void }) {
  const set = async (overrides: Partial<GlobalSettings["claude"]>) => {
    onChanged(await settingsSetGlobal(patchSection(settings, "claude", overrides)));
  };
  return (
    <div className="space-y-3 text-xs">
      <label className="block">
        <span className="mb-1 block font-medium">Model</span>
        <input defaultValue={settings.claude.model} onBlur={(e) => void set({ model: e.target.value })} className={inputClass} />
      </label>
      <label className="flex items-center gap-2">
        <input type="checkbox" checked={settings.claude.showThinking} onChange={(e) => void set({ showThinking: e.target.checked })} />
        Show thinking blocks
      </label>
      <label className="flex items-center gap-2">
        <input type="checkbox" checked={settings.claude.showCostEstimate} onChange={(e) => void set({ showCostEstimate: e.target.checked })} />
        Show cost estimate
      </label>
      <ApiKeySection />
    </div>
  );
}

function AppearanceSection() {
  const { theme, setTheme } = useTheme();
  return (
    <div className="space-y-2 text-xs">
      <span className="mb-1 block font-medium">{strings.settings.theme}</span>
      <div className="flex gap-2">
        {(["system", "light", "dark"] as ThemeSetting[]).map((t) => (
          <button
            key={t}
            type="button"
            onClick={() => void setTheme(t)}
            aria-pressed={theme === t}
            className={`rounded border px-2.5 py-1 ${
              theme === t
                ? "border-neutral-900 bg-neutral-900 text-neutral-50 dark:border-neutral-100 dark:bg-neutral-100 dark:text-neutral-900"
                : "border-neutral-300 dark:border-neutral-700"
            }`}
          >
            {t === "system" ? strings.settings.themeSystem : t === "light" ? strings.settings.themeLight : strings.settings.themeDark}
          </button>
        ))}
      </div>
    </div>
  );
}

function EditorSection({ settings, onChanged }: { settings: GlobalSettings; onChanged: (s: GlobalSettings) => void }) {
  const set = async (overrides: Partial<GlobalSettings["editor"]>) => {
    onChanged(await settingsSetGlobal(patchSection(settings, "editor", overrides)));
  };
  return (
    <div className="space-y-3 text-xs">
      <label className="block">
        <span className="mb-1 block font-medium">Editor command (empty = auto-detect $VISUAL/$EDITOR/known editors)</span>
        <input defaultValue={settings.editor.command ?? ""} onBlur={(e) => void set({ command: e.target.value || null })} className={inputClass} />
      </label>
      <label className="block">
        <span className="mb-1 block font-medium">"Go to line" argument template</span>
        <input defaultValue={settings.editor.gotoLineArgTemplate} onBlur={(e) => void set({ gotoLineArgTemplate: e.target.value })} className={inputClass} />
      </label>
    </div>
  );
}

function AdvancedSection({ settings, onChanged }: { settings: GlobalSettings; onChanged: (s: GlobalSettings) => void }) {
  const set = async (overrides: Partial<GlobalSettings["advanced"]>) => {
    onChanged(await settingsSetGlobal(patchSection(settings, "advanced", overrides)));
  };
  return (
    <div className="space-y-3 text-xs">
      <label className="flex items-center gap-2">
        <input type="checkbox" checked={settings.advanced.keepProcessLogs} onChange={(e) => void set({ keepProcessLogs: e.target.checked })} />
        Keep process logs
      </label>
      <p className="text-neutral-500 dark:text-neutral-400">
        Unrestricted permission policy: {settings.advanced.allowUnrestrictedPolicy ? "allowed" : "reset to Guarded on the last app update"}.
      </p>
    </div>
  );
}

type UpdateStage = "idle" | "checking" | "up-to-date" | "available" | "installing" | "ready" | "failed";

/** `NFR-D2`/`NFR-D3`: version + git SHA, and the opt-out update check. `check()` talks to
 * the signed manifest at the endpoint in `tauri.conf.json`'s `plugins.updater` — it fails
 * (rejects) in `tauri dev`/unsigned local builds with no real release to compare against,
 * which is surfaced as the ordinary "failed" state rather than thrown at the caller. */
function AboutSection({ settings, onChanged }: { settings: GlobalSettings; onChanged: (s: GlobalSettings) => void }) {
  const [info, setInfo] = useState<AppInfo | null>(null);
  const [stage, setStage] = useState<UpdateStage>("idle");
  const [update, setUpdate] = useState<Update | null>(null);

  useEffect(() => {
    void appInfoGet().then(setInfo);
  }, []);

  const setAutoCheck = async (value: boolean) => {
    onChanged(await settingsSetGlobal(patchSection(settings, "updates", { checkForUpdates: value })));
  };

  const runCheck = async () => {
    setStage("checking");
    try {
      const found = await checkForUpdate();
      if (found?.available) {
        setUpdate(found);
        setStage("available");
      } else {
        setStage("up-to-date");
      }
    } catch {
      setStage("failed");
    }
  };

  const install = async () => {
    if (!update) return;
    setStage("installing");
    try {
      await update.downloadAndInstall();
      setStage("ready");
    } catch {
      setStage("failed");
    }
  };

  return (
    <div className="space-y-4 text-xs">
      <div className="space-y-1">
        <p className="font-medium">Vibe Hardware {info?.version ?? "…"}</p>
        <p className="text-neutral-500 dark:text-neutral-400">Build {info?.gitSha ?? "…"}</p>
      </div>
      <label className="flex items-center gap-2">
        <input type="checkbox" checked={settings.updates.checkForUpdates} onChange={(e) => void setAutoCheck(e.target.checked)} />
        {strings.about.checkForUpdates}
      </label>
      <div className="space-y-2 border-t border-neutral-200 pt-3 dark:border-neutral-800">
        {stage === "idle" && (
          <button type="button" onClick={() => void runCheck()} className="rounded border border-neutral-300 px-2.5 py-1 font-medium hover:bg-neutral-100 dark:border-neutral-700 dark:hover:bg-neutral-800">
            {strings.about.checkNow}
          </button>
        )}
        {stage === "checking" && <p className="text-neutral-500 dark:text-neutral-400">{strings.about.checking}</p>}
        {stage === "up-to-date" && <p className="text-neutral-500 dark:text-neutral-400">{strings.about.upToDate}</p>}
        {stage === "failed" && <p className="text-red-500">{strings.about.checkFailed}</p>}
        {stage === "available" && update && (
          <div className="space-y-2">
            <p>
              {strings.about.updateAvailable}: {update.version}
            </p>
            <button type="button" onClick={() => void install()} className="rounded border border-neutral-300 px-2.5 py-1 font-medium hover:bg-neutral-100 dark:border-neutral-700 dark:hover:bg-neutral-800">
              {strings.about.downloadAndInstall}
            </button>
          </div>
        )}
        {stage === "installing" && <p className="text-neutral-500 dark:text-neutral-400">{strings.about.installing}</p>}
        {stage === "ready" && (
          <div className="space-y-2">
            <p>{strings.about.restartToFinish}</p>
            <button type="button" onClick={() => void relaunch()} className="rounded border border-neutral-300 px-2.5 py-1 font-medium hover:bg-neutral-100 dark:border-neutral-700 dark:hover:bg-neutral-800">
              {strings.about.restartNow}
            </button>
          </div>
        )}
      </div>
    </div>
  );
}

export function GlobalSettingsScreen({ open, onClose }: { open: boolean; onClose: () => void }) {
  const [settings, setSettings] = useState<GlobalSettings | null>(null);
  const [tab, setTab] = useState<SettingsTab>("toolchain");
  useEscapeToClose(open, onClose);

  useEffect(() => {
    if (open) void settingsGetGlobal().then(setSettings);
  }, [open]);

  if (!open) return null;

  const tabs: { id: SettingsTab; label: string }[] = [
    { id: "toolchain", label: strings.settings.toolchain },
    { id: "claude", label: strings.settings.claude },
    { id: "pio", label: strings.settings.pio },
    { id: "appearance", label: strings.settings.appearance },
    { id: "editor", label: strings.settings.editor },
    { id: "advanced", label: strings.settings.advanced },
    { id: "about", label: strings.settings.about },
  ];

  return (
    <div role="dialog" aria-modal="true" aria-label={strings.settings.title} className="fixed inset-0 z-50 flex items-center justify-center bg-black/30" onClick={onClose}>
      <div
        className="flex h-[70vh] w-full max-w-2xl overflow-hidden rounded-lg border border-neutral-200 bg-white shadow-xl dark:border-neutral-700 dark:bg-neutral-900"
        onClick={(e) => e.stopPropagation()}
      >
        <nav className="w-40 shrink-0 border-r border-neutral-200 py-2 dark:border-neutral-800">
          {tabs.map((t) => (
            <button
              key={t.id}
              type="button"
              onClick={() => setTab(t.id)}
              className={`block w-full px-3 py-1.5 text-left text-xs ${
                tab === t.id ? "bg-neutral-100 font-medium dark:bg-neutral-800" : "text-neutral-600 hover:bg-neutral-50 dark:text-neutral-400 dark:hover:bg-neutral-800/50"
              }`}
            >
              {t.label}
            </button>
          ))}
        </nav>
        <div className="flex min-w-0 flex-1 flex-col">
          <header className="flex items-center justify-between border-b border-neutral-200 px-4 py-2 dark:border-neutral-800">
            <span className="text-sm font-medium">{strings.settings.title}</span>
            <button type="button" onClick={onClose} className="text-xs text-neutral-500 hover:underline dark:text-neutral-400">
              {strings.settings.close}
            </button>
          </header>
          <div className="min-h-0 flex-1 overflow-y-auto p-4">
            {!settings && <p className="text-xs text-neutral-500 dark:text-neutral-400">Loading…</p>}
            {settings && tab === "toolchain" && <ToolchainSection settings={settings} onChanged={setSettings} />}
            {settings && tab === "claude" && <ClaudeSection settings={settings} onChanged={setSettings} />}
            {tab === "pio" && <PioSettingsTab />}
            {tab === "appearance" && <AppearanceSection />}
            {settings && tab === "editor" && <EditorSection settings={settings} onChanged={setSettings} />}
            {settings && tab === "advanced" && <AdvancedSection settings={settings} onChanged={setSettings} />}
            {settings && tab === "about" && <AboutSection settings={settings} onChanged={setSettings} />}
          </div>
        </div>
      </div>
    </div>
  );
}
