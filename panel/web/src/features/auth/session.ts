import type { UserPublic } from "../../api/types";
import { createPersistedStore } from "../../lib/store";

export interface Session {
  userKey: string;
  user: UserPublic;
  verifiedAt: number;
}

function isSession(v: unknown): v is Session | null {
  if (v === null) return true;
  if (typeof v !== "object") return false;
  const s = v as Partial<Session>;
  return typeof s.userKey === "string" && typeof s.user === "object" && s.user !== null && typeof s.user.user_id === "string";
}

export const sessionStore = createPersistedStore<Session | null>("memory-panel.session", null, isSession);

export function signOut(): void {
  sessionStore.set(null);
}
