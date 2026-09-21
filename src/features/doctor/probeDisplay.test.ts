import { describe, expect, it } from "vitest";
import type { ProbeResult } from "../../lib/bindings";
import { probeSummary, probeTone, remediationLabel } from "./probeDisplay";

describe("probeTone", () => {
  it("maps every status to the right tone", () => {
    expect(probeTone({ status: "ok", version: "1.0.0", path: null, detail: null })).toBe("ok");
    expect(probeTone({ status: "degraded", reason: "x", remediation: null })).toBe("warn");
    expect(probeTone({ status: "missing", installAvailable: true })).toBe("bad");
    expect(probeTone({ status: "error", detail: "x" })).toBe("bad");
    expect(probeTone({ status: "probing" })).toBe("pending");
  });
});

describe("probeSummary", () => {
  it("joins version, path, and detail when all present", () => {
    const r: ProbeResult = { status: "ok", version: "2.1.211", path: "/bin/claude", detail: "cached" };
    expect(probeSummary(r)).toBe("2.1.211 — /bin/claude — cached");
  });

  it("falls back to OK when ok result has no version or detail", () => {
    const r: ProbeResult = { status: "ok", version: "", path: null, detail: null };
    expect(probeSummary(r)).toBe("OK");
  });

  it("uses the reason for degraded", () => {
    const r: ProbeResult = { status: "degraded", reason: "too old", remediation: null };
    expect(probeSummary(r)).toBe("too old");
  });
});

describe("remediationLabel", () => {
  it("offers Install for a missing tool with install available", () => {
    const r: ProbeResult = { status: "missing", installAvailable: true };
    expect(remediationLabel(r)).toBe("Install");
  });

  it("offers nothing for a missing tool with no install path", () => {
    const r: ProbeResult = { status: "missing", installAvailable: false };
    expect(remediationLabel(r)).toBeNull();
  });

  it("uses the remediation's own label when degraded", () => {
    const r: ProbeResult = {
      status: "degraded",
      reason: "not signed in",
      remediation: { kind: "authenticateClaude", label: "Sign in", commandPreview: null, url: null },
    };
    expect(remediationLabel(r)).toBe("Sign in");
  });

  it("offers nothing for ok/error/probing", () => {
    expect(remediationLabel({ status: "ok", version: "1", path: null, detail: null })).toBeNull();
    expect(remediationLabel({ status: "error", detail: "x" })).toBeNull();
    expect(remediationLabel({ status: "probing" })).toBeNull();
  });
});
