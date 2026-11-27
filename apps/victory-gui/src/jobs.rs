//! Child processes (cargo, the layer's launcher) run on their own; their
//! output comes to the app a line at a time, so the window stays responsive.
//! Each runs in a process group of its own (a Windows job object), so that
//! Stop ends what it started too: a launcher's game, a preload's workers.

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
    /// When it was started.
    pub began: std::time::Instant,
    output: Arc<Mutex<Lines>>,
    state: Arc<Mutex<State>>,
    child: Arc<Mutex<Option<Child>>>,
    group: Option<group::Group>,
}

/// A process group: every process started by the one put in it belongs to
/// it too, so ending the group ends them all.
#[cfg(windows)]
mod group {
    use std::os::windows::io::AsRawHandle;
    use windows_sys::Win32::Foundation::{CloseHandle, HANDLE};
    use windows_sys::Win32::System::JobObjects::{AssignProcessToJobObject, CreateJobObjectW, TerminateJobObject};

    /// The job object's handle (as a number: it is used from other threads).
    pub struct Group(usize);

    impl Group {
        /// A group holding `child` (and what it starts from now on).
        pub fn of(child: &std::process::Child) -> Option<Group> {
            // SAFETY: plain Win32 calls; the handle is closed on drop.
            unsafe {
                let job: HANDLE = CreateJobObjectW(std::ptr::null(), std::ptr::null());
                if job.is_null() {
                    return None;
                }
                if AssignProcessToJobObject(job, child.as_raw_handle() as HANDLE) == 0 {
                    CloseHandle(job);
                    return None;
                }
                Some(Group(job as usize))
            }
        }

        /// Ends every process in the group.
        pub fn end(&self) {
            // SAFETY: our job object.
            unsafe { TerminateJobObject(self.0 as HANDLE, 1) };
        }
    }

    impl Drop for Group {
        fn drop(&mut self) {
            // Closing the handle ends nothing (no kill-on-close limit): a game
            // goes on when the app is closed.
            // SAFETY: our job object, closed once.
            unsafe { CloseHandle(self.0 as HANDLE) };
        }
    }
}

#[cfg(not(windows))]
mod group {
    pub struct Group;
    impl Group {
        pub fn of(_: &std::process::Child) -> Option<Group> {
            None
        }
        pub fn end(&self) {}
    }
}

/// The ids of every layer process running on this PC (`victory.exe`),
/// whoever started them.
#[cfg(windows)]
pub fn layers_running() -> Vec<u32> {
    use windows_sys::Win32::Foundation::{CloseHandle, INVALID_HANDLE_VALUE};
    use windows_sys::Win32::System::Diagnostics::ToolHelp::{CreateToolhelp32Snapshot, PROCESSENTRY32W, Process32FirstW, Process32NextW, TH32CS_SNAPPROCESS};
    let mut ids = Vec::new();
    // SAFETY: a process snapshot read and closed here.
    unsafe {
        let snap = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0);
        if snap == INVALID_HANDLE_VALUE {
            return ids;
        }
        let mut e: PROCESSENTRY32W = std::mem::zeroed();
        e.dwSize = size_of::<PROCESSENTRY32W>() as u32;
        let mut more = Process32FirstW(snap, &mut e) != 0;
        while more {
            let end = e.szExeFile.iter().position(|&c| c == 0).unwrap_or(e.szExeFile.len());
            if String::from_utf16_lossy(&e.szExeFile[..end]).eq_ignore_ascii_case("victory.exe") {
                ids.push(e.th32ProcessID);
            }
            more = Process32NextW(snap, &mut e) != 0;
        }
        CloseHandle(snap);
    }
    ids
}

#[cfg(not(windows))]
pub fn layers_running() -> Vec<u32> {
    Vec::new()
}

/// Ends the processes `ids` (the player asked for it); how many ended.
#[cfg(windows)]
pub fn end_processes(ids: &[u32]) -> usize {
    use windows_sys::Win32::Foundation::CloseHandle;
    use windows_sys::Win32::System::Threading::{OpenProcess, PROCESS_TERMINATE, TerminateProcess};
    ids.iter()
        .filter(|&&id| {
            // SAFETY: a process opened for termination and closed here.
            unsafe {
                let h = OpenProcess(PROCESS_TERMINATE, 0, id);
                if h.is_null() {
                    return false;
                }
                let ok = TerminateProcess(h, 1) != 0;
                CloseHandle(h);
                ok
            }
        })
        .count()
}

#[cfg(not(windows))]
pub fn end_processes(_: &[u32]) -> usize {
    0
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
        let mut grouped = None;
        match cmd.spawn() {
            Ok(mut c) => {
                if let Some(o) = c.stdout.take() {
                    pump(output.clone(), o);
                }
                if let Some(e) = c.stderr.take() {
                    pump(output.clone(), e);
                }
                grouped = group::Group::of(&c);
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
        Job { name: name.to_owned(), began: std::time::Instant::now(), output, state, child, group: grouped }
    }

    pub fn state(&self) -> State {
        self.state.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }

    pub fn running(&self) -> bool {
        self.state() == State::Running
    }

    /// Ends the child and everything it started, at once (Windows has no
    /// gentle signal for a process without a window of its own).
    pub fn stop(&self) {
        if let Some(g) = &self.group {
            g.end();
        }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lines_from_goes_on_across_dropped_lines() {
        let out = Mutex::new(Lines::default());
        for i in 0..KEEP + 5 {
            push(&out, i.to_string());
        }
        let job = Job { name: String::new(), began: std::time::Instant::now(), output: Arc::new(out), state: Arc::new(Mutex::new(State::Running)), child: Arc::new(Mutex::new(None)), group: None };
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

    /// Stop ends what the job's process started too.
    #[cfg(windows)]
    #[test]
    fn stop_ends_the_processes_a_job_started() {
        let dir = std::env::temp_dir();
        // cmd starts a second cmd that waits about a minute.
        let job = Job::start("tree", &dir, "cmd", &["/C".into(), "cmd /C ping -n 60 127.0.0.1 >NUL".into()], &[]);
        assert!(job.group.is_some(), "the job got no process group");
        std::thread::sleep(std::time::Duration::from_millis(500));
        job.stop();
        for _ in 0..100 {
            if !job.running() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
        assert!(!job.running());
    }
}
