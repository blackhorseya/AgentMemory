import { useEffect, useRef } from "react";

/**
 * Calls `tick` every `intervalMs` while not paused and the tab is visible.
 * The first call happens after one interval (callers load immediately themselves).
 */
export function usePolling(tick: () => unknown, intervalMs: number, paused: boolean): void {
  const tickRef = useRef(tick);
  tickRef.current = tick;

  useEffect(() => {
    if (paused) return;
    const id = window.setInterval(() => {
      if (document.visibilityState === "visible") void tickRef.current();
    }, intervalMs);
    return () => window.clearInterval(id);
  }, [intervalMs, paused]);
}
