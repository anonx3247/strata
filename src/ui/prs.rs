//! The PRs mode: pull requests through the local `gh` (the viewer never
//! holds a token): the list, a page per PR with its stack, and its diff from
//! base to head, structural for `.strata` files and by line for the rest;
//! and the actions: comment, approve, request changes, merge.
//!
//! A stack merges top-down: the top PR into its base (the PR below's
//! branch), then that one into its base, down to the default branch. A PR
//! that is behind its base is brought up to date first.

use super::activity::{diff_programs, Repo};
use crate::ast::Program;
use crate::fx::HashMap;
use serde_json::{json, Value as Json};
use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::Mutex;

/// `gh ARGS` in the repository (`STRATA_UI_GH` replaces `gh`, for tests).
fn gh(repo: &Repo, args: &[&str], input: Option<&str>) -> Result<String, String> {
    let bin = std::env::var("STRATA_UI_GH").unwrap_or_else(|_| "gh".to_string());
    let mut c = Command::new(&bin);
    c.args(args).current_dir(&repo.top).stdout(Stdio::piped()).stderr(Stdio::piped());
    c.stdin(if input.is_some() { Stdio::piped() } else { Stdio::null() });
    let mut child = c.spawn().map_err(|e| format!("cannot run `{bin}` (the GitHub CLI): {e}"))?;
    if let (Some(text), Some(mut w)) = (input, child.stdin.take()) {
        let _ = w.write_all(text.as_bytes());
    }
    let o = child.wait_with_output().map_err(|e| e.to_string())?;
    if !o.status.success() {
        let err = String::from_utf8_lossy(&o.stderr).trim().to_string();
        let out = String::from_utf8_lossy(&o.stdout).trim().to_string();
        return Err(if err.is_empty() { out } else { err });
    }
    Ok(String::from_utf8_lossy(&o.stdout).to_string())
}

fn gh_json(repo: &Repo, args: &[&str]) -> Result<Json, String> {
    let out = gh(repo, args, None)?;
    serde_json::from_str(&out).map_err(|e| format!("gh answered something that isn't JSON: {e}"))
}

const LIST_FIELDS: &str = "number,title,headRefName,baseRefName,state,isDraft,author,updatedAt,reviewDecision,url";
const VIEW_FIELDS: &str = "number,title,body,state,isDraft,url,author,createdAt,updatedAt,mergedAt,baseRefName,headRefName,headRefOid,baseRefOid,commits,comments,reviews,statusCheckRollup,files,additions,deletions,mergeable,mergeStateStatus,reviewDecision,labels";

/// `/api/prs?state=open|closed|merged|all`
pub fn list(repo: &Repo, state: &str) -> Json {
    let state = if ["open", "closed", "merged", "all"].contains(&state) { state } else { "open" };
    match gh_json(repo, &["pr", "list", "--state", state, "--limit", "80", "--json", LIST_FIELDS]) {
        Ok(j) => json!({"prs": j, "state": state}),
        Err(e) => json!({"error": e}),
    }
}

fn number(n: &str) -> Result<String, String> {
    n.parse::<u64>().map(|n| n.to_string()).map_err(|_| format!("`{n}` is not a pull request number"))
}

/// The open PRs: what stacks are made from.
fn open_prs(repo: &Repo) -> Vec<Json> {
    gh_json(repo, &["pr", "list", "--state", "open", "--limit", "200", "--json", "number,title,headRefName,baseRefName,state,isDraft"])
        .ok()
        .and_then(|j| j.as_array().cloned())
        .unwrap_or_default()
}

/// A PR's stack: the open PRs below it (each one's head is the base of the
/// one above), nearest first, and those stacked on it, nearest first.
fn stack(pr: &Json, open: &[Json]) -> (Vec<Json>, Vec<Json>) {
    let mut below = vec![];
    let mut base = pr["baseRefName"].as_str().unwrap_or("").to_string();
    while let Some(p) = open.iter().find(|p| p["headRefName"].as_str() == Some(&base) && !below.contains(*p) && p["number"] != pr["number"]) {
        below.push(p.clone());
        base = p["baseRefName"].as_str().unwrap_or("").to_string();
    }
    let mut above = vec![];
    let mut heads = vec![pr["headRefName"].as_str().unwrap_or("").to_string()];
    while let Some(h) = heads.pop() {
        for p in open.iter().filter(|p| p["baseRefName"].as_str() == Some(&h)) {
            if !above.contains(p) && p["number"] != pr["number"] {
                above.push(p.clone());
                heads.push(p["headRefName"].as_str().unwrap_or("").to_string());
            }
        }
    }
    (below, above)
}

/// `/api/prs/N`: everything GitHub shows about it.
pub fn view(repo: &Repo, n: &str) -> Json {
    let n = match number(n) {
        Ok(n) => n,
        Err(e) => return json!({"error": e}),
    };
    let mut pr = match gh_json(repo, &["pr", "view", &n, "--json", VIEW_FIELDS]) {
        Ok(j) => j,
        Err(e) => return json!({"error": e}),
    };
    let open = open_prs(repo);
    let (below, above) = stack(&pr, &open);
    pr["below"] = json!(below);
    pr["above"] = json!(above);
    let head = pr["headRefName"].as_str().unwrap_or("").to_string();
    pr["runs"] = gh_json(repo, &["run", "list", "--branch", &head, "--limit", "8", "--json", "databaseId,name,displayTitle,status,conclusion,url,createdAt,event,headSha"]).unwrap_or(json!([]));
    pr
}

/// Diffs are worked out once per head commit.
static DIFFS: Mutex<Option<HashMap<String, Json>>> = Mutex::new(None);

/// The `.strata` files under `rel` (a package's folder, or one file) at a revision.
fn sources_in(repo: &Repo, rel: &str, rev: &str, file: bool) -> Result<Program, String> {
    if file {
        return match crate::git::bytes(Some(&repo.top), &["show", &format!("{rev}:{rel}")]) {
            Ok(b) => crate::package::decode_file(Path::new(rel), &b),
            Err(_) => Ok(Program::default()),
        };
    }
    let mut files = vec![];
    let listed = repo.git(&["ls-tree", "-r", "--name-only", rev, "--", if rel.is_empty() { "." } else { rel }])?;
    for f in listed.lines().filter(|l| l.ends_with(".strata")) {
        let inside = if rel.is_empty() { f } else { f.strip_prefix(rel).map(|s| s.trim_start_matches('/')).unwrap_or(f) };
        files.push((std::path::PathBuf::from(inside), crate::git::bytes(Some(&repo.top), &["show", &format!("{rev}:{f}")])?));
    }
    crate::package::decode(files)
}

/// The package a changed `.strata` file belongs to at a revision: its folder
/// (the nearest with a `strata.toml`), or None for a script.
fn package_of(repo: &Repo, file: &str, revs: &[&str]) -> Option<String> {
    let mut dir = Path::new(file).parent();
    while let Some(d) = dir {
        let toml = if d.as_os_str().is_empty() { "strata.toml".to_string() } else { format!("{}/strata.toml", d.display()) };
        if revs.iter().any(|r| crate::git::ok(Some(&repo.top), &["cat-file", "-e", &format!("{r}:{toml}")])) {
            return Some(d.display().to_string());
        }
        dir = d.parent();
    }
    None
}

/// `/api/prs/N/diff`: from where the PR branched off its base to its head.
pub fn diff(repo: &Repo, n: &str) -> Json {
    let n = match number(n) {
        Ok(n) => n,
        Err(e) => return json!({"error": e}),
    };
    let pr = match gh_json(repo, &["pr", "view", &n, "--json", "baseRefName,headRefName,headRefOid,baseRefOid"]) {
        Ok(j) => j,
        Err(e) => return json!({"error": e}),
    };
    let (base, head_oid) = (pr["baseRefName"].as_str().unwrap_or(""), pr["headRefOid"].as_str().unwrap_or(""));
    if base.starts_with('-') || head_oid.is_empty() {
        return json!({"error": "gh gave no base or head for this PR"});
    }
    let key = format!("{n}:{head_oid}:{base}");
    if let Some(j) = DIFFS.lock().unwrap_or_else(|e| e.into_inner()).get_or_insert_with(HashMap::default).get(&key) {
        return j.clone();
    }
    // the head by its pull ref (it may come from a deleted branch), the base by name
    let _ = repo.git(&["fetch", "-q", "origin", &format!("pull/{n}/head"), base]);
    let base_ref = if crate::git::ok(Some(&repo.top), &["rev-parse", "--verify", "--quiet", &format!("origin/{base}")]) { format!("origin/{base}") } else { pr["baseRefOid"].as_str().unwrap_or("").to_string() };
    if !crate::git::ok(Some(&repo.top), &["cat-file", "-e", &format!("{head_oid}^{{commit}}")]) {
        return json!({"error": format!("the PR's head ({}) isn't in this clone, and fetching it failed", &head_oid[..head_oid.len().min(8)])});
    }
    let mb = match repo.git(&["merge-base", &base_ref, head_oid]) {
        Ok(s) => s.trim().to_string(),
        Err(e) => return json!({"error": e}),
    };
    let names = repo.git(&["diff", "--name-only", &mb, head_oid]).unwrap_or_default();
    // .strata files: structurally, one diff per package (or script) they're in
    let mut groups: Vec<(String, bool)> = vec![];
    for f in names.lines().filter(|f| f.ends_with(".strata")) {
        let g = match package_of(repo, f, &[head_oid, &mb]) {
            Some(d) => (d, false),
            None => (f.to_string(), true),
        };
        if !groups.contains(&g) {
            groups.push(g);
        }
    }
    let structural: Vec<Json> = groups
        .iter()
        .map(|(rel, file)| {
            let read = |rev: &str| sources_in(repo, rel, rev, *file);
            match (read(&mb), read(head_oid)) {
                (Ok(a), Ok(b)) => json!({"path": rel, "diff": diff_programs(&a, &b)}),
                (Err(e), _) => json!({"path": rel, "error": format!("the base can't be read (older format): {e}")}),
                (_, Err(e)) => json!({"path": rel, "error": format!("the head can't be read (older format): {e}")}),
            }
        })
        .collect();
    // every other file: by line
    let patch = repo.git(&["diff", "--no-color", "-M", &mb, head_oid, "--", ".", ":(exclude)*.strata"]).unwrap_or_default();
    let mut text: Vec<Json> = vec![];
    let mut total = 0;
    for chunk in patch.split("\ndiff --git ").filter(|c| !c.trim().is_empty()) {
        let chunk = chunk.strip_prefix("diff --git ").unwrap_or(chunk);
        let file = chunk.lines().next().and_then(|l| l.rsplit_once(" b/")).map(|(_, b)| b.to_string()).unwrap_or_default();
        total += chunk.len();
        let body = if total > 600_000 { "(too large to show here)".to_string() } else { chunk.lines().skip_while(|l| !l.starts_with("@@")).collect::<Vec<_>>().join("\n") };
        let binary = chunk.contains("\nBinary files ");
        text.push(json!({"file": file, "patch": body, "binary": binary}));
    }
    let j = json!({"base": base, "merge_base": &mb[..mb.len().min(12)], "head": &head_oid[..head_oid.len().min(12)], "structural": structural, "text": text, "files": names.lines().collect::<Vec<_>>()});
    DIFFS.lock().unwrap_or_else(|e| e.into_inner()).get_or_insert_with(HashMap::default).insert(key, j.clone());
    j
}

/// `POST /api/prs/N/comment {body}`
pub fn comment(repo: &Repo, n: &str, body: &str) -> Json {
    let n = match number(n) {
        Ok(n) => n,
        Err(e) => return json!({"error": e}),
    };
    if body.trim().is_empty() {
        return json!({"error": "the comment is empty"});
    }
    match gh(repo, &["pr", "comment", &n, "--body-file", "-"], Some(body)) {
        Ok(out) => json!({"ok": true, "out": out.trim()}),
        Err(e) => json!({"error": e}),
    }
}

/// `POST /api/prs/N/review {event: approve | request_changes | comment, body}`
pub fn review(repo: &Repo, n: &str, event: &str, body: &str) -> Json {
    let n = match number(n) {
        Ok(n) => n,
        Err(e) => return json!({"error": e}),
    };
    let flag = match event {
        "approve" => "--approve",
        "request_changes" => "--request-changes",
        "comment" => "--comment",
        _ => return json!({"error": format!("no review action `{event}`")}),
    };
    if event != "approve" && body.trim().is_empty() {
        return json!({"error": "say what should change"});
    }
    match gh(repo, &["pr", "review", &n, flag, "--body-file", "-"], Some(body)) {
        Ok(out) => json!({"ok": true, "out": out.trim()}),
        Err(e) => json!({"error": e}),
    }
}

fn repo_name(repo: &Repo) -> Result<String, String> {
    gh(repo, &["repo", "view", "--json", "nameWithOwner", "-q", ".nameWithOwner"], None).map(|s| s.trim().to_string())
}

/// Merge one PR: bring it up to date if it's behind, then `gh pr merge`; if
/// gh refuses (a stack), the merge API; if that is refused too, auto-merge,
/// which merges once CI passes. Each thing tried is a step in `log`.
/// Whether it was merged now.
fn merge_one(repo: &Repo, n: &str, log: &mut Vec<Json>) -> Result<bool, String> {
    let st = gh_json(repo, &["pr", "view", n, "--json", "state,mergeStateStatus,baseRefName,headRefName"])?;
    if st["state"] == "MERGED" {
        log.push(json!({"pr": n, "step": "already merged", "ok": true}));
        return Ok(true);
    }
    if st["mergeStateStatus"] == "BEHIND" {
        let r = gh(repo, &["pr", "update-branch", n], None);
        log.push(json!({"pr": n, "step": format!("update-branch (behind {})", st["baseRefName"].as_str().unwrap_or("its base")), "ok": r.is_ok(), "out": r.clone().unwrap_or_else(|e| e)}));
    }
    let r = gh(repo, &["pr", "merge", n, "--merge"], None);
    log.push(json!({"pr": n, "step": format!("merge into {}", st["baseRefName"].as_str().unwrap_or("its base")), "ok": r.is_ok(), "out": r.clone().unwrap_or_else(|e| e)}));
    if r.is_ok() {
        return Ok(true);
    }
    let name = repo_name(repo)?;
    let r = gh(repo, &["api", "-X", "PUT", &format!("repos/{name}/pulls/{n}/merge-async"), "-f", "merge_method=merge"], None);
    log.push(json!({"pr": n, "step": "merge through the API (merge-async)", "ok": r.is_ok(), "out": r.clone().unwrap_or_else(|e| e)}));
    if r.is_ok() {
        // it merges in the background: wait for it, so the next PR down has it
        for _ in 0..30 {
            if gh_json(repo, &["pr", "view", n, "--json", "state"]).is_ok_and(|j| j["state"] == "MERGED") {
                return Ok(true);
            }
            std::thread::sleep(std::time::Duration::from_secs(2));
        }
        log.push(json!({"pr": n, "step": "still merging after a minute", "ok": false}));
        return Ok(false);
    }
    let r = gh(repo, &["pr", "merge", n, "--merge", "--auto"], None);
    log.push(json!({"pr": n, "step": "auto-merge: merges once CI passes", "ok": r.is_ok(), "out": r.clone().unwrap_or_else(|e| e)}));
    Ok(false)
}

/// `POST /api/prs/N/merge {stack}`: the PR, or (`stack`) the PR and every
/// open PR below it, top-down. Stops at the first PR that isn't merged now
/// (it may merge later, by auto-merge): the next one down needs it.
pub fn merge(repo: &Repo, n: &str, whole_stack: bool) -> Json {
    let n = match number(n) {
        Ok(n) => n,
        Err(e) => return json!({"error": e}),
    };
    let mut order = vec![n.clone()];
    if whole_stack {
        let pr = match gh_json(repo, &["pr", "view", &n, "--json", "number,baseRefName,headRefName"]) {
            Ok(j) => j,
            Err(e) => return json!({"error": e}),
        };
        let (below, _) = stack(&pr, &open_prs(repo));
        order.extend(below.iter().filter_map(|p| p["number"].as_u64()).map(|x| x.to_string()));
    }
    let mut log = vec![];
    let mut merged = vec![];
    for p in &order {
        match merge_one(repo, p, &mut log) {
            Ok(true) => merged.push(p.clone()),
            Ok(false) => break,
            Err(e) => {
                log.push(json!({"pr": p, "step": "read its state", "ok": false, "out": e}));
                break;
            }
        }
    }
    let done = merged.len() == order.len();
    json!({"ok": done, "order": order, "merged": merged, "log": log})
}
