import { useEffect, useState } from "react";
import { api, type GenerationLayer, type GenerationLogItem, type GenerationLogList, type GenerationStatus } from "../../api";
import { Badge } from "../../components/Badge";
import { Button } from "../../components/Button";
import { Card } from "../../components/Card";
import { DataTable, type Column } from "../../components/DataTable";
import { ErrorBanner } from "../../components/Feedback";
import { Field, Select } from "../../components/Field";
import { PageHeader } from "../../components/PageHeader";
import { useAction } from "../../hooks/useAction";
import { useAsync } from "../../hooks/useAsync";
import { formatNumber, formatRelative, formatTime } from "../../lib/format";
import { LogDetail } from "./LogDetail";

const PAGE = 50;

const LAYERS = [
  { value: "", label: "All layers" },
  { value: "l1", label: "L1" },
  { value: "l2", label: "L2" },
  { value: "l3", label: "L3" },
] as const;

const STATUSES = [
  { value: "", label: "All statuses" },
  { value: "succeeded", label: "succeeded" },
  { value: "failed", label: "failed" },
] as const;

const COLUMNS: ReadonlyArray<Column<GenerationLogItem>> = [
  { key: "time", header: "Finished", render: (l) => <span className="mono whitespace-nowrap" title={formatRelative(l.finished_at_ms)}>{formatTime(l.finished_at_ms)}</span> },
  { key: "layer", header: "Layer", render: (l) => <Badge>{l.layer}</Badge> },
  { key: "status", header: "Status", render: (l) => <Badge tone={l.status === "succeeded" ? "ok" : "err"}>{l.status}</Badge> },
  { key: "anchor", header: "Anchor memory", render: (l) => <span className="mono text-muted">{l.anchor_memory_id ?? "—"}</span> },
  { key: "id", header: "Log ID", render: (l) => <span className="mono">{l.log_id}</span> },
  { key: "size", header: "Size", className: "text-right", render: (l) => <span className="mono text-muted">{formatNumber(l.size)}</span> },
];

export function LogsPage() {
  const [layer, setLayer] = useState<GenerationLayer | "">("");
  const [status, setStatus] = useState<GenerationStatus | "">("");
  const [selected, setSelected] = useState<string>();
  const [more, setMore] = useState<GenerationLogList>();

  const filter = { layer: layer || undefined, status: status || undefined, limit: PAGE };
  const first = useAsync(() => api.generationLog.list(filter), [layer, status]);
  useEffect(() => setMore(undefined), [first.data]);

  const items = first.data ? [...first.data.items, ...(more?.items ?? [])] : undefined;
  const cursor = more ? more.next_cursor : first.data?.next_cursor;

  const loadMore = useAction(async () => {
    const page = await api.generationLog.list({ ...filter, cursor: cursor ?? undefined });
    setMore((prev) => ({ items: [...(prev?.items ?? []), ...page.items], next_cursor: page.next_cursor }));
  });

  return (
    <div className="space-y-4">
      <PageHeader
        title="Generation logs"
        subtitle="L1/L2/L3 generation provenance (Core default window: last 7 days)"
        actions={
          <>
            <Field label="Layer">
              <Select options={LAYERS} value={layer} onChange={(e) => setLayer(e.target.value as GenerationLayer | "")} />
            </Field>
            <Field label="Status">
              <Select options={STATUSES} value={status} onChange={(e) => setStatus(e.target.value as GenerationStatus | "")} />
            </Field>
            <Button className="self-end" onClick={() => void first.reload()} busy={first.loading}>
              ↻ Refresh
            </Button>
          </>
        }
      />
      <ErrorBanner error={first.error ?? loadMore.error} onRetry={() => void first.reload()} />
      <Card bodyClassName="p-0">
        <DataTable
          columns={COLUMNS}
          rows={items}
          rowKey={(l) => l.log_id}
          loading={first.loading}
          empty="No generation logs in this window"
          onRowClick={(l) => setSelected(l.log_id)}
          selectedKey={selected}
        />
        {cursor && (
          <div className="flex justify-center border-t border-line p-2">
            <Button size="sm" busy={loadMore.busy} onClick={() => void loadMore.run()}>
              Load more
            </Button>
          </div>
        )}
      </Card>
      <LogDetail logId={selected} onClose={() => setSelected(undefined)} />
    </div>
  );
}
