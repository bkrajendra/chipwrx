import { useCallback, useEffect, useState } from "react";
import type { DoctorReport, InstallEvent, RemediationKind } from "../../lib/bindings";
import {
  doctorGetCached,
  doctorRun,
  toolchainInstall,
  toolchainOpenAuthTerminal,
} from "../../lib/ipc";

export interface InstallLogState {
  kind: RemediationKind;
  events: InstallEvent[];
  done: boolean;
  success: boolean | null;
}

export function useDoctor() {
  const [report, setReport] = useState<DoctorReport | null>(null);
  const [loading, setLoading] = useState(true);
  const [installLog, setInstallLog] = useState<InstallLogState | null>(null);

  const refresh = useCallback(async (force: boolean) => {
    setLoading(true);
    try {
      setReport(await doctorRun(force));
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => {
    let cancelled = false;
    void (async () => {
      const cached = await doctorGetCached();
      if (cancelled) return;
      if (cached) {
        setReport(cached);
        setLoading(false);
      } else {
        await refresh(false);
      }
    })();
    return () => {
      cancelled = true;
    };
  }, [refresh]);

  const install = useCallback(
    async (kind: RemediationKind) => {
      setInstallLog({ kind, events: [], done: false, success: null });
      await toolchainInstall(kind, (event) => {
        setInstallLog((prev) => {
          if (!prev) return prev;
          const events = [...prev.events, event];
          if (event.type === "finished") {
            return { ...prev, events, done: true, success: event.data.success };
          }
          return { ...prev, events };
        });
      });
      await refresh(true);
    },
    [refresh],
  );

  const authenticate = useCallback(async () => {
    await toolchainOpenAuthTerminal();
  }, []);

  const dismissInstallLog = useCallback(() => setInstallLog(null), []);

  return { report, loading, refresh, install, installLog, dismissInstallLog, authenticate };
}
