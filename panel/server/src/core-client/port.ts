/**
 * Port between the HTTP layer and MemoryCore. The HTTP routes depend only on
 * this interface; `FetchCoreClient` is the production adapter.
 */
export interface CoreCallContext {
  /** Browser-supplied user key, forwarded as `x-memory-user-key`. */
  readonly userKey?: string;
  readonly requestId: string;
}

export interface CoreResponse {
  readonly status: number;
  /** Parsed JSON body (envelope for /v3, bare object for /health). */
  readonly body: unknown;
}

export interface CoreClient {
  /** `POST /v3/{path}` with a JSON body. `path` has no leading slash, e.g. `atomic/query`. */
  callV3(path: string, body: unknown, ctx: CoreCallContext): Promise<CoreResponse>;
  /** `GET /health` on Core. */
  health(ctx: CoreCallContext): Promise<CoreResponse>;
}
