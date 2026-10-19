import { Hono, type Context } from "hono";
import type { ContentfulStatusCode } from "hono/utils/http-status";
import type { UserKeyVerifier } from "../../auth/user-key-verifier.js";
import type { CoreClient, CoreResponse } from "../../core-client/port.js";
import type { AppEnv } from "../app-env.js";
import { ADMIN_ONLY_V3_PATHS, PUBLIC_V3_PATHS, resolveV3Path } from "../allowlist.js";
import { errorEnvelope, PanelErrors } from "../envelope.js";

export interface CoreProxyDeps {
  readonly core: CoreClient;
  /** When set, non-public paths require a valid `x-memory-user-key`. */
  readonly verifier: UserKeyVerifier | undefined;
}

const PREFIX = "/api/core/";

/** `POST /api/core/*` → Core `POST /v3/*` (allowlisted) and `GET /api/core-health`. */
export function coreProxyRoutes(deps: CoreProxyDeps): Hono<AppEnv> {
  const app = new Hono<AppEnv>();

  app.get("/api/core-health", async (c) => {
    const response = await deps.core.health({ requestId: c.get("requestId") });
    return relay(c, response);
  });

  app.post(`${PREFIX}*`, async (c) => {
    const requestId = c.get("requestId");
    const path = resolveV3Path(c.req.path.slice(PREFIX.length));
    if (!path) return c.json(errorEnvelope(404, PanelErrors.NOT_FOUND, requestId), 404);

    const body = await readJsonBody(c);
    if (body === INVALID_JSON) return c.json(errorEnvelope(400, PanelErrors.BAD_REQUEST, requestId), 400);

    const userKey = c.req.header("x-memory-user-key")?.trim() || undefined;
    const denied = await checkUserKey(deps.verifier, path, userKey, requestId);
    if (denied) return c.json(errorEnvelope(denied.status, denied.message, requestId), denied.status);

    const response = await deps.core.callV3(path, body, { requestId, userKey });
    return relay(c, response);
  });

  return app;
}

async function checkUserKey(
  verifier: UserKeyVerifier | undefined,
  path: string,
  userKey: string | undefined,
  requestId: string,
): Promise<{ status: 401 | 403; message: string } | undefined> {
  if (!verifier || PUBLIC_V3_PATHS.has(path)) return undefined;
  if (!userKey) return { status: 401, message: PanelErrors.MISSING_USER_KEY };
  const user = await verifier.verify(userKey, requestId);
  if (!user) return { status: 401, message: PanelErrors.UNAUTHORIZED };
  if (ADMIN_ONLY_V3_PATHS.has(path) && user.user_type !== "system_admin") {
    return { status: 403, message: PanelErrors.ADMIN_ONLY };
  }
  return undefined;
}

const INVALID_JSON = Symbol("invalid-json");

async function readJsonBody(c: Context<AppEnv>): Promise<unknown> {
  const text = await c.req.text();
  if (!text.trim()) return {};
  try {
    return JSON.parse(text) as unknown;
  } catch {
    return INVALID_JSON;
  }
}

function relay(c: Context<AppEnv>, response: CoreResponse): Response {
  const status = toContentfulStatus(response.status);
  return c.json(response.body ?? errorEnvelope(status, "EMPTY_CORE_RESPONSE", c.get("requestId")), status);
}

function toContentfulStatus(status: number): ContentfulStatusCode {
  // 1xx/204/304 cannot carry a body; Core never sends them for JSON routes.
  if (status < 200 || status === 204 || status === 205 || status === 304 || status > 599) return 502;
  return status as ContentfulStatusCode;
}
