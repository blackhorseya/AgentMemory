import type { Scope } from "../../api";
import { createPersistedStore } from "../../lib/store";

export const DEFAULT_SCOPE: Scope = { team_id: "default", agent_id: "default", user_id: "default" };

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
