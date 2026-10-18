# Architecture

This repository contains **memory-core**, an agent memory service written in
Rust, plus a TypeScript **Monitor Panel**.

The service depends on no vendor-specific cloud product. Storage is SQLite +
local files; LLM and embedding providers are any OpenAI-compatible endpoint.

## Characteristics

| Feature | Where |
| --- | --- |
| Layered memory L0 conversation → L1 atom → L2 scenario → L3 persona | `memory-domain::memory`, `memory-application::pipeline` |
| Async distillation pipeline (every-N / idle trigger, L2 delay & interval, L3 every-N scenes) | `memory-application::pipeline`, `memory-infrastructure::scheduler` |
| Hybrid recall: BM25 (FTS5) + vector + RRF, capped by count / char budget / timeout | `memory-domain::retrieval`, `memory-application::recall` |
| Strong isolation by instance (`x-memory-service-id`) and team / agent / user | `memory-domain::scope` |
| Skills with versions, resources, optimistic locking, listing injection block | `memory-domain::skill`, `memory-application::skill` |
| Metadata plane: users, keys, teams, members, agents, tasks, assets, bindings, ACL | `memory-domain::meta`, `memory-application::meta` |
| Visibility `private` / `team` / `restricted` / `agent` + ACL allow/deny | `memory-domain::meta::access` |
| v3 RPC-style API with `{code,message,request_id,data}` envelope | `memory-gateway` |

## Layers (Clean Architecture)

```text
            ┌──────────────────────────────────────────────┐
            │ memory-server  (composition root, config)     │
            └───────────────┬──────────────────────────────┘
                            │ wires
   ┌────────────────────────┼─────────────────────────────┐
   │ memory-gateway (axum)  │  memory-infrastructure       │  ← frameworks & drivers
   │ HTTP DTOs, envelope,   │  SQLite repos, FS stores,    │
   │ auth middleware        │  OpenAI LLM/embedding, timers│
   └───────────┬────────────┴──────────────┬──────────────┘
               │ calls use cases           │ implements ports
   ┌───────────▼───────────────────────────▼──────────────┐
   │ memory-application  (use cases + port traits)         │  ← application rules
   └───────────────────────────┬──────────────────────────┘
                               │ uses
   ┌───────────────────────────▼──────────────────────────┐
   │ memory-domain (entities, value objects, policies)     │  ← enterprise rules
   └──────────────────────────────────────────────────────┘
```

Dependency rule: arrows point inward only. `memory-domain` has no I/O
dependencies; `memory-application` depends only on the domain and defines
the ports (traits) that `memory-infrastructure` implements. The gateway
knows use cases but never touches SQLite directly.

## Monitor Panel

`panel/` is a TypeScript project:

* `panel/server` — a small Hono BFF that holds the Core API key server-side,
  forwards a whitelisted set of v3 calls and serves the static web build.
* `panel/web` — React + Vite dashboard: health, layer counts, pipeline status,
  memory browser (L0–L3), skills, metadata (teams/agents/assets).

## Design notes

* **Storage layout** — `<data>/system.db` holds durable pipeline timers for
  every instance; `<data>/instances/<id>/memory.db` holds L0–L3, metadata,
  skills and logs of one instance; `<data>/instances/<id>/{sessions,skills}/`
  holds raw session turns and skill resource files.
* **Profiles** — L2 scenes and the L3 persona are versioned `ProfileFile`s of
  a team + agent (`scene_blocks/*.md`, `persona.md`), so every edit is
  recoverable by version.
* **Full-text search** — documents and queries share one tokenizer:
  lower-cased Latin words plus overlapping CJK bigrams, indexed with FTS5.
  Keyword scores are normalised against the best hit so the recall
  threshold is corpus-size independent.
* **Pipeline** — `PipelineService` makes the decisions (domain policies in
  `memory_domain::pipeline`); the infrastructure worker only serialises jobs
  per session/profile, coalesces duplicates and retries with exponential
  backoff (5s, 15s, 45s) before dead-lettering.
* **LLM output** — every generation step uses a strict JSON (or markdown)
  contract, parsed tolerantly (`<think>` blocks, code fences). Custom
  memory prompts are appended inside a guard that keeps the contract.
* **Late binding** — two construction cycles (memory ↔ pipeline, meta ↔
  chat-memory) are broken with `OnceLock`-registered collaborators instead
  of making the dependency graph bidirectional.

## Backends

Adapters are chosen in one place, `crates/memory-server/src/backends.rs`;
nothing else in the composition root knows which backend is active.

```text
                 ┌──────────── storage.backend ────────────┐
 ports           │ sqlite                 │ postgres        │
 ─────────────── ┼────────────────────────┼─────────────────┤
 L0/L1/L2/L3,    │ one file per instance  │ one schema per   │
 meta, skills,   │ FTS5 + sqlite-vec vec0 │ instance, tsvector│
 prompts, logs   │ (brute-force fallback) │ + pgvector HNSW  │
                 └────────────────────────┴─────────────────┘
                 ┌────────── coordination.backend ─────────┐
                 │ local                  │ redis           │
 ─────────────── ┼────────────────────────┼─────────────────┤
 pipeline state  │ system.db              │ JSON + due ZSETs │
 job queue       │ in-process channel     │ ready list, delayed/processing ZSETs, leases │
 KeyLock         │ striped async mutexes  │ SET NX PX + renewal │
                 └────────────────────────┴─────────────────┘
 files (skill resources, raw session turns): always <data>/instances/<id>/
```

* **Ports** — `KeyLock` (`ports/coordination.rs`) guards pipeline state
  read-modify-write; the worker uses a *separate* `KeyLock` for per-job
  exclusivity (local stripes would otherwise let a job wait on itself).
  `QueueBackend` (infrastructure `worker.rs`) gives the worker
  pop / ack / extend / push_later / dead_letter, so delivery semantics
  belong to the backend.
* **Cursors** — L1 and L2 consume `after_seq` cursors assigned by the store
  (SQLite `AUTOINCREMENT`, Postgres sequence under an advisory lock), never
  reused and renewed on rewrite, so equal or back-dated timestamps cannot
  skip rows.
* **Redis delivery** is at-least-once: pop moves a job to a processing set
  with a lease; the worker extends the lease while running and acks only
  after the outcome (success, retry scheduled, or dead letter) is stored.
* **Multi-node** — with `coordination.backend = redis` and
  `storage.backend = postgres`, several memory-core replicas share all
  state; `<data>` must then be a shared volume for skill resources and raw
  session objects.
* **Instance destruction** composes the backend's cleanup steps
  (`CompositeLifecycle`): storage (drop file or schema), files, state.
