// Canvas "Changes" tab (`FR-UI-3`, `FR-SAFE-*`) — wraps `ChangesPanel` with a turn picker,
// since a persistent tab (unlike the old per-turn "Changes" side panel) needs to pick
// *which* turn's snapshot to diff against rather than always being opened for one.

import { useEffect, useState } from "react";
import type { TurnRecord } from "../../lib/bindings";
import { ChangesPanelBody } from "./ChangesPanel";

function turnLabel(turn: TurnRecord): string {
  const preview = turn.prompt.length > 60 ? `${turn.prompt.slice(0, 60)}…` : turn.prompt;
  return `${new Date(turn.startedAt).toLocaleTimeString()} — ${preview}`;
}

export function ChangesTab({ workspaceId, turns }: { workspaceId: string; turns: TurnRecord[] }) {
  const withIds = turns.filter((t) => t.turnId);
  const [selectedTurnId, setSelectedTurnId] = useState<string | null>(null);

  useEffect(() => {
    // Default to (and keep following) the most recent turn until the user explicitly
    // picks an earlier one.
    setSelectedTurnId((prev) => {
      if (prev && withIds.some((t) => t.turnId === prev)) return prev;
      return withIds.length > 0 ? withIds[withIds.length - 1].turnId : null;
    });
    // eslint-disable-next-line react-hooks/exhaustive-deps -- only re-derive when the *set* of turn ids actually changes
  }, [withIds.map((t) => t.turnId).join(",")]);

  if (!selectedTurnId) {
    return <p className="p-4 text-xs text-neutral-500 dark:text-neutral-400">No turns yet — changes appear here once Claude edits something.</p>;
  }

  return (
    <div className="flex h-full flex-col">
      <div className="border-b border-neutral-200 px-4 py-2 dark:border-neutral-800">
        <label className="flex items-center gap-2 text-xs">
          <span className="text-neutral-500 dark:text-neutral-400">Turn</span>
          <select
            value={selectedTurnId}
            onChange={(e) => setSelectedTurnId(e.target.value)}
            className="min-w-0 flex-1 rounded border border-neutral-300 bg-white px-2 py-1 text-xs text-neutral-900 dark:border-neutral-700 dark:bg-neutral-900 dark:text-neutral-100"
          >
            {withIds.map((t) => (
              <option key={t.turnId} value={t.turnId}>
                {turnLabel(t)}
              </option>
            ))}
          </select>
        </label>
      </div>
      <div className="min-h-0 flex-1">
        <ChangesPanelBody workspaceId={workspaceId} turnId={selectedTurnId} />
      </div>
    </div>
  );
}
