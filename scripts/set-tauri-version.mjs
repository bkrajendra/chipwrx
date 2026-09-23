#!/usr/bin/env node
// Stamps `src-tauri/tauri.conf.json`'s `version` from the release tag (`.github/workflows/
// release.yml`) — a script file rather than an inline `--config` JSON string on the build
// command, so there's no cross-platform shell-quoting to get wrong on Windows vs. bash.
//
// Usage: node scripts/set-tauri-version.mjs <version>   (no leading "v")

import { readFileSync, writeFileSync } from "node:fs";
import { fileURLToPath } from "node:url";

const version = process.argv[2];
if (!version || !/^\d+\.\d+\.\d+/.test(version)) {
  console.error(`usage: set-tauri-version.mjs <semver, no leading "v"> — got ${JSON.stringify(version)}`);
  process.exit(1);
}

const confPath = fileURLToPath(new URL("../src-tauri/tauri.conf.json", import.meta.url));
const conf = JSON.parse(readFileSync(confPath, "utf8"));
conf.version = version;
writeFileSync(confPath, JSON.stringify(conf, null, 2) + "\n");
console.log(`${confPath} -> version ${version}`);
