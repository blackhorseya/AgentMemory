/** Hono environment shared by all routes and middleware. */
export interface AppEnv {
  Variables: {
    requestId: string;
  };
}
