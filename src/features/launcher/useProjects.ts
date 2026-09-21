import { useCallback, useEffect, useState } from "react";
import type { ProjectEntry } from "../../lib/bindings";
import { projectForget, projectList, projectOpen } from "../../lib/ipc";

export function useProjects() {
  const [projects, setProjects] = useState<ProjectEntry[]>([]);
  const [loading, setLoading] = useState(true);

  const refresh = useCallback(async () => {
    setLoading(true);
    try {
      setProjects(await projectList());
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => {
    void refresh();
  }, [refresh]);

  const open = useCallback(
    async (path: string) => {
      const entry = await projectOpen(path);
      await refresh();
      return entry;
    },
    [refresh],
  );

  const forget = useCallback(
    async (id: string) => {
      await projectForget(id);
      await refresh();
    },
    [refresh],
  );

  return { projects, loading, refresh, open, forget };
}
