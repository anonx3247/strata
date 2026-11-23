//! Colours for what the tools say (`check`, `verify`, `test`, `edit`, …) when
//! it goes to a terminal, by the rules the site's tour colours recorded output
//! with (site/common.js, `termLine`): proven lines green, failures red, fuzzed,
//! incomplete and assumed ones orange, a counterexample's input amber, hashes
//! dim, holes yellow.
//!
//! The commands print with `println!` in many places, so the colour is put on
//! where the output leaves: `start` points stdout and stderr (each one that is
//! a terminal) at a pipe, and a thread colours each line on its way to the
//! terminal. `finish` (also run at exit) lets the threads write what is left.
//! `NO_COLOR` turns it off; `CLICOLOR_FORCE` turns it on when the output isn't
//! a terminal. A program's own output (`run`, `serve`) and code (`render`,
//! `view`) are left as they are.

const OK: &str = "\x1b[38;2;95;208;122m";
const BAD: &str = "\x1b[38;2;255;107;107m";
const WARN: &str = "\x1b[38;2;255;176;92m";
const INPUT: &str = "\x1b[38;2;240;180;92m";
const ACCENT: &str = "\x1b[38;2;142;162;255m";
const DIM: &str = "\x1b[38;2;111;112;121m";
const HOLE: &str = "\x1b[1;38;2;245;213;101m";
const BOLD: &str = "\x1b[1m";
const RESET: &str = "\x1b[0m";

/// The commands whose output is the tools' own words, and so is coloured.
pub fn colours(cmd: &str) -> bool {
    matches!(
        cmd,
        "check" | "verify" | "test" | "holes" | "add" | "edit" | "rename" | "move" | "done" | "abort" | "diff" | "log" | "sync" | "upgrade" | "build" | "init" | "setup-git" | "resolve"
    )
}

/// One line of output, coloured by what it says.
pub fn line(l: &str) -> String {
    let t = l.trim_start();
    let whole = |c: &str| format!("{c}{l}{RESET}");
    // `strata diff`: section headings, added and removed lines, folded ones, words changed in a line
    if l.len() > 2 && l.chars().all(|c| c.is_ascii_uppercase() || c == ' ') {
        return format!("{BOLD}{INPUT}{l}{RESET}");
    }
    if l.starts_with("  + ") {
        return whole(OK);
    }
    if l.starts_with("  - ") {
        return whole(BAD);
    }
    if l.starts_with("  ~ ") || l.contains("[-") || l.contains("{+") {
        return words(l);
    }
    if l.starts_with(char::is_whitespace) && t.starts_with("… ") {
        return whole(DIM);
    }
    if t.starts_with('✓') {
        return whole(OK);
    }
    if t.starts_with('✗') || l.starts_with("error") || l.starts_with("rejected") || l.starts_with("fault") {
        return whole(BAD);
    }
    if (t.starts_with('~') || t.starts_with('○') || t.starts_with('≈')) && t.chars().nth(1).is_some_and(char::is_whitespace) {
        return whole(WARN);
    }
    if l.starts_with(char::is_whitespace) && t.starts_with("input:") {
        return whole(INPUT);
    }
    if t.starts_with("re-check dependents") {
        return whole(WARN);
    }
    if ["committed to git", "Auto-merging", "Automatic merge"].iter().any(|p| l.starts_with(p)) {
        return whole(DIM);
    }
    if l.starts_with("strata: ") && l.contains("applied") {
        return whole(ACCENT);
    }
    let definition = ["fn ", "type ", "test ", "trait ", "impl "].iter().any(|k| l.starts_with(k));
    let mut out = holes_and_hashes(l);
    if definition {
        for w in ["added", "changed"] {
            if let Some(i) = word_at(&out, w) {
                out = format!("{}{BOLD}{w}{RESET}{}", &out[..i], &out[i + w.len()..]);
                break;
            }
        }
        out = out.replace("(interface changed)", &format!("({WARN}interface changed{RESET})"));
    }
    out
}

/// A diff's changed line: `~` orange, `[-removed-]` red, `{+added+}` green, `◀ …` orange.
fn words(l: &str) -> String {
    let mut out = String::new();
    let mut rest = l;
    if let Some(r) = rest.strip_prefix("  ~ ") {
        out += &format!("  {WARN}~{RESET} ");
        rest = r;
    }
    while !rest.is_empty() {
        let next = [("[-", "-]", BAD), ("{+", "+}", OK), ("◀ ", "", WARN)].into_iter().filter_map(|(a, z, c)| rest.find(a).map(|i| (i, a, z, c))).min_by_key(|x| x.0);
        let Some((i, _, z, c)) = next else {
            out += rest;
            break;
        };
        out += &rest[..i];
        let end = if z.is_empty() { rest.len() } else { rest[i..].find(z).map_or(rest.len(), |j| i + j + z.len()) };
        out += &format!("{c}{}{RESET}", &rest[i..end]);
        rest = &rest[end..];
    }
    out
}

/// `w` as a word of `s`: its byte offset.
fn word_at(s: &str, w: &str) -> Option<usize> {
    let word = |c: Option<char>| c.is_some_and(|c| c.is_alphanumeric() || c == '_');
    s.match_indices(w).map(|(i, _)| i).find(|&i| !word(s[..i].chars().next_back()) && !word(s[i + w.len()..].chars().next()))
}

/// Holes (`?name`) in yellow, hashes (`#` and 8 or more hex digits, maybe `…`) dim.
fn holes_and_hashes(l: &str) -> String {
    let b = l.as_bytes();
    let (mut out, mut i, mut from) = (String::with_capacity(l.len()), 0, 0);
    let name = |c: u8| c.is_ascii_lowercase() || c.is_ascii_digit() || c == b'_';
    let hex = |c: u8| c.is_ascii_digit() || (b'a'..=b'f').contains(&c);
    while i < b.len() {
        let before_ok = i == 0 || !(b[i - 1].is_ascii_alphanumeric() || b[i - 1] == b'_');
        if b[i] == b'?' && i + 1 < b.len() && (b[i + 1].is_ascii_lowercase() || b[i + 1] == b'_') && before_ok {
            let mut j = i + 1;
            while j < b.len() && name(b[j]) {
                j += 1;
            }
            out += &l[from..i];
            out += &format!("{HOLE}{}{RESET}", &l[i..j]);
            (i, from) = (j, j);
            continue;
        }
        if b[i] == b'#' && before_ok {
            let mut j = i + 1;
            while j < b.len() && hex(b[j]) {
                j += 1;
            }
            if j - i > 8 {
                if l[j..].starts_with('…') {
                    j += '…'.len_utf8();
                }
                out += &l[from..i];
                out += &format!("{DIM}{}{RESET}", &l[i..j]);
                (i, from) = (j, j);
                continue;
            }
        }
        i += 1;
    }
    out + &l[from..]
}

#[cfg(unix)]
mod pipe {
    use std::io::{Read, Write};
    use std::os::fd::FromRawFd;
    use std::sync::Mutex;

    unsafe extern "C" {
        fn pipe(fds: *mut i32) -> i32;
        fn dup(fd: i32) -> i32;
        fn dup2(from: i32, to: i32) -> i32;
        fn close(fd: i32) -> i32;
        fn isatty(fd: i32) -> i32;
        fn atexit(f: extern "C" fn()) -> i32;
    }

    /// Each redirected stream: its fd, the terminal it was, and the thread colouring it.
    static STREAMS: Mutex<Vec<(i32, i32, std::thread::JoinHandle<()>)>> = Mutex::new(Vec::new());

    pub fn start() {
        let off = std::env::var_os("NO_COLOR").is_some_and(|v| !v.is_empty());
        let force = std::env::var_os("CLICOLOR_FORCE").is_some_and(|v| !v.is_empty() && v != "0");
        if off {
            return;
        }
        let mut streams = STREAMS.lock().unwrap();
        for fd in [1, 2] {
            if !force && unsafe { isatty(fd) } != 1 {
                continue;
            }
            let mut p = [0i32; 2];
            if unsafe { pipe(p.as_mut_ptr()) } != 0 {
                continue;
            }
            let term = unsafe { dup(fd) };
            unsafe {
                dup2(p[1], fd);
                close(p[1]);
            }
            let (read, write) = unsafe { (std::fs::File::from_raw_fd(p[0]), std::fs::File::from_raw_fd(term)) };
            let thread = std::thread::spawn(move || colour(read, write));
            streams.push((fd, term, thread));
        }
        if !streams.is_empty() {
            unsafe { atexit(at_exit) };
        }
    }

    extern "C" fn at_exit() {
        finish();
    }

    /// Lets the threads write what is left, and gives the streams back their terminal.
    pub fn finish() {
        let _ = std::io::stdout().flush();
        let _ = std::io::stderr().flush();
        let streams = std::mem::take(&mut *STREAMS.lock().unwrap_or_else(|e| e.into_inner()));
        for (fd, term, thread) in streams {
            // the pipe's last writer goes: the thread reads to its end
            unsafe { dup2(term, fd) };
            let _ = thread.join();
        }
    }

    /// Whole lines coloured; a line not finished yet (a prompt) written as it comes, uncoloured.
    fn colour(mut read: std::fs::File, mut write: std::fs::File) {
        let (mut buf, mut chunk, mut raw_line) = (Vec::new(), [0u8; 8192], false);
        loop {
            let n = match read.read(&mut chunk) {
                Ok(0) | Err(_) => break,
                Ok(n) => n,
            };
            buf.extend_from_slice(&chunk[..n]);
            let mut out = Vec::new();
            while let Some(i) = buf.iter().position(|&c| c == b'\n') {
                let l: Vec<u8> = buf.drain(..=i).collect();
                if raw_line {
                    out.extend_from_slice(&l);
                    raw_line = false;
                } else {
                    out.extend_from_slice(super::line(&String::from_utf8_lossy(&l[..l.len() - 1])).as_bytes());
                    out.push(b'\n');
                }
            }
            if !buf.is_empty() {
                out.append(&mut buf);
                raw_line = true;
            }
            if write.write_all(&out).and_then(|_| write.flush()).is_err() {
                break;
            }
        }
        let _ = write.write_all(&buf);
    }
}

/// Colours stdout and stderr from here on, where each is a terminal.
pub fn start() {
    #[cfg(unix)]
    pipe::start();
}

/// Writes what is left; the output is the terminal's again.
pub fn finish() {
    #[cfg(unix)]
    pipe::finish();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn colour_lines_as_the_tour_does() {
        assert_eq!(line("✓ gcd                      proven (5 checks)"), format!("{OK}✓ gcd                      proven (5 checks){RESET}"));
        assert!(line("✗ average                  Runtime violation (found by the solver)").starts_with(BAD));
        assert!(line("error in status: non-exhaustive match").starts_with(BAD));
        assert!(line("~ isqrt                    fuzzed: 200 cases").starts_with(WARN));
        assert!(line("○ refund                   incomplete").starts_with(WARN));
        assert!(line("    input:   [9]").starts_with(INPUT));
        assert_eq!(line("4 passed, 0 failed"), "4 passed, 0 failed");
        assert_eq!(line("~/shop"), "~/shop");
        let edit = line("fn     discount                 changed  #fc40aad7… → #2e54e58d…  (interface changed)");
        assert!(edit.contains(&format!("{BOLD}changed{RESET}")) && edit.contains(&format!("{DIM}#fc40aad7…{RESET}")) && edit.contains(&format!("{WARN}interface changed")));
        assert_eq!(line("?refund_policy  in refund"), format!("{HOLE}?refund_policy{RESET}  in refund"));
        assert_eq!(line("a ? b"), "a ? b");
        assert_eq!(line("issue #12"), "issue #12");
        assert!(line("  + OutOfStock").starts_with(OK) && line("  - old").starts_with(BAD));
        assert_eq!(line("TYPES"), format!("{BOLD}{INPUT}TYPES{RESET}"));
        assert_eq!(line("  ~   | [-Shipped-]{+Sent+} {"), format!("  {WARN}~{RESET}   | {BAD}[-Shipped-]{RESET}{OK}{{+Sent+}}{RESET} {{"));
    }
}
