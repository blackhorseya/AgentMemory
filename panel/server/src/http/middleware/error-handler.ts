import type { Context } from "hono";
import type { ContentfulStatusCode } from "hono/utils/http-status";
import { CoreTimeoutError, CoreUnavailableError } from "../../core-client/errors.js";
import type { AppEnv } from "../app-env.js";
import { errorEnvelope, PanelErrors } from "../envelope.js";

export interface MappedError {
  readonly status: ContentfulStatusCode;
  readonly message: string;
}

/** Maps any thrown error to an HTTP status + envelope message. */
export function mapError(err: unknown): MappedError {
  if (err instanceof CoreTimeoutError) return { status: 504, message: PanelErrors.CORE_TIMEOUT };
  if (err instanceof CoreUnavailableError) return { status: 502, message: PanelErrors.CORE_UNAVAILABLE };
  return { status: 500, message: PanelErrors.INTERNAL };
}

export function handleError(err: Error, c: Context<AppEnv>): Response {
  const { status, message } = mapError(err);
  if (status === 500) console.error("[panel] unhandled error", err);
  else console.warn(`[panel] ${message}: ${err.message}`);
  return c.json(errorEnvelope(status, message, c.get("requestId") ?? "unknown"), status);
}
