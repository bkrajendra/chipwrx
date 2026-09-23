// `FR-UI-1`: "Two-pane layout: sidebar ~25% (min 280px, resizable, collapsible), main
// canvas ~75%." The real shell M2–M7 deferred in favor of side panels — this replaces that
// interim pattern entirely.

import { useCallback, useMemo, useRef, useState } from "react";
import { WebviewWindow } from "@tauri-apps/api/webviewWindow";
import type { ProjectEntry } from "../../lib/bindings";
import { strings } from "../../lib/strings";
import { useChat } from "../chat/useChat";
import { ChatTab } from "../chat/ChatTab";
import { ChangesTab } from "../changes/ChangesTab";
import { useDeviceTelemetry } from "../devices/useDeviceTelemetry";
import { MonitorTab } from "../devices/MonitorTab";
import { useDoctor } from "../doctor/useDoctor";
import { DoctorScreen } from "../doctor/DoctorScreen";
import { IniPanel } from "../ini/IniPanel";
import { LogsTab } from "../pipeline/LogsTab";
import { ProblemsTab } from "../pipeline/ProblemsTab";
import { TestsTab } from "../pipeline/TestsTab";
import { PipelineStrip } from "../pipeline/PipelineStrip";
import { usePipeline } from "../pipeline/usePipeline";
import { PermissionPolicyControl } from "../settings-global/PermissionPolicyControl";
import { CanvasTabBar, type CanvasTab } from "./CanvasTabBar";
import { CommandPalette, type PaletteCommand } from "./CommandPalette";
import { GlobalSettingsScreen } from "./GlobalSettingsScreen";
import { Modal } from "./Modal";
import { Sidebar } from "./Sidebar";
import { ThemeToggle } from "./ThemeToggle";
import { useGlobalSettingsCaps } from "./useGlobalSettingsCaps";
import { useKeyboardShortcuts } from "./useKeyboardShortcuts";

const MIN_SIDEBAR_WIDTH = 280;
const DEFAULT_SIDEBAR_WIDTH = 300;

export function Workspace({ project, onBack }: { project: ProjectEntry; onBack: () => void }) {
  const chat = useChat(project.id);
  const caps = useGlobalSettingsCaps();
  const pipeline = usePipeline(project.id, caps.logMaxLines);
  const device = useDeviceTelemetry(project.id);
  const doctor = useDoctor();

  const [tab, setTab] = useState<CanvasTab>("chat");
  const [sidebarCollapsed, setSidebarCollapsed] = useState(false);
  const [sidebarWidth, setSidebarWidth] = useState(DEFAULT_SIDEBAR_WIDTH);
  const [paletteOpen, setPaletteOpen] = useState(false);
  const [settingsOpen, setSettingsOpen] = useState(false);
  const [doctorOpen, setDoctorOpen] = useState(false);
  const resizing = useRef(false);

  const onDividerMouseDown = useCallback(() => {
    resizing.current = true;
    const onMove = (e: MouseEvent) => {
      if (!resizing.current) return;
      setSidebarWidth(Math.max(MIN_SIDEBAR_WIDTH, Math.min(600, e.clientX)));
    };
    const onUp = () => {
      resizing.current = false;
      window.removeEventListener("mousemove", onMove);
      window.removeEventListener("mouseup", onUp);
    };
    window.addEventListener("mousemove", onMove);
    window.addEventListener("mouseup", onUp);
  }, []);

  const openInNewWindow = useCallback(() => {
    // `FR-UI-9`: one workspace per window, sharing the same Rust core (and so the same
    // global `PortBroker`) — the new window is just this same frontend bundle, told which
    // workspace to open via the URL instead of showing the launcher.
    const label = `workspace-${project.id}-${Date.now()}`;
    void new WebviewWindow(label, {
      url: `index.html?workspace=${encodeURIComponent(project.id)}`,
      title: project.name,
      width: 1100,
      height: 750,
    });
  }, [project.id, project.name]);

  const handleStop = useCallback(() => {
    // `Esc` closes whichever modal is on top before it falls through to actually stopping a
    // running build/turn — otherwise there's no way to dismiss Settings/Doctor with the
    // keyboard alone (`NFR-A1`: every action reachable by keyboard, no traps).
    if (paletteOpen) {
      setPaletteOpen(false);
      return;
    }
    if (settingsOpen) {
      setSettingsOpen(false);
      return;
    }
    if (doctorOpen) {
      setDoctorOpen(false);
      return;
    }
    if (pipeline.running) pipeline.stop();
    if (chat.live?.running) chat.stop();
  }, [paletteOpen, settingsOpen, doctorOpen, pipeline, chat]);

  useKeyboardShortcuts({
    onBuild: () => void pipeline.build(),
    onUpload: () => void pipeline.upload(),
    onMonitor: () => setTab("monitor"),
    onPalette: () => setPaletteOpen((v) => !v),
    onSettings: () => setSettingsOpen(true),
    onStop: handleStop,
  });

  const paletteCommands: PaletteCommand[] = useMemo(
    () => [
      { id: "build", label: `${strings.shortcuts.build}`, hint: "⌘B", run: () => void pipeline.build() },
      { id: "upload", label: `${strings.shortcuts.upload}`, hint: "⌘U", run: () => void pipeline.upload() },
      { id: "tab-chat", label: "Go to Chat", run: () => setTab("chat") },
      { id: "tab-logs", label: "Go to Logs", run: () => setTab("logs") },
      { id: "tab-changes", label: "Go to Changes", run: () => setTab("changes") },
      { id: "tab-problems", label: "Go to Problems", run: () => setTab("problems") },
      { id: "tab-tests", label: "Go to Tests", run: () => setTab("tests") },
      { id: "tab-monitor", label: "Go to Monitor", hint: "⌘M", run: () => setTab("monitor") },
      { id: "tab-project-settings", label: "Go to Project Settings", run: () => setTab("project-settings") },
      { id: "run-check", label: "Run static analysis (pio check)", run: () => void pipeline.check() },
      { id: "run-tests", label: "Run tests (pio test)", run: () => void pipeline.test() },
      { id: "new-session", label: strings.shell.newSession, run: () => void chat.newSession() },
      { id: "settings", label: "Open Global Settings", hint: "⌘,", run: () => setSettingsOpen(true) },
      { id: "doctor", label: "Open Doctor", run: () => setDoctorOpen(true) },
      { id: "new-window", label: strings.shell.openInNewWindow, run: openInNewWindow },
      { id: "back", label: `← ${strings.shell.back}`, run: onBack },
    ],
    [pipeline, chat, onBack, openInNewWindow],
  );

  const errorCount = pipeline.defects.filter((d) => d.severity === "error").length;

  return (
    <div className="flex h-full flex-col">
      <header className="flex items-center justify-between border-b border-neutral-200 px-4 py-2 dark:border-neutral-800">
        <div className="flex items-center gap-3">
          <button type="button" onClick={onBack} className="text-xs text-neutral-500 hover:underline dark:text-neutral-400">
            ← {strings.shell.back}
          </button>
          <PipelineStrip state={pipeline.state} onStop={pipeline.stop} onOpen={() => setTab("logs")} />
        </div>
        <div className="flex items-center gap-2">
          <ThemeToggle />
          <PermissionPolicyControl workspacePath={project.path} />
          <button
            type="button"
            onClick={openInNewWindow}
            title={strings.shell.openInNewWindow}
            className="rounded border border-neutral-300 px-2.5 py-1 text-xs font-medium hover:bg-neutral-100 dark:border-neutral-700 dark:hover:bg-neutral-800"
          >
            ⧉
          </button>
          <button
            type="button"
            onClick={() => void chat.newSession()}
            disabled={chat.live?.running}
            className="rounded border border-neutral-300 px-2.5 py-1 text-xs font-medium hover:bg-neutral-100 disabled:opacity-40 dark:border-neutral-700 dark:hover:bg-neutral-800"
          >
            {strings.shell.newSession}
          </button>
        </div>
      </header>

      <div className="flex flex-1 overflow-hidden">
        <div style={{ width: sidebarCollapsed ? undefined : sidebarWidth }} className="shrink-0 border-r border-neutral-200 dark:border-neutral-800">
          <Sidebar
            project={project}
            hasSession={chat.history.length > 0 || !!chat.live}
            chatRunning={chat.live?.running ?? false}
            device={device}
            pipelineState={pipeline.state}
            pipelineRunning={pipeline.running}
            size={pipeline.size}
            defects={pipeline.defects}
            onBuild={() => void pipeline.build()}
            onUpload={() => void pipeline.upload()}
            onRunTarget={(t) => void pipeline.runTarget(t)}
            onStop={pipeline.stop}
            onOpenMonitor={() => setTab("monitor")}
            onOpenProblems={() => setTab("problems")}
            doctorReport={doctor.report}
            onOpenDoctor={() => setDoctorOpen(true)}
            collapsed={sidebarCollapsed}
            onToggleCollapsed={() => setSidebarCollapsed((v) => !v)}
          />
        </div>

        {!sidebarCollapsed && (
          // eslint-disable-next-line jsx-a11y/no-noninteractive-element-interactions -- a resize divider has no meaningful keyboard equivalent to a mouse drag; the sidebar width is still reachable via collapse/expand
          <div
            onMouseDown={onDividerMouseDown}
            className="w-1 shrink-0 cursor-col-resize bg-transparent hover:bg-neutral-300 dark:hover:bg-neutral-700"
            role="separator"
            aria-orientation="vertical"
            aria-label="Resize sidebar"
          />
        )}

        <div className="flex min-w-0 flex-1 flex-col overflow-hidden">
          <CanvasTabBar active={tab} onChange={setTab} problemCount={pipeline.defects.length} />
          <div className="min-h-0 flex-1">
            {tab === "chat" && (
              <ChatTab
                workspaceId={project.id}
                chat={chat}
                onOpenChanges={() => setTab("changes")}
                onBuild={() => void pipeline.build()}
                onUpload={() => void pipeline.upload()}
                onOpenMonitor={() => setTab("monitor")}
                onOpenDoctor={() => setDoctorOpen(true)}
              />
            )}
            {tab === "logs" && <LogsTab lines={pipeline.lines} />}
            {tab === "changes" && <ChangesTab workspaceId={project.id} turns={chat.history} />}
            {tab === "problems" && (
              <ProblemsTab
                defects={pipeline.defects}
                lines={pipeline.lines}
                running={pipeline.running}
                onAskClaudeToFix={(prompt) => {
                  setTab("chat");
                  void chat.send(prompt);
                }}
                onRunCheck={() => void pipeline.check()}
                checking={pipeline.running}
              />
            )}
            {tab === "tests" && <TestsTab suites={pipeline.testSuites} running={pipeline.running} onRun={() => void pipeline.test()} />}
            {tab === "monitor" && (
              <MonitorTab
                workspaceId={project.id}
                hasSelectedPort={!!device.selectedPort}
                maxLines={caps.monitorMaxLines}
                maxBytes={caps.monitorMaxBytes}
                onSendToClaude={(prompt) => {
                  setTab("chat");
                  void chat.send(prompt);
                }}
              />
            )}
            {tab === "project-settings" && <IniPanel workspaceId={project.id} />}
          </div>
        </div>
      </div>

      {errorCount > 0 && tab !== "problems" && (
        <div className="border-t border-red-300 bg-red-50 px-4 py-1.5 text-xs text-red-700 dark:border-red-800 dark:bg-red-950 dark:text-red-400">
          {errorCount} build error{errorCount === 1 ? "" : "s"} —{" "}
          <button type="button" onClick={() => setTab("problems")} className="underline">
            view Problems
          </button>
        </div>
      )}

      <CommandPalette open={paletteOpen} onClose={() => setPaletteOpen(false)} commands={paletteCommands} />
      <GlobalSettingsScreen open={settingsOpen} onClose={() => setSettingsOpen(false)} />
      <Modal open={doctorOpen} onClose={() => setDoctorOpen(false)} label="Doctor">
        <DoctorScreen />
      </Modal>
    </div>
  );
}
