import { useEffect, useState } from "react";
import { api, type Agent, type Asset, type Team, type TeamMember } from "../../api";
import { Badge } from "../../components/Badge";
import { Card } from "../../components/Card";
import { DataTable, type Column } from "../../components/DataTable";
import { ErrorBanner } from "../../components/Feedback";
import { Select } from "../../components/Field";
import { PageHeader } from "../../components/PageHeader";
import { useAsync } from "../../hooks/useAsync";
import { useStore } from "../../hooks/useStore";
import { formatTime } from "../../lib/format";
import { sessionStore } from "../auth/session";

const LIST_LIMIT = 100;

const ASSET_TYPES = [
  { value: "", label: "All types" },
  { value: "skill", label: "skill" },
  { value: "llm_wiki", label: "llm_wiki" },
  { value: "code_graph", label: "code_graph" },
  { value: "chat_memory", label: "chat_memory" },
] as const;

const mono = (v: string | null | undefined) => <span className="mono">{v ?? "—"}</span>;
const statusBadge = (s: string) => <Badge tone={s === "active" ? "ok" : "neutral"}>{s}</Badge>;
const time = (v: string | null | undefined) => <span className="mono whitespace-nowrap text-muted">{formatTime(v)}</span>;

const TEAM_COLUMNS: ReadonlyArray<Column<Team>> = [
  { key: "name", header: "Team", render: (t) => <span className="font-medium">{t.name}</span> },
  { key: "id", header: "team_id", render: (t) => mono(t.team_id) },
  { key: "owner", header: "Owner", render: (t) => mono(t.owner_user_id) },
  { key: "status", header: "Status", render: (t) => statusBadge(t.status) },
];

const MEMBER_COLUMNS: ReadonlyArray<Column<TeamMember>> = [
  { key: "user", header: "User", render: (m) => <span className="font-medium">{m.username ?? m.user_id}</span> },
  { key: "id", header: "user_id", render: (m) => mono(m.user_id) },
  { key: "role", header: "Role", render: (m) => <Badge tone={m.role === "admin" ? "accent" : "neutral"}>{m.role}</Badge> },
  { key: "status", header: "Status", render: (m) => statusBadge(m.status) },
  { key: "joined", header: "Joined", render: (m) => time(m.joined_at) },
];

const AGENT_COLUMNS: ReadonlyArray<Column<Agent>> = [
  { key: "name", header: "Agent", render: (a) => <span className="font-medium">{a.name}</span> },
  { key: "id", header: "agent_id", render: (a) => mono(a.agent_id) },
  { key: "owner", header: "Owner", render: (a) => mono(a.owner_user_id) },
  { key: "vis", header: "Visibility", render: (a) => <Badge>{a.visibility}</Badge> },
  { key: "status", header: "Status", render: (a) => statusBadge(a.status) },
];

const ASSET_COLUMNS: ReadonlyArray<Column<Asset>> = [
  { key: "name", header: "Asset", render: (a) => <span className="font-medium">{a.name}</span> },
  { key: "type", header: "Type", render: (a) => <Badge tone="accent">{a.asset_type}</Badge> },
  { key: "id", header: "asset_id", render: (a) => mono(a.asset_id) },
  { key: "team", header: "Team", render: (a) => mono(a.team_id) },
  { key: "vis", header: "Visibility", render: (a) => <Badge>{a.visibility}</Badge> },
  { key: "ver", header: "Ver", render: (a) => mono(`v${a.version}`) },
  { key: "usage", header: "Uses", className: "text-right", render: (a) => mono(String(a.usage_count)) },
  { key: "updated", header: "Updated", render: (a) => time(a.updated_at) },
];

export function MetaPage() {
  const session = useStore(sessionStore);
  const userId = session?.user.user_id ?? "";
  const [teamId, setTeamId] = useState<string>();
  const [assetType, setAssetType] = useState("");

  const teams = useAsync(() => api.meta.teams(userId, { limit: LIST_LIMIT }), [userId], !!userId);
  useEffect(() => {
    const first = teams.data?.items[0]?.team_id;
    setTeamId((current) => current ?? first);
  }, [teams.data]);

  const members = useAsync(() => api.meta.members(teamId ?? "", { limit: LIST_LIMIT }), [teamId], !!teamId);
  const agents = useAsync(() => api.meta.agents(teamId ?? "", { limit: LIST_LIMIT }), [teamId], !!teamId);
  const assets = useAsync(
    () => api.meta.accessibleAssets(userId, { team_id: teamId, asset_type: assetType || undefined, limit: LIST_LIMIT }),
    [userId, teamId, assetType],
    !!userId,
  );

  return (
    <div className="space-y-4">
      <PageHeader title="Metadata" subtitle={<>Teams, members, agents and assets visible to <span className="mono">{userId}</span></>} />
      <Card title={`Teams${teams.data ? ` (${teams.data.total})` : ""}`} bodyClassName="p-0">
        <ErrorBanner error={teams.error} onRetry={teams.reload} className="m-3" />
        <DataTable
          columns={TEAM_COLUMNS}
          rows={teams.data?.items}
          rowKey={(t) => t.team_id}
          loading={teams.loading}
          empty="This user belongs to no team"
          onRowClick={(t) => setTeamId(t.team_id)}
          selectedKey={teamId}
        />
      </Card>
      {teamId && (
        <div className="grid gap-4 xl:grid-cols-2">
          <Card title={<>Members · <span className="mono normal-case">{teamId}</span></>} bodyClassName="p-0">
            <ErrorBanner error={members.error} className="m-3" />
            <DataTable columns={MEMBER_COLUMNS} rows={members.data?.items} rowKey={(m) => m.id} loading={members.loading} empty="No members" />
          </Card>
          <Card title={<>Agents · <span className="mono normal-case">{teamId}</span></>} bodyClassName="p-0">
            <ErrorBanner error={agents.error} className="m-3" />
            <DataTable columns={AGENT_COLUMNS} rows={agents.data?.items} rowKey={(a) => a.agent_id} loading={agents.loading} empty="No agents" />
          </Card>
        </div>
      )}
      <Card
        title={`Accessible assets${assets.data ? ` (${assets.data.total})` : ""}`}
        bodyClassName="p-0"
        actions={
          <Select aria-label="Asset type" options={ASSET_TYPES} value={assetType} onChange={(e) => setAssetType(e.target.value)} />
        }
      >
        <ErrorBanner error={assets.error} onRetry={assets.reload} className="m-3" />
        <DataTable columns={ASSET_COLUMNS} rows={assets.data?.items} rowKey={(a) => a.asset_id} loading={assets.loading} empty="No accessible assets" />
      </Card>
    </div>
  );
}
