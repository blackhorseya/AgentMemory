/** Core did not answer within the configured timeout. */
export class CoreTimeoutError extends Error {
  override readonly name = "CoreTimeoutError";
  constructor(readonly timeoutMs: number) {
    super(`Core did not respond within ${timeoutMs} ms`);
  }
}

/** Core could not be reached (connection refused, DNS failure, reset, ...). */
export class CoreUnavailableError extends Error {
  override readonly name = "CoreUnavailableError";
  constructor(cause: unknown) {
    super(cause instanceof Error ? cause.message : String(cause), { cause });
  }
}
