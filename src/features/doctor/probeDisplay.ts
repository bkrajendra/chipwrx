import type { DoctorReport, ProbeResult } from "../../lib/bindings";

export interface ProbeDef {
  key: keyof DoctorReport & string;
  label: string;
}

// `probed_at`/`claude_capabilities` aren't probe rows — every other DoctorReport field is.
export const PROBE_DEFS: ProbeDef[] = [
  { key: "claudeBinary", label: "Claude Code" },
  { key: "claudeAuth", label: "Claude sign-in" },
  { key: "pioBinary", label: "PlatformIO Core" },
  { key: "pioCoreDir", label: "PlatformIO core directory" },
  { key: "python", label: "Python" },
  { key: "networkRegistry", label: "PlatformIO registry" },
  { key: "serialPermissions", label: "Serial permissions" },
  { key: "git", label: "git" },
];

export type ProbeTone = "ok" | "warn" | "bad" | "pending";

export function probeTone(result: ProbeResult): ProbeTone {
  switch (result.status) {
    case "ok":
      return "ok";
    case "degraded":
      return "warn";
    case "missing":
    case "error":
      return "bad";
    case "probing":
      return "pending";
  }
}

export function probeSummary(result: ProbeResult): string {
  switch (result.status) {
    case "ok":
      return [result.version, result.path, result.detail].filter(Boolean).join(" — ") || "OK";
    case "missing":
      return "Not found";
    case "degraded":
      return result.reason;
    case "error":
      return result.detail;
    case "probing":
      return "Probing…";
  }
}

export function remediationLabel(result: ProbeResult): string | null {
  if (result.status === "missing" && result.installAvailable) return "Install";
  if (result.status === "degraded" && result.remediation) return result.remediation.label;
  return null;
}
