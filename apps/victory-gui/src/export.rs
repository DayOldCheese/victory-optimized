//! The diagnostic log, made small enough to share. The layer logs about
//! 25 detail lines a frame and one a shader, so a few minutes of play are
//! megabytes of the same lines with other numbers. The compact form keeps
//! the first few lines of each kind in their place, and the last one of
//! each at the end; warnings, errors and the frame-rate summaries are all
//! kept. Colour codes are removed.

use std::collections::HashMap;

/// Lines of one kind kept where they fall.
const FIRST: usize = 5;

/// `line` without terminal colour codes.
pub fn plain(line: &str) -> String {
    let mut out = String::with_capacity(line.len());
    let mut chars = line.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\u{1b}' && chars.peek() == Some(&'[') {
            chars.next();
            for d in chars.by_ref() {
                if d.is_ascii_alphabetic() {
                    break;
                }
            }
        } else {
            out.push(c);
        }
    }
    out
}

/// What makes lines "of one kind": the line without its time stamp, with
/// every number (decimal or hex) as `N`, its first 100 characters.
fn kind(line: &str) -> String {
    let mut rest = line;
    if let Some((first, after)) = line.split_once(' ')
        && first.contains('T')
        && first.ends_with('Z')
    {
        rest = after.trim_start();
    }
    let mut out = String::new();
    let mut word = String::new();
    let flush = |word: &mut String, out: &mut String| {
        if word.chars().any(|c| c.is_ascii_digit()) {
            out.push('N');
        } else {
            out.push_str(word);
        }
        word.clear();
    };
    for c in rest.chars() {
        if c.is_ascii_hexdigit() || c == 'x' || c == '.' {
            word.push(c);
        } else {
            flush(&mut word, &mut out);
            out.push(c);
        }
        if out.len() >= 100 {
            break;
        }
    }
    flush(&mut word, &mut out);
    out.chars().take(100).collect()
}

/// Always kept, however often they come.
fn always(line: &str) -> bool {
    line.contains(" WARN ") || line.contains(" ERROR ") || line.contains("panicked") || line.contains(" fps, ") || line.starts_with("error")
}

/// The compact form of one job's output.
pub fn compact(lines: &[String]) -> String {
    let plain: Vec<String> = lines.iter().map(|l| plain(l)).collect();
    let mut seen: HashMap<String, usize> = HashMap::new();
    let kinds: Vec<String> = plain.iter().map(|l| kind(l)).collect();
    for k in &kinds {
        *seen.entry(k.clone()).or_default() += 1;
    }
    let mut at: HashMap<&str, usize> = HashMap::new();
    let mut last: HashMap<&str, usize> = HashMap::new();
    let mut out = String::new();
    for (i, (l, k)) in plain.iter().zip(&kinds).enumerate() {
        let n = at.entry(k).or_default();
        *n += 1;
        if *n <= FIRST || always(l) {
            out += l;
            out.push('\n');
        } else {
            last.insert(k, i);
        }
    }
    let mut tail: Vec<(usize, &str)> = last.iter().map(|(k, &i)| (i, *k)).collect();
    if !tail.is_empty() {
        tail.sort_unstable();
        let left = tail.iter().map(|(_, k)| seen[*k] - FIRST).sum::<usize>();
        out += &format!("\n---- {left} repeated lines left out; the last of each kind ({} kinds), as of the end ----\n", tail.len());
        for (i, k) in tail {
            out += &format!("[{}x] {}\n", seen[k], plain[i]);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `VICTORY_GUI_LOG=<an exported full log> cargo test -p victory-gui -- --ignored --nocapture compact_a_real_log`
    #[test]
    #[ignore]
    fn compact_a_real_log() {
        let text = std::fs::read_to_string(std::env::var("VICTORY_GUI_LOG").unwrap()).unwrap();
        let lines: Vec<String> = text.lines().map(str::to_owned).collect();
        let out = compact(&lines);
        println!("{} lines, {} KB -> {} lines, {} KB", lines.len(), text.len() >> 10, out.lines().count(), out.len() >> 10);
    }

    #[test]
    fn colour_codes_go() {
        assert_eq!(plain("\u{1b}[2mtime\u{1b}[0m \u{1b}[32m INFO\u{1b}[0m x"), "time  INFO x");
    }

    #[test]
    fn numbers_and_stamps_do_not_make_a_new_kind() {
        let a = kind("2026-11-26T21:25:05.341265Z  INFO host: 2.0 ms, hash=02384a60fda6f2d0");
        let b = kind("2026-11-26T21:25:11.338507Z  INFO host: 12.75 ms, hash=369cbe906ea72764");
        assert_eq!(a, b);
        assert_ne!(a, kind("2026-11-26T21:25:11.338507Z  INFO host: 12.75 fills"));
    }

    #[test]
    fn repeats_are_cut_but_warnings_summaries_and_the_last_stay() {
        let mut lines: Vec<String> = (0..50).map(|i| format!("INFO host: per frame {i} ms")).collect();
        lines.push(" WARN pipeline refused 1".into());
        lines.extend((0..20).map(|i| format!("{i}.0 fps, 3{i}.0 ms/frame")));
        let out = compact(&lines);
        assert!(out.contains("per frame 4 ms") && !out.contains("per frame 5 ms"));
        assert!(out.contains("[50x] INFO host: per frame 49 ms"));
        assert!(out.contains("WARN pipeline refused"));
        assert_eq!(out.matches(" fps, ").count(), 20);
        assert!(out.contains("45 repeated lines left out"));
    }
}
