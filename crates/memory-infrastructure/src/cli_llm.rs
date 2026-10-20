//! An LLM served by a local command-line tool (e.g. `claude -p`,
//! `ollama run <model>`, `codex exec`, `gemini -p`), run inside a project
//! directory so the tool can use that project's context and settings.
//!
//! The process is spawned directly (no shell). The prompt goes to stdin by
//! default, which avoids argument-length limits and keeps conversation text
//! out of the process list; `Arg` mode substitutes it into the arguments.

use std::path::PathBuf;
use std::process::Stdio;
use std::time::Duration;

use async_trait::async_trait;
use memory_application::ports::system::{ChatMessage, LlmClient};
use memory_domain::error::{DomainError, DomainResult, ErrorKind};
use tokio::io::AsyncWriteExt;
use tokio::process::Command;
use tokio::sync::Semaphore;

pub const PROMPT_PLACEHOLDER: &str = "{prompt}";
const STDERR_SNIPPET_CHARS: usize = 300;
/// Linux rejects any single argument longer than `MAX_ARG_STRLEN` (128 KiB).
const MAX_ARG_BYTES: usize = 128 * 1024 - 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PromptMode {
    /// Write the prompt to the process's stdin.
    Stdin,
    /// Replace `{prompt}` in the arguments (appended when absent).
    Arg,
}

#[derive(Debug, Clone)]
pub struct CliLlmSettings {
    pub command: String,
    pub args: Vec<String>,
    pub prompt_mode: PromptMode,
    /// Project base path the tool runs in (its working directory).
    pub working_dir: Option<PathBuf>,
    pub env: Vec<(String, String)>,
    pub timeout: Duration,
    /// CLI agents are heavy; bound how many run at once.
    pub max_concurrency: usize,
}

pub struct CliLlm {
    settings: CliLlmSettings,
    permits: Semaphore,
    label: String,
}

impl CliLlm {
    pub fn new(settings: CliLlmSettings) -> Self {
        let name = std::path::Path::new(&settings.command)
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| settings.command.clone());
        Self {
            permits: Semaphore::new(settings.max_concurrency.max(1)),
            label: format!("cli:{name}"),
            settings,
        }
    }

    fn arguments(&self, prompt: &str) -> Vec<String> {
        match self.settings.prompt_mode {
            PromptMode::Stdin => self.settings.args.clone(),
            PromptMode::Arg
                if self
                    .settings
                    .args
                    .iter()
                    .any(|a| a.contains(PROMPT_PLACEHOLDER)) =>
            {
                self.settings
                    .args
                    .iter()
                    .map(|a| a.replace(PROMPT_PLACEHOLDER, prompt))
                    .collect()
            }
            PromptMode::Arg => self
                .settings
                .args
                .iter()
                .cloned()
                .chain([prompt.to_owned()])
                .collect(),
        }
    }

    async fn run(&self, prompt: String) -> DomainResult<String> {
        if self.settings.prompt_mode == PromptMode::Arg && prompt.len() > MAX_ARG_BYTES {
            return Err(unavailable(format!(
                "prompt is {} bytes, over the {MAX_ARG_BYTES}-byte argument limit; use a CLI that reads stdin",
                prompt.len()
            )));
        }
        let mut command = Command::new(&self.settings.command);
        command
            .args(self.arguments(&prompt))
            .envs(
                self.settings
                    .env
                    .iter()
                    .map(|(k, v)| (k.as_str(), v.as_str())),
            )
            .stdin(if self.settings.prompt_mode == PromptMode::Stdin {
                Stdio::piped()
            } else {
                Stdio::null()
            })
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);
        if let Some(dir) = &self.settings.working_dir {
            command.current_dir(dir);
        }
        let mut child = command
            .spawn()
            .map_err(|e| unavailable(format!("cannot start `{}`: {e}", self.settings.command)))?;
        if let Some(mut stdin) = child.stdin.take() {
            stdin
                .write_all(prompt.as_bytes())
                .await
                .map_err(|e| unavailable(format!("writing prompt failed: {e}")))?;
            // Dropping stdin sends EOF so the tool starts answering.
        }
        let output = tokio::time::timeout(self.settings.timeout, child.wait_with_output())
            .await
            .map_err(|_| {
                unavailable(format!(
                    "`{}` timed out after {:?}",
                    self.settings.command, self.settings.timeout
                ))
            })?
            .map_err(|e| unavailable(format!("`{}` failed: {e}", self.settings.command)))?;
        if !output.status.success() {
            return Err(unavailable(format!(
                "`{}` exited with {}: {}",
                self.settings.command,
                output.status,
                failure_reason(&output.stderr, &output.stdout)
            )));
        }
        let text = String::from_utf8_lossy(&output.stdout).trim().to_owned();
        if text.is_empty() {
            return Err(unavailable(format!(
                "`{}` produced no output",
                self.settings.command
            )));
        }
        Ok(text)
    }
}

/// Some tools print errors (e.g. "Not logged in") on stdout, not stderr.
/// Keeps the end of the output: tools such as `codex exec` echo a banner and
/// the whole prompt first, and print the actual error last.
fn failure_reason(stderr: &[u8], stdout: &[u8]) -> String {
    let stderr = String::from_utf8_lossy(stderr);
    let source = if stderr.trim().is_empty() {
        String::from_utf8_lossy(stdout)
    } else {
        stderr
    };
    let source = source.trim();
    let chars = source.chars().count();
    if chars <= STDERR_SNIPPET_CHARS {
        return source.to_owned();
    }
    let tail: String = source.chars().skip(chars - STDERR_SNIPPET_CHARS).collect();
    format!("…{tail}")
}

fn unavailable(detail: String) -> DomainError {
    DomainError::new(ErrorKind::Unavailable, "LLM_UNAVAILABLE", detail)
}

/// Flattens chat messages into one prompt: the system prompt is wrapped in
/// tags so tools without a system-prompt flag still keep it apart.
pub fn render_prompt(messages: &[ChatMessage]) -> String {
    messages
        .iter()
        .map(|m| match m.role.as_str() {
            "system" => format!("<system>\n{}\n</system>", m.content),
            _ => m.content.clone(),
        })
        .collect::<Vec<_>>()
        .join("\n\n")
}

#[async_trait]
impl LlmClient for CliLlm {
    fn is_configured(&self) -> bool {
        !self.settings.command.trim().is_empty()
    }

    fn model(&self) -> &str {
        &self.label
    }

    async fn complete(&self, messages: &[ChatMessage]) -> DomainResult<String> {
        let _permit = self
            .permits
            .acquire()
            .await
            .map_err(|_| unavailable("CLI LLM is shutting down".into()))?;
        self.run(render_prompt(messages)).await
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;

    fn cli(command: &str, args: &[&str], mode: PromptMode) -> CliLlm {
        CliLlm::new(CliLlmSettings {
            command: command.into(),
            args: args.iter().map(|a| a.to_string()).collect(),
            prompt_mode: mode,
            working_dir: None,
            env: Vec::new(),
            timeout: Duration::from_secs(5),
            max_concurrency: 2,
        })
    }

    fn ask() -> Vec<ChatMessage> {
        vec![ChatMessage::system("be brief"), ChatMessage::user("hello")]
    }

    #[tokio::test]
    async fn stdin_mode_sends_the_rendered_prompt() {
        let out = cli("cat", &[], PromptMode::Stdin)
            .complete(&ask())
            .await
            .unwrap();
        assert_eq!(out, "<system>\nbe brief\n</system>\n\nhello");
    }

    #[tokio::test]
    async fn arg_mode_substitutes_or_appends_the_prompt() {
        let llm = cli("echo", &["Q:", "{prompt}"], PromptMode::Arg);
        assert!(llm
            .complete(&ask())
            .await
            .unwrap()
            .starts_with("Q: <system>"));
        let appended = cli("echo", &["Q:"], PromptMode::Arg);
        assert!(appended.complete(&ask()).await.unwrap().ends_with("hello"));
    }

    #[tokio::test]
    async fn runs_in_the_project_directory() {
        let dir = tempfile::tempdir().unwrap();
        let mut llm = cli("pwd", &[], PromptMode::Arg);
        llm.settings.args.clear();
        llm.settings.prompt_mode = PromptMode::Stdin;
        llm.settings.working_dir = Some(dir.path().to_path_buf());
        let out = llm.complete(&ask()).await.unwrap();
        assert_eq!(
            std::fs::canonicalize(out).unwrap(),
            std::fs::canonicalize(dir.path()).unwrap()
        );
    }

    #[tokio::test]
    async fn failures_and_timeouts_are_unavailable_errors() {
        assert_eq!(failure_reason(b"", b"Not logged in\n"), "Not logged in");
        assert_eq!(failure_reason(b"boom", b"ignored"), "boom");
        let echoed = format!(
            "OpenAI Codex\n--------\nuser\n{}\nERROR: Selected model is at capacity.\n",
            "prompt ".repeat(200)
        );
        let reason = failure_reason(echoed.as_bytes(), b"");
        assert!(reason.starts_with('…'));
        assert!(reason.ends_with("ERROR: Selected model is at capacity."));
        assert_eq!(reason.chars().count(), STDERR_SNIPPET_CHARS + 1);
        let failing = cli("false", &[], PromptMode::Arg)
            .complete(&ask())
            .await
            .unwrap_err();
        assert_eq!(failing.code, "LLM_UNAVAILABLE");
        let mut slow = cli("sleep", &["5"], PromptMode::Stdin);
        slow.settings.timeout = Duration::from_millis(100);
        assert!(slow
            .complete(&ask())
            .await
            .unwrap_err()
            .detail
            .contains("timed out"));
        let huge = vec![ChatMessage::user("x".repeat(MAX_ARG_BYTES + 1))];
        let too_long = cli("echo", &[], PromptMode::Arg)
            .complete(&huge)
            .await
            .unwrap_err();
        assert!(too_long.detail.contains("argument limit"));
        let missing = cli("definitely-not-a-command", &[], PromptMode::Stdin)
            .complete(&ask())
            .await
            .unwrap_err();
        assert!(missing.detail.contains("cannot start"));
    }
}
