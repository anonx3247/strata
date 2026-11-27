//! Text helpers the Strata program in this folder calls (`extern rust`).
//! Parameters are `Deserialize` and results `Serialize` (serde): values
//! cross as JSON.

use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

/// A post, as Strata's `Post` crosses: an object with its fields (and a
/// `"type"` key, which serde ignores).
#[derive(Deserialize)]
pub struct Post {
    pub title: String,
    pub body: String,
    pub tags: Vec<String>,
}

/// A post summed up.
#[derive(Serialize)]
pub struct Card {
    pub slug: String,
    pub words: usize,
    pub teaser: String,
    pub tags: Vec<String>,
}

/// A title as a URL slug: lowercase ASCII words joined by dashes (a few
/// accented Latin letters as their plain one).
pub fn slugify(title: String) -> String {
    let mut out = String::new();
    for c in title.chars() {
        let c = match c {
            'à' | 'á' | 'â' | 'ä' | 'ã' | 'å' => 'a',
            'ç' => 'c',
            'è' | 'é' | 'ê' | 'ë' => 'e',
            'ì' | 'í' | 'î' | 'ï' => 'i',
            'ñ' => 'n',
            'ò' | 'ó' | 'ô' | 'ö' | 'õ' => 'o',
            'ù' | 'ú' | 'û' | 'ü' => 'u',
            c => c,
        };
        if c.is_ascii_alphanumeric() {
            out.push(c.to_ascii_lowercase());
        } else if !out.ends_with('-') && !out.is_empty() {
            out.push('-');
        }
    }
    out.trim_end_matches('-').to_string()
}

/// How many times each word appears, by word.
pub fn word_counts(text: String) -> BTreeMap<String, i64> {
    let mut counts = BTreeMap::new();
    for word in text
        .to_lowercase()
        .split(|c: char| !(c.is_ascii_lowercase() || c == '\''))
        .filter(|w| !w.is_empty())
    {
        *counts.entry(word.to_string()).or_insert(0) += 1;
    }
    counts
}

/// A post summed up as a Card.
pub fn summary(post: Post) -> Card {
    let words: Vec<&str> = post.body.split_whitespace().collect();
    Card {
        slug: slugify(post.title),
        words: words.len(),
        teaser: words[..words.len().min(5)].join(" "),
        tags: post.tags.into_iter().collect::<BTreeSet<_>>().into_iter().collect(),
    }
}

/// The year in `text`: an `Err` when there is none.
pub fn parse_year(text: String) -> Result<i64, String> {
    text.split(|c: char| !c.is_ascii_digit())
        .find(|w| w.len() == 4 && (w.starts_with('1') || w.starts_with("20")))
        .map(|w| w.parse().unwrap())
        .ok_or_else(|| format!("no year in {text:?}"))
}
