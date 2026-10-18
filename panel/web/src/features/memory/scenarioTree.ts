import type { ScenarioEntry } from "../../api";

export interface TreeRow {
  /** Full path; directories end with "/". */
  path: string;
  name: string;
  depth: number;
  isDir: boolean;
  entry?: ScenarioEntry;
}

/**
 * Flattens `scenario/ls` entries into a depth-annotated, sorted row list.
 * Directories are derived from file paths so the tree is correct whether or
 * not Core lists directory entries explicitly.
 */
export function buildScenarioTree(entries: readonly ScenarioEntry[]): TreeRow[] {
  const files = entries.filter((e) => !e.path.endsWith("/"));
  const dirs = new Set<string>();
  for (const file of files) {
    const parts = file.path.split("/").slice(0, -1);
    parts.forEach((_, i) => dirs.add(`${parts.slice(0, i + 1).join("/")}/`));
  }
  for (const e of entries) if (e.path.endsWith("/")) dirs.add(e.path);

  const rows: TreeRow[] = [
    ...[...dirs].map((path) => toRow(path, true)),
    ...files.map((entry) => ({ ...toRow(entry.path, false), entry })),
  ];
  return rows.sort((a, b) => comparePaths(a.path, b.path));
}

function toRow(path: string, isDir: boolean): TreeRow {
  const parts = path.replace(/\/$/, "").split("/");
  return { path, name: parts[parts.length - 1] ?? path, depth: parts.length - 1, isDir };
}

/** Sorts so a directory precedes its children and siblings sort dirs-first. */
function comparePaths(a: string, b: string): number {
  const pa = segments(a);
  const pb = segments(b);
  const n = Math.min(pa.length, pb.length);
  for (let i = 0; i < n; i++) {
    const x = pa[i] ?? "";
    const y = pb[i] ?? "";
    if (x === y) continue;
    const xDir = i < pa.length - 1 || a.endsWith("/");
    const yDir = i < pb.length - 1 || b.endsWith("/");
    if (xDir !== yDir) return xDir ? -1 : 1;
    return x.localeCompare(y);
  }
  return pa.length - pb.length;
}

function segments(path: string): string[] {
  return path.replace(/\/$/, "").split("/");
}
