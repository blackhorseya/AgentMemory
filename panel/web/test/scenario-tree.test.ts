import { describe, expect, it } from "vitest";
import { buildScenarioTree } from "../src/features/memory/scenarioTree";

const entry = (path: string) => ({ path, version: path.endsWith("/") ? 0 : 1 });

describe("buildScenarioTree", () => {
  it("derives directories and orders dirs before files", () => {
    const rows = buildScenarioTree([entry("z.md"), entry("work/release.md"), entry("work/deep/a.md"), entry("a.md")]);
    expect(rows.map((r) => `${"  ".repeat(r.depth)}${r.name}${r.isDir ? "/" : ""}`)).toEqual([
      "work/",
      "  deep/",
      "    a.md",
      "  release.md",
      "a.md",
      "z.md",
    ]);
  });

  it("does not duplicate explicit directory entries", () => {
    const rows = buildScenarioTree([entry("work/"), entry("work/x.md")]);
    expect(rows.filter((r) => r.isDir)).toHaveLength(1);
  });
});
