import { describe, expect, it } from "vitest";
import type { AppError } from "./bindings";
import { renderAppError } from "./errors";

describe("renderAppError", () => {
  it("renders a remediation action when install is available", () => {
    const err: AppError = { code: "TOOL_MISSING", tool: "pio", installAction: true };
    const info = renderAppError(err);
    expect(info.title).toBe("Tool not found");
    expect(info.remediation?.label).toBe("Install pio");
  });

  it("omits the remediation action when install is not available", () => {
    const err: AppError = { code: "TOOL_MISSING", tool: "pio", installAction: false };
    expect(renderAppError(err).remediation).toBeUndefined();
  });

  it("renders a unit-variant error with no extra fields", () => {
    const err: AppError = { code: "CLAUDE_UNAUTHENTICATED" };
    expect(renderAppError(err).title).toBe("Claude isn't signed in");
  });

  it("summarizes a single permission denial by name", () => {
    const err: AppError = {
      code: "CLAUDE_PERMISSION_DENIED",
      denials: [{ tool: "Bash(rm *)", reason: "not in the allowed-tools list" }],
    };
    expect(renderAppError(err).message).toContain("Bash(rm *)");
  });

  it("formats an IniParse error with a line number", () => {
    const err: AppError = {
      code: "INI_PARSE",
      path: "platformio.ini",
      line: 12,
      message: "unexpected token",
    };
    expect(renderAppError(err).message).toBe("platformio.ini:12 — unexpected token");
  });
});
