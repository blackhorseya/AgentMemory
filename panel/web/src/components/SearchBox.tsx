import { useEffect, useState, type FormEvent } from "react";
import { Button } from "./Button";
import { Input } from "./Field";

/** Submit-on-enter search input with a clear button. Calls `onSearch("")` to clear. */
export function SearchBox({ value, onSearch, placeholder }: { value: string; onSearch: (q: string) => void; placeholder?: string }) {
  const [draft, setDraft] = useState(value);
  useEffect(() => setDraft(value), [value]);

  function submit(e: FormEvent) {
    e.preventDefault();
    onSearch(draft.trim());
  }

  return (
    <form onSubmit={submit} className="flex gap-1">
      <Input value={draft} onChange={(e) => setDraft(e.target.value)} placeholder={placeholder ?? "Search…"} className="w-64" />
      <Button type="submit" size="md">
        Search
      </Button>
      {value && (
        <Button variant="ghost" onClick={() => onSearch("")}>
          Clear
        </Button>
      )}
    </form>
  );
}
