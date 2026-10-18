import { randomUUID } from "node:crypto";
import type { MiddlewareHandler } from "hono";
import type { AppEnv } from "../app-env.js";

const VALID_REQUEST_ID = /^[A-Za-z0-9._:-]{1,128}$/;

/** Reuses a sane incoming `x-request-id` or mints one; echoes it on the response. */
export function requestId(): MiddlewareHandler<AppEnv> {
  return async (c, next) => {
    const incoming = c.req.header("x-request-id");
    const id = incoming && VALID_REQUEST_ID.test(incoming) ? incoming : `panel-${randomUUID()}`;
    c.set("requestId", id);
    await next();
    c.header("x-request-id", id);
  };
}
