//! OpenAI-compatible chat-completion and embedding clients.

use std::time::Duration;

use async_trait::async_trait;
use memory_application::ports::system::{ChatMessage, EmbeddingProvider, LlmClient};
use memory_domain::error::{DomainError, DomainResult, ErrorKind};
use serde::Deserialize;
use serde_json::json;

#[derive(Debug, Clone)]
pub struct LlmSettings {
    pub base_url: String,
    pub api_key: String,
    pub model: String,
    pub max_tokens: u32,
    pub timeout: Duration,
}

pub struct OpenAiLlm {
    http: reqwest::Client,
    settings: LlmSettings,
}

impl OpenAiLlm {
    pub fn new(settings: LlmSettings) -> Self {
        let http = reqwest::Client::builder()
            .timeout(settings.timeout)
            .build()
            .expect("reqwest client");
        Self { http, settings }
    }
}

fn llm_unavailable(detail: impl Into<String>) -> DomainError {
    DomainError::new(ErrorKind::Unavailable, "LLM_UNAVAILABLE", detail)
}

#[derive(Deserialize)]
struct ChatResponse {
    choices: Vec<ChatChoice>,
}

#[derive(Deserialize)]
struct ChatChoice {
    message: ChatMessage,
}

#[async_trait]
impl LlmClient for OpenAiLlm {
    fn is_configured(&self) -> bool {
        !self.settings.api_key.is_empty() && !self.settings.base_url.is_empty()
    }

    fn model(&self) -> &str {
        &self.settings.model
    }

    async fn complete(&self, messages: &[ChatMessage]) -> DomainResult<String> {
        if !self.is_configured() {
            return Err(llm_unavailable("LLM is not configured"));
        }
        let url = format!(
            "{}/chat/completions",
            self.settings.base_url.trim_end_matches('/')
        );
        let body = json!({
            "model": self.settings.model,
            "messages": messages,
            "max_tokens": self.settings.max_tokens,
            "temperature": 0.2,
        });
        let response = self
            .http
            .post(url)
            .bearer_auth(&self.settings.api_key)
            .json(&body)
            .send()
            .await
            .map_err(|e| llm_unavailable(format!("LLM request failed: {e}")))?;
        let status = response.status();
        if !status.is_success() {
            let text = response.text().await.unwrap_or_default();
            let snippet: String = text.chars().take(300).collect();
            return Err(llm_unavailable(format!("LLM returned {status}: {snippet}")));
        }
        let parsed: ChatResponse = response
            .json()
            .await
            .map_err(|e| llm_unavailable(format!("bad LLM response: {e}")))?;
        parsed
            .choices
            .into_iter()
            .next()
            .map(|c| c.message.content)
            .ok_or_else(|| llm_unavailable("LLM returned no choices"))
    }
}

#[derive(Debug, Clone)]
pub struct EmbeddingSettings {
    pub base_url: String,
    pub api_key: String,
    pub model: String,
    pub dimensions: usize,
    pub timeout: Duration,
}

const MAX_BATCH: usize = 256;
const MAX_INPUT_CHARS: usize = 5000;

pub struct OpenAiEmbedding {
    http: reqwest::Client,
    settings: EmbeddingSettings,
}

impl OpenAiEmbedding {
    pub fn new(settings: EmbeddingSettings) -> Self {
        let http = reqwest::Client::builder()
            .timeout(settings.timeout)
            .build()
            .expect("reqwest client");
        Self { http, settings }
    }

    async fn embed_batch(&self, batch: &[String]) -> DomainResult<Vec<Vec<f32>>> {
        #[derive(Deserialize)]
        struct Item {
            index: usize,
            embedding: Vec<f32>,
        }
        #[derive(Deserialize)]
        struct Response {
            data: Vec<Item>,
        }
        let input: Vec<String> = batch
            .iter()
            .map(|t| t.chars().take(MAX_INPUT_CHARS).collect())
            .collect();
        let url = format!(
            "{}/embeddings",
            self.settings.base_url.trim_end_matches('/')
        );
        let response = self
            .http
            .post(url)
            .bearer_auth(&self.settings.api_key)
            .json(&json!({"input": input, "model": self.settings.model, "dimensions": self.settings.dimensions}))
            .send()
            .await
            .map_err(|e| DomainError::unavailable(format!("embedding request failed: {e}")))?;
        if !response.status().is_success() {
            return Err(DomainError::unavailable(format!(
                "embedding service returned {}",
                response.status()
            )));
        }
        let mut parsed: Response = response
            .json()
            .await
            .map_err(|e| DomainError::unavailable(format!("bad embedding response: {e}")))?;
        parsed.data.sort_by_key(|d| d.index);
        Ok(parsed
            .data
            .into_iter()
            .map(|d| normalise(d.embedding))
            .collect())
    }
}

/// Replaces non-finite values and L2-normalises.
fn normalise(mut v: Vec<f32>) -> Vec<f32> {
    for x in v.iter_mut() {
        if !x.is_finite() {
            *x = 0.0;
        }
    }
    let norm = v.iter().map(|x| x * x).sum::<f32>().sqrt();
    if norm > 1e-10 {
        v.iter_mut().for_each(|x| *x /= norm);
    }
    v
}

#[async_trait]
impl EmbeddingProvider for OpenAiEmbedding {
    fn dimensions(&self) -> Option<usize> {
        Some(self.settings.dimensions)
    }

    async fn embed(&self, texts: &[String]) -> DomainResult<Vec<Vec<f32>>> {
        let mut out = Vec::with_capacity(texts.len());
        for batch in texts.chunks(MAX_BATCH) {
            out.extend(self.embed_batch(batch).await?);
        }
        Ok(out)
    }
}

/// Embedding disabled: recall uses BM25 only.
pub struct NoEmbedding;

#[async_trait]
impl EmbeddingProvider for NoEmbedding {
    fn dimensions(&self) -> Option<usize> {
        None
    }

    async fn embed(&self, _texts: &[String]) -> DomainResult<Vec<Vec<f32>>> {
        Ok(Vec::new())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalise_produces_unit_vectors_and_scrubs_nan() {
        let v = normalise(vec![3.0, 4.0, f32::NAN]);
        assert!((v[0] - 0.6).abs() < 1e-6 && (v[1] - 0.8).abs() < 1e-6 && v[2] == 0.0);
    }
}
