import { readFile, stat } from "node:fs/promises";
import { extname, resolve, sep } from "node:path";
import { Hono, type Context } from "hono";
import type { AppEnv } from "../app-env.js";
import { errorEnvelope, PanelErrors } from "../envelope.js";

const MIME_TYPES: Readonly<Record<string, string>> = {
  ".html": "text/html; charset=utf-8",
  ".js": "text/javascript; charset=utf-8",
  ".mjs": "text/javascript; charset=utf-8",
  ".css": "text/css; charset=utf-8",
  ".json": "application/json; charset=utf-8",
  ".svg": "image/svg+xml",
  ".png": "image/png",
  ".jpg": "image/jpeg",
  ".ico": "image/x-icon",
  ".woff2": "font/woff2",
  ".txt": "text/plain; charset=utf-8",
  ".map": "application/json; charset=utf-8",
};

/**
 * Serves the built SPA. Unknown non-/api GETs fall back to index.html; unknown
 * /api paths get a JSON 404 envelope. Path traversal outside `distDir` is refused.
 */
export function staticUiRoutes(distDir: string | undefined): Hono<AppEnv> {
  const app = new Hono<AppEnv>();
  const root = distDir ? resolve(distDir) : undefined;

  app.all("/api/*", (c) => c.json(errorEnvelope(404, PanelErrors.NOT_FOUND, c.get("requestId")), 404));

  app.get("*", async (c) => {
    if (!root) return c.text("UI not built (UI_DIST_DIR is not set)", 404);
    const file = (await resolveFile(root, c.req.path)) ?? resolve(root, "index.html");
    return sendFile(c, file);
  });

  return app;
}

async function resolveFile(root: string, urlPath: string): Promise<string | undefined> {
  let decoded: string;
  try {
    decoded = decodeURIComponent(urlPath);
  } catch {
    return undefined;
  }
  const candidate = resolve(root, `.${decoded}`);
  if (candidate !== root && !candidate.startsWith(root + sep)) return undefined;
  try {
    return (await stat(candidate)).isFile() ? candidate : undefined;
  } catch {
    return undefined;
  }
}

async function sendFile(c: Context<AppEnv>, file: string): Promise<Response> {
  let content: Buffer;
  try {
    content = await readFile(file);
  } catch {
    return c.text("Not found", 404);
  }
  const type = MIME_TYPES[extname(file).toLowerCase()] ?? "application/octet-stream";
  const immutable = file.includes(`${sep}assets${sep}`);
  return c.body(new Uint8Array(content), 200, {
    "content-type": type,
    "cache-control": immutable ? "public, max-age=31536000, immutable" : "no-cache",
  });
}
