//! Language-neutral tokenisation for FTS5/BM25.
//!
//! Latin-script words are lower-cased; runs of CJK characters become
//! overlapping bigrams ("記憶體" → "記憶 憶體"), the standard dictionary-free
//! way to index Chinese/Japanese text. The same function tokenises
//! documents and queries, so they always agree.

const STOP_WORDS: [&str; 12] = [
    "the", "a", "an", "of", "to", "and", "or", "is", "are", "in", "on", "for",
];

fn is_cjk(c: char) -> bool {
    matches!(c as u32,
        0x3040..=0x30FF   // Hiragana, Katakana
        | 0x3400..=0x4DBF // CJK Ext A
        | 0x4E00..=0x9FFF // CJK Unified
        | 0xAC00..=0xD7AF // Hangul
        | 0xF900..=0xFAFF // Compatibility
    )
}

pub fn tokenize(text: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    let mut word = String::new();
    let mut cjk: Vec<char> = Vec::new();
    let flush_word = |word: &mut String, tokens: &mut Vec<String>| {
        if !word.is_empty() {
            tokens.push(std::mem::take(word).to_lowercase());
        }
    };
    let flush_cjk = |cjk: &mut Vec<char>, tokens: &mut Vec<String>| {
        match cjk.len() {
            0 => {}
            1 => tokens.push(cjk[0].to_string()),
            _ => tokens.extend(cjk.windows(2).map(|w| w.iter().collect())),
        }
        cjk.clear();
    };
    for c in text.chars() {
        if is_cjk(c) {
            flush_word(&mut word, &mut tokens);
            cjk.push(c);
        } else if c.is_alphanumeric() || c == '_' {
            flush_cjk(&mut cjk, &mut tokens);
            word.push(c);
        } else {
            flush_word(&mut word, &mut tokens);
            flush_cjk(&mut cjk, &mut tokens);
        }
    }
    flush_word(&mut word, &mut tokens);
    flush_cjk(&mut cjk, &mut tokens);
    tokens
}

/// Tokens too common to be worth matching on in a query.
pub fn is_stop_word(token: &str) -> bool {
    STOP_WORDS.contains(&token)
}

/// Space-joined tokens for the FTS `tokens` column.
pub fn index_text(text: &str) -> String {
    tokenize(text).join(" ")
}

/// An FTS5 MATCH expression: quoted, de-duplicated tokens joined with OR.
/// `None` when nothing searchable remains.
pub fn match_query(query: &str) -> Option<String> {
    let mut seen = std::collections::HashSet::new();
    let terms: Vec<String> = tokenize(query)
        .into_iter()
        .filter(|t| !is_stop_word(t))
        .filter(|t| seen.insert(t.clone()))
        .map(|t| format!("\"{}\"", t.replace('"', "")))
        .collect();
    (!terms.is_empty()).then(|| terms.join(" OR "))
}

/// FTS5 `bm25()` is lower-is-better and negative; map it to 0..1.
pub fn bm25_to_score(rank: f64) -> f64 {
    if !rank.is_finite() {
        return 0.001;
    }
    if rank < 0.0 {
        let r = -rank;
        r / (1.0 + r)
    } else {
        1.0 / (1.0 + rank)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mixes_cjk_bigrams_and_words() {
        assert_eq!(
            tokenize("用Rust寫記憶體"),
            ["用", "rust", "寫記", "記憶", "憶體"]
        );
    }

    #[test]
    fn match_query_quotes_and_drops_stop_words() {
        assert_eq!(
            match_query("the Release plan").unwrap(),
            "\"release\" OR \"plan\""
        );
        assert_eq!(match_query("?!"), None);
    }

    #[test]
    fn bm25_scores_are_bounded() {
        assert!(bm25_to_score(-5.0) > bm25_to_score(-1.0));
        assert!((0.0..=1.0).contains(&bm25_to_score(3.0)));
    }
}
