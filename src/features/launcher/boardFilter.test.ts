import { describe, expect, it } from "vitest";
import type { BoardBrief } from "../../lib/bindings";
import { boardFilterOptions, buildBoardIndex, EMPTY_FILTERS, filterBoards } from "./boardFilter";

function board(overrides: Partial<BoardBrief>): BoardBrief {
  return {
    id: "esp32dev",
    name: "Espressif ESP32 Dev Module",
    platform: "espressif32",
    mcu: "ESP32",
    fcpu: 240_000_000,
    ram: 327_680,
    rom: 4_194_304,
    frameworks: ["arduino", "espidf"],
    vendor: "Espressif",
    url: "https://example.com",
    connectivity: ["wifi", "bluetooth"],
    debugTools: [],
    ...overrides,
  };
}

const SAMPLE: BoardBrief[] = [
  board({ id: "esp32dev" }),
  board({
    id: "uno",
    name: "Arduino Uno",
    platform: "atmelavr",
    mcu: "ATMEGA328P",
    vendor: "Arduino",
    frameworks: ["arduino"],
    connectivity: [],
  }),
  board({
    id: "bluepill",
    name: "BluePill F103C8",
    platform: "ststm32",
    mcu: "STM32F103C8",
    vendor: "Generic",
    frameworks: ["arduino", "mbed"],
    connectivity: [],
  }),
];

describe("filterBoards", () => {
  it("returns everything with no filters", () => {
    const index = buildBoardIndex(SAMPLE);
    expect(filterBoards(index, EMPTY_FILTERS)).toHaveLength(3);
  });

  it("matches the query against id, name, mcu, vendor, platform, and frameworks", () => {
    const index = buildBoardIndex(SAMPLE);
    expect(filterBoards(index, { ...EMPTY_FILTERS, query: "esp32" }).map((b) => b.id)).toEqual(["esp32dev"]);
    expect(filterBoards(index, { ...EMPTY_FILTERS, query: "stm32" }).map((b) => b.id)).toEqual(["bluepill"]);
    expect(filterBoards(index, { ...EMPTY_FILTERS, query: "arduino" }).map((b) => b.id).sort()).toEqual([
      "bluepill",
      "esp32dev",
      "uno",
    ]);
  });

  it("query matching is case-insensitive", () => {
    const index = buildBoardIndex(SAMPLE);
    expect(filterBoards(index, { ...EMPTY_FILTERS, query: "ESP32" }).map((b) => b.id)).toEqual(["esp32dev"]);
  });

  it("filters by platform", () => {
    const index = buildBoardIndex(SAMPLE);
    expect(filterBoards(index, { ...EMPTY_FILTERS, platform: "atmelavr" }).map((b) => b.id)).toEqual(["uno"]);
  });

  it("filters by framework membership", () => {
    const index = buildBoardIndex(SAMPLE);
    expect(filterBoards(index, { ...EMPTY_FILTERS, framework: "mbed" }).map((b) => b.id)).toEqual(["bluepill"]);
  });

  it("filters by connectivity membership", () => {
    const index = buildBoardIndex(SAMPLE);
    expect(filterBoards(index, { ...EMPTY_FILTERS, connectivity: "wifi" }).map((b) => b.id)).toEqual(["esp32dev"]);
  });

  it("combines query and filters with AND semantics", () => {
    const index = buildBoardIndex(SAMPLE);
    const result = filterBoards(index, { ...EMPTY_FILTERS, query: "arduino", platform: "ststm32" });
    expect(result.map((b) => b.id)).toEqual(["bluepill"]);
  });
});

describe("boardFilterOptions", () => {
  it("collects distinct, sorted values for each facet", () => {
    const opts = boardFilterOptions(SAMPLE);
    expect(opts.platforms).toEqual(["atmelavr", "espressif32", "ststm32"]);
    expect(opts.vendors).toEqual(["Arduino", "Espressif", "Generic"]);
    expect(opts.frameworks).toEqual(["arduino", "espidf", "mbed"]);
    expect(opts.connectivity).toEqual(["bluetooth", "wifi"]);
  });
});

describe("NFR-P4: ~1500 entries filter in well under 50ms", () => {
  it("stays fast at realistic catalogue scale", () => {
    const many: BoardBrief[] = Array.from({ length: 1500 }, (_, i) =>
      board({ id: `board-${i}`, name: `Board ${i}`, platform: i % 2 === 0 ? "espressif32" : "atmelavr" }),
    );
    const index = buildBoardIndex(many);

    const start = performance.now();
    const result = filterBoards(index, { ...EMPTY_FILTERS, query: "board-1", platform: "espressif32" });
    const elapsed = performance.now() - start;

    expect(result.length).toBeGreaterThan(0);
    expect(elapsed).toBeLessThan(50);
  });
});
