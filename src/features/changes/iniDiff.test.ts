import { describe, expect, it } from "vitest";
import { diffIni } from "./iniDiff";

describe("diffIni", () => {
  it("reports a changed key", () => {
    const before = "[env:esp32dev]\nboard = esp32dev\nframework = arduino\n";
    const after = "[env:esp32dev]\nboard = esp32-s3-devkitc-1\nframework = arduino\n";
    const changes = diffIni(before, after);
    expect(changes).toEqual([{ section: "env:esp32dev", key: "board", before: "esp32dev", after: "esp32-s3-devkitc-1" }]);
  });

  it("reports an added key", () => {
    const before = "[env:esp32dev]\nboard = esp32dev\n";
    const after = "[env:esp32dev]\nboard = esp32dev\nmonitor_speed = 115200\n";
    const changes = diffIni(before, after);
    expect(changes).toEqual([{ section: "env:esp32dev", key: "monitor_speed", before: null, after: "115200" }]);
  });

  it("reports a removed key", () => {
    const before = "[env:esp32dev]\nboard = esp32dev\nmonitor_speed = 115200\n";
    const after = "[env:esp32dev]\nboard = esp32dev\n";
    const changes = diffIni(before, after);
    expect(changes).toEqual([{ section: "env:esp32dev", key: "monitor_speed", before: "115200", after: null }]);
  });

  it("ignores comments, blank lines, and unchanged keys", () => {
    const before = "; a comment\n[env:esp32dev]\nboard = esp32dev\n\n# another comment\nframework = arduino\n";
    const after = "; a comment\n[env:esp32dev]\nboard = esp32dev\n\n# another comment\nframework = arduino\n";
    expect(diffIni(before, after)).toEqual([]);
  });

  it("distinguishes the same key across different sections", () => {
    const before = "[env]\nmonitor_speed = 9600\n[env:esp32dev]\nmonitor_speed = 115200\n";
    const after = "[env]\nmonitor_speed = 9600\n[env:esp32dev]\nmonitor_speed = 921600\n";
    const changes = diffIni(before, after);
    expect(changes).toEqual([{ section: "env:esp32dev", key: "monitor_speed", before: "115200", after: "921600" }]);
  });
});
