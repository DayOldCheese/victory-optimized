//! Git, through the `git` command: branch, changes, history, and an audit
//! of what is about to be committed. Nothing is pushed without the user's
//! confirmation in the GUI.

use std::path::Path;

use crate::jobs::run_now;

pub fn branch(repo: &Path) -> String {
    run_now(repo, "git", &["rev-parse", "--abbrev-ref", "HEAD"]).map(|s| s.trim().to_owned()).unwrap_or_else(|e| format!("(git: {e})"))
}

pub fn head(repo: &Path) -> String {
    run_now(repo, "git", &["log", "-1", "--format=%h %s"]).map(|s| s.trim().to_owned()).unwrap_or_default()
}

/// Changed files as (status, path), from `git status --porcelain`.
pub fn changes(repo: &Path) -> Vec<(String, String)> {
    run_now(repo, "git", &["status", "--porcelain", "--untracked-files=all"])
        .unwrap_or_default()
        .lines()
        .filter(|l| l.len() > 3)
        .map(|l| (l[..2].trim().to_owned(), l[3..].trim_matches('"').to_owned()))
        .collect()
}

pub fn history(repo: &Path, n: usize) -> Vec<String> {
    run_now(repo, "git", &["log", &format!("-{n}"), "--format=%h %ad %an: %s", "--date=short"]).unwrap_or_default().lines().map(str::to_owned).collect()
}

/// Paths that must never be committed: run logs, dumps, the game's files,
/// the user's own config.
fn blocked_path(path: &str) -> Option<&'static str> {
    let p = path.replace('\\', "/").to_ascii_lowercase();
    let ext = p.rsplit('.').next().unwrap_or("");
    if p.starts_with("diagnostics/") {
        return Some("run logs (diagnostics/) are kept out of this fork");
    }
    if p == "victory.toml" || p.ends_with("/victory.toml") || p.ends_with(".env") {
        return Some("local configuration (paths on this machine)");
    }
    match ext {
        "log" | "dmp" | "mdmp" => Some("a log or crash dump"),
        "exe" | "dll" | "pdb" | "pak" | "xvd" | "msixvc" | "bin" if !p.starts_with("crates/victory-formats/src/bcpack-tables") => {
            Some("a binary: game files and build output may not be committed")
        }
        "png" | "jpg" | "jpeg" | "dds" | "bmp" => Some("an image (screenshots may show the game)"),
        _ => None,
    }
}

/// Content that looks like a secret or personal information.
fn suspicious(text: &str) -> Option<&'static str> {
    // Built from pieces, so this file does not hold the markers it looks for.
    let key = ["-----", "BEGIN "].concat();
    let users = ["\\", "Users", "\\"].concat();
    let markers: [(&str, &str); 5] = [
        (&key, "a private key"),
        ("ghp_", "a GitHub token"),
        ("github_pat_", "a GitHub token"),
        ("AKIA", "an AWS key"),
        (&users, "a Windows user folder path"),
    ];
    for (m, what) in markers {
        if let Some(at) = text.find(m) {
            // `AKIA` and `ghp_` only count followed by a run of key characters.
            let tail = &text[at + m.len()..];
            let run = tail.chars().take_while(|c| c.is_ascii_alphanumeric()).count();
            if matches!(m, "ghp_" | "github_pat_" | "AKIA") && run < 16 {
                continue;
            }
            return Some(what);
        }
    }
    // An email address that is not GitHub's no-reply form.
    for word in text.split(|c: char| c.is_whitespace() || "<>()\"',;".contains(c)) {
        if let Some((user, domain)) = word.split_once('@')
            && !user.is_empty()
            && domain.contains('.')
            && domain.chars().all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '-')
            && !domain.ends_with("users.noreply.github.com")
            && !domain.ends_with("anthropic.com")
            && !domain.starts_with("example.")
        {
            return Some("an email address");
        }
    }
    None
}

/// Problems with committing `files`; empty when they may go.
pub fn audit(repo: &Path, files: &[String]) -> Vec<String> {
    let mut problems = Vec::new();
    for f in files {
        if let Some(why) = blocked_path(f) {
            problems.push(format!("{f}: {why}"));
            continue;
        }
        let full = repo.join(f);
        let Ok(meta) = std::fs::metadata(&full) else { continue }; // deleted: nothing to leak
        if meta.len() > 5 << 20 {
            problems.push(format!("{f}: {} MB, over the 5 MB limit", meta.len() >> 20));
            continue;
        }
        if let Ok(text) = std::fs::read_to_string(&full)
            && let Some(what) = suspicious(&text)
        {
            problems.push(format!("{f}: contains what looks like {what}"));
        }
    }
    // The repository's own rule: nothing from the game (xtask policy).
    if let Err(e) = run_now(repo, "cargo", &["run", "-q", "--release", "-p", "xtask", "--", "policy"]) {
        let first = e.lines().filter(|l| l.contains("POLICY")).take(5).collect::<Vec<_>>().join("; ");
        problems.push(format!("xtask asset policy: {}", if first.is_empty() { e.lines().last().unwrap_or("failed").to_owned() } else { first }));
    }
    problems
}

/// Stages exactly `files` and commits them.
pub fn commit(repo: &Path, files: &[String], message: &str) -> Result<String, String> {
    if message.trim().is_empty() {
        return Err("a commit message is needed".into());
    }
    if files.is_empty() {
        return Err("no files selected".into());
    }
    let mut add = vec!["add", "--"];
    add.extend(files.iter().map(String::as_str));
    run_now(repo, "git", &add)?;
    run_now(repo, "git", &["commit", "-m", message])
}

/// Upstream commits this branch does not have, from a fetch of Belobug's
/// default branch (the foundation this fork is built on).
pub fn upstream_news(repo: &Path) -> Result<Vec<String>, String> {
    run_now(repo, "git", &["fetch", "-q", "https://github.com/Belobug/VIctory-Belobug-AMD.git", "HEAD"])?;
    let list = run_now(repo, "git", &["log", "--format=%h %ad %s", "--date=short", "HEAD..FETCH_HEAD"])?;
    Ok(list.lines().map(|l| l.chars().take(160).collect()).collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn logs_dumps_binaries_and_config_are_blocked() {
        for p in ["diagnostics/run-1.txt", "victory.toml", "x/crash.dmp", "game/Game.exe", "shot.png", "a.log"] {
            assert!(blocked_path(p).is_some(), "{p}");
        }
        for p in ["crates/victory-gpu/src/host.rs", "README.md", "crates/victory-formats/src/bcpack-tables.bin"] {
            assert!(blocked_path(p).is_none(), "{p}");
        }
    }

    #[test]
    fn secrets_and_personal_details_are_found() {
        assert!(suspicious(&["token ghp", "_abcdefghijklmnopqrstuvwx"].concat()).is_some());
        assert!(suspicious(&["path C:\\", "Users", "\\someone"].concat()).is_some());
        assert!(suspicious(&["mail me: someone", "@", "example-mail.com"].concat()).is_some());
        assert!(suspicious("Co-Authored-By: Claude <noreply@anthropic.com>").is_none());
        assert!(suspicious("Name <1+x@users.noreply.github.com>").is_none());
        assert!(suspicious("let ghp_ = 1; AKIA").is_none());
        assert!(suspicious("fn main() { a.b@c }").is_none());
    }
}
