import { Button } from "./Button";

export function Pager({
  offset,
  limit,
  total,
  count,
  onChange,
}: {
  offset: number;
  limit: number;
  /** Total rows if the endpoint reports it. */
  total?: number;
  /** Rows on the current page (used when total is unknown). */
  count: number;
  onChange: (offset: number) => void;
}) {
  const hasNext = total !== undefined ? offset + limit < total : count >= limit;
  const end = offset + count;
  return (
    <div className="flex items-center justify-between gap-2 px-2 py-2 text-xs text-muted">
      <span className="mono">
        {count === 0 ? 0 : offset + 1}–{end}
        {total !== undefined && ` of ${total}`}
      </span>
      <div className="flex gap-1">
        <Button size="sm" disabled={offset === 0} onClick={() => onChange(Math.max(0, offset - limit))}>
          ‹ Prev
        </Button>
        <Button size="sm" disabled={!hasNext} onClick={() => onChange(offset + limit)}>
          Next ›
        </Button>
      </div>
    </div>
  );
}
