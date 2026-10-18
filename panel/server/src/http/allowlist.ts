/**
 * Explicit allowlist of Core `/v3/*` paths the panel UI may call through the
 * BFF. Anything else is rejected with 404 so the BFF (which holds the Core
 * API key) never becomes an open proxy for destructive operations such as
 * `instance/destroy` or `internal/meta/*`.
 */
export const CORE_V3_ALLOWLIST: ReadonlySet<string> = new Set([
  // monitoring
  "pipeline/status",
  "stats/overview",
  // recall test
  "recall",
  // L0
  "conversation/query",
  "conversation/search",
  "conversation/delete",
  // L1
  "atomic/query",
  "atomic/search",
  "atomic/update",
  "atomic/delete",
  // L2
  "scenario/ls",
  "scenario/read",
  "scenario/write",
  // L3
  "core/read",
  "core/write",
  // generation logs
  "memory-generation-log/list",
  "memory-generation-log/get",
  // skills
  "skill/list",
  "skill/search",
  "skill/get",
  "skill/versions",
  // metadata
  "meta/auth/verify",
  "meta/team/list",
  "meta/team-member/list",
  "meta/agent/list",
  "meta/asset/list-accessible",
]);

/** Paths callable without a verified user key (the login call itself). */
export const PUBLIC_V3_PATHS: ReadonlySet<string> = new Set(["meta/auth/verify"]);

const SAFE_SEGMENT = /^[a-z0-9-]+$/;

/**
 * Turns the wildcard tail of `/api/core/<tail>` into a Core v3 path, or
 * `undefined` when it is malformed or not allowlisted.
 */
export function resolveV3Path(tail: string, allowlist: ReadonlySet<string> = CORE_V3_ALLOWLIST): string | undefined {
  const segments = tail.split("/").filter((s) => s.length > 0);
  if (segments.length === 0 || !segments.every((s) => SAFE_SEGMENT.test(s))) return undefined;
  const path = segments.join("/");
  return allowlist.has(path) ? path : undefined;
}
