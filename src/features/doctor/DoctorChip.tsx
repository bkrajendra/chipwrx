// Sidebar "Doctor status chip" (`FR-UI-2`) — a compact overall-health summary, worst tone
// wins. `NFR-A2`: icon + text, never colour alone.

import type { DoctorReport } from "../../lib/bindings";
import { PROBE_DEFS, probeTone, type ProbeTone } from "./probeDisplay";

const TONE_ORDER: ProbeTone[] = ["bad", "warn", "pending", "ok"];
const TONE_DOT: Record<ProbeTone, string> = {
  ok: "bg-emerald-500",
  warn: "bg-amber-500",
  bad: "bg-red-500",
  pending: "bg-neutral-400 animate-pulse",
};
const TONE_LABEL: Record<ProbeTone, string> = {
  ok: "All checks passing",
  warn: "Needs attention",
  bad: "Action required",
  pending: "Checking…",
};

function worstTone(report: DoctorReport): ProbeTone {
  const tones = PROBE_DEFS.map((d) => probeTone((report as unknown as Record<string, DoctorReport[keyof DoctorReport]>)[d.key] as never));
  for (const t of TONE_ORDER) {
    if (tones.includes(t)) return t;
  }
  return "ok";
}

export function DoctorChip({ report, onOpen }: { report: DoctorReport | null; onOpen: () => void }) {
  const tone = report ? worstTone(report) : "pending";
  return (
    <button type="button" onClick={onOpen} className="flex w-full items-center gap-2 p-3 text-left text-xs">
      <span className={`h-2.5 w-2.5 shrink-0 rounded-full ${TONE_DOT[tone]}`} aria-hidden />
      <span>
        <span className="block font-medium">Doctor</span>
        <span className="text-neutral-500 dark:text-neutral-400">{TONE_LABEL[tone]}</span>
      </span>
    </button>
  );
}
