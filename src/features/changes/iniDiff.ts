// `FR-SAFE-5`: "The Changes panel renders an ini-aware diff for `platformio.ini`." This is
// a minimal key=value scanner mirroring `core/project/env.rs`'s approach — explicitly not
// the format-preserving parser M7 owns — just enough to show *which settings changed*
// (e.g. "board: esp32dev → esp32-s3-devkitc-1") above the raw text diff, since that's the
// change a user actually cares about (board/upload-protocol/flash-layout changes are
// called out by FR-SAFE-5 as consequential).

export interface IniKeyChange {
  section: string;
  key: string;
  before: string | null;
  after: string | null;
}

function parseIni(text: string): Map<string, string> {
  const map = new Map<string, string>();
  let section = "";
  for (const rawLine of text.split(/\r?\n/)) {
    const line = rawLine.trim();
    if (!line || line.startsWith(";") || line.startsWith("#")) continue;
    const sectionMatch = /^\[(.+)\]$/.exec(line);
    if (sectionMatch) {
      section = sectionMatch[1];
      continue;
    }
    const eq = line.indexOf("=");
    if (eq === -1) continue;
    const key = line.slice(0, eq).trim();
    const value = line.slice(eq + 1).trim();
    if (key) map.set(`${section}\u0000${key}`, value);
  }
  return map;
}

export function diffIni(before: string, after: string): IniKeyChange[] {
  const beforeMap = parseIni(before);
  const afterMap = parseIni(after);
  const keys = new Set([...beforeMap.keys(), ...afterMap.keys()]);
  const changes: IniKeyChange[] = [];
  for (const fullKey of keys) {
    const b = beforeMap.get(fullKey) ?? null;
    const a = afterMap.get(fullKey) ?? null;
    if (b !== a) {
      const [section, key] = fullKey.split("\u0000");
      changes.push({ section, key, before: b, after: a });
    }
  }
  return changes.sort((x, y) => (x.section + x.key).localeCompare(y.section + y.key));
}
