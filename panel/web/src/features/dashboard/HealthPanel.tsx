import type { CoreHealth } from "../../api";
import { Badge, StatusDot } from "../../components/Badge";
import { Card } from "../../components/Card";
import { ErrorBanner } from "../../components/Feedback";
import { formatDuration } from "../../lib/format";
import type { Slot } from "./useDashboardData";

const STORE_LABELS: ReadonlyArray<[keyof NonNullable<CoreHealth["stores"]>, string]> = [
  ["vectorStore", "Vector store"],
  ["embeddingService", "Embedding"],
  ["ftsAvailable", "FTS5"],
  ["llmConfigured", "LLM"],
];

export function HealthPanel({ slot }: { slot: Slot<CoreHealth> }) {
  const health = slot.data;
  const healthy = slot.error ? false : health ? health.status === "ok" : undefined;

  return (
    <Card title="Core health">
      <div className="flex flex-wrap items-center gap-x-6 gap-y-2">
        <div className="flex items-center gap-2">
          <StatusDot ok={healthy} />
          <span className="font-mono text-sm font-semibold">{slot.error ? "unreachable" : (health?.status ?? "…")}</span>
        </div>
        <Meta label="version" value={health?.version ?? "—"} />
        <Meta label="uptime" value={formatDuration(health?.uptime)} />
        <div className="flex flex-wrap gap-1.5">
          {STORE_LABELS.map(([key, label]) => (
            <FlagBadge key={key} label={label} value={health?.stores?.[key]} />
          ))}
          {Object.entries(health?.services ?? {}).map(([key, value]) => (
            <FlagBadge key={key} label={key} value={value} />
          ))}
        </div>
      </div>
      <ErrorBanner error={slot.error} className="mt-3" />
    </Card>
  );
}

function Meta({ label, value }: { label: string; value: string }) {
  return (
    <div className="flex items-baseline gap-1.5">
      <span className="label">{label}</span>
      <span className="mono">{value}</span>
    </div>
  );
}

/** Renders boolean flags as ok/err and string service states verbatim. */
function FlagBadge({ label, value }: { label: string; value: unknown }) {
  if (value === undefined || value === null) return <Badge>{label}: —</Badge>;
  if (typeof value === "boolean") return <Badge tone={value ? "ok" : "err"}>{label}</Badge>;
  const text = typeof value === "object" ? JSON.stringify(value) : String(value);
  const good = /^(ok|up|running|ready|healthy|sqlite|true)/i.test(text);
  return (
    <Badge tone={good ? "ok" : "warn"}>
      {label}: {text}
    </Badge>
  );
}
