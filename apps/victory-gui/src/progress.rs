//! How far a long task has got, read from the lines it prints:
//! `victory warm` ("1650 of 3145, 240 s", after "... 3145 to make, 8
//! processes") and `victory shaders` ("12250 shaders in the cache, 95 s").
//! Only what the task printed is shown: nothing is estimated but the time
//! left, from its own rate so far.

#[derive(Debug, Clone, PartialEq)]
pub struct Progress {
    pub done: u64,
    /// Known for pipelines (the task says how many it will make).
    pub total: Option<u64>,
    /// What is counted ("pipelines", "shaders").
    pub what: &'static str,
}

impl Progress {
    pub fn fraction(&self) -> Option<f32> {
        self.total.filter(|&t| t > 0).map(|t| (self.done as f32 / t as f32).clamp(0.0, 1.0))
    }

    /// Time left at the rate so far, once a little is done.
    pub fn left_s(&self, elapsed_s: f64) -> Option<f64> {
        let total = self.total?;
        if self.done == 0 || self.done >= total || elapsed_s < 5.0 {
            return None;
        }
        Some(elapsed_s / self.done as f64 * (total - self.done) as f64)
    }
}

fn num(t: &str) -> Option<u64> {
    t.trim().parse().ok()
}

/// One line's progress, if it reports some.
fn parse(line: &str) -> Option<Progress> {
    let line = line.trim();
    // warm: "{done} of {total}, {s} s"
    if let Some((done, rest)) = line.split_once(" of ")
        && let Some((total, tail)) = rest.split_once(", ")
        && tail.ends_with(" s")
        && let (Some(done), Some(total)) = (num(done), num(total))
    {
        return Some(Progress { done, total: Some(total), what: "pipelines" });
    }
    // warm, at its start: "...; {n} to make, {j} processes"
    if let Some(at) = line.find(" to make, ")
        && line.ends_with(" processes")
        && let Some(n) = line[..at].rsplit(' ').next().and_then(num)
    {
        return Some(Progress { done: 0, total: Some(n), what: "pipelines" });
    }
    // shaders: "{n} shaders in the cache, {s} s"
    if let Some((n, _)) = line.split_once(" shaders in the cache, ")
        && let Some(n) = num(n)
    {
        return Some(Progress { done: n, total: None, what: "shaders" });
    }
    None
}

/// The latest progress among a task's lines (looked for from the end).
pub fn latest(lines: &[String]) -> Option<Progress> {
    lines.iter().rev().take(400).find_map(|l| parse(l))
}

/// "4 min 05 s" and the like.
pub fn duration(s: f64) -> String {
    let s = s.max(0.0).round() as u64;
    match s {
        0..60 => format!("{s} s"),
        60..3600 => format!("{} min {:02} s", s / 60, s % 60),
        _ => format!("{} h {:02} min", s / 3600, s % 3600 / 60),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lines(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn warm_progress_is_read_from_its_lines() {
        let l = lines(&["> victory.exe warm --used", "84000 pipelines known, 1200 made before (abc), 0 bad; 3145 to make, 8 processes"]);
        assert_eq!(latest(&l), Some(Progress { done: 0, total: Some(3145), what: "pipelines" }));
        let l = lines(&["3145 to make, 8 processes", "1650 of 3145, 240 s", "pipeline 12 abc: refused"]);
        let p = latest(&l).unwrap();
        assert_eq!((p.done, p.total), (1650, Some(3145)));
        assert!((p.fraction().unwrap() - 0.5246).abs() < 0.001);
        assert!((p.left_s(240.0).unwrap() - 217.4).abs() < 0.5);
    }

    #[test]
    fn shader_progress_has_a_count_and_no_total() {
        let p = latest(&lines(&["12250 shaders in the cache, 95 s"])).unwrap();
        assert_eq!((p.done, p.total, p.what), (12250, None, "shaders"));
        assert_eq!(p.fraction(), None);
        assert_eq!(p.left_s(95.0), None);
    }

    #[test]
    fn other_lines_are_not_progress() {
        assert_eq!(latest(&lines(&["one of these, then s", "2.8 fps, 352.0 ms/frame", "[exit code 0 (0x00000000)]"])), None);
    }

    #[test]
    fn durations_read_as_people_say_them() {
        assert_eq!(duration(42.0), "42 s");
        assert_eq!(duration(245.0), "4 min 05 s");
        assert_eq!(duration(4000.0), "1 h 06 min");
    }
}
