//! The ranges a diff is taken over, and the steps they are made of: the
//! patches in a codebase's log, or the git commits of a folder of sources
//! (plus uncommitted changes in the working tree).

use crate::ast::Program;
use crate::diff;
use crate::store::{self, Codebase};
use crate::verify::Verdict;
use serde_json::{json, Value as Json};
use crate::fx::HashMap;
use std::path::{Path, PathBuf};

pub struct Step {
    /// stable id: the log index, or the position in the commit list
    pub id: usize,
    pub title: String,
    /// the commit a patch belongs to, or the commit itself (`a1b2c3d`)
    pub group: String,
    pub when: Option<u64>,
    pub before: Program,
    pub after: Program,
    pub renames: Vec<(String, String)>,
}

pub struct Range {
    pub kind: &'static str,
    pub label: String,
    pub base: Program,
    pub head: Program,
    pub steps: Vec<Step>,
    pub verdicts: HashMap<String, Verdict>,
    /// what the base can be set to, for the viewer
    pub options: Json,
}

impl Range {
    /// The programs as written, to be shown: each reference as short as its
    /// module's `use`s allow.
    fn shown(mut self) -> Range {
        let w = |p: &Program| crate::resolve::written_form(p);
        self.base = w(&self.base);
        self.head = w(&self.head);
        for s in &mut self.steps {
            s.before = w(&s.before);
            s.after = w(&s.after);
        }
        self
    }
}

/// Definitions a step touched (by content hash).
fn touched(s: &Step) -> Vec<String> {
    let before: HashMap<String, String> = s.before.decls.iter().map(|d| (d.qname(), store::self_hash(d))).collect();
    let after: HashMap<String, String> = s.after.decls.iter().map(|d| (d.qname(), store::self_hash(d))).collect();
    let mut out: Vec<String> = after.iter().filter(|(n, h)| before.get(*n) != Some(h)).map(|(n, _)| n.clone()).collect();
    out.extend(before.keys().filter(|n| !after.contains_key(*n)).cloned());
    out.sort();
    out
}

impl Range {
    /// The diff of the whole range, or of one step.
    pub fn diff(&self, step: Option<usize>) -> Result<Json, String> {
        let names: diff::StepNames = self.steps.iter().map(|s| (s.id, touched(s))).collect();
        let (d, range_label) = match step {
            None => {
                let renames: Vec<(String, String)> = self.steps.iter().flat_map(|s| s.renames.clone()).collect();
                (diff::diff(&self.base, &self.head, &renames, &names, &self.verdicts), self.label.clone())
            }
            Some(k) => {
                let s = self.steps.iter().find(|s| s.id == k).ok_or(format!("no step {k} in this range"))?;
                let mine: diff::StepNames = names.into_iter().filter(|(id, _)| *id == k).collect();
                (diff::diff(&s.before, &s.after, &s.renames, &mine, &self.verdicts), s.title.clone())
            }
        };
        let mut d = d;
        d["range"] = json!({"kind": self.kind, "label": range_label, "step": step});
        d["steps"] = Json::Array(
            self.steps
                .iter()
                .enumerate()
                .map(|(i, s)| json!({"id": s.id, "n": i + 1, "title": s.title, "group": s.group, "when": s.when, "names": touched(s)}))
                .collect(),
        );
        d["options"] = self.options.clone();
        Ok(d)
    }
}

/// What a log range can be set to: its entries, and the current bounds.
pub fn log_options(cb: &Codebase, from: Option<usize>, to: Option<usize>) -> Json {
    let log = cb.log();
    json!({"kind": "log", "entries": log.iter().enumerate().map(|(i, e)| json!({"index": i, "message": e["message"], "time": e["time"]})).collect::<Vec<_>>(), "from": from, "to": to})
}

/// What a git range can be set to: the current base, branches, recent commits.
pub fn git_options(path: &Path, base: Option<&str>) -> Json {
    let dir = if path.is_dir() { path.to_path_buf() } else { path.parent().filter(|p| !p.as_os_str().is_empty()).unwrap_or(Path::new(".")).to_path_buf() };
    let recent: Vec<Json> = crate::git::run(Some(&dir), &["log", "-15", "--format=%h%x09%s", "--", "."])
        .unwrap_or_default()
        .lines()
        .map(|l| {
            let (h, s) = l.split_once('\t').unwrap_or((l, ""));
            json!({"rev": h, "message": s})
        })
        .collect();
    let branches: Vec<String> = crate::git::run(Some(&dir), &["branch", "--format=%(refname:short)"]).unwrap_or_default().lines().map(|s| s.to_string()).collect();
    json!({"kind": "git", "base": base.unwrap_or("HEAD"), "commits": recent, "branches": branches})
}

/// A codebase's log: the patches from entry `from` to `to` (default: the
/// latest). By default it starts at the first entry, or after it when that
/// entry built the package from nothing (an import).
pub fn log_range(cb: &Codebase, from: Option<usize>, to: Option<usize>) -> Result<Range, String> {
    let log = cb.log();
    let states = cb.states()?;
    let to = to.unwrap_or(log.len()).min(log.len());
    let import = log.len() > 1 && states[0].decls.is_empty();
    let from = from.unwrap_or(if import { 1 } else { 0 }).min(to);
    let steps = (from..to)
        .map(|k| {
            let e = &log[k];
            let renames = store::renames_of(e);
            Step { id: k, title: e["message"].as_str().unwrap_or("").to_string(), group: String::new(), when: e["time"].as_u64(), before: states[k].clone(), after: states[k + 1].clone(), renames }
        })
        .collect();
    let options = log_options(cb, Some(from), Some(to));
    Ok(Range {
        kind: "log",
        label: format!("patches {}–{} of {}", from + 1, to, log.len()),
        base: states[from].clone(),
        head: states[to].clone(),
        steps,
        verdicts: cb.load_verdicts(),
        options,
    }
    .shown())
}

/// The program at a git revision: a file, or a folder of `.strata` module files.
pub(crate) fn sources_at(path: &Path, rev: &str) -> Result<Program, String> {
    if path.is_dir() {
        let mut files = Vec::new();
        for rel in crate::git::run(Some(path), &["ls-tree", "-r", "--name-only", rev, "--", "."])?.lines().filter(|l| l.ends_with(".strata")) {
            files.push((PathBuf::from(rel), crate::git::bytes(Some(path), &["show", &format!("{rev}:./{rel}")])?));
        }
        crate::package::decode(files)
    } else {
        let dir = path.parent().filter(|p| !p.as_os_str().is_empty()).unwrap_or(Path::new("."));
        let name = path.file_name().ok_or("not a file")?.to_string_lossy().to_string();
        match crate::git::bytes(Some(dir), &["show", &format!("{rev}:./{name}")]) {
            Ok(bytes) => crate::package::decode_file(Path::new(&name), &bytes).map_err(|e| format!("at {rev}: {e}")),
            Err(_) => Ok(Program::default()),
        }
    }
}

/// The log entries of a package (a folder with a `strata.log`) at a revision.
fn log_at(dir: &Path, rev: &str) -> Vec<Json> {
    crate::git::run(Some(dir), &["show", &format!("{rev}:./{}", store::LOG)]).map(|t| store::parse_log(&t)).unwrap_or_default()
}

fn ids(log: &[Json]) -> crate::fx::HashSet<String> {
    log.iter().filter_map(|e| e["id"].as_str().map(|s| s.to_string())).collect()
}

/// The same definitions, whatever their order.
fn same(a: &Program, b: &Program) -> bool {
    let set = |p: &Program| p.decls.iter().map(|d| (d.qname(), store::self_hash(d))).collect::<std::collections::BTreeSet<_>>();
    set(a) == set(b)
}

fn renames_of(e: &Json) -> Vec<(String, String)> {
    store::renames_of(e)
}

/// The steps from `before` to `after`, one per log entry that is in `log`
/// but not in `known`, replayed in order; what the entries don't explain
/// (edits made outside strata) is a last step titled `rest`.
fn log_steps(steps: &mut Vec<Step>, before: &Program, after: &Program, log: &[Json], known: &crate::fx::HashSet<String>, group: &str, when: Option<u64>, rest: &str) {
    let mut cur = before.clone();
    let mut any = false;
    for e in log.iter().filter(|e| e["id"].as_str().is_some_and(|id| !known.contains(id))) {
        let mut next = cur.clone();
        if store::apply(&mut next, e).is_err() {
            break; // an entry that no longer applies: the rest is one step
        }
        any = true;
        steps.push(Step { id: steps.len() + 1, title: e["message"].as_str().unwrap_or("").to_string(), group: group.to_string(), when: e["time"].as_u64().or(when), before: cur, after: next.clone(), renames: renames_of(e) });
        cur = next;
    }
    if !same(&cur, after) {
        let title = if any { format!("{rest}: edits outside strata") } else { rest.to_string() };
        steps.push(Step { id: steps.len() + 1, title, group: group.to_string(), when, before: cur, after: after.clone(), renames: vec![] });
    }
}

/// Source files under git: the commits from `base` (default `HEAD`) to
/// `HEAD`, then uncommitted changes in the working tree. In a package with a
/// `strata.log`, each commit is split into the log entries it added (with
/// their renames), plus a step for edits made outside strata.
pub fn git_range(path: &Path, base: Option<&str>) -> Result<Range, String> {
    let dir = if path.is_dir() { path.to_path_buf() } else { path.parent().filter(|p| !p.as_os_str().is_empty()).unwrap_or(Path::new(".")).to_path_buf() };
    crate::git::run(Some(&dir), &["rev-parse", "--show-toplevel"]).map_err(|_| format!("{} is not in a git repository, so there is nothing to compare it with", path.display()))?;
    let base = base.unwrap_or("HEAD");
    // the viewer passes what the page sent: never let it read as a git option
    if base.starts_with('-') {
        return Err(format!("`{base}` is not a branch or commit in this repository"));
    }
    let base_sha = crate::git::run(Some(&dir), &["rev-parse", "--short", "--verify", &format!("{base}^{{commit}}")])
        .map_err(|_| format!("`{base}` is not a branch or commit in this repository"))?
        .trim()
        .to_string();
    let base_prog = sources_at(path, base).map_err(|e| format!("the code at `{base}` can't be read by this version of strata ({e}), so there is nothing to compare with; pick another base"))?;
    let package = path.is_dir();
    let mut known = if package { ids(&log_at(&dir, base)) } else { Default::default() };
    let mut steps = Vec::new();
    let mut prev = base_prog.clone();
    let commits = crate::git::run(Some(&dir), &["log", "--reverse", "--format=%h%x09%ct%x09%s", &format!("{base}..HEAD"), "--", "."])?;
    // a commit whose code can't be read is folded into the next one that can
    let mut skipped: Vec<String> = Vec::new();
    for l in commits.lines() {
        let mut parts = l.splitn(3, '\t');
        let (sha, when, subject) = (parts.next().unwrap_or(""), parts.next().and_then(|t| t.parse().ok()), parts.next().unwrap_or(""));
        let Ok(after) = sources_at(path, sha) else {
            skipped.push(sha.to_string());
            continue;
        };
        let title = if skipped.is_empty() { subject.to_string() } else { format!("{subject} (with {}, which can't be read)", skipped.join(", ")) };
        skipped.clear();
        let log = if package { log_at(&dir, sha) } else { vec![] };
        log_steps(&mut steps, &prev, &after, &log, &known, sha, when, &title);
        known.extend(ids(&log));
        prev = after;
    }
    let head = crate::read_sources(&path.to_string_lossy())?;
    let log = if package { store::parse_log(&std::fs::read_to_string(dir.join(store::LOG)).unwrap_or_default()) } else { vec![] };
    log_steps(&mut steps, &prev, &head, &log, &known, "working tree", None, "uncommitted changes");
    Ok(Range {
        kind: "git",
        label: format!("{base} ({base_sha}) → working tree"),
        base: base_prog,
        head,
        steps,
        verdicts: HashMap::default(),
        options: git_options(path, Some(base)),
    }
    .shown())
}
