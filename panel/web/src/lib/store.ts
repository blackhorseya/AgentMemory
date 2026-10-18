import { readJson, remove, writeJson } from "./storage";

export interface Store<T> {
  get(): T;
  set(next: T): void;
  subscribe(listener: () => void): () => void;
}

/** Minimal observable value persisted to localStorage (best effort). */
export function createPersistedStore<T>(key: string, initial: T, isValid: (v: unknown) => v is T): Store<T> {
  let value: T = readJson(key, isValid) ?? initial;
  const listeners = new Set<() => void>();

  return {
    get: () => value,
    set(next) {
      value = next;
      if (next === null || next === undefined) remove(key);
      else writeJson(key, next);
      listeners.forEach((l) => l());
    },
    subscribe(listener) {
      listeners.add(listener);
      return () => listeners.delete(listener);
    },
  };
}
