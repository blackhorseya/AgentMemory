/** localStorage wrapper that never throws (private mode, blocked storage, SSR). */

function store(): Storage | undefined {
  try {
    return typeof localStorage === "undefined" ? undefined : localStorage;
  } catch {
    return undefined;
  }
}

export function readJson<T>(key: string, isValid: (v: unknown) => v is T): T | undefined {
  try {
    const raw = store()?.getItem(key);
    if (!raw) return undefined;
    const parsed: unknown = JSON.parse(raw);
    return isValid(parsed) ? parsed : undefined;
  } catch {
    return undefined;
  }
}

export function writeJson(key: string, value: unknown): void {
  try {
    store()?.setItem(key, JSON.stringify(value));
  } catch {
    /* storage unavailable: session lives in memory only */
  }
}

export function remove(key: string): void {
  try {
    store()?.removeItem(key);
  } catch {
    /* ignore */
  }
}
