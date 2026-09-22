// The only file that calls `invoke` (IPC-CONTRACT.md §10 rule 1). Feature code imports
// these typed wrappers instead.

import { invoke, Channel } from "@tauri-apps/api/core";
import type {
  BoardBrief,
  ChatEvent,
  CreateProjectRequest,
  DoctorReport,
  FileChange,
  FileDiff,
  GlobalSettings,
  GlobalSettingsPatch,
  InstallEvent,
  PipelineState,
  ProbeResult,
  ProcEvent,
  ProjectEntry,
  RemediationKind,
  SerialDevice,
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

export function changesForTurn(workspace: string, turnId: string): Promise<FileChange[]> {
  return invoke("changes_for_turn", { workspace, turnId });
}

export function changesDiff(workspace: string, turnId: string, path: string): Promise<FileDiff> {
  return invoke("changes_diff", { workspace, turnId, path });
}

export function changesRevertFile(workspace: string, turnId: string, path: string): Promise<void> {
  return invoke("changes_revert_file", { workspace, turnId, path });
}

export function changesRevertTurn(workspace: string, turnId: string): Promise<void> {
  return invoke("changes_revert_turn", { workspace, turnId });
}

export function changesResetToLastGoodBuild(workspace: string): Promise<string> {
  return invoke("changes_reset_to_last_good_build", { workspace });
}

export function fileRead(workspace: string, path: string): Promise<string> {
  return invoke("file_read", { workspace, path });
}

export function pipelineBuild(workspace: string, onEvent: (event: ProcEvent) => void): Promise<string> {
  const channel = new Channel<ProcEvent>();
  channel.onmessage = onEvent;
  return invoke("pipeline_build", { workspace, onEvent: channel });
}

export function pipelineUpload(workspace: string, onEvent: (event: ProcEvent) => void): Promise<string> {
  const channel = new Channel<ProcEvent>();
  channel.onmessage = onEvent;
  return invoke("pipeline_upload", { workspace, onEvent: channel });
}

export function pipelineRunTarget(workspace: string, target: string, onEvent: (event: ProcEvent) => void): Promise<string> {
  const channel = new Channel<ProcEvent>();
  channel.onmessage = onEvent;
  return invoke("pipeline_run_target", { workspace, target, onEvent: channel });
}

export function pipelineStop(procId: string): Promise<void> {
  return invoke("pipeline_stop", { procId });
}

export function pipelineTargets(workspace: string, refresh: boolean): Promise<string[]> {
  return invoke("pipeline_targets", { workspace, refresh });
}

export function pipelineState(workspace: string): Promise<PipelineState> {
  return invoke("pipeline_state", { workspace });
}

export function deviceList(): Promise<SerialDevice[]> {
  return invoke("device_list");
}

export function deviceSetPreferredPort(workspace: string, port: string | null): Promise<void> {
  return invoke("device_set_preferred_port", { workspace, port });
}
