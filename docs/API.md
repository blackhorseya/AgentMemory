# MemoryCore (Rust) HTTP API

RPC-style v3 API. This file lists every implemented endpoint and the
response conventions.

## Conventions

* Port `8420`. All `/v3/*` endpoints are `POST` with a JSON body.
* Envelope: `{ "code": 0, "message": "ok", "request_id": "req-…", "data": … }`.
  HTTP status equals `code` for 4xx/5xx; skill errors use five-digit codes
  (e.g. `40901`) with the matching HTTP status.
* Headers
  * `Authorization: Bearer <AGENT_MEMORY_API_KEY>` — required on every route
    except `GET /health` when an API key is configured.
  * `x-memory-service-id: <instance id>` — required on every `/v3/*` route
    except `/v3/instance/destroy` and `/v3/internal/meta/upstream/*`.
  * `x-memory-user-key: sk-mem-…` — required on `/v3/meta/*`
    (except `/v3/meta/auth/verify`).
  * Isolation: `team_id`/`agent_id`/`user_id`/`task_id`/`session_id` in the
    body, or `x-memory-team-id` / `x-memory-agent-id` / `x-memory-user-id` /
    `x-memory-task-id` / `x-memory-session-id`. Body wins. Missing ⇒ `default`.
* Pagination: `limit` (default 20, max 100) and `offset`.

## Endpoints

### Health and monitoring

| Method | Path | Notes |
| --- | --- | --- |
| GET | `/health` | `{status, version, uptime, stores{vectorStore,embeddingService,ftsAvailable,llmConfigured}, services{pipelineWorker, stateBackend}}` |
| POST | `/v3/pipeline/status` | **Added.** `{worker: WorkerMetrics, queue: {pending, running, dead_letter}, scopes: ScopeState[]}` |
| POST | `/v3/stats/overview` | **Added.** Layer counts for the instance: `{l0, l1: {total, episodic, persona, instruction}, l2, l3, skills, generation_logs}` |

`WorkerMetrics`: `{tasks_enqueued, tasks_completed, tasks_failed, tasks_retried, tasks_dead_lettered, last_task_at, last_error}`.
`ScopeState`: `{scope, team_id, agent_id, user_id, session_id, conversation_count, warmup_threshold, l1_cursor, l2_pending, last_l1_at, last_l2_at, last_l3_at, memories_since_persona}`.

### Recall

| Path | Notes |
| --- | --- |
| `/v3/recall` | **Added (v3 form of legacy `/recall`).** Body `{query, max_results?, strategy?}` + isolation. Returns `{strategy, memories: [{id, type, content, score}], prepend_context, append_system_context}` |
| `/recall`, `/capture` | Legacy v1 compatibility endpoints. |

### Data plane (L0–L3)

`/v3/conversation/{add,query,search,delete,count}`,
`/v3/atomic/{update,query,search,delete,count}`,
`/v3/scenario/{ls,read,write,rm,count}`,
`/v3/core/{read,write,count}`, `/v3/session/add`.

L2/L3 are scoped by team + agent only.

### Skills

`/v3/skill/{create,update,patch,delete,get,get-by-name,list,search,versions,listing,extract}`,
`/v3/skill/files/{write,remove,read}`,
`/v3/skill/conversation/{add,force-archive}`.
`/v3/skill/export` is not implemented (returns 501).

### Knowledge metadata, chat memory, prompts, generation logs

`/v3/knowledge/{create,get,update,delete,list}`,
`/v3/chat-memory/clear`,
`/v3/memory-prompt/{create,get,update,delete,set,setting/list,log}`,
`/v3/memory-generation-log/{list,get}`.

### Metadata plane (`x-memory-user-key` required)

`/v3/meta/user/{create,create-with-key,get,delete,list}`,
`/v3/meta/user-key/{create,list,get,revoke,update}`,
`/v3/meta/team/{create,get,update,delete,list}`,
`/v3/meta/team-member/{add,remove,list,get}`,
`/v3/meta/agent/{create,get,update,delete,list,archive}`,
`/v3/meta/task/{create,get,update,delete,list,archive}`,
`/v3/meta/task-agent/{link,unlink,list}`,
`/v3/meta/participation-log/{append,list}`,
`/v3/meta/asset/{create,get,update,delete,list,list-accessible,touch-usage}`,
`/v3/meta/agent-fixed-asset/{set,list,list-with-detail,summary-by-agents}`,
`/v3/meta/acl/{grant,revoke,list,check}`,
`/v3/meta/auth/verify`, `/v3/meta/instance-quota/get`,
`/v3/meta/config/user/{get,set}`.

Meta list responses: `{items, total, limit, offset}`.

### Internal / operations (Bearer only)

`/v3/internal/meta/user/init-admin` → `{user_id, user_key}` (409 if already initialised),
`/v3/internal/meta/user/list-by-instance`,
`/v3/instance/destroy` `{instance_id}`.

## Design decisions

* No vendor-specific integrations: analytics, external task sources and
  upstream model-group routes are not provided.
* Correctness guarantees: `atomic/update` looks up by id;
  recall and search always apply tenant isolation; `time_start`/`time_end`
  are honoured by search; `scenario/ls` validates `path_prefix`; new L1
  records start at version 1.
* L2 scene and L3 persona generation use structured JSON operations
  instead of an agentic tool loop; prompts follow the strategy
  (UPDATE > MERGE > CREATE, heat, scene cap, META header, Scene Navigation).
