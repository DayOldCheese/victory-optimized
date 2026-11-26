//! Frame rate and frame time from the layer's own summary line, which
//! `victory-gpu` logs every few seconds while the title draws:
//! `6.7 fps, 148.5 ms/frame (69.8 preparing: ...)`. It is read from the
//! title's output as the GUI captures it. Nothing is estimated: a figure is
//! shown only if the log has it.

use crate::jobs::Job;

/// One summary line.
#[derive(Debug, Clone, PartialEq)]
pub struct Reading {
    pub fps: f64,
    /// The mean time of a frame over the line's period.
    pub frame_ms: f64,
}

fn number(t: &str) -> Option<f64> {
    let t = t.trim_matches(|c: char| !(c.is_ascii_digit() || c == '.'));
    if t.is_empty() { None } else { t.parse().ok() }
}

/// Parses one log line, if it is a summary (colour codes around it allowed).
pub fn parse(line: &str) -> Option<Reading> {
    let at = line.find(" fps, ")?;
    let fps = number(line[..at].rsplit(char::is_whitespace).next()?)?;
    let rest = &line[at + " fps, ".len()..];
    let frame_ms = number(rest.split(" ms/frame").next().filter(|_| rest.contains(" ms/frame"))?)?;
    Some(Reading { fps, frame_ms })
}

/// Follows the title's output as it grows.
#[derive(Default)]
pub struct Follower {
    /// The place in the title's whole output read up to.
    at: usize,
    pub readings: Vec<Reading>,
}

impl Follower {
    /// Reads the lines the title printed since the last poll.
    pub fn poll(&mut self, title: &Job) {
        let (lines, end) = title.lines_from(self.at);
        self.at = end;
        for line in lines {
            if let Some(r) = parse(&line) {
                self.readings.push(r);
                if self.readings.len() > 600 {
                    self.readings.remove(0);
                }
            }
        }
    }

    pub fn last(&self) -> Option<&Reading> {
        self.readings.last()
    }

    /// The last `n` readings' frame times, oldest first.
    pub fn recent_frames(&self, n: usize) -> Vec<f64> {
        let all: Vec<f64> = self.readings.iter().map(|r| r.frame_ms).collect();
        all[all.len().saturating_sub(n)..].to_vec()
    }

    /// The summary lines read so far, as text (for the exported log).
    pub fn text(&self) -> String {
        self.readings.iter().map(|r| format!("{:.1} fps, {:.1} ms/frame\n", r.fps, r.frame_ms)).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_summary_is_parsed_as_the_layer_logs_it() {
        let line = "\u{1b}[2m2026-11-26T18:07:33.475801Z\u{1b}[0m \u{1b}[32m INFO\u{1b}[0m \u{1b}[2mvictory_gpu::host\u{1b}[0m\u{1b}[2m:\u{1b}[0m 6.7 fps, 148.5 ms/frame (69.8 preparing: copies 33.3 ms 78.4 MB)";
        assert_eq!(parse(line), Some(Reading { fps: 6.7, frame_ms: 148.5 }));
        assert_eq!(parse("2.8 fps, 358.1 ms/frame"), Some(Reading { fps: 2.8, frame_ms: 358.1 }));
        assert!(parse("dropped 456.3/frame without a host pipeline").is_none());
        assert!(parse("the low 1% at 12 fps, then").is_none());
    }
}
