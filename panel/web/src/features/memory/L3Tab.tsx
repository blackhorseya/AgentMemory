import { useEffect, useState } from "react";
import { api, type Scope } from "../../api";
import { Button } from "../../components/Button";
import { Card } from "../../components/Card";
import { CodeBlock } from "../../components/CodeBlock";
import { EmptyState, ErrorBanner, Notice, Spinner } from "../../components/Feedback";
import { TextArea } from "../../components/Field";
import { useAction } from "../../hooks/useAction";
import { useAsync } from "../../hooks/useAsync";
import { formatTime } from "../../lib/format";

export function L3Tab({ scope }: { scope: Scope }) {
  const persona = useAsync(() => api.core.read(scope), [scope]);
  const [editing, setEditing] = useState(false);
  const [draft, setDraft] = useState("");

  useEffect(() => setEditing(false), [scope]);

  const save = useAction(async () => {
    await api.core.write(scope, draft);
    setEditing(false);
    await persona.reload();
  });

  const content = persona.data?.content ?? null;

  function startEdit() {
    setDraft(content ?? "");
    setEditing(true);
  }

  return (
    <Card
      title="L3 · Core persona"
      actions={
        editing ? (
          <>
            <Button size="sm" variant="ghost" onClick={() => setEditing(false)}>
              Cancel
            </Button>
            <Button size="sm" variant="primary" busy={save.busy} onClick={() => void save.run()}>
              Save
            </Button>
          </>
        ) : (
          <Button size="sm" onClick={startEdit} disabled={persona.loading}>
            {content === null ? "Create" : "Edit"}
          </Button>
        )
      }
    >
      <div className="space-y-3">
        {persona.loading && <Spinner />}
        <ErrorBanner error={persona.error ?? save.error} onRetry={persona.reload} />
        {persona.data && (
          <div className="mono text-muted">
            version {persona.data.version ?? "—"} · updated {formatTime(persona.data.updated_at)}
          </div>
        )}
        {editing ? (
          <>
            <Notice>Core strips any “Scene Navigation” block and surrounding whitespace on write.</Notice>
            <TextArea rows={26} value={draft} onChange={(e) => setDraft(e.target.value)} />
          </>
        ) : content !== null ? (
          <CodeBlock className="max-h-[70vh]">{content}</CodeBlock>
        ) : (
          persona.data && <EmptyState>No persona generated for this team/agent yet.</EmptyState>
        )}
      </div>
    </Card>
  );
}
