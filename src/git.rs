//! Running git.

use std::path::Path;
use std::process::Command;

fn command(dir: Option<&Path>, args: &[&str]) -> Command {
    let mut c = Command::new("git");
    if let Some(d) = dir {
        c.arg("-C").arg(d);
    }
    c.args(args);
    c
}

/// `git ARGS`, in `dir` (or the current folder): what it printed, or why it failed.
pub fn run(dir: Option<&Path>, args: &[&str]) -> Result<String, String> {
    let o = command(dir, args).output().map_err(|e| format!("git: {e}"))?;
    if !o.status.success() {
        // the subcommand only: the arguments may be long (a commit message)
        let sub = args.iter().find(|a| !a.starts_with('-')).unwrap_or(&"");
        return Err(format!("`git {sub}` failed: {}", String::from_utf8_lossy(&o.stderr).trim()));
    }
    Ok(String::from_utf8_lossy(&o.stdout).to_string())
}

/// Whether `git ARGS` succeeds (`merge-base --is-ancestor`, `diff --quiet`, ...).
pub fn ok(dir: Option<&Path>, args: &[&str]) -> bool {
    command(dir, args).output().is_ok_and(|o| o.status.success())
}
