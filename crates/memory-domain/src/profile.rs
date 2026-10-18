//! L2/L3 "profile" rules. Scenes and the persona belong to a team + agent
//! (users and sessions are not part of the profile scope): an agent's
//! scenes and persona are shared by everyone who talks to it.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::error::{DomainError, DomainResult};
use crate::scope::{MemoryScope, SegmentId};

pub const META_START: &str = "-----META-START-----";
pub const META_END: &str = "-----META-END-----";
pub const NAVIGATION_MARKER: &str = "---\n## 🗺️ Scene Navigation (Scene Index)";
pub const DELETED_MARKER: &str = "[DELETED]";
pub const MAX_SCENE_CHARS: usize = 1500;
pub const SCENE_DIR: &str = "scene_blocks/";
pub const PERSONA_PATH: &str = "persona.md";

/// A versioned text file of a profile: `persona.md` (L3) or
/// `scene_blocks/<name>.md` (L2).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProfileFile {
    pub path: String,
    pub content: String,
    pub version: u32,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub updated_at: chrono::DateTime<chrono::Utc>,
}

pub fn scene_path(relative: &str) -> String {
    format!("{SCENE_DIR}{relative}")
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct ProfileScope {
    pub team_id: SegmentId,
    pub agent_id: SegmentId,
}

impl From<&MemoryScope> for ProfileScope {
    fn from(scope: &MemoryScope) -> Self {
        Self {
            team_id: scope.team_id.clone(),
            agent_id: scope.agent_id.clone(),
        }
    }
}

impl ProfileScope {
    pub fn key(&self) -> String {
        format!("team:{}|agent:{}", self.team_id, self.agent_id)
    }
}

/// The `-----META-START-----` header of a scene file.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct SceneMeta {
    pub created: String,
    pub updated: String,
    pub summary: String,
    pub heat: u32,
    /// Unknown keys are preserved verbatim on rewrite.
    pub extra: BTreeMap<String, String>,
}

/// Splits a scene file into its META header (if any) and body.
pub fn parse_scene(text: &str) -> (Option<SceneMeta>, String) {
    let text = text.replace("\r\n", "\n");
    let Some(rest) = text.strip_prefix(&format!("{META_START}\n")) else {
        return (None, text);
    };
    let Some(end) = rest.find(&format!("\n{META_END}")) else {
        return (None, text.clone());
    };
    let mut meta = SceneMeta::default();
    for line in rest[..end].lines() {
        let Some((key, value)) = line.split_once(':') else {
            continue;
        };
        let value = value.trim().to_owned();
        match key.trim() {
            "created" => meta.created = value,
            "updated" => meta.updated = value,
            "summary" => meta.summary = value,
            "heat" => meta.heat = value.parse().unwrap_or(0),
            other => {
                meta.extra.insert(other.to_owned(), value);
            }
        }
    }
    let body = &rest[end + META_END.len() + 1..];
    let body = body.strip_prefix('\n').unwrap_or(body);
    let body = body.strip_prefix('\n').unwrap_or(body);
    (Some(meta), body.to_owned())
}

pub fn render_scene(meta: &SceneMeta, body: &str) -> String {
    let mut header = format!(
        "{META_START}\ncreated: {}\nupdated: {}\nsummary: {}\nheat: {}\n",
        meta.created, meta.updated, meta.summary, meta.heat
    );
    for (k, v) in &meta.extra {
        header.push_str(&format!("{k}: {v}\n"));
    }
    format!("{header}{META_END}\n\n{}", body.trim_start_matches('\n'))
}

/// A scene body that the extractor asked to delete, or that is empty.
pub fn is_tombstone(body: &str) -> bool {
    let trimmed = body.trim();
    trimmed.is_empty() || trimmed == DELETED_MARKER
}

/// Repairs an LLM-proposed scene filename: spaces become `-`, characters
/// other than letters, digits, CJK, `-`, `_` and `.` are dropped, `.md` is
/// enforced.
pub fn normalize_scene_filename(raw: &str) -> DomainResult<String> {
    let base = raw.trim().trim_end_matches(".md");
    let base = base.rsplit(['/', '\\']).next().unwrap_or(base);
    let cleaned: String = base
        .chars()
        .map(|c| if c.is_whitespace() { '-' } else { c })
        .filter(|c| c.is_alphanumeric() || matches!(c, '-' | '_' | '.'))
        .collect();
    let cleaned = cleaned.trim_matches(['-', '.']).to_owned();
    if cleaned.is_empty() {
        return Err(DomainError::invalid(format!(
            "invalid scene filename: {raw}"
        )));
    }
    Ok(format!("{cleaned}.md"))
}

/// Validates a client-supplied relative scenario path.
pub fn validate_relative_path(path: &str) -> DomainResult<()> {
    let bad = path.contains('\0')
        || path.contains('\\')
        || path.starts_with('/')
        || path.split('/').any(|seg| seg == "..");
    if bad {
        return Err(DomainError::invalid(format!("invalid path: {path}")));
    }
    Ok(())
}

pub fn strip_scene_navigation(persona: &str) -> String {
    match persona.find(NAVIGATION_MARKER) {
        Some(idx) => persona[..idx].trim().to_owned(),
        None => persona.trim().to_owned(),
    }
}

pub struct NavigationEntry<'a> {
    pub filename: &'a str,
    pub summary: &'a str,
    pub heat: u32,
    pub updated: &'a str,
}

fn heat_flames(heat: u32) -> String {
    let flames = [50, 100, 200, 500, 1000]
        .iter()
        .filter(|t| heat >= **t)
        .count();
    "🔥".repeat(flames)
}

/// The scene index appended to persona.md, hottest scenes first.
pub fn render_navigation(entries: &mut [NavigationEntry<'_>]) -> String {
    entries.sort_by(|a, b| b.heat.cmp(&a.heat).then_with(|| a.filename.cmp(b.filename)));
    let mut out = format!(
        "{NAVIGATION_MARKER}\n*Index of the current scene memories; read a file for details when needed.*\n\n"
    );
    for e in entries.iter() {
        out.push_str(&format!(
            "### Path: scene_blocks/{}\n**Heat**: {}{} | **Updated**: {}\nSummary: {}\n\n",
            e.filename,
            e.heat,
            heat_flames(e.heat),
            e.updated,
            e.summary
        ));
    }
    out.push_str("📌 Usage: consult a scene only when the current task needs that context.\n");
    out
}

/// Persona body + freshly rendered navigation.
pub fn compose_persona(body: &str, navigation: &str) -> String {
    let body = strip_scene_navigation(body);
    if navigation.is_empty() {
        body
    } else {
        format!("{body}\n\n{navigation}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scene_round_trips_through_meta_header() {
        let meta = SceneMeta {
            created: "2026-01-01T00:00:00Z".into(),
            updated: "2026-01-02T00:00:00Z".into(),
            summary: "release process".into(),
            heat: 3,
            extra: BTreeMap::new(),
        };
        let text = render_scene(&meta, "## Body\ntext");
        let (parsed, body) = parse_scene(&text);
        assert_eq!(parsed.unwrap(), meta);
        assert_eq!(body, "## Body\ntext");
    }

    #[test]
    fn file_without_header_is_all_body() {
        let (meta, body) = parse_scene("plain");
        assert!(meta.is_none());
        assert_eq!(body, "plain");
    }

    #[test]
    fn filenames_are_normalised() {
        assert_eq!(
            normalize_scene_filename("Release Process!").unwrap(),
            "Release-Process.md"
        );
        assert_eq!(
            normalize_scene_filename("../发版 流程.md").unwrap(),
            "发版-流程.md"
        );
        assert!(normalize_scene_filename("!!!").is_err());
    }

    #[test]
    fn navigation_is_replaced_not_duplicated() {
        let mut entries = [NavigationEntry {
            filename: "a.md",
            summary: "s",
            heat: 60,
            updated: "u",
        }];
        let nav = render_navigation(&mut entries);
        assert!(nav.contains("60🔥"));
        let once = compose_persona("# Persona", &nav);
        let twice = compose_persona(&once, &nav);
        assert_eq!(once, twice);
    }

    #[test]
    fn tombstones_detected() {
        assert!(is_tombstone("  [DELETED] "));
        assert!(!is_tombstone("content"));
    }
}
