// The only file that calls `invoke` (IPC-CONTRACT.md §10 rule 1). Feature code imports
// these typed wrappers instead.

import { invoke, Channel } from "@tauri-apps/api/core";
import type {
  BoardBrief,
  ChatEvent,
  CreateProjectRequest,
  DoctorReport,
  GlobalSettings,
  GlobalSettingsPatch,
  InstallEvent,
  ProbeResult,
  ProcEvent,
  ProjectEntry,
  RemediationKind,
  ToolchainTool,
  TrustScan,
  TurnRecord,
  TurnRequest,
} from "./bindings";

export function doctorRun(force: boolean): Promise<DoctorReport> {
  return invoke("doctor_run", { force });
}

export function doctorGetCached(): Promise<DoctorReport | null> {
  return invoke("doctor_get_cached");
}

export function toolchainInstall(
  kind: RemediationKind,
  onEvent: (event: InstallEvent) => void,
): Promise<string> {
  const channel = new Channel<InstallEvent>();
  channel.onmessage = onEvent;
  return invoke("toolchain_install", { kind, onEvent: channel });
}

export function toolchainSetPath(tool: ToolchainTool, path: string): Promise<ProbeResult> {
  return invoke("toolchain_set_path", { tool, path });
}

export function toolchainOpenAuthTerminal(): Promise<void> {
  return invoke("toolchain_open_auth_terminal");
}

export function diagnosticsExport(): Promise<string> {
  return invoke("diagnostics_export");
}

export function settingsGetGlobal(): Promise<GlobalSettings> {
  return invoke("settings_get_global");
}

export function settingsSetGlobal(patch: GlobalSettingsPatch): Promise<GlobalSettings> {
  return invoke("settings_set_global", { patch });
}

export function boardsList(refresh: boolean, installedOnly: boolean): Promise<BoardBrief[]> {
  return invoke("boards_list", { refresh, installedOnly });
}

export function projectCreate(
  req: CreateProjectRequest,
  onEvent: (event: ProcEvent) => void,
): Promise<string> {
  const channel = new Channel<ProcEvent>();
  channel.onmessage = onEvent;
  return invoke("project_create", { req, onEvent: channel });
}

export function projectCancelCreate(procId: string): Promise<void> {
  return invoke("project_cancel_create", { procId });
}

export function projectOpen(path: string): Promise<ProjectEntry> {
  return invoke("project_open", { path });
}

export function projectList(): Promise<ProjectEntry[]> {
  return invoke("project_list");
}

export function projectForget(id: string): Promise<void> {
  return invoke("project_forget", { id });
}

export function projectScanTrust(path: string): Promise<TrustScan> {
  return invoke("project_scan_trust", { path });
}

export function projectTrust(id: string): Promise<void> {
  return invoke("project_trust", { id });
}

export function projectListEnvs(id: string): Promise<string[]> {
  return invoke("project_list_envs", { id });
}

export function projectSetEnv(id: string, env: string): Promise<void> {
  return invoke("project_set_env", { id, env });
}

export function projectOpenInEditor(id: string, file?: string, line?: number): Promise<void> {
  return invoke("project_open_in_editor", { id, file: file ?? null, line: line ?? null });
}

export function projectReveal(id: string): Promise<void> {
  return invoke("project_reveal", { id });
}

export function projectRegenerateClaudeMd(id: string): Promise<string> {
  return invoke("project_regenerate_claude_md", { id });
}

export function claudeSendTurn(req: TurnRequest, onEvent: (event: ChatEvent) => void): Promise<string> {
  const channel = new Channel<ChatEvent>();
  channel.onmessage = onEvent;
  return invoke("claude_send_turn", { req, onEvent: channel });
}

export function claudeStopTurn(turnId: string, hard: boolean): Promise<void> {
  return invoke("claude_stop_turn", { turnId, hard });
}

export function claudeNewSession(workspace: string): Promise<string> {
  return invoke("claude_new_session", { workspace });
}

export function claudeHistory(workspace: string, limit: number, before?: string): Promise<TurnRecord[]> {
  return invoke("claude_history", { workspace, limit, before: before ?? null });
}
