// `NFR-P3`: the log pane and serial monitor buffer caps come from `GlobalSettings`
// (`logs.maxLines`, `monitor.{maxLines,maxBytes}`) rather than being hardcoded per
// component. Fetched once per `Workspace` instance; falls back to the same defaults the
// backend ships (`core::settings::{LogSettings,MonitorSettings}::default()`) until the
// real settings load.

import { useEffect, useState } from "react";
import { settingsGetGlobal } from "../../lib/ipc";

const DEFAULTS = { logMaxLines: 50_000, monitorMaxLines: 20_000, monitorMaxBytes: 8_388_608 };

export function useGlobalSettingsCaps() {
  const [caps, setCaps] = useState(DEFAULTS);

  useEffect(() => {
    let cancelled = false;
    void settingsGetGlobal().then((s) => {
      if (cancelled) return;
      setCaps({ logMaxLines: s.logs.maxLines, monitorMaxLines: s.monitor.maxLines, monitorMaxBytes: s.monitor.maxBytes });
    });
    return () => {
      cancelled = true;
    };
  }, []);

  return caps;
}
