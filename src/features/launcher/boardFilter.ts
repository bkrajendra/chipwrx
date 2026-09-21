// Client-side board catalogue filtering (NFR-P4: ~1500 entries, filters in <50ms — index
// it once on load rather than re-scanning raw fields per keystroke).

import type { BoardBrief } from "../../lib/bindings";

export interface IndexedBoard {
  board: BoardBrief;
  /** Lowercase concatenation of id, name, mcu, vendor, platform, frameworks. */
  searchBlob: string;
}

export function buildBoardIndex(boards: BoardBrief[]): IndexedBoard[] {
  return boards.map((board) => ({
    board,
    searchBlob: [board.id, board.name, board.mcu, board.vendor, board.platform, ...board.frameworks]
      .join(" ")
      .toLowerCase(),
  }));
}

export interface BoardFilters {
  query: string;
  platform: string | null;
  framework: string | null;
  vendor: string | null;
  connectivity: string | null;
}

export const EMPTY_FILTERS: BoardFilters = {
  query: "",
  platform: null,
  framework: null,
  vendor: null,
  connectivity: null,
};

export function filterBoards(index: IndexedBoard[], filters: BoardFilters): BoardBrief[] {
  const q = filters.query.trim().toLowerCase();
  return index
    .filter(({ board, searchBlob }) => {
      if (q && !searchBlob.includes(q)) return false;
      if (filters.platform && board.platform !== filters.platform) return false;
      if (filters.framework && !board.frameworks.includes(filters.framework)) return false;
      if (filters.vendor && board.vendor !== filters.vendor) return false;
      if (filters.connectivity && !board.connectivity.includes(filters.connectivity)) return false;
      return true;
    })
    .map((e) => e.board);
}

function uniqueSorted(values: Iterable<string>): string[] {
  return Array.from(new Set(values)).sort((a, b) => a.localeCompare(b));
}

/** Distinct filter option values, for populating the platform/framework/vendor/connectivity dropdowns. */
export function boardFilterOptions(boards: BoardBrief[]) {
  return {
    platforms: uniqueSorted(boards.map((b) => b.platform)),
    frameworks: uniqueSorted(boards.flatMap((b) => b.frameworks)),
    vendors: uniqueSorted(boards.map((b) => b.vendor)),
    connectivity: uniqueSorted(boards.flatMap((b) => b.connectivity)),
  };
}
