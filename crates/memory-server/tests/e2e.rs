//! End-to-end: real HTTP gateway + SQLite (or Postgres when
//! `AGENT_MEMORY_TEST_POSTGRES_URL` is set), with a scripted
//! OpenAI-compatible LLM, driving L0 → L1 → L2 → L3 and skill extraction.

use std::time::Duration;

use axum::routing::post;
use axum::{Json, Router};
use memory_server::config::Config;
use memory_server::wiring;
use serde_json::{json, Value};

const L1_REPLY: &str = r###"[{"scene_name":"I (the AI) am planning a release with the user","memories":[
  {"content":"The user plans to release v2 next Monday","type":"episodic","priority":85,"source_message_ids":[]},
  {"content":"The user always runs integration tests before a release","type":"persona","priority":80,"source_message_ids":[]},
  {"content":"low value chatter","type":"persona","priority":10,"source_message_ids":[]}]}]"###;
const L2_REPLY: &str = r###"{"operations":[{"action":"create","file":"Release Planning","summary":"How the user ships releases","content":"## Core narrative\nRuns integration tests, then releases on Monday."}],"persona_update_request":null}"###;
const L3_REPLY: &str =
    "```markdown\n# User Narrative Profile\n> **Archetype**: careful shipper\n```";
const SKILL_REPLY: &str = r###"{"actions":[{"action":"create","name":"release-checklist","content":"---\nname: release-checklist\ndescription: Checklist before shipping a release\n---\n1. Run integration tests\n2. Tag and release"}],"summary":"saved release checklist"}"###;

/// Picks the canned reply by the system prompt's opening words.
async fn fake_llm(Json(req): Json<Value>) -> Json<Value> {
    let system = req["messages"][0]["content"].as_str().unwrap_or_default();
    let reply = if system.starts_with("You are a scene-segmentation") {
        L1_REPLY
    } else if system.starts_with("You reconcile") {
        "[]"
    } else if system.starts_with("You are a Memory Consolidation") {
        L2_REPLY
    } else if system.starts_with("You are a Persona Architect") {
        L3_REPLY
    } else if system.starts_with("You review a finished agent conversation") {
        SKILL_REPLY
    } else {
        "{}"
    };
    Json(json!({"choices": [{"message": {"role": "assistant", "content": reply}}]}))
}

async fn spawn_fake_llm() -> String {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(
            listener,
            Router::new().route("/v1/chat/completions", post(fake_llm)),
        )
        .await
        .unwrap()
    });
    format!("http://{addr}/v1")
}

struct Client {
    http: reqwest::Client,
    base: String,
    /// `x-memory-service-id` of every call.
    instance: String,
}

impl Client {
    async fn call(&self, path: &str, body: Value) -> Value {
        let res = self
            .http
            .post(format!("{}{path}", self.base))
            .bearer_auth("test-key")
            .header("x-memory-service-id", &self.instance)
            .json(&body)
            .send()
            .await
            .unwrap();
        res.json().await.unwrap()
    }

    /// Polls `stats/overview` until `pred` holds.
    async fn wait_for(&self, pred: impl Fn(&Value) -> bool) -> Value {
        for _ in 0..100 {
            let stats = self.call("/v3/stats/overview", json!({})).await["data"].clone();
            if pred(&stats) {
                return stats;
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
        panic!("pipeline did not reach the expected state");
    }
}

async fn start() -> (tempfile::TempDir, Client) {
    let base_url = spawn_fake_llm().await;
    start_with(|config| {
        config.llm.base_url = base_url;
        config.llm.api_key = "fake".into();
    })
    .await
}

async fn start_with(configure: impl FnOnce(&mut Config)) -> (tempfile::TempDir, Client) {
    let data = tempfile::tempdir().unwrap();
    let mut config = Config::default();
    config.data.base_dir = data.path().to_string_lossy().into_owned();
    config.server.api_key = Some("test-key".into());
    configure(&mut config);
    config.memory.pipeline.l2_delay_after_l1_seconds = 0;
    config.memory.pipeline.l2_min_interval_seconds = 0;
    config.worker.scan_interval_ms = 50;
    let app = wiring::build(&config).await.unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    tokio::spawn(memory_gateway::serve(
        listener,
        app.state.clone(),
        std::future::pending(),
    ));
    (
        data,
        Client {
            http: reqwest::Client::new(),
            base,
            instance: "ins-e2e".into(),
        },
    )
}

const SCOPE: (&str, &str, &str) = ("team-e2e", "agent-e2e", "user-e2e");

fn scoped(mut body: Value) -> Value {
    body["team_id"] = json!(SCOPE.0);
    body["agent_id"] = json!(SCOPE.1);
    body["user_id"] = json!(SCOPE.2);
    body
}

#[tokio::test(flavor = "multi_thread")]
async fn conversation_is_distilled_into_atoms_scenes_and_persona() {
    let (_data, client) = start().await;
    assert_conversation_is_distilled(&client).await;
}

/// The same flow with Redis coordinating state, queue and locks. Skipped
/// unless `AGENT_MEMORY_TEST_REDIS_URL` is set.
#[tokio::test(flavor = "multi_thread")]
async fn redis_coordination_runs_the_pipeline() {
    let Ok(redis_url) = std::env::var("AGENT_MEMORY_TEST_REDIS_URL") else {
        eprintln!("skipping: set AGENT_MEMORY_TEST_REDIS_URL to run against Redis");
        return;
    };
    let base_url = spawn_fake_llm().await;
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let (_data, client) = start_with(|config| {
        config.llm.base_url = base_url;
        config.llm.api_key = "fake".into();
        config.coordination.backend = "redis".into();
        config.coordination.redis.url = redis_url;
        // A fresh namespace, so earlier runs' state never interferes.
        config.coordination.redis.key_prefix = format!("amem-e2e-{nanos}");
    })
    .await;
    assert_conversation_is_distilled(&client).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn the_pipeline_runs_on_the_postgres_backend() {
    let Ok(url) = std::env::var("AGENT_MEMORY_TEST_POSTGRES_URL") else {
        eprintln!("skipping: set AGENT_MEMORY_TEST_POSTGRES_URL to run against Postgres");
        return;
    };
    let base_url = spawn_fake_llm().await;
    let (_data, mut client) = start_with(|config| {
        config.llm.base_url = base_url;
        config.llm.api_key = "fake".into();
        config.storage.backend = "postgres".into();
        config.storage.postgres.url = url;
    })
    .await;
    // The database outlives the test; a fresh instance keeps runs apart.
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    client.instance = format!("ins-e2e-pg-{}-{nanos}", std::process::id());

    let health: Value = client
        .http
        .get(format!("{}/health", client.base))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(health["stores"]["backend"], "postgres", "{health}");
    assert_eq!(health["stores"]["ftsAvailable"], true);

    assert_conversation_is_distilled(&client).await;
    assert_tool_calls_become_a_skill(&client).await;

    let destroyed = client
        .call(
            "/v3/instance/destroy",
            json!({"instance_id": client.instance}),
        )
        .await;
    assert_eq!(
        destroyed["data"]["cleaned"]["storage"]["postgres"]["removed"], true,
        "{destroyed}"
    );
    let stats = client.call("/v3/stats/overview", json!({})).await;
    assert_eq!(stats["data"]["l1"]["total"], 0, "{stats}");
    // Reading re-created the (empty) schema lazily; leave nothing behind.
    client
        .call(
            "/v3/instance/destroy",
            json!({"instance_id": client.instance}),
        )
        .await;
}

async fn assert_conversation_is_distilled(client: &Client) {
    let added = client
        .call(
            "/v3/conversation/add",
            scoped(json!({"session_id": "s1", "messages": [
                {"role": "user", "content": "We release v2 next Monday; I always run integration tests first."},
                {"role": "assistant", "content": "Noted, I will remind you."}
            ]})),
        )
        .await;
    assert_eq!(added["code"], 0, "{added}");

    // Warm-up threshold 1 → L1 immediately → L2 (no delay) → L3 cold start.
    let stats = client
        .wait_for(|s| {
            // Each layer logs its run after writing its output.
            s["l1"]["total"] == 2
                && s["l2"] == 1
                && s["l3"] == 1
                && s["generation_logs"].as_u64().unwrap_or(0) >= 3
        })
        .await;
    assert_eq!(
        stats["l1"]["episodic"], 1,
        "low-priority memory must be dropped: {stats}"
    );

    let scenes = client.call("/v3/scenario/ls", scoped(json!({}))).await;
    assert_eq!(scenes["data"]["entries"][0]["path"], "Release-Planning.md");
    assert_eq!(scenes["data"]["entries"][0]["heat"], 1);

    let persona = client.call("/v3/core/read", scoped(json!({}))).await;
    let content = persona["data"]["content"].as_str().unwrap();
    assert!(content.starts_with("# User Narrative Profile"), "{content}");
    assert!(content.contains("Scene Navigation"));

    let recall = client
        .call(
            "/v3/recall",
            scoped(json!({"query": "when is the v2 release?"})),
        )
        .await;
    let memories = recall["data"]["memories"].as_array().unwrap();
    assert!(
        memories
            .iter()
            .any(|m| m["content"].as_str().unwrap().contains("next Monday")),
        "{recall}"
    );
    assert!(recall["data"]["append_system_context"]
        .as_str()
        .unwrap()
        .contains("<user-persona>"));

    // Isolation: another user of the same agent shares L2/L3 but not L1.
    let other = client
        .call(
            "/v3/atomic/query",
            json!({"team_id": SCOPE.0, "agent_id": SCOPE.1, "user_id": "someone-else"}),
        )
        .await;
    assert_eq!(other["data"]["total"], 0);
}

#[tokio::test(flavor = "multi_thread")]
async fn tool_heavy_conversations_are_archived_into_skills() {
    let (_data, client) = start().await;
    assert_tool_calls_become_a_skill(&client).await;
}

async fn assert_tool_calls_become_a_skill(client: &Client) {
    let tool_calls: Vec<Value> = (0..10)
        .flat_map(|i| {
            [
                json!({"role": "tool_call", "content": format!("run step {i}"), "tool_name": "shell", "tool_call_id": format!("c{i}")}),
                json!({"role": "tool_result", "content": "ok", "tool_name": "shell", "tool_call_id": format!("c{i}")}),
            ]
        })
        .collect();
    let result = client
        .call(
            "/v3/skill/conversation/add",
            scoped(json!({"session_id": "s-skill", "messages": tool_calls})),
        )
        .await;
    assert_eq!(result["data"]["status"], "archived", "{result}");
    assert_eq!(result["data"]["archived"]["reason"], "tool_calls");

    client.wait_for(|s| s["skills"] == 1).await;
    let listing = client.call("/v3/skill/listing", scoped(json!({}))).await;
    assert!(
        listing["data"]["listing"]
            .as_str()
            .unwrap()
            .contains("release-checklist"),
        "{listing}"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn requests_without_bearer_token_are_rejected() {
    let (_data, client) = start().await;
    let res = client
        .http
        .post(format!("{}/v3/stats/overview", client.base))
        .header("x-memory-service-id", &client.instance)
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), 401);
    let health = client
        .http
        .get(format!("{}/health", client.base))
        .send()
        .await
        .unwrap();
    assert_eq!(health.status(), 200);
}

impl Client {
    async fn call_as(&self, path: &str, user_key: &str, body: Value) -> Value {
        self.http
            .post(format!("{}{path}", self.base))
            .bearer_auth("test-key")
            .header("x-memory-service-id", &self.instance)
            .header("x-memory-user-key", user_key)
            .json(&body)
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap()
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn other_users_cannot_read_or_change_foreign_data() {
    let (_data, client) = start().await;
    let admin = client
        .call(
            "/v3/internal/meta/user/init-admin",
            json!({"username": "admin"}),
        )
        .await["data"]
        .clone();
    let admin_key = admin["user_key"].as_str().unwrap();
    let team = admin["team_id"].as_str().unwrap();
    let agent = admin["agent_id"].as_str().unwrap();
    let outsider = client
        .call_as(
            "/v3/meta/user/create",
            admin_key,
            json!({"username": "mallory"}),
        )
        .await["data"]
        .clone();
    let mallory_key = outsider["default_user_key"].as_str().unwrap();

    let team_read = client
        .call_as("/v3/meta/team/get", mallory_key, json!({"team_id": team}))
        .await;
    assert_eq!(
        team_read["code"], 403,
        "non-members must not read the team: {team_read}"
    );
    let spy = client
        .call_as(
            "/v3/meta/asset/list-accessible",
            mallory_key,
            json!({"user_id": admin["user_id"]}),
        )
        .await;
    assert_eq!(
        spy["code"], 403,
        "listing another user's assets must be refused: {spy}"
    );
    let loadout = client
        .call_as(
            "/v3/meta/agent-fixed-asset/list-with-detail",
            mallory_key,
            json!({"agent_id": agent}),
        )
        .await;
    assert_eq!(loadout["code"], 403, "{loadout}");

    let skill = client
        .call(
            "/v3/skill/create",
            json!({"team_id": team, "agent_id": agent, "user_id": admin["user_id"], "name": "secret-runbook",
                   "content": "---\nname: secret-runbook\ndescription: internal\n---\nsteps"}),
        )
        .await["data"]
        .clone();
    let skill_id = skill["skill_id"].as_str().unwrap();
    let anonymous_write = client
        .call(
            "/v3/skill/update",
            json!({"skill_id": skill_id, "expected_version": 1, "content": "x"}),
        )
        .await;
    assert_eq!(
        anonymous_write["code"], 40001,
        "writes need a full identity: {anonymous_write}"
    );
    let foreign_read = client
        .call(
            "/v3/skill/get",
            json!({"skill_id": skill_id, "team_id": "another-team"}),
        )
        .await;
    assert_eq!(foreign_read["code"], 40401, "{foreign_read}");
}

/// A stand-in for a local LLM CLI (`claude -p`, `ollama run` …): reads the
/// prompt from stdin, records its working directory, prints a canned reply.
#[cfg(unix)]
fn write_fake_cli(dir: &std::path::Path) -> std::path::PathBuf {
    use std::os::unix::fs::PermissionsExt;
    let reply = |text: &str| text.replace('\'', "'\\''");
    let script = format!(
        "#!/bin/sh\npwd > ran-in.txt\nprompt=$(cat)\ncase \"$prompt\" in\n\
         *'scene-segmentation'*) printf '%s' '{}' ;;\n\
         *'You reconcile'*) printf '[]' ;;\n\
         *'Memory Consolidation'*) printf '%s' '{}' ;;\n\
         *'Persona Architect'*) printf '%s' '{}' ;;\n\
         *) printf '{{}}' ;;\nesac\n",
        reply(L1_REPLY),
        reply(L2_REPLY),
        reply(L3_REPLY)
    );
    let path = dir.join("fake-llm.sh");
    std::fs::write(&path, script).unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
    path
}

#[cfg(unix)]
#[tokio::test(flavor = "multi_thread")]
async fn a_local_cli_can_serve_as_the_llm() {
    let project = tempfile::tempdir().unwrap();
    let cli = write_fake_cli(project.path());
    let workdir = project.path().to_string_lossy().into_owned();
    let (_data, client) = start_with(|config| {
        config.llm.provider = "cli".into();
        config.llm.cli.command = cli.to_string_lossy().into_owned();
        config.llm.cli.working_dir = Some(workdir);
    })
    .await;
    client
        .call(
            "/v3/conversation/add",
            scoped(json!({"session_id": "s-cli", "messages": [{"role": "user", "content": "We release v2 next Monday."}]})),
        )
        .await;
    client
        .wait_for(|s| s["l1"]["total"] == 2 && s["l3"] == 1)
        .await;
    let ran_in = std::fs::read_to_string(project.path().join("ran-in.txt")).unwrap();
    assert_eq!(
        std::fs::canonicalize(ran_in.trim()).unwrap(),
        std::fs::canonicalize(project.path()).unwrap(),
        "the CLI must run in the configured project directory"
    );
}
