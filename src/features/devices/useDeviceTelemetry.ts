// Device selection + telemetry, shared between the Sidebar's Hardware card (picker,
// telemetry summary) and the Monitor canvas tab (which just needs to know whether a port
// is selected) — one instance per workspace, owned by the shell so both consumers see the
// same state instead of each re-fetching independently.

import { useCallback, useEffect, useState } from "react";
import type { Telemetry } from "../../lib/bindings";
import { deviceSelect, deviceTelemetry } from "../../lib/ipc";
import { useDeviceList } from "./useDeviceList";

export function useDeviceTelemetry(workspaceId: string) {
  const deviceList = useDeviceList();
  const [selectedPort, setSelectedPort] = useState<string | null>(null);
  const [telemetry, setTelemetry] = useState<Telemetry | null>(null);
  const [telemetryLoading, setTelemetryLoading] = useState(false);

  const refreshTelemetry = useCallback(async () => {
    setTelemetryLoading(true);
    try {
      const t = await deviceTelemetry(workspaceId, true);
      setTelemetry(t);
      if (t.port) setSelectedPort(t.port);
    } finally {
      setTelemetryLoading(false);
    }
  }, [workspaceId]);

  useEffect(() => {
    void refreshTelemetry();
    // Re-checks after every `device://changed` too — picks up a silent `FR-DEV-2` sticky
    // rebind (backend-driven, e.g. this workspace's board reappearing on a new port while
    // nothing in this window explicitly noticed it).
  }, [refreshTelemetry, deviceList.changeCount]);

  const selectPort = useCallback(
    async (port: string) => {
      setSelectedPort(port);
      await deviceSelect(workspaceId, port);
      void refreshTelemetry();
    },
    [workspaceId, refreshTelemetry],
  );

  return { ...deviceList, selectedPort, telemetry, telemetryLoading, refreshTelemetry, selectPort };
}

export type UseDeviceTelemetry = ReturnType<typeof useDeviceTelemetry>;
