import { open as openDialog } from "@tauri-apps/plugin-dialog";
import { useEffect, useState } from "react";
import type { AppError, ProjectEntry } from "../../lib/bindings";
import { renderAppError } from "../../lib/errors";
import { projectOpenInEditor, projectReveal, projectScanTrust, projectTrust } from "../../lib/ipc";
import { CreateProjectForm } from "./CreateProjectForm";
import { useProjects } from "./useProjects";

function isAppError(e: unknown): e is AppError {
  return typeof e === "object" && e !== null && "code" in e;
}

/** Splits an OS path into (parent, base-name) without assuming a separator style. */
function splitPath(path: string): { parent: string; name: string } {
  const normalized = path.replace(/\\/g, "/").replace(/\/+$/, "");
  const idx = normalized.lastIndexOf("/");
  return idx === -1 ? { parent: "", name: normalized } : { parent: normalized.slice(0, idx), name: normalized.slice(idx + 1) };
}

function TrustPrompt({ path, onTrusted, onCancel }: { path: string; id: string; onTrusted: () => void; onCancel: () => void }) {
  const [scan, setScan] = useState<{ hooks: string[]; mcpServers: string[]; agents: string[] } | null>(null);

  useEffect(() => {
    void projectScanTrust(path).then(setScan);
  }, [path]);

  return (
    <div className="rounded border border-amber-400 bg-amber-50 p-3 text-xs dark:border-amber-700 dark:bg-amber-950">
      <p className="mb-2 font-medium">This folder wasn't created by Vibe Hardware</p>
      <p className="mb-2 text-neutral-600 dark:text-neutral-400">
        Claude Code runs a project's hooks and connects its MCP servers with no prompt of its own. Review what this
        folder contains before the first turn.
      </p>
      {scan && (
        <ul className="mb-3 list-disc pl-4">
          {scan.hooks.length > 0 && <li>Hooks: {scan.hooks.join(", ")}</li>}
          {scan.mcpServers.length > 0 && <li>MCP servers: {scan.mcpServers.join(", ")}</li>}
          {scan.agents.length > 0 && <li>Agents: {scan.agents.join(", ")}</li>}
          {scan.hooks.length === 0 && scan.mcpServers.length === 0 && scan.agents.length === 0 && <li>Nothing found.</li>}
        </ul>
      )}
      <div className="flex gap-2">
        <button
          type="button"
          onClick={onTrusted}
          className="rounded bg-neutral-900 px-2.5 py-1 font-medium text-neutral-50 dark:bg-neutral-100 dark:text-neutral-900"
        >
          Trust this folder
        </button>
        <button type="button" onClick={onCancel} className="rounded border border-neutral-300 px-2.5 py-1 dark:border-neutral-700">
          Cancel
        </button>
      </div>
    </div>
  );
}

function ProjectRow({
  entry,
  onOpenInEditor,
  onReveal,
  onOpenChat,
}: {
  entry: ProjectEntry;
  onOpenInEditor: () => void;
  onReveal: () => void;
  onOpenChat: () => void;
}) {
  return (
    <div className="flex items-center justify-between border-b border-neutral-200 py-2.5 last:border-0 dark:border-neutral-800">
      <div>
        <div className="flex items-center gap-2 text-sm font-medium">
          {entry.name}
          {!entry.exists && (
            <span className="rounded bg-red-100 px-1.5 py-0.5 text-[10px] font-normal text-red-700 dark:bg-red-950 dark:text-red-400">
              missing
            </span>
          )}
          {!entry.trusted && (
            <span className="rounded bg-amber-100 px-1.5 py-0.5 text-[10px] font-normal text-amber-700 dark:bg-amber-950 dark:text-amber-400">
              untrusted
            </span>
          )}
        </div>
        <div className="text-xs text-neutral-500 dark:text-neutral-400">
          {entry.path} {entry.boardId ? `· ${entry.boardId}` : ""}
        </div>
      </div>
      {entry.exists && (
        <div className="flex gap-2">
          <button type="button" onClick={onReveal} className="text-xs text-neutral-500 hover:underline dark:text-neutral-400">
            Reveal
          </button>
          <button type="button" onClick={onOpenInEditor} className="text-xs text-neutral-500 hover:underline dark:text-neutral-400">
            Open in editor
          </button>
          <button
            type="button"
            onClick={onOpenChat}
            className="rounded bg-neutral-900 px-2 py-0.5 text-xs font-medium text-neutral-50 dark:bg-neutral-100 dark:text-neutral-900"
          >
            Chat
          </button>
        </div>
      )}
    </div>
  );
}

export function LauncherScreen({ onOpenWorkspace }: { onOpenWorkspace: (entry: ProjectEntry) => void }) {
  const { projects, loading, refresh, open } = useProjects();
  const [showCreate, setShowCreate] = useState(false);
  const [pendingInit, setPendingInit] = useState<{ parent: string; name: string } | null>(null);
  const [pendingTrust, setPendingTrust] = useState<ProjectEntry | null>(null);
  const [trustThenOpenChat, setTrustThenOpenChat] = useState(false);
  const [openError, setOpenError] = useState<string | null>(null);

  const handleOpenChat = (entry: ProjectEntry) => {
    if (entry.trusted) {
      onOpenWorkspace(entry);
    } else {
      setTrustThenOpenChat(true);
      setPendingTrust(entry);
    }
  };

  /** `projectOpenInEditor`/`projectReveal` were previously fire-and-forget (`void
   * projectX(...)`), which silently swallowed a failed spawn — the button just appeared to
   * do nothing. Surfaces the error instead. */
  const runAction = async (action: () => Promise<void>) => {
    setOpenError(null);
    try {
      await action();
    } catch (e) {
      setOpenError(isAppError(e) ? renderAppError(e).message : String(e));
    }
  };

  const handleOpenFolder = async () => {
    const dir = await openDialog({ directory: true, multiple: false });
    if (typeof dir !== "string") return;
    setOpenError(null);
    try {
      const entry = await open(dir);
      if (!entry.trusted) {
        setPendingTrust(entry);
      }
    } catch (e) {
      if (isAppError(e) && e.code === "NOT_A_PIO_PROJECT") {
        setPendingInit(splitPath(e.path));
      } else {
        setOpenError(String(isAppError(e) ? e.code : e));
      }
    }
  };

  return (
    <div className="mx-auto max-w-xl px-4 py-6">
      <div className="mb-4 flex items-center justify-between">
        <h2 className="text-sm font-semibold">Projects</h2>
        <div className="flex gap-2">
          <button
            type="button"
            onClick={() => void handleOpenFolder()}
            className="rounded border border-neutral-300 px-2.5 py-1 text-xs font-medium hover:bg-neutral-100 dark:border-neutral-700 dark:hover:bg-neutral-800"
          >
            Open existing folder
          </button>
          <button
            type="button"
            onClick={() => setShowCreate(true)}
            className="rounded bg-neutral-900 px-2.5 py-1 text-xs font-medium text-neutral-50 dark:bg-neutral-100 dark:text-neutral-900"
          >
            Create a project
          </button>
        </div>
      </div>

      {openError && <p className="mb-3 text-xs text-red-500">{openError}</p>}

      {pendingInit && (
        <div className="mb-4">
          <p className="mb-2 text-xs text-neutral-600 dark:text-neutral-400">
            <strong>{pendingInit.name}</strong> has no <code>platformio.ini</code> — pick a board to initialize it.
          </p>
          <CreateProjectForm
            onCreated={() => {
              setPendingInit(null);
              void refresh();
            }}
            onClose={() => setPendingInit(null)}
          />
        </div>
      )}

      {pendingTrust && (
        <div className="mb-4">
          <TrustPrompt
            path={pendingTrust.path}
            id={pendingTrust.id}
            onTrusted={() => {
              void projectTrust(pendingTrust.id).then(() => {
                if (trustThenOpenChat) {
                  onOpenWorkspace({ ...pendingTrust, trusted: true });
                }
                setPendingTrust(null);
                setTrustThenOpenChat(false);
                void refresh();
              });
            }}
            onCancel={() => {
              setPendingTrust(null);
              setTrustThenOpenChat(false);
            }}
          />
        </div>
      )}

      {showCreate && !pendingInit && (
        <div className="mb-4">
          <CreateProjectForm
            onCreated={() => {
              setShowCreate(false);
              void refresh();
            }}
            onClose={() => setShowCreate(false)}
          />
        </div>
      )}

      {loading && projects.length === 0 && <p className="text-sm text-neutral-500 dark:text-neutral-400">Loading…</p>}
      {!loading && projects.length === 0 && !showCreate && !pendingInit && (
        <p className="text-sm text-neutral-500 dark:text-neutral-400">No projects yet — create or open one to get started.</p>
      )}

      {projects.length > 0 && (
        <div className="rounded border border-neutral-200 px-3 dark:border-neutral-800">
          {projects.map((entry) => (
            <ProjectRow
              key={entry.id}
              entry={entry}
              onOpenInEditor={() => void runAction(() => projectOpenInEditor(entry.id))}
              onReveal={() => void runAction(() => projectReveal(entry.id))}
              onOpenChat={() => handleOpenChat(entry)}
            />
          ))}
        </div>
      )}
    </div>
  );
}
