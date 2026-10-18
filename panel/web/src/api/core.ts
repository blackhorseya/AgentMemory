import type { CallOptions, PanelClient } from "./client";
import type {
  Agent,
  Asset,
  AtomicDetail,
  AtomicQueryResult,
  AtomicType,
  AtomicUpdateResult,
  AuthVerifyResult,
  ConversationItem,
  ConversationQueryResult,
  CoreHealth,
  CoreMemory,
  GenerationLayer,
  GenerationLog,
  GenerationLogList,
  GenerationStatus,
  MetaList,
  Page,
  PanelConfig,
  PipelineStatus,
  RecallResult,
  ScenarioFile,
  ScenarioList,
  Scope,
  SkillDetail,
  SkillSearchHit,
  SkillSummary,
  SkillVersion,
  StatsOverview,
  Team,
  TeamMember,
  WriteResult,
} from "./types";

/**
 * One typed function per Core v3 endpoint the panel uses. Every v3 path here
 * must also be in the BFF allowlist (panel/server/src/http/allowlist.ts).
 */
export function createCoreApi(client: PanelClient) {
  const call = <T>(path: string, body: object, opts?: CallOptions) => client.core<T>(path, body, opts);

  return {
    panelConfig: (opts?: CallOptions) => client.getJson<PanelConfig>("/api/config", opts),
    health: (opts?: CallOptions) => client.getJson<CoreHealth>("/api/core-health", opts),
    pipelineStatus: (opts?: CallOptions) => call<PipelineStatus>("pipeline/status", {}, opts),
    statsOverview: (opts?: CallOptions) => call<StatsOverview>("stats/overview", {}, opts),

    verifyUserKey: (userKey: string) => call<AuthVerifyResult>("meta/auth/verify", { user_key: userKey }),

    recall: (scope: Scope, query: string, maxResults?: number) =>
      call<RecallResult>("recall", { ...scope, query, max_results: maxResults }),

    conversation: {
      query: (scope: Scope, page: Page & { session_id?: string }) =>
        call<ConversationQueryResult>("conversation/query", { ...scope, ...page }),
      search: (scope: Scope, query: string, limit = 20) =>
        call<{ messages: ConversationItem[] }>("conversation/search", { ...scope, query, limit }),
      delete: (scope: Scope, messageIds: string[]) =>
        call<{ deleted_count: number }>("conversation/delete", { ...scope, message_ids: messageIds }),
    },

    atomic: {
      query: (scope: Scope, page: Page & { type?: AtomicType }) =>
        call<AtomicQueryResult>("atomic/query", { ...scope, ...page }),
      search: (scope: Scope, query: string, type?: AtomicType, limit = 20) =>
        call<{ items: AtomicDetail[] }>("atomic/search", { ...scope, query, type, limit }),
      update: (scope: Scope, id: string, content: string, background?: string) =>
        call<AtomicUpdateResult>("atomic/update", { ...scope, id, content, background }),
      delete: (scope: Scope, ids: string[]) => call<{ deleted_count: number }>("atomic/delete", { ...scope, ids }),
    },

    scenario: {
      ls: (scope: Scope, pathPrefix?: string) => call<ScenarioList>("scenario/ls", { ...scope, path_prefix: pathPrefix }),
      read: (scope: Scope, path: string) => call<ScenarioFile>("scenario/read", { ...scope, path }),
      write: (scope: Scope, path: string, content: string, summary?: string) =>
        call<WriteResult & { path: string }>("scenario/write", { ...scope, path, content, summary }),
    },

    core: {
      read: (scope: Scope) => call<CoreMemory>("core/read", { ...scope }),
      write: (scope: Scope, content: string) => call<WriteResult>("core/write", { ...scope, content }),
    },

    generationLog: {
      list: (filter: { layer?: GenerationLayer; status?: GenerationStatus; limit?: number; cursor?: string }) =>
        call<GenerationLogList>("memory-generation-log/list", filter),
      get: (logId: string) => call<GenerationLog>("memory-generation-log/get", { log_id: logId }),
    },

    skill: {
      list: (scope: Scope, filters: { name_prefix?: string; status?: string[] }, page: Page) =>
        call<{ items: SkillSummary[]; total: number }>("skill/list", { ...scope, filters, pagination: page }),
      search: (scope: Scope, query: string, topK = 20) =>
        call<{ items: SkillSearchHit[] }>("skill/search", { ...scope, query, top_k: topK, scope: "team" }),
      get: (scope: Scope, skillId: string, version?: number) =>
        call<SkillDetail>("skill/get", { ...scope, skill_id: skillId, version, include_content: true }),
      versions: (scope: Scope, skillId: string) =>
        call<{ items: SkillVersion[]; total: number }>("skill/versions", { ...scope, skill_id: skillId }),
    },

    meta: {
      teams: (userId: string, page: Page = {}) => call<MetaList<Team>>("meta/team/list", { user_id: userId, ...page }),
      members: (teamId: string, page: Page = {}) =>
        call<MetaList<TeamMember>>("meta/team-member/list", { team_id: teamId, ...page }),
      agents: (teamId: string, page: Page = {}) => call<MetaList<Agent>>("meta/agent/list", { team_id: teamId, ...page }),
      accessibleAssets: (userId: string, filter: { team_id?: string; asset_type?: string } & Page = {}) =>
        call<MetaList<Asset>>("meta/asset/list-accessible", { user_id: userId, ...filter }),
    },
  };
}

export type CoreApi = ReturnType<typeof createCoreApi>;
