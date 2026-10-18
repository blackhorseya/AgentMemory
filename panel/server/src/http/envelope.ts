/** Same envelope shape as MemoryCore so the browser has one error path. */
export interface Envelope<T = unknown> {
  readonly code: number;
  readonly message: string;
  readonly request_id: string;
  readonly data?: T;
}

export function errorEnvelope(code: number, message: string, requestId: string): Envelope<never> {
  return { code, message, request_id: requestId };
}

export const PanelErrors = {
  NOT_FOUND: "NOT_FOUND",
  BAD_REQUEST: "BAD_REQUEST",
  UNAUTHORIZED: "unauthorized: invalid_user_key",
  MISSING_USER_KEY: "unauthorized: missing_user_key",
  CORE_TIMEOUT: "CORE_TIMEOUT",
  CORE_UNAVAILABLE: "CORE_UNAVAILABLE",
  INTERNAL: "INTERNAL_ERROR",
} as const;
