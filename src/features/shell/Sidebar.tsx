// `FR-UI-1/2`: the sidebar itself (resizing/collapsing is the caller's job — `Workspace`
// owns the drag handle and width state so both the sidebar and the divider share one
// source of truth). Top to bottom: Project card, Hardware telemetry card, Firmware size
// card, Control deck, Problems badge, Doctor status chip.

import type { DoctorReport, Defect, PipelineState, ProjectEntry, SizeUsage } from "../../lib/bindings";
import type { UseDeviceTelemetry } from "../devices/useDeviceTelemetry";
import { TelemetryCard } from "../devices/TelemetryCard";
import { DoctorChip } from "../doctor/DoctorChip";
import { ProjectCard } from "../launcher/ProjectCard";
import { ControlDeck } from "../pipeline/ControlDeck";
import { ProblemsBadge } from "../pipeline/ProblemsBadge";
import { SizeCard } from "../pipeline/SizeCard";
import { strings } from "../../lib/strings";

export function Sidebar({
  project,
  hasSession,
  chatRunning,
  device,
  pipelineState,
  pipelineRunning,
  size,
  defects,
  onBuild,
  onUpload,
  onRunTarget,
  onStop,
  onOpenMonitor,
  onOpenProblems,
  doctorReport,
  onOpenDoctor,
  collapsed,
  onToggleCollapsed,
}: {
  project: ProjectEntry;
  hasSession: boolean;
  chatRunning: boolean;
  device: UseDeviceTelemetry;
  pipelineState: PipelineState | null;
  pipelineRunning: boolean;
  size: SizeUsage | null;
  defects: Defect[];
  onBuild: () => void;
  onUpload: () => void;
  onRunTarget: (target: string) => void;
  onStop: () => void;
  onOpenMonitor: () => void;
  onOpenProblems: () => void;
  doctorReport: DoctorReport | null;
  onOpenDoctor: () => void;
  collapsed: boolean;
  onToggleCollapsed: () => void;
}) {
  if (collapsed) {
    return (
      <div className="flex w-10 shrink-0 flex-col items-center border-r border-neutral-200 py-2 dark:border-neutral-800">
        <button
          type="button"
          onClick={onToggleCollapsed}
          aria-label={strings.shell.expandSidebar}
          title={strings.shell.expandSidebar}
          className="rounded p-1.5 text-neutral-500 hover:bg-neutral-100 dark:text-neutral-400 dark:hover:bg-neutral-800"
        >
          »
        </button>
      </div>
    );
  }

  return (
    <div className="flex h-full flex-col overflow-y-auto">
      <div className="flex justify-end p-1">
        <button
          type="button"
          onClick={onToggleCollapsed}
          aria-label={strings.shell.collapseSidebar}
          title={strings.shell.collapseSidebar}
          className="rounded p-1 text-neutral-500 hover:bg-neutral-100 hover:text-neutral-700 dark:text-neutral-400 dark:hover:bg-neutral-800 dark:hover:text-neutral-200"
        >
          «
        </button>
      </div>
      <ProjectCard project={project} hasSession={hasSession} running={chatRunning} />
      <TelemetryCard device={device} />
      <SizeCard size={size} />
      <ControlDeck
        workspaceId={project.id}
        state={pipelineState}
        running={pipelineRunning}
        onBuild={onBuild}
        onUpload={onUpload}
        onRunTarget={onRunTarget}
        onStop={onStop}
        onOpenMonitor={onOpenMonitor}
      />
      <ProblemsBadge defects={defects} onOpen={onOpenProblems} />
      <DoctorChip report={doctorReport} onOpen={onOpenDoctor} />
    </div>
  );
}
