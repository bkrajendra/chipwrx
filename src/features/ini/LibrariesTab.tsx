// `FR-INI-7/8/9`: the Libraries tab — registry search with qualifier chips, install/
// uninstall (which writes `lib_deps` for you — this tab never also patches the ini itself,
// `CLI-CONTRACT.md` §7.2), and the installed/outdated lists for the active env.

import { useCallback, useEffect, useState } from "react";
import type { AppError, InstalledPackage, OutdatedPackage, RegistryPackage } from "../../lib/bindings";
import { renderAppError } from "../../lib/errors";
import { pkgInstall, pkgInstalled, pkgOutdated, pkgSearch, pkgUninstall } from "../../lib/ipc";

function isAppError(e: unknown): e is AppError {
  return typeof e === "object" && e !== null && "code" in e;
}
function describe(e: unknown): string {
  return isAppError(e) ? renderAppError(e).message : String(e);
}

type Pin = "latest" | "exact" | "caret" | "tilde";

function buildSpec(owner: string, name: string, version: string, pin: Pin): string {
  const full = `${owner}/${name}`;
  switch (pin) {
    case "latest":
      return full;
    case "exact":
      return `${full}@${version}`;
    case "caret":
      return `${full}@^${version}`;
    case "tilde":
      return `${full}@~${version}`;
  }
}

export function LibrariesTab({ workspaceId, onBusyChange }: { workspaceId: string; onBusyChange: (busy: boolean) => void }) {
  const [query, setQuery] = useState("");
  const [framework, setFramework] = useState("");
  const [results, setResults] = useState<RegistryPackage[]>([]);
  const [searching, setSearching] = useState(false);
  const [installed, setInstalled] = useState<InstalledPackage[]>([]);
  const [outdated, setOutdated] = useState<OutdatedPackage[]>([]);
  const [error, setError] = useState<string | null>(null);
  const [busySpec, setBusySpec] = useState<string | null>(null);
  const [pinBySpec, setPinBySpec] = useState<Record<string, Pin>>({});

  const refreshInstalled = useCallback(async () => {
    try {
      const [inst, out] = await Promise.all([pkgInstalled(workspaceId), pkgOutdated(workspaceId)]);
      setInstalled(inst);
      setOutdated(out);
    } catch (e) {
      setError(describe(e));
    }
  }, [workspaceId]);

  useEffect(() => {
    void refreshInstalled();
  }, [refreshInstalled]);

  const search = useCallback(async () => {
    setSearching(true);
    setError(null);
    try {
      const qualifiers: [string, string][] = framework ? [["framework", framework]] : [];
      const page = await pkgSearch(query, qualifiers, 1);
      setResults(page.items);
    } catch (e) {
      setError(describe(e));
    } finally {
      setSearching(false);
    }
  }, [query, framework]);

  const runInstall = useCallback(
    async (owner: string, name: string) => {
      const key = `${owner}/${name}`;
      const pin = pinBySpec[key] ?? "caret";
      const pkg = results.find((r) => r.owner === owner && r.name === name);
      const spec = buildSpec(owner, name, pkg?.version ?? "", pin);
      setBusySpec(spec);
      onBusyChange(true);
      setError(null);
      try {
        await new Promise<void>((resolve, reject) => {
          void pkgInstall(workspaceId, spec, "library", (ev) => {
            if (ev.type === "finished") {
              if (ev.data.success) resolve();
              else reject(new Error("install failed — see the pipeline log"));
            }
          }).catch(reject);
        });
        await refreshInstalled();
      } catch (e) {
        setError(describe(e));
      } finally {
        setBusySpec(null);
        onBusyChange(false);
      }
    },
    [workspaceId, results, pinBySpec, refreshInstalled, onBusyChange],
  );

  const runUninstall = useCallback(
    async (pkg: InstalledPackage) => {
      setBusySpec(pkg.name);
      onBusyChange(true);
      setError(null);
      try {
        await new Promise<void>((resolve, reject) => {
          void pkgUninstall(workspaceId, pkg.name, pkg.kind, (ev) => {
            if (ev.type === "finished") {
              if (ev.data.success) resolve();
              else reject(new Error("uninstall failed — see the pipeline log"));
            }
          }).catch(reject);
        });
        await refreshInstalled();
      } catch (e) {
        setError(describe(e));
      } finally {
        setBusySpec(null);
        onBusyChange(false);
      }
    },
    [workspaceId, refreshInstalled, onBusyChange],
  );

  const outdatedNames = new Set(outdated.map((o) => o.name));

  return (
    <div className="flex h-full flex-col overflow-y-auto">
      <div className="space-y-2 border-b border-neutral-200 px-4 py-2 dark:border-neutral-800">
        <div className="flex gap-2">
          <input
            value={query}
            onChange={(e) => setQuery(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === "Enter") void search();
            }}
            placeholder="Search the PlatformIO registry…"
            className="flex-1 rounded border border-neutral-300 bg-transparent px-2 py-1 text-xs dark:border-neutral-700"
          />
          <button type="button" onClick={() => void search()} disabled={searching} className="rounded border border-neutral-300 px-2.5 py-1 text-xs font-medium hover:bg-neutral-100 disabled:opacity-40 dark:border-neutral-700 dark:hover:bg-neutral-800">
            {searching ? "Searching…" : "Search"}
          </button>
        </div>
        <input
          value={framework}
          onChange={(e) => setFramework(e.target.value)}
          placeholder="Filter by framework: (e.g. arduino)"
          className="w-full rounded border border-neutral-300 bg-transparent px-2 py-1 text-xs dark:border-neutral-700"
        />
      </div>

      {error && <p className="px-4 py-1.5 text-xs text-red-500">{error}</p>}

      {results.length > 0 && (
        <div className="border-b border-neutral-200 px-4 py-2 dark:border-neutral-800">
          <p className="mb-1 text-[11px] font-medium text-neutral-500 dark:text-neutral-400">Results</p>
          {results.map((pkg) => {
            const key = `${pkg.owner}/${pkg.name}`;
            const pin = pinBySpec[key] ?? "caret";
            return (
              <div key={key} className="border-b border-neutral-100 py-2 text-xs last:border-0 dark:border-neutral-900">
                <div className="flex items-center justify-between gap-2">
                  <span className="font-medium">
                    {pkg.owner}/{pkg.name}
                  </span>
                  <span className="text-neutral-500 dark:text-neutral-400">v{pkg.version}</span>
                </div>
                <p className="mt-0.5 line-clamp-2 text-neutral-500 dark:text-neutral-400">{pkg.description}</p>
                <div className="mt-1 flex items-center gap-2">
                  <select
                    value={pin}
                    onChange={(e) => setPinBySpec((prev) => ({ ...prev, [key]: e.target.value as Pin }))}
                    className="rounded border border-neutral-300 bg-white px-1.5 py-0.5 text-[11px] text-neutral-900 dark:border-neutral-700 dark:bg-neutral-900 dark:text-neutral-100"
                  >
                    <option value="caret">^{pkg.version} (caret)</option>
                    <option value="tilde">~{pkg.version} (tilde)</option>
                    <option value="exact">{pkg.version} (exact)</option>
                    <option value="latest">latest</option>
                  </select>
                  <button
                    type="button"
                    onClick={() => void runInstall(pkg.owner, pkg.name)}
                    disabled={busySpec !== null}
                    className="rounded bg-neutral-900 px-2 py-0.5 text-[11px] font-medium text-neutral-50 disabled:opacity-40 dark:bg-neutral-100 dark:text-neutral-900"
                  >
                    Install
                  </button>
                </div>
              </div>
            );
          })}
        </div>
      )}

      <div className="px-4 py-2">
        <p className="mb-1 text-[11px] font-medium text-neutral-500 dark:text-neutral-400">Installed</p>
        {installed.length === 0 && <p className="text-xs text-neutral-500 dark:text-neutral-400">Nothing installed for this environment yet.</p>}
        {installed.map((pkg) => (
          <div key={`${pkg.kind}-${pkg.name}`} className="flex items-center justify-between gap-2 border-b border-neutral-100 py-1.5 text-xs last:border-0 dark:border-neutral-900">
            <div>
              <span className="font-medium">{pkg.name}</span>
              <span className="ml-1.5 text-neutral-500 dark:text-neutral-400">v{pkg.version}</span>
              {outdatedNames.has(pkg.name) && (
                <span className="ml-1.5 rounded-full bg-amber-100 px-1.5 py-0.5 text-[10px] text-amber-800 dark:bg-amber-950 dark:text-amber-300">outdated</span>
              )}
            </div>
            {pkg.kind === "library" && (
              <button
                type="button"
                onClick={() => void runUninstall(pkg)}
                disabled={busySpec !== null}
                className="rounded border border-neutral-300 px-2 py-0.5 text-[11px] hover:bg-neutral-100 disabled:opacity-40 dark:border-neutral-700 dark:hover:bg-neutral-800"
              >
                Uninstall
              </button>
            )}
          </div>
        ))}
      </div>
    </div>
  );
}
