//! The activity sidebar: the target's history by branch (local branches and
//! the repository's other worktrees), then by commit, then by the
//! definitions each commit changed; and the structural diff between any two
//! points of it (commits, branches, a worktree's files on disk).
//!
//! It reads git only. A commit whose `.strata` files don't decode (an older
//! format) is shown as such rather than failing.

use crate::ast::Program;
use crate::fx::HashMap;
use crate::store;
use serde_json::{json, Value as Json};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

/// Where the target is in its repository.
#[derive(Clone)]
pub struct Repo {
    /// the repository's top folder (of the worktree the viewer runs in)
    pub top: PathBuf,
    /// the target, relative to `top` (`examples/tasks`, or empty for the top itself)
    pub rel: PathBuf,
    /// the target as given (a folder or a file)
    pub path: PathBuf,
}

impl Repo {
    pub fn of(path: &Path) -> Option<Repo> {
        let path = std::fs::canonicalize(path).ok()?;
        let dir = if path.is_dir() { path.clone() } else { path.parent()?.to_path_buf() };
        let top = PathBuf::from(crate::git::run(Some(&dir), &["rev-parse", "--show-toplevel"]).ok()?.trim());
        let top = std::fs::canonicalize(&top).unwrap_or(top);
        let rel = path.strip_prefix(&top).ok()?.to_path_buf();
        Some(Repo { top, rel, path })
    }

    pub fn git(&self, args: &[&str]) -> Result<String, String> {
        crate::git::run(Some(&self.top), args)
    }

    /// The target as seen from another worktree of the repository.
    pub fn in_worktree(&self, wt: &Path) -> PathBuf {
        if self.rel.as_os_str().is_empty() {
            wt.to_path_buf()
        } else {
            wt.join(&self.rel)
        }
    }

    /// The branch new work is compared with: `origin`'s default, else `main`.
    pub fn default_branch(&self) -> String {
        if let Ok(r) = self.git(&["symbolic-ref", "--short", "refs/remotes/origin/HEAD"]) {
            return r.trim().to_string();
        }
        for b in ["main", "master"] {
            if crate::git::ok(Some(&self.top), &["rev-parse", "--verify", "--quiet", b]) {
                return b.to_string();
            }
        }
        "HEAD".to_string()
    }

    /// The repository's worktrees: `(path, branch or None, head)`.
    pub fn worktrees(&self) -> Vec<(PathBuf, Option<String>, String)> {
        let mut out = vec![];
        let (mut path, mut branch, mut head) = (None::<PathBuf>, None, String::new());
        for l in self.git(&["worktree", "list", "--porcelain"]).unwrap_or_default().lines().chain(std::iter::once("")) {
            if let Some(p) = l.strip_prefix("worktree ") {
                path = Some(PathBuf::from(p));
            } else if let Some(h) = l.strip_prefix("HEAD ") {
                head = h.to_string();
            } else if let Some(b) = l.strip_prefix("branch ") {
                branch = Some(b.trim_start_matches("refs/heads/").to_string());
            } else if l.is_empty() {
                if let Some(p) = path.take() {
                    let p = std::fs::canonicalize(&p).unwrap_or(p);
                    out.push((p, branch.take(), std::mem::take(&mut head)));
                }
            }
        }
        out
    }
}

/// A revision as the page names it: a git revision, or `wt:<path>`, the
/// files on disk in one of the repository's worktrees.
pub fn program_at(repo: &Repo, spec: &str) -> Result<Program, String> {
    if let Some(wt) = spec.strip_prefix("wt:") {
        let wt = std::fs::canonicalize(wt).map_err(|e| format!("{wt}: {e}"))?;
        if !repo.worktrees().iter().any(|(p, _, _)| *p == wt) {
            return Err(format!("{} is not a worktree of this repository", wt.display()));
        }
        return crate::package::program(&repo.in_worktree(&wt));
    }
    if spec == "empty" {
        return Ok(Program::default());
    }
    // what the page sent: never let it read as a git option
    if spec.starts_with('-') || spec.is_empty() {
        return Err(format!("`{spec}` is not a branch or commit"));
    }
    crate::history::sources_at(&repo.path, spec).map_err(|e| format!("can't be read (older format): {e}"))
}

/// The structural diff from one revision to another (`/api/diff`).
pub fn diff(repo: &Repo, base: &str, head: &str) -> Json {
    let (a, b) = match (program_at(repo, base), program_at(repo, head)) {
        (Ok(a), Ok(b)) => (a, b),
        (Err(e), _) | (_, Err(e)) => return json!({"error": e}),
    };
    diff_programs(&a, &b)
}

/// The structural diff of two programs, as the page renders it.
pub fn diff_programs(a: &Program, b: &Program) -> Json {
    let w = crate::resolve::written_form;
    let mut d = crate::diff::diff(&w(a), &w(b), &[], &Vec::new(), &crate::store::Verdicts::none());
    d["steps"] = json!([]);
    d
}

/// What a step changed, by definition: added, removed, changed (and whether
/// its interface did).
pub fn changed_defs(before: &Program, after: &Program) -> Vec<Json> {
    let hashes = |p: &Program| -> HashMap<String, (String, String, String)> { p.decls.iter().map(|d| (d.qname(), (store::self_hash(d), store::interface_hash(d), d.kind_str().to_string()))).collect() };
    let (a, b) = (hashes(before), hashes(after));
    let mut out: Vec<Json> = vec![];
    for (n, (h, i, k)) in &b {
        match a.get(n) {
            None => out.push(json!({"name": n, "mk": "add", "kind": k})),
            Some((h0, i0, _)) if h0 != h => out.push(json!({"name": n, "mk": "chg", "kind": k, "iface": i0 != i})),
            _ => {}
        }
    }
    for (n, (_, _, k)) in &a {
        if !b.contains_key(n) {
            out.push(json!({"name": n, "mk": "del", "kind": k}));
        }
    }
    // `use` lines aren't definitions anyone looks for
    out.retain(|d| d["kind"] != "use");
    out.sort_by(|x, y| x["name"].as_str().cmp(&y["name"].as_str()));
    out
}

/// Commits never change: what each changed is computed once.
static COMMITS: Mutex<Option<HashMap<String, Json>>> = Mutex::new(None);

/// What a commit changed in the target: its definitions, or why they can't be read.
fn commit_defs(repo: &Repo, sha: &str) -> Json {
    if let Some(j) = COMMITS.lock().unwrap_or_else(|e| e.into_inner()).get_or_insert_with(HashMap::default).get(sha) {
        return j.clone();
    }
    let parent = format!("{sha}^");
    let has_parent = crate::git::ok(Some(&repo.top), &["rev-parse", "--verify", "--quiet", &parent]);
    let before = if has_parent { crate::history::sources_at(&repo.path, &parent) } else { Ok(Program::default()) };
    let after = crate::history::sources_at(&repo.path, sha);
    let j = match (before, after) {
        (_, Err(e)) => json!({"unreadable": e}),
        // the change from a commit that can't be read: everything is new
        (Err(_), Ok(after)) => json!({"defs": changed_defs(&Program::default(), &after), "base_unreadable": true}),
        (Ok(b), Ok(a)) => json!({"defs": changed_defs(&b, &a)}),
    };
    COMMITS.lock().unwrap_or_else(|e| e.into_inner()).get_or_insert_with(HashMap::default).insert(sha.to_string(), j.clone());
    j
}

fn commits(repo: &Repo, range: &str, limit: usize) -> Vec<Json> {
    let rel = if repo.rel.as_os_str().is_empty() { ".".to_string() } else { repo.rel.display().to_string() };
    let n = format!("-{limit}");
    repo.git(&["log", &n, "--format=%h%x09%ct%x09%an%x09%s", range, "--", &rel])
        .unwrap_or_default()
        .lines()
        .filter_map(|l| {
            let mut p = l.splitn(4, '\t');
            let (sha, when, author, subject) = (p.next()?, p.next()?, p.next()?, p.next().unwrap_or(""));
            let mut c = json!({"sha": sha, "when": when.parse::<u64>().ok(), "author": author, "subject": subject});
            let d = commit_defs(repo, sha);
            for (k, v) in d.as_object().into_iter().flatten() {
                c[k] = v.clone();
            }
            Some(c)
        })
        .collect()
}

/// What changed on disk in a worktree since its last commit (`None`:
/// nothing), worked out again only when its HEAD or its files changed.
fn uncommitted(repo: &Repo, wt: &Path) -> Option<Json> {
    static SEEN: Mutex<Option<HashMap<PathBuf, (u64, Option<Json>)>>> = Mutex::new(None);
    let head = rev_of_worktree(wt)?;
    let dir = repo.in_worktree(wt);
    let key = {
        use std::hash::{Hash, Hasher};
        let mut h = std::collections::hash_map::DefaultHasher::new();
        head.hash(&mut h);
        super::fingerprint_of(&dir).hash(&mut h);
        h.finish()
    };
    if let Some((k, j)) = SEEN.lock().unwrap_or_else(|e| e.into_inner()).get_or_insert_with(HashMap::default).get(wt) {
        if *k == key {
            return j.clone();
        }
    }
    let j = (|| {
        let before = crate::history::sources_at(&repo.path, &head).unwrap_or_default();
        let now = crate::package::program(&dir).ok()?;
        let defs = changed_defs(&before, &now);
        (!defs.is_empty()).then(|| json!({"defs": defs, "spec": format!("wt:{}", wt.display())}))
    })();
    SEEN.lock().unwrap_or_else(|e| e.into_inner()).get_or_insert_with(HashMap::default).insert(wt.to_path_buf(), (key, j.clone()));
    j
}

fn rev_of_worktree(wt: &Path) -> Option<String> {
    crate::git::run(Some(wt), &["rev-parse", "HEAD"]).ok().map(|s| s.trim().to_string())
}

/// The sidebar's timeline (`/api/activity`): one entry per branch with work
/// of its own (checked out in a worktree, or with commits the default branch
/// doesn't have), then the default branch's latest commits.
pub fn activity(repo: &Repo) -> Json {
    // the same branches, heads and files as last time: the same timeline
    static LAST: Mutex<Option<(u64, Json)>> = Mutex::new(None);
    let key = {
        use std::hash::{Hash, Hasher};
        let mut h = std::collections::hash_map::DefaultHasher::new();
        repo.git(&["for-each-ref", "refs/heads", "refs/remotes/origin", "--format=%(refname)%(objectname)"]).unwrap_or_default().hash(&mut h);
        for (p, b, head) in repo.worktrees() {
            (&p, b, head).hash(&mut h);
            super::fingerprint_of(&repo.in_worktree(&p)).hash(&mut h);
        }
        h.finish()
    };
    if let Some((k, j)) = LAST.lock().unwrap_or_else(|e| e.into_inner()).as_ref() {
        if *k == key {
            return j.clone();
        }
    }
    let j = timeline(repo);
    *LAST.lock().unwrap_or_else(|e| e.into_inner()) = Some((key, j.clone()));
    j
}

fn timeline(repo: &Repo) -> Json {
    let main = repo.default_branch();
    let wts = repo.worktrees();
    let here = std::fs::canonicalize(&repo.top).unwrap_or(repo.top.clone());
    let refs = repo.git(&["for-each-ref", "refs/heads", "--sort=-committerdate", "--format=%(refname:short)%09%(committerdate:unix)"]).unwrap_or_default();
    let mut branches = vec![];
    let mut seen = crate::fx::HashSet::default();
    // the worktrees' branches first, the viewer's own first of all
    let mut order: Vec<(Option<PathBuf>, String)> = vec![];
    for (p, b, head) in wts.iter().filter(|w| w.0 == here).chain(wts.iter().filter(|w| w.0 != here)) {
        order.push((Some(p.clone()), b.clone().unwrap_or_else(|| head.chars().take(7).collect())));
    }
    for l in refs.lines().take(40) {
        let b = l.split('\t').next().unwrap_or("").to_string();
        if !order.iter().any(|(_, x)| *x == b) {
            order.push((None, b));
        }
    }
    let base_of = |b: &str| repo.git(&["merge-base", &main, b]).ok().map(|s| s.trim().chars().take(12).collect::<String>());
    for (wt, b) in order {
        if !seen.insert(b.clone()) {
            continue;
        }
        let is_main = main.ends_with(&format!("/{b}")) || main == b;
        let range = if is_main { b.clone() } else { format!("{main}..{b}") };
        let cs = commits(repo, &range, if is_main { 12 } else { 30 });
        let unc = wt.as_deref().and_then(|w| uncommitted(repo, w));
        if cs.is_empty() && unc.is_none() && wt.is_none() {
            continue;
        }
        let tip = repo.git(&["rev-parse", "--short", &b]).unwrap_or_default().trim().to_string();
        branches.push(json!({
            "name": b, "worktree": wt, "here": wt.as_deref() == Some(here.as_path()), "main": is_main,
            "tip": tip, "base": if is_main { None } else { base_of(&b) }, "commits": cs, "uncommitted": unc,
        }));
    }
    // the default branch's latest commits, when no worktree has it checked out
    if !branches.iter().any(|b| b["main"] == true) {
        let short = main.trim_start_matches("origin/").to_string();
        let cs = commits(repo, &main, 12);
        branches.push(json!({"name": short, "worktree": null, "here": false, "main": true, "tip": "", "base": null, "commits": cs, "uncommitted": null}));
    }
    json!({"main": main, "top": repo.top, "rel": repo.rel, "branches": branches,
           "worktrees": wts.iter().map(|(p, b, _)| json!({"path": p, "branch": b})).collect::<Vec<_>>()})
}
