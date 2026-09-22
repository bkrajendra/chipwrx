// `FR-BUILD-5` Fix loop: "a prominent 'Ask Claude to fix these N errors' button composes a
// prompt containing the structured defects plus the last ~100 lines of build output."

import type { Defect } from "../../lib/bindings";
import type { LogLineView } from "./usePipeline";

export function composeFixPrompt(defects: Defect[], lines: LogLineView[]): string {
  const list = defects.map((d) => `- ${d.file}:${d.line}${d.column ? `:${d.column}` : ""} ${d.severity}: ${d.message}`).join("\n");
  const tailLines = lines.slice(-100);
  const tail = tailLines.map((l) => l.text).join("\n");
  return [
    `The last build failed with ${defects.length} error${defects.length === 1 ? "" : "s"}:`,
    "",
    list,
    "",
    `Build output (last ${tailLines.length} lines):`,
    "```",
    tail,
    "```",
    "",
    "Please fix these errors.",
  ].join("\n");
}
