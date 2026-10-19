import type { Scope } from "../../api";
import { createPersistedStore } from "../../lib/store";

/** Empty until the picker selects a real team and agent (Core has no default bucket). */
export const DEFAULT_SCOPE: Scope = { team_id: "", agent_id: "", user_id: "" };

export function isComplete(scope: Scope): boolean {
  return !!scope.team_id && !!scope.agent_id;
}

function isScope(v: unknown): v is Scope {
  if (typeof v !== "object" || v === null) return false;
  const s = v as Record<string, unknown>;
  return typeof s.team_id === "string" && typeof s.agent_id === "string" && typeof s.user_id === "string";
}

/** Memory-browser isolation scope, remembered per browser. */
export const scopeStore = createPersistedStore<Scope>("memory-panel.scope", DEFAULT_SCOPE, isScope);

export function scopeKey(scope: Scope): string {
  return `${scope.team_id}/${scope.agent_id}/${scope.user_id}`;
}
