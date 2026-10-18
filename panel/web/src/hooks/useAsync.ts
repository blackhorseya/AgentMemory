import { useCallback, useEffect, useRef, useState, type DependencyList } from "react";
import { isAbort } from "../api";

export interface AsyncState<T> {
  data: T | undefined;
  error: unknown;
  loading: boolean;
  /** Re-runs the loader; resolves when done. */
  reload: () => Promise<void>;
}

/**
 * Runs `loader` when `deps` change, cancelling stale runs via AbortSignal.
 * Pass `enabled = false` to skip loading (e.g. until a required input exists).
 */
export function useAsync<T>(
  loader: (signal: AbortSignal) => Promise<T>,
  deps: DependencyList,
  enabled = true,
): AsyncState<T> {
  const [data, setData] = useState<T>();
  const [error, setError] = useState<unknown>();
  const [loading, setLoading] = useState(enabled);
  const controller = useRef<AbortController | null>(null);
  const loaderRef = useRef(loader);
  loaderRef.current = loader;

  const run = useCallback(async () => {
    controller.current?.abort();
    const ctrl = new AbortController();
    controller.current = ctrl;
    setLoading(true);
    try {
      const result = await loaderRef.current(ctrl.signal);
      if (ctrl.signal.aborted) return;
      setData(result);
      setError(undefined);
    } catch (err) {
      if (ctrl.signal.aborted || isAbort(err)) return;
      setError(err);
    } finally {
      if (!ctrl.signal.aborted) setLoading(false);
    }
  }, []);

  useEffect(() => {
    if (!enabled) {
      setLoading(false);
      return;
    }
    void run();
    return () => controller.current?.abort();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [enabled, run, ...deps]);

  return { data, error, loading, reload: run };
}
