import { useMemo, useState } from "react";
import type { BoardBrief } from "../../lib/bindings";
import { boardFilterOptions, buildBoardIndex, EMPTY_FILTERS, filterBoards, type BoardFilters } from "./boardFilter";

const MAX_VISIBLE = 200;

function formatBytes(bytes: number): string {
  if (bytes >= 1024 * 1024) return `${Math.round(bytes / (1024 * 1024))} MB`;
  return `${Math.round(bytes / 1024)} KB`;
}

function Select({
  label,
  value,
  options,
  onChange,
}: {
  label: string;
  value: string | null;
  options: string[];
  onChange: (v: string | null) => void;
}) {
  return (
    <select
      aria-label={label}
      value={value ?? ""}
      onChange={(e) => onChange(e.target.value || null)}
      className="rounded border border-neutral-300 bg-white px-2 py-1 text-xs text-neutral-900 dark:border-neutral-700 dark:bg-neutral-900 dark:text-neutral-100"
    >
      <option value="">{label}</option>
      {options.map((o) => (
        <option key={o} value={o}>
          {o}
        </option>
      ))}
    </select>
  );
}

export function BoardPicker({
  boards,
  loading,
  selectedId,
  onSelect,
}: {
  boards: BoardBrief[];
  loading: boolean;
  selectedId: string | null;
  onSelect: (board: BoardBrief) => void;
}) {
  const index = useMemo(() => buildBoardIndex(boards), [boards]);
  const options = useMemo(() => boardFilterOptions(boards), [boards]);
  const [filters, setFilters] = useState<BoardFilters>(EMPTY_FILTERS);
  const filtered = useMemo(() => filterBoards(index, filters), [index, filters]);
  const visible = filtered.slice(0, MAX_VISIBLE);

  return (
    <div className="flex flex-col gap-2">
      <input
        type="text"
        placeholder="Search boards…"
        value={filters.query}
        onChange={(e) => setFilters((f) => ({ ...f, query: e.target.value }))}
        className="rounded border border-neutral-300 bg-transparent px-2 py-1.5 text-sm dark:border-neutral-700"
      />
      <div className="flex flex-wrap gap-2">
        <Select label="Platform" value={filters.platform} options={options.platforms} onChange={(v) => setFilters((f) => ({ ...f, platform: v }))} />
        <Select label="Framework" value={filters.framework} options={options.frameworks} onChange={(v) => setFilters((f) => ({ ...f, framework: v }))} />
        <Select label="Vendor" value={filters.vendor} options={options.vendors} onChange={(v) => setFilters((f) => ({ ...f, vendor: v }))} />
        <Select
          label="Connectivity"
          value={filters.connectivity}
          options={options.connectivity}
          onChange={(v) => setFilters((f) => ({ ...f, connectivity: v }))}
        />
      </div>

      {loading ? (
        <p className="text-xs text-neutral-500 dark:text-neutral-400">Loading boards…</p>
      ) : (
        <div className="max-h-64 overflow-y-auto rounded border border-neutral-200 dark:border-neutral-800">
          <table className="w-full text-left text-xs">
            <thead className="sticky top-0 bg-neutral-100 dark:bg-neutral-900">
              <tr>
                <th className="px-2 py-1 font-medium">Board</th>
                <th className="px-2 py-1 font-medium">Flash</th>
                <th className="px-2 py-1 font-medium">RAM</th>
                <th className="px-2 py-1 font-medium">MHz</th>
              </tr>
            </thead>
            <tbody>
              {visible.map((b) => (
                <tr
                  key={b.id}
                  onClick={() => onSelect(b)}
                  aria-selected={b.id === selectedId}
                  className={`cursor-pointer border-t border-neutral-200 hover:bg-neutral-100 dark:border-neutral-800 dark:hover:bg-neutral-800 ${
                    b.id === selectedId ? "bg-neutral-100 dark:bg-neutral-800" : ""
                  }`}
                >
                  <td className="px-2 py-1">
                    <div className="font-medium">{b.name}</div>
                    <div className="text-neutral-500 dark:text-neutral-400">
                      {b.id} · {b.vendor}
                    </div>
                  </td>
                  <td className="px-2 py-1">{formatBytes(b.rom)}</td>
                  <td className="px-2 py-1">{formatBytes(b.ram)}</td>
                  <td className="px-2 py-1">{Math.round(b.fcpu / 1_000_000)}</td>
                </tr>
              ))}
            </tbody>
          </table>
          {filtered.length > visible.length && (
            <p className="px-2 py-1 text-neutral-500 dark:text-neutral-400">
              {filtered.length - visible.length} more — refine your search
            </p>
          )}
          {filtered.length === 0 && (
            <p className="px-2 py-2 text-neutral-500 dark:text-neutral-400">No boards match.</p>
          )}
        </div>
      )}
    </div>
  );
}
