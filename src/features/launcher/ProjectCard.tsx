// Sidebar "Project card" (`FR-UI-2`): name, path, active env picker, board id, Claude
// session health.

import { useEffect, useState } from "react";
import type { ProjectEntry } from "../../lib/bindings";
import { projectListEnvs, projectSetEnv } from "../../lib/ipc";

export function ProjectCard({
  project,
  hasSession,
  running,
}: {
  project: ProjectEntry;
  hasSession: boolean;
  running: boolean;
}) {
  const [envs, setEnvs] = useState<string[]>([]);
  const [activeEnv, setActiveEnv] = useState(project.activeEnv ?? "");

  useEffect(() => {
    void projectListEnvs(project.id).then(setEnvs);
  }, [project.id]);

  const changeEnv = async (env: string) => {
    setActiveEnv(env);
    await projectSetEnv(project.id, env);
  };

  return (
    <div className="space-y-2 border-b border-neutral-200 p-3 dark:border-neutral-800">
      <div>
        <h2 className="truncate text-sm font-semibold" title={project.name}>
          {project.name}
        </h2>
        <p className="truncate text-[11px] text-neutral-500 dark:text-neutral-400" title={project.path}>
          {project.path}
        </p>
      </div>

      <label className="block text-xs">
        <span className="mb-0.5 block text-neutral-500 dark:text-neutral-400">Active env</span>
        <select
          value={activeEnv}
          onChange={(e) => void changeEnv(e.target.value)}
          className="w-full rounded border border-neutral-300 bg-white px-2 py-1 text-xs text-neutral-900 dark:border-neutral-700 dark:bg-neutral-900 dark:text-neutral-100"
        >
          {envs.length === 0 && <option value="">No environments</option>}
          {envs.map((e) => (
            <option key={e} value={e}>
              {e}
            </option>
          ))}
        </select>
      </label>

      {project.boardId && (
        <p className="text-xs text-neutral-500 dark:text-neutral-400">
          Board <span className="font-mono text-neutral-700 dark:text-neutral-300">{project.boardId}</span>
        </p>
      )}

      <div className="flex items-center gap-1.5 text-xs">
        <span className={`h-2 w-2 shrink-0 rounded-full ${running ? "animate-pulse bg-blue-500" : hasSession ? "bg-emerald-500" : "bg-neutral-300 dark:bg-neutral-700"}`} aria-hidden />
        <span className="text-neutral-500 dark:text-neutral-400">{running ? "Claude is working…" : hasSession ? "Claude session active" : "No Claude session yet"}</span>
      </div>
    </div>
  );
}
