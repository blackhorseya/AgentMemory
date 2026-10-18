//! Skill rules: SKILL.md frontmatter, naming, string patches, resource paths
//! and the `<available_skills>` prompt block.

use serde::{Deserialize, Serialize};

use crate::error::{DomainError, DomainResult, ErrorKind};

pub const MAX_NAME_CHARS: usize = 64;
pub const MAX_DESCRIPTION_CHARS: usize = 1024;
pub const MAX_BODY_CHARS: usize = 50_000;
pub const MAX_RESOURCE_BYTES: usize = 5_000_000;
pub const MAX_TOTAL_RESOURCE_BYTES: usize = 50_000_000;
pub const DEFAULT_LISTING_BUDGET: usize = 8000;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SkillStatus {
    Active,
    Archived,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ResourceEntry {
    pub path: String,
    pub size_bytes: u64,
    pub mime_type: String,
    pub is_executable: bool,
}

/// One immutable `(skill_id, version)` snapshot.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SkillVersion {
    pub skill_id: String,
    pub version: u32,
    pub is_head: bool,
    pub name: String,
    pub description: String,
    pub content: String,
    pub content_hash: String,
    pub manifest: Vec<ResourceEntry>,
    pub status: SkillStatus,
    pub owner_user_id: String,
    pub owner_agent_id: String,
    pub team_id: String,
    pub task_id: Option<String>,
    pub metadata: serde_json::Value,
    pub created_at_ms: i64,
    pub updated_at_ms: i64,
}

impl SkillVersion {
    pub fn is_expired(&self, now_ms: i64, ttl_seconds: i64) -> bool {
        !self.is_head && ttl_seconds > 0 && now_ms - self.created_at_ms > ttl_seconds * 1000
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Frontmatter {
    pub name: String,
    pub description: String,
    pub body: String,
}

fn skill_err(kind: ErrorKind, code: &str, detail: impl Into<String>) -> DomainError {
    DomainError::new(kind, code, detail)
}

fn invalid_frontmatter(detail: impl Into<String>) -> DomainError {
    skill_err(
        ErrorKind::Unprocessable,
        "SKILL_FRONTMATTER_INVALID",
        detail,
    )
}

/// Parses the `---` fenced header of a SKILL.md. Only flat `key: value`
/// scalars are interpreted; other keys are preserved in the text untouched.
pub fn parse_frontmatter(content: &str) -> DomainResult<Frontmatter> {
    let text = content.replace("\r\n", "\n");
    let rest = text
        .strip_prefix("---\n")
        .ok_or_else(|| invalid_frontmatter("SKILL.md must start with '---'"))?;
    let (header, body) =
        split_closing_fence(rest).ok_or_else(|| invalid_frontmatter("unterminated frontmatter"))?;

    let mut name = None;
    let mut description = None;
    for line in header.lines() {
        let Some((key, value)) = line.split_once(':') else {
            continue;
        };
        let value = unquote(value.trim());
        match key.trim() {
            "name" => name = Some(value),
            "description" => description = Some(value),
            _ => {}
        }
    }
    let name = name
        .filter(|n| !n.is_empty())
        .ok_or_else(|| invalid_frontmatter("missing name"))?;
    let description = description
        .filter(|d| !d.is_empty())
        .ok_or_else(|| invalid_frontmatter("missing description"))?;
    validate_name(&name)?;
    if description.chars().count() > MAX_DESCRIPTION_CHARS {
        return Err(invalid_frontmatter("description exceeds 1024 characters"));
    }
    let body = body.strip_prefix('\n').unwrap_or(body).to_owned();
    if body.chars().count() > MAX_BODY_CHARS {
        return Err(invalid_frontmatter("body exceeds 50000 characters"));
    }
    Ok(Frontmatter {
        name,
        description,
        body,
    })
}

fn split_closing_fence(rest: &str) -> Option<(&str, &str)> {
    if let Some(body) = rest.strip_prefix("---") {
        if body.is_empty() || body.starts_with('\n') {
            return Some(("", body));
        }
    }
    let mut search = 0;
    while let Some(pos) = rest[search..].find("\n---") {
        let at = search + pos;
        let after = &rest[at + 4..];
        if after.is_empty() || after.starts_with('\n') {
            return Some((&rest[..at], after));
        }
        search = at + 4;
    }
    None
}

fn unquote(value: &str) -> String {
    let stripped = value
        .strip_prefix('"')
        .and_then(|v| v.strip_suffix('"'))
        .or_else(|| value.strip_prefix('\'').and_then(|v| v.strip_suffix('\'')));
    stripped.unwrap_or(value).trim().to_owned()
}

/// `^[a-z0-9][a-z0-9-]*$`, at most 64 characters.
pub fn validate_name(name: &str) -> DomainResult<()> {
    let mut chars = name.chars();
    let first_ok = chars
        .next()
        .is_some_and(|c| c.is_ascii_lowercase() || c.is_ascii_digit());
    let rest_ok = chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-');
    if !first_ok || !rest_ok || name.len() > MAX_NAME_CHARS {
        return Err(invalid_frontmatter(format!("invalid skill name: {name}")));
    }
    Ok(())
}

/// Replaces `old` with `new`. Without `replace_all` the match must be unique.
pub fn apply_patch(content: &str, old: &str, new: &str, replace_all: bool) -> DomainResult<String> {
    let not_unique = |n: usize| {
        skill_err(
            ErrorKind::Unprocessable,
            "SKILL_PATCH_NOT_UNIQUE",
            format!("old_string matched {n} times"),
        )
    };
    if old.is_empty() {
        return Err(not_unique(0));
    }
    let occurrences = content.matches(old).count();
    match (occurrences, replace_all) {
        (0, _) => Err(not_unique(0)),
        (1, _) | (_, true) => Ok(content.replace(old, new)),
        (n, false) => Err(not_unique(n)),
    }
}

/// Rejects empty, absolute, NUL-containing or `..` resource paths.
pub fn validate_resource_path(path: &str) -> DomainResult<()> {
    let bad = path.is_empty()
        || path.starts_with('/')
        || path.starts_with('\\')
        || path.contains('\0')
        || path.split(['/', '\\']).any(|seg| seg == "..")
        || path.len() > 512;
    if bad {
        return Err(skill_err(
            ErrorKind::InvalidInput,
            "INVALID_PATH",
            format!("invalid resource path: {path}"),
        ));
    }
    Ok(())
}

pub fn guess_mime(path: &str) -> &'static str {
    match path
        .rsplit('.')
        .next()
        .unwrap_or("")
        .to_ascii_lowercase()
        .as_str()
    {
        "md" => "text/markdown",
        "txt" => "text/plain",
        "json" => "application/json",
        "yaml" | "yml" => "application/yaml",
        "py" => "text/x-python",
        "sh" => "application/x-sh",
        "js" => "text/javascript",
        "ts" => "text/typescript",
        "html" => "text/html",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "pdf" => "application/pdf",
        _ => "application/octet-stream",
    }
}

pub struct ListingLine<'a> {
    pub skill_id: &'a str,
    pub name: &'a str,
    pub description: &'a str,
}

/// Renders the `<available_skills>` block injected into agent prompts.
/// Returns the block and how many lines are fully visible.
pub fn render_listing(lines: &[ListingLine<'_>], char_budget: usize) -> (String, usize) {
    const OPEN: &str = "<available_skills>\n";
    const CLOSE: &str = "</available_skills>";
    if lines.is_empty() {
        return (format!("{OPEN}(none)\n{CLOSE}"), 0);
    }
    let mut out = String::from(OPEN);
    out.push_str("# One skill per line: id=<skill_id>, name=<name>, desc=<description>.\n");
    out.push_str("# Pass the id value (skl-xxxxxx) when calling skill_view.\n");
    let mut visible = 0;
    for line in lines {
        let desc = line
            .description
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ");
        out.push_str(&format!(
            "- id={}, name={}, desc={}\n",
            line.skill_id, line.name, desc
        ));
        if out.chars().count() + CLOSE.len() <= char_budget {
            visible += 1;
        }
    }
    out.push_str(CLOSE);
    if out.chars().count() <= char_budget {
        return (out, visible);
    }
    let suffix = format!("\n... [truncated]\n{CLOSE}");
    let keep = char_budget.saturating_sub(suffix.chars().count());
    let truncated: String = out.chars().take(keep).collect();
    (format!("{truncated}{suffix}"), visible)
}

#[cfg(test)]
mod tests {
    use super::*;

    const DOC: &str =
        "---\nname: code-review\ndescription: \"Review a diff\"\n---\n# Code Review\nsteps";

    #[test]
    fn parses_frontmatter_and_strips_leading_newline() {
        let fm = parse_frontmatter(DOC).unwrap();
        assert_eq!(fm.name, "code-review");
        assert_eq!(fm.description, "Review a diff");
        assert_eq!(fm.body, "# Code Review\nsteps");
    }

    #[test]
    fn rejects_invalid_names_and_missing_fences() {
        assert!(parse_frontmatter("name: x").is_err());
        assert!(parse_frontmatter("---\nname: Bad_Name\ndescription: d\n---\n").is_err());
    }

    #[test]
    fn patch_requires_unique_match_unless_replace_all() {
        assert_eq!(apply_patch("a b a", "b", "c", false).unwrap(), "a c a");
        assert_eq!(
            apply_patch("a b a", "a", "z", false).unwrap_err().code,
            "SKILL_PATCH_NOT_UNIQUE"
        );
        assert_eq!(apply_patch("a b a", "a", "z", true).unwrap(), "z b z");
    }

    #[test]
    fn resource_paths_cannot_escape() {
        assert!(validate_resource_path("scripts/run.sh").is_ok());
        for bad in ["", "/etc/passwd", "a/../b", "..\\x"] {
            assert!(validate_resource_path(bad).is_err());
        }
    }

    #[test]
    fn listing_truncates_to_budget() {
        let lines: Vec<_> = (0..50)
            .map(|_| ListingLine {
                skill_id: "skl-abcdefabcdef",
                name: "name",
                description: "a fairly long description",
            })
            .collect();
        let (block, visible) = render_listing(&lines, 400);
        assert!(block.chars().count() <= 400);
        assert!(block.ends_with("</available_skills>"));
        assert!(visible > 0 && visible < 50);
    }
}
