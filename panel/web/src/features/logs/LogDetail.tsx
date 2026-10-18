import { api, type GenerationRef } from "../../api";
import { Badge } from "../../components/Badge";
import { CodeBlock, JsonView, KeyValues } from "../../components/CodeBlock";
import { Drawer } from "../../components/Drawer";
import { ErrorBanner, Spinner } from "../../components/Feedback";
import { useAsync } from "../../hooks/useAsync";
import { formatTime } from "../../lib/format";

export function LogDetail({ logId, onClose }: { logId: string | undefined; onClose: () => void }) {
  const log = useAsync(() => api.generationLog.get(logId ?? ""), [logId], !!logId);
  const d = log.data;

  return (
    <Drawer open={!!logId} title={<span className="mono">{logId}</span>} onClose={onClose}>
      {log.loading && <Spinner />}
      <ErrorBanner error={log.error} onRetry={log.reload} />
      {d && !log.loading && (
        <div className="space-y-4">
          <KeyValues
            entries={[
              ["layer", <Badge key="l">{d.layer}</Badge>],
              ["status", <Badge key="s" tone={d.status === "succeeded" ? "ok" : "err"}>{d.status}</Badge>],
              ["generation", d.generation_id],
              ["anchor memory", d.anchor_memory_id],
              ["scope", [d.team_id, d.agent_id, d.user_id, d.session_id].map((v) => v ?? "·").join(" / ")],
              ["model", d.model],
              ["prompt", d.prompt ? `${d.prompt.memory_prompt_id} v${d.prompt.version} (${d.prompt.source})` : undefined],
              ["prompt mode", d.prompt_mode],
              ["started", formatTime(d.started_at_ms)],
              ["finished", formatTime(d.finished_at_ms)],
              ["latency", d.latency_ms !== undefined ? `${d.latency_ms} ms` : undefined],
            ]}
          />
          {(d.error_code || d.error_message) && (
            <CodeBlock className="border-err/40 text-err">{[d.error_code, d.error_message].filter(Boolean).join(": ")}</CodeBlock>
          )}
          <RefList title="Inputs" refs={d.input_refs} />
          <RefList title="Outputs" refs={d.output_refs} />
          <details>
            <summary className="label cursor-pointer">Raw JSON</summary>
            <JsonView value={d} className="mt-2" />
          </details>
        </div>
      )}
    </Drawer>
  );
}

function RefList({ title, refs }: { title: string; refs: GenerationRef[] | undefined }) {
  if (!refs?.length) return null;
  return (
    <div>
      <div className="label mb-1">
        {title} ({refs.length})
      </div>
      <ul className="space-y-0.5">
        {refs.map((r) => (
          <li key={`${r.layer}:${r.record_id}`} className="mono flex gap-2">
            <Badge>{r.layer}</Badge>
            <span className="break-all">{r.record_id}</span>
          </li>
        ))}
      </ul>
    </div>
  );
}
