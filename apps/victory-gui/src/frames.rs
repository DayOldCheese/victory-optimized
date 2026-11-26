//! Frame rate and frame times from the layer's own frame log
//! (`VICTORY_FRAME_LOG_FILE`, written by `victory-gpu::framelog`). Nothing
//! is estimated: a figure is shown only if the log has it.

use std::io::{Read, Seek, SeekFrom};
use std::path::PathBuf;

/// One reading of the log: `frame times: N frames in S s: ...; each: a b c`.
#[derive(Debug, Clone, PartialEq)]
pub struct Reading {
    pub frames: u32,
    pub seconds: f64,
    pub median_ms: f64,
    pub longest_ms: f64,
    /// Every frame's time, in order.
    pub each_ms: Vec<f64>,
}

impl Reading {
    pub fn fps(&self) -> f64 {
        if self.seconds > 0.0 { f64::from(self.frames) / self.seconds } else { 0.0 }
    }
}

/// Parses one log line, if it is a reading.
pub fn parse(line: &str) -> Option<Reading> {
    let at = line.find("frame times: ")?;
    let rest = &line[at + "frame times: ".len()..];
    let frames: u32 = rest.split_whitespace().next()?.parse().ok()?;
    let seconds: f64 = rest.split(" frames in ").nth(1)?.split(" s").next()?.trim().parse().ok()?;
    let num_after = |key: &str| -> Option<f64> {
        let t = rest.split(key).nth(1)?;
        t.trim_start().split(|c: char| !(c.is_ascii_digit() || c == '.')).next()?.parse().ok()
    };
    let median_ms = num_after("median ")?;
    let longest_ms = num_after("longest ")?;
    let each_ms = rest
        .split("each: ")
        .nth(1)
        .map(|t| t.split_whitespace().filter_map(|x| x.parse().ok()).collect())
        .unwrap_or_default();
    Some(Reading { frames, seconds, median_ms, longest_ms, each_ms })
}

/// Follows a frame log as the layer appends to it.
pub struct Follower {
    pub path: PathBuf,
    at: u64,
    partial: String,
    pub readings: Vec<Reading>,
}

impl Follower {
    pub fn new(path: PathBuf) -> Follower {
        Follower { path, at: 0, partial: String::new(), readings: Vec::new() }
    }

    /// Reads what was appended since the last poll.
    pub fn poll(&mut self) {
        let Ok(mut f) = std::fs::File::open(&self.path) else { return };
        let len = f.metadata().map(|m| m.len()).unwrap_or(0);
        if len < self.at {
            // The file was started again.
            self.at = 0;
            self.readings.clear();
        }
        if f.seek(SeekFrom::Start(self.at)).is_err() {
            return;
        }
        let mut more = String::new();
        if f.read_to_string(&mut more).is_err() {
            return;
        }
        self.at += more.len() as u64;
        self.partial.push_str(&more);
        while let Some(nl) = self.partial.find('\n') {
            let line: String = self.partial.drain(..=nl).collect();
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

    /// The last `n` frame times across readings, oldest first.
    pub fn recent_frames(&self, n: usize) -> Vec<f64> {
        let all: Vec<f64> = self.readings.iter().flat_map(|r| r.each_ms.iter().copied()).collect();
        all[all.len().saturating_sub(n)..].to_vec()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_reading_is_parsed_as_the_layer_writes_it() {
        let line = "frame times: 4 frames in 0.5 s: shortest 98 ms, median 110, longest 140; 1 long (over 165 ms, the usual frame of the last 64 being 110); each: 98 110 112 140";
        let r = parse(line).unwrap();
        assert_eq!((r.frames, r.seconds, r.median_ms, r.longest_ms), (4, 0.5, 110.0, 140.0));
        assert_eq!(r.each_ms, [98.0, 110.0, 112.0, 140.0]);
        assert_eq!(r.fps(), 8.0);
        assert!(parse("usual frame (the mean of 3 not long): x").is_none());
    }

    #[test]
    fn the_follower_reads_appended_lines_only_once() {
        let path = std::env::temp_dir().join(format!("victory-gui-test-{}.txt", std::process::id()));
        std::fs::write(&path, "frame times: 2 frames in 1.0 s: shortest 1 ms, median 2, longest 3; 0 long (over 9 ms, the usual frame of the last 64 being 2); each: 1 3\npart").unwrap();
        let mut f = Follower::new(path.clone());
        f.poll();
        assert_eq!(f.readings.len(), 1);
        f.poll();
        assert_eq!(f.readings.len(), 1);
        std::fs::write(&path, "").unwrap();
        f.poll();
        assert!(f.readings.is_empty(), "a restarted file starts over");
        let _ = std::fs::remove_file(path);
    }
}
