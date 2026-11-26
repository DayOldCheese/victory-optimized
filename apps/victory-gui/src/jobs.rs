//! Child processes (cargo, git, victory.exe) run on their own; their output
//! comes to the GUI a line at a time, so the window stays responsive.

use std::io::{BufRead, BufReader, Read};
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};

/// How a job ended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum State {
    Running,
    /// Exit code 0.
    Succeeded,
    /// Another exit code, or none (killed).
    Failed(Option<i32>),
    /// Could not be started.
    NotStarted(String),
}

/// A running or finished child process and what it printed.
pub struct Job {
    pub name: String,
    output: Arc<Mutex<Lines>>,
    state: Arc<Mutex<State>>,
    child: Arc<Mutex<Option<Child>>>,
}

/// Lines kept per job: older ones are dropped.
const KEEP: usize = 20_000;

/// A job's output: the lines kept, and how many older ones were dropped
/// (so a reader can go on from a line's place in the whole output).
#[derive(Default)]
struct Lines {
    kept: Vec<String>,
    dropped: usize,
}

fn push(out: &Mutex<Lines>, line: String) {
    let mut o = out.lock().unwrap_or_else(|e| e.into_inner());
    o.kept.push(line);
    if o.kept.len() > KEEP {
        let drop = o.kept.len() - KEEP;
        o.kept.drain(..drop);
        o.dropped += drop;
    }
}

fn pump(out: Arc<Mutex<Lines>>, from: impl Read + Send + 'static) {
    std::thread::spawn(move || {
        for line in BufReader::new(from).lines() {
            match line {
                Ok(l) => push(&out, l),
                Err(_) => break,
            }
        }
    });
}

impl Job {
    /// Starts `program args...` in `dir` with `env` added to ours.
    pub fn start(name: &str, dir: &Path, program: &str, args: &[String], env: &[(String, String)]) -> Job {
        let output = Arc::new(Mutex::new(Lines { kept: vec![format!("> {program} {}", args.join(" "))], dropped: 0 }));
        let state = Arc::new(Mutex::new(State::Running));
        let child = Arc::new(Mutex::new(None));
        let mut cmd = Command::new(program);
        cmd.args(args).current_dir(dir).stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped());
        for (k, v) in env {
            cmd.env(k, v);
        }
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            // No console window of its own for the child.
            const CREATE_NO_WINDOW: u32 = 0x0800_0000;
            cmd.creation_flags(CREATE_NO_WINDOW);
        }
        match cmd.spawn() {
            Ok(mut c) => {
                if let Some(o) = c.stdout.take() {
                    pump(output.clone(), o);
                }
                if let Some(e) = c.stderr.take() {
                    pump(output.clone(), e);
                }
                *child.lock().unwrap() = Some(c);
                // Waits for the end without holding the lock all along, so
                // `stop` can kill the child meanwhile.
                let (st, ch, out) = (state.clone(), child.clone(), output.clone());
                std::thread::spawn(move || loop {
                    let done = {
                        let mut g = ch.lock().unwrap_or_else(|e| e.into_inner());
                        match g.as_mut().map(|c| c.try_wait()) {
                            Some(Ok(Some(status))) => Some(status.code()),
                            Some(Ok(None)) => None,
                            _ => Some(None),
                        }
                    };
                    if let Some(code) = done {
                        // Let the readers finish the last lines.
                        std::thread::sleep(std::time::Duration::from_millis(150));
                        push(&out, format!("[exit code {}]", code.map_or("none (stopped)".into(), |c| format!("{c} (0x{:08X})", c as u32))));
                        *st.lock().unwrap() = if code == Some(0) { State::Succeeded } else { State::Failed(code) };
                        break;
                    }
                    std::thread::sleep(std::time::Duration::from_millis(100));
                });
            }
            Err(e) => {
                push(&output, format!("could not start {program}: {e}"));
                *state.lock().unwrap() = State::NotStarted(e.to_string());
            }
        }
        Job { name: name.to_owned(), output, state, child }
    }

    pub fn state(&self) -> State {
        self.state.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }

    pub fn running(&self) -> bool {
        self.state() == State::Running
    }

    /// Ends the child at once (Windows has no gentle signal for a process
    /// without a window of its own).
    pub fn stop(&self) {
        if let Some(c) = self.child.lock().unwrap_or_else(|e| e.into_inner()).as_mut() {
            let _ = c.kill();
        }
    }

    pub fn lines(&self) -> Vec<String> {
        self.output.lock().unwrap_or_else(|e| e.into_inner()).kept.clone()
    }

    /// The lines from place `from` in the whole output on (those dropped
    /// meanwhile are skipped), and the place after the last.
    pub fn lines_from(&self, from: usize) -> (Vec<String>, usize) {
        let o = self.output.lock().unwrap_or_else(|e| e.into_inner());
        let start = from.max(o.dropped) - o.dropped;
        let end = o.dropped + o.kept.len();
        (o.kept.get(start..).map(<[String]>::to_vec).unwrap_or_default(), end)
    }
}

/// Runs a short command to the end and returns its output (for git queries).
pub fn run_now(dir: &Path, program: &str, args: &[&str]) -> Result<String, String> {
    let mut cmd = Command::new(program);
    cmd.args(args).current_dir(dir).stdin(Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000);
    }
    let out = cmd.output().map_err(|e| format!("{program}: {e}"))?;
    let text = String::from_utf8_lossy(&out.stdout).into_owned();
    if out.status.success() {
        Ok(text)
    } else {
        Err(format!("{program} {}: {}{}", args.join(" "), text, String::from_utf8_lossy(&out.stderr)))
    }
}

/// The sum of libtest's "test result:" lines: passed, failed, ignored, and
/// how many test binaries reported.
pub fn test_totals(lines: &[String]) -> (u64, u64, u64, u64) {
    let (mut p, mut f, mut i, mut n) = (0, 0, 0, 0);
    for l in lines.iter().filter(|l| l.starts_with("test result:")) {
        let num = |key: &str| {
            l.split(';')
                .find(|part| part.trim_end().ends_with(key))
                .and_then(|part| part.split_whitespace().rev().nth(1))
                .and_then(|x| x.parse::<u64>().ok())
                .unwrap_or(0)
        };
        p += num("passed");
        f += num("failed");
        i += num("ignored");
        n += 1;
    }
    (p, f, i, n)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_lines_are_summed() {
        let lines: Vec<String> = [
            "test result: ok. 191 passed; 0 failed; 7 ignored; 0 measured; 0 filtered out; finished in 0.9s",
            "noise",
            "test result: FAILED. 4 passed; 1 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.1s",
        ]
        .map(String::from)
        .to_vec();
        assert_eq!(test_totals(&lines), (195, 1, 7, 2));
    }

    #[test]
    fn lines_from_goes_on_across_dropped_lines() {
        let out = Mutex::new(Lines::default());
        for i in 0..KEEP + 5 {
            push(&out, i.to_string());
        }
        let job = Job { name: String::new(), output: Arc::new(out), state: Arc::new(Mutex::new(State::Running)), child: Arc::new(Mutex::new(None)) };
        let (l, end) = job.lines_from(0);
        assert_eq!((l.len(), l[0].as_str(), end), (KEEP, "5", KEEP + 5));
        let (l, end2) = job.lines_from(KEEP + 3);
        assert_eq!((l, end2), (vec![(KEEP + 3).to_string(), (KEEP + 4).to_string()], KEEP + 5));
        assert!(job.lines_from(end2).0.is_empty());
    }

    #[test]
    fn a_job_reports_its_output_and_exit() {
        let dir = std::env::temp_dir();
        let job = Job::start("echo", &dir, "cmd", &["/C".into(), "echo hello".into()], &[]);
        for _ in 0..100 {
            if !job.running() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
        assert_eq!(job.state(), State::Succeeded);
        assert!(job.lines().iter().any(|l| l.trim() == "hello"), "{:?}", job.lines());
    }
}
