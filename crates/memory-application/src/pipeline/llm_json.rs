//! Tolerant extraction of JSON from LLM output (think blocks, code fences,
//! prose around the payload).

use memory_domain::error::{DomainError, DomainResult};
use serde::de::DeserializeOwned;

/// Strips `<think>…</think>` and parses the outermost JSON value that
/// starts with `open` (`[` or `{`).
pub fn parse_llm_json<T: DeserializeOwned>(raw: &str, open: char) -> DomainResult<T> {
    let text = strip_think(raw);
    let close = if open == '[' { ']' } else { '}' };
    let start = text.find(open);
    let end = text.rfind(close);
    let (Some(start), Some(end)) = (start, end) else {
        return Err(DomainError::unprocessable(
            "llm_output_invalid",
            "no JSON payload in LLM output",
        ));
    };
    if end < start {
        return Err(DomainError::unprocessable(
            "llm_output_invalid",
            "malformed JSON payload",
        ));
    }
    serde_json::from_str(&text[start..=end]).map_err(|e| {
        DomainError::unprocessable("llm_output_invalid", format!("invalid JSON from LLM: {e}"))
    })
}

pub fn strip_think(raw: &str) -> String {
    let mut text = raw.to_owned();
    while let Some(start) = text.find("<think>") {
        let end = text[start..]
            .find("</think>")
            .map(|e| start + e + "</think>".len())
            .unwrap_or(text.len());
        text.replace_range(start..end, "");
    }
    text
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_fenced_json_after_thinking() {
        let raw = "<think>hmm [x]</think>Sure:\n```json\n[{\"a\":1}]\n```";
        let v: Vec<serde_json::Value> = parse_llm_json(raw, '[').unwrap();
        assert_eq!(v[0]["a"], 1);
    }

    #[test]
    fn errors_without_payload() {
        assert!(parse_llm_json::<Vec<i32>>("nothing", '[').is_err());
    }
}
