import { sessionStore, signOut } from "../features/auth/session";
import { PanelClient } from "./client";
import { createCoreApi } from "./core";

export const panelClient = new PanelClient({
  getUserKey: () => sessionStore.get()?.userKey,
  onUnauthorized: signOut,
});

/** Singleton typed API used by all features. */
export const api = createCoreApi(panelClient);

export { ApiError, errorMessage, isAbort } from "./client";
export type * from "./types";
export { ATOMIC_TYPES } from "./types";
