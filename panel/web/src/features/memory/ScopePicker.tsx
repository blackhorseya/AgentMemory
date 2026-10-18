import { useEffect, useState, type FormEvent } from "react";
import type { Scope } from "../../api";
import { Button } from "../../components/Button";
import { Field, Input } from "../../components/Field";
import { DEFAULT_SCOPE } from "./scope";

const FIELDS: ReadonlyArray<[keyof Scope, string]> = [
  ["team_id", "team_id"],
  ["agent_id", "agent_id"],
  ["user_id", "user_id"],
];

export function ScopePicker({ value, onChange }: { value: Scope; onChange: (scope: Scope) => void }) {
  const [draft, setDraft] = useState(value);
  useEffect(() => setDraft(value), [value]);
  const dirty = FIELDS.some(([k]) => draft[k] !== value[k]);

  function apply(e: FormEvent) {
    e.preventDefault();
    onChange({
      team_id: draft.team_id.trim() || "default",
      agent_id: draft.agent_id.trim() || "default",
      user_id: draft.user_id.trim() || "default",
    });
  }

  return (
    <form onSubmit={apply} className="flex flex-wrap items-end gap-2 rounded-md border border-line bg-surface p-3">
      {FIELDS.map(([key, label]) => (
        <Field key={key} label={label}>
          <Input
            value={draft[key]}
            onChange={(e) => setDraft({ ...draft, [key]: e.target.value })}
            placeholder="default"
            className="mono w-48"
          />
        </Field>
      ))}
      <Button type="submit" variant={dirty ? "primary" : "secondary"}>
        Apply scope
      </Button>
      <Button variant="ghost" onClick={() => onChange(DEFAULT_SCOPE)}>
        Reset
      </Button>
      <p className="basis-full text-xxs text-muted">L0/L1 are isolated by team + agent + user; L2/L3 by team + agent only.</p>
    </form>
  );
}
