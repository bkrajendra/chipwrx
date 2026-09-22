// The frontend error-renderer registry (`ARCHITECTURE.md` §6): every `AppError` variant
// maps to its own rendering, never a generic "Something went wrong" dialog. The mapping
// type below is exhaustive over `AppError["code"]` — adding a new Rust variant and
// regenerating bindings.ts will fail `tsc` here until this registry is updated too.

import type { AppError } from "./bindings";

export interface ErrorRenderInfo {
  title: string;
  message: string;
  remediation?: { label: string };
}

type Code = AppError["code"];
type Variant<C extends Code> = Extract<AppError, { code: C }>;

const registry: { [C in Code]: (err: Variant<C>) => ErrorRenderInfo } = {
  TOOL_MISSING: (err) => ({
    title: "Tool not found",
    message: `${err.tool} isn't installed, or isn't on the resolved path.`,
    remediation: err.installAction ? { label: `Install ${err.tool}` } : undefined,
  }),
  TOOL_TOO_OLD: (err) => ({
    title: "Tool needs an update",
    message: `${err.tool} ${err.found} is older than the required ${err.minimum}.`,
  }),
  CLAUDE_UNAUTHENTICATED: () => ({
    title: "Claude isn't signed in",
    message: "Sign in with a Pro, Max, Team, Enterprise, or Console account to continue.",
    remediation: { label: "Sign in" },
  }),
  CLAUDE_PERMISSION_DENIED: (err) => ({
    title: "Permission denied",
    message:
      err.denials.length === 1
        ? `Claude tried to use ${err.denials[0].tool}: ${err.denials[0].reason}`
        : `Claude was denied ${err.denials.length} tool calls under the current policy.`,
  }),
  CLAUDE_INTERRUPTED: () => ({
    title: "Turn interrupted",
    message: "The turn was stopped before it finished. Resuming will continue it.",
  }),
  CLAUDE_PROCESS_FAILED: (err) => ({
    title: "Claude exited unexpectedly",
    message: `The claude process exited with code ${err.exitCode}.`,
  }),
  NETWORK_UNAVAILABLE: (err) => ({
    title: "Network unavailable",
    message: `Couldn't reach ${err.host}. Some features will run in offline mode.`,
  }),
  PIO_COMMAND_FAILED: (err) => ({
    title: "PlatformIO command failed",
    message: `${err.argv.join(" ")} exited with code ${err.exitCode}.`,
  }),
  BUILD_FAILED: (err) => ({
    title: "Build failed",
    message: `${err.defects} ${err.defects === 1 ? "problem" : "problems"} found — see the Problems tab.`,
  }),
  PORT_BUSY: (err) => ({
    title: "Port busy",
    message: `${err.port} is currently held by ${err.heldBy}.`,
  }),
  PORT_DISAPPEARED: (err) => ({
    title: "Device disconnected",
    message: `${err.port} is no longer available. Waiting for it to reappear.`,
  }),
  NOT_A_PIO_PROJECT: (err) => ({
    title: "Not a PlatformIO project",
    message: `${err.path} has no platformio.ini.`,
    remediation: { label: "Initialize project" },
  }),
  INI_PARSE: (err) => ({
    title: "platformio.ini couldn't be parsed",
    message: err.line != null ? `${err.path}:${err.line} — ${err.message}` : `${err.path} — ${err.message}`,
  }),
  INI_CHANGED_ON_DISK: (err) => ({
    title: "platformio.ini changed on disk",
    message: `${err.path} was modified outside the app.`,
    remediation: { label: "Reload" },
  }),
  WORKSPACE_UNTRUSTED: (err) => ({
    title: "Workspace not trusted",
    message: `This folder has ${err.hooks.length} hook(s) and ${err.mcpServers.length} MCP server(s) that will run with no prompt. Review before continuing.`,
    remediation: { label: "Review and trust" },
  }),
  SNAPSHOT_FAILED: (err) => ({
    title: "Couldn't create a safety snapshot",
    message: err.message,
  }),
  UPLOAD_BLOCKED: (err) => ({
    title: "Upload blocked",
    message: err.reason,
  }),
  IO: (err) => ({
    title: "Unexpected error",
    message: err.message,
  }),
};

export function renderAppError(err: AppError): ErrorRenderInfo {
  const render = registry[err.code] as (err: AppError) => ErrorRenderInfo;
  return render(err);
}
