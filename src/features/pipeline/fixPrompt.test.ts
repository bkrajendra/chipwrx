import { describe, expect, it } from "vitest";
import type { Defect } from "../../lib/bindings";
import { composeFixPrompt } from "./fixPrompt";
import type { LogLineView } from "./usePipeline";

function defect(overrides: Partial<Defect> = {}): Defect {
  return {
    file: "src/main.cpp",
    line: 5,
    column: 11,
    severity: "error",
    message: "expected primary-expression before ';' token",
    source: "compiler",
    raw: "src/main.cpp:5:11: error: expected primary-expression before ';' token",
    ...overrides,
  };
}

describe("composeFixPrompt", () => {
  it("lists every defect with file:line:col and message", () => {
    const prompt = composeFixPrompt([defect()], []);
    expect(prompt).toContain("src/main.cpp:5:11 error: expected primary-expression before ';' token");
    expect(prompt).toContain("failed with 1 error:");
  });

  it("pluralizes correctly for multiple errors", () => {
    const prompt = composeFixPrompt([defect(), defect({ line: 9, column: 40 })], []);
    expect(prompt).toContain("failed with 2 errors:");
  });

  it("omits the column when a defect has none", () => {
    const prompt = composeFixPrompt([defect({ column: null })], []);
    expect(prompt).toContain("src/main.cpp:5 error:");
    expect(prompt).not.toContain("src/main.cpp:5:11");
  });

  it("includes only the last 100 lines of output", () => {
    const lines: LogLineView[] = Array.from({ length: 150 }, (_, i) => ({ stream: "stdout", text: `line ${i}` }));
    const prompt = composeFixPrompt([defect()], lines);
    expect(prompt).toContain("last 100 lines");
    expect(prompt).toContain("line 149");
    expect(prompt).not.toContain("line 49\n");
  });

  it("ends with an explicit fix request", () => {
    const prompt = composeFixPrompt([defect()], []);
    expect(prompt.trim().endsWith("Please fix these errors.")).toBe(true);
  });
});
