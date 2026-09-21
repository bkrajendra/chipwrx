import { open as openDialog } from "@tauri-apps/plugin-dialog";
import { useEffect, useState } from "react";
import type { BoardBrief, ProcEvent } from "../../lib/bindings";
import { boardsList, projectCancelCreate, projectCreate } from "../../lib/ipc";
import { BoardPicker } from "./BoardPicker";

export function CreateProjectForm({ onCreated, onClose }: { onCreated: () => void; onClose: () => void }) {
  const [boards, setBoards] = useState<BoardBrief[]>([]);
  const [boardsLoading, setBoardsLoading] = useState(true);
  const [selectedBoard, setSelectedBoard] = useState<BoardBrief | null>(null);
  const [framework, setFramework] = useState("");
  const [parentDir, setParentDir] = useState("");
  const [name, setName] = useState("");
  const [sampleCode, setSampleCode] = useState(true);
  const [initGit, setInitGit] = useState(true);
  const [generateClaudeMd, setGenerateClaudeMd] = useState(true);

  const [creating, setCreating] = useState(false);
  const [procId, setProcId] = useState<string | null>(null);
  const [events, setEvents] = useState<ProcEvent[]>([]);
  const [failed, setFailed] = useState<string | null>(null);

  useEffect(() => {
    let cancelled = false;
    void (async () => {
      try {
        const list = await boardsList(false, false);
        if (!cancelled) setBoards(list);
      } finally {
        if (!cancelled) setBoardsLoading(false);
      }
    })();
    return () => {
      cancelled = true;
    };
  }, []);

  const pickParentDir = async () => {
    const dir = await openDialog({ directory: true, multiple: false });
    if (typeof dir === "string") setParentDir(dir);
  };

  const selectBoard = (board: BoardBrief) => {
    setSelectedBoard(board);
    setFramework(board.frameworks[0] ?? "");
  };

  const canCreate = Boolean(selectedBoard && framework && parentDir && name.trim() && !creating);

  const handleCreate = async () => {
    if (!selectedBoard) return;
    setCreating(true);
    setEvents([]);
    setFailed(null);
    try {
      const id = await projectCreate(
        {
          parentDir,
          name: name.trim(),
          boardId: selectedBoard.id,
          framework,
          sampleCode,
          initGit,
          generateClaudeMd,
          extraOptions: [],
        },
        (event) => {
          setEvents((prev) => [...prev, event]);
          if (event.type === "finished") {
            setCreating(false);
            if (event.data.success) {
              onCreated();
            } else {
              setFailed(`pio project init exited with code ${event.data.exitCode}`);
            }
          }
        },
      );
      setProcId(id);
    } catch (e) {
      setCreating(false);
      setFailed(String(e));
    }
  };

  const handleCancel = async () => {
    if (procId) await projectCancelCreate(procId);
  };

  return (
    <div className="rounded border border-neutral-200 p-4 dark:border-neutral-800">
      <div className="mb-3 flex items-center justify-between">
        <h3 className="text-sm font-semibold">Create a project</h3>
        <button type="button" onClick={onClose} className="text-xs text-neutral-500 hover:underline dark:text-neutral-400">
          Cancel
        </button>
      </div>

      <div className="flex flex-col gap-4">
        <div>
          <label className="mb-1 block text-xs font-medium">Board</label>
          <BoardPicker boards={boards} loading={boardsLoading} selectedId={selectedBoard?.id ?? null} onSelect={selectBoard} />
        </div>

        {selectedBoard && selectedBoard.frameworks.length > 1 && (
          <div>
            <label className="mb-1 block text-xs font-medium">Framework</label>
            <select
              value={framework}
              onChange={(e) => setFramework(e.target.value)}
              className="rounded border border-neutral-300 bg-white px-2 py-1 text-sm text-neutral-900 dark:border-neutral-700 dark:bg-neutral-900 dark:text-neutral-100"
            >
              {selectedBoard.frameworks.map((f) => (
                <option key={f} value={f}>
                  {f}
                </option>
              ))}
            </select>
          </div>
        )}

        <div className="flex gap-2">
          <div className="flex-1">
            <label className="mb-1 block text-xs font-medium">Parent directory</label>
            <div className="flex gap-2">
              <input
                readOnly
                value={parentDir}
                placeholder="Choose a folder…"
                className="flex-1 rounded border border-neutral-300 bg-transparent px-2 py-1.5 text-sm dark:border-neutral-700"
              />
              <button
                type="button"
                onClick={() => void pickParentDir()}
                className="rounded border border-neutral-300 px-2.5 py-1 text-xs font-medium hover:bg-neutral-100 dark:border-neutral-700 dark:hover:bg-neutral-800"
              >
                Choose…
              </button>
            </div>
          </div>
          <div>
            <label className="mb-1 block text-xs font-medium">Project name</label>
            <input
              value={name}
              onChange={(e) => setName(e.target.value)}
              placeholder="greenhouse-sensor"
              className="rounded border border-neutral-300 bg-transparent px-2 py-1.5 text-sm dark:border-neutral-700"
            />
          </div>
        </div>

        <div className="flex flex-wrap gap-4 text-xs">
          <label className="flex items-center gap-1.5">
            <input type="checkbox" checked={sampleCode} onChange={(e) => setSampleCode(e.target.checked)} />
            Sample code
          </label>
          <label className="flex items-center gap-1.5">
            <input type="checkbox" checked={initGit} onChange={(e) => setInitGit(e.target.checked)} />
            Initialize git
          </label>
          <label className="flex items-center gap-1.5">
            <input type="checkbox" checked={generateClaudeMd} onChange={(e) => setGenerateClaudeMd(e.target.checked)} />
            Generate CLAUDE.md
          </label>
        </div>

        {!creating && (
          <button
            type="button"
            disabled={!canCreate}
            onClick={() => void handleCreate()}
            className="self-start rounded bg-neutral-900 px-3 py-1.5 text-xs font-medium text-neutral-50 disabled:opacity-40 dark:bg-neutral-100 dark:text-neutral-900"
          >
            Create
          </button>
        )}

        {creating && (
          <div>
            <div className="mb-2 flex items-center justify-between">
              <span className="text-xs text-neutral-500 dark:text-neutral-400">
                Creating — first platform download can take a few minutes…
              </span>
              <button
                type="button"
                onClick={() => void handleCancel()}
                className="rounded border border-neutral-300 px-2.5 py-1 text-xs font-medium hover:bg-neutral-100 dark:border-neutral-700 dark:hover:bg-neutral-800"
              >
                Cancel
              </button>
            </div>
            <div className="max-h-40 overflow-y-auto rounded bg-neutral-950 p-2 font-mono text-xs text-neutral-100">
              {events.map((event, i) => {
                if (event.type === "started") return <div key={i}>$ {event.data.argv.join(" ")}</div>;
                if (event.type === "lines")
                  return event.data.lines.map((l, j) => <div key={`${i}-${j}`}>{l.text}</div>);
                if (event.type === "finished")
                  return (
                    <div key={i} className={event.data.success ? "text-emerald-400" : "text-red-400"}>
                      {event.data.success ? "Done." : `Failed (exit ${event.data.exitCode}).`}
                    </div>
                  );
                return null;
              })}
            </div>
          </div>
        )}

        {failed && <p className="text-xs text-red-500">{failed}</p>}
      </div>
    </div>
  );
}
