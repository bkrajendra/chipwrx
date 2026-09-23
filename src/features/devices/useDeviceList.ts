// `FR-DEV-1/2/9`: live device enumeration + hot-plug awareness for one workspace. Seeds
// from `deviceList()` once, then applies the backend's `device://changed` diffs — the
// backend already polls at the focused/background cadence (`commands::device`), so the
// frontend never polls itself.

import { listen } from "@tauri-apps/api/event";
import { useEffect, useState } from "react";
import type { DeviceChanged, SerialDevice } from "../../lib/bindings";
import { deviceList } from "../../lib/ipc";

export function useDeviceList() {
  const [devices, setDevices] = useState<SerialDevice[]>([]);
  const [loaded, setLoaded] = useState(false);
  /** The most recently *newly connected* known-bridge device — `FR-DEV-9`'s toast source.
   * `null` once dismissed or superseded. */
  const [justConnected, setJustConnected] = useState<SerialDevice | null>(null);
  /** Increments on every `device://changed` — lets a consumer (e.g. `TelemetryCard`) react
   * to a change (including a silent `FR-DEV-2` sticky rebind) without re-deriving it from
   * the `devices` array itself. */
  const [changeCount, setChangeCount] = useState(0);

  useEffect(() => {
    let cancelled = false;
    void deviceList().then((list) => {
      if (!cancelled) {
        setDevices(list);
        setLoaded(true);
      }
    });

    const unlisten = listen<DeviceChanged>("device://changed", (event) => {
      const { added, removed } = event.payload;
      setDevices((prev) => {
        const withoutRemoved = prev.filter((d) => !removed.includes(d.port));
        const withoutReAdded = withoutRemoved.filter((d) => !added.some((a) => a.port === d.port));
        return [...withoutReAdded, ...added];
      });
      const bridgeArrival = added.find((d) => d.knownBridge !== null);
      if (bridgeArrival) setJustConnected(bridgeArrival);
      setChangeCount((c) => c + 1);
    });

    return () => {
      cancelled = true;
      void unlisten.then((f) => f());
    };
  }, []);

  return { devices, loaded, justConnected, dismissJustConnected: () => setJustConnected(null), changeCount };
}
