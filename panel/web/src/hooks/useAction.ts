import { useCallback, useState } from "react";

export interface ActionState<A extends unknown[]> {
  run: (...args: A) => Promise<boolean>;
  busy: boolean;
  error: unknown;
  clearError: () => void;
}

/** Wraps a mutation: tracks busy/error and resolves `true` on success. */
export function useAction<A extends unknown[]>(action: (...args: A) => Promise<unknown>): ActionState<A> {
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<unknown>();

  const run = useCallback(
    async (...args: A) => {
      setBusy(true);
      setError(undefined);
      try {
        await action(...args);
        return true;
      } catch (err) {
        setError(err);
        return false;
      } finally {
        setBusy(false);
      }
    },
    [action],
  );

  return { run, busy, error, clearError: useCallback(() => setError(undefined), []) };
}
