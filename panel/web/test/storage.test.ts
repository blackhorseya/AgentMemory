import { afterEach, describe, expect, it, vi } from "vitest";
import { readJson, writeJson } from "../src/lib/storage";
import { formatDuration, formatRelative, formatTime } from "../src/lib/format";

const isNum = (v: unknown): v is number => typeof v === "number";

describe("storage", () => {
  afterEach(() => vi.unstubAllGlobals());

  it("is a no-op when localStorage is missing", () => {
    expect(() => writeJson("k", 1)).not.toThrow();
    expect(readJson("k", isNum)).toBeUndefined();
  });

  it("swallows storage exceptions", () => {
    vi.stubGlobal("localStorage", {
      getItem: () => {
        throw new Error("SecurityError");
      },
      setItem: () => {
        throw new Error("QuotaExceeded");
      },
    });
    expect(() => writeJson("k", 1)).not.toThrow();
    expect(readJson("k", isNum)).toBeUndefined();
  });

  it("round-trips and validates values", () => {
    const data = new Map<string, string>();
    vi.stubGlobal("localStorage", {
      getItem: (k: string) => data.get(k) ?? null,
      setItem: (k: string, v: string) => data.set(k, v),
    });
    writeJson("n", 42);
    writeJson("s", "x");
    expect(readJson("n", isNum)).toBe(42);
    expect(readJson("s", isNum)).toBeUndefined();
  });
});

describe("format", () => {
  it("formats durations", () => {
    expect(formatDuration(42)).toBe("42s");
    expect(formatDuration(3_725)).toBe("1h 2m");
    expect(formatDuration(90_000)).toBe("1d 1h");
    expect(formatDuration(undefined)).toBe("—");
  });

  it("accepts epoch seconds, ms and ISO strings", () => {
    const ms = Date.UTC(2026, 0, 2, 3, 4, 5);
    expect(formatTime(ms)).toBe(formatTime(ms / 1000));
    expect(formatTime(new Date(ms).toISOString())).toBe(formatTime(ms));
    expect(formatTime(null)).toBe("—");
  });

  it("formats relative times", () => {
    expect(formatRelative(1_800_000_000_000 - 30_000, 1_800_000_000_000)).toBe("30s ago");
  });
});
