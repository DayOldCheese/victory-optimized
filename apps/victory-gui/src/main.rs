//! Mandy: a desktop front end for building, testing,
//! benchmarking and running the layer, with live diagnostics and Git.
//!
//! It drives the existing tools (`cargo`, `git`, `target\release\victory.exe`)
//! as child processes; nothing of the layer runs inside the GUI.

#![cfg_attr(not(test), windows_subsystem = "windows")]

mod dialog;
mod export;
mod frames;
mod git;
mod jobs;
mod progress;
mod settings;
mod sysinfo;

use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use eframe::egui::{self, Color32, RichText};
use jobs::{Job, State};
use victory_core::profiles::{self, Profile};

const ACCENT: Color32 = Color32::from_rgb(110, 170, 255);
const GOOD: Color32 = Color32::from_rgb(110, 200, 120);
const BAD: Color32 = Color32::from_rgb(235, 100, 90);
const WARN: Color32 = Color32::from_rgb(230, 180, 80);

#[derive(PartialEq, Clone, Copy)]
enum Tab {
    Dashboard,
    Play,
    Settings,
    Diagnostics,
    Git,
}

/// What the last finished task of a kind gave.
#[derive(Default, Clone)]
struct Outcome {
    state: Option<State>,
    at: Option<Instant>,
    summary: String,
}

type Pending<T> = Arc<Mutex<Option<T>>>;

struct App {
    s: settings::Settings,
    tab: Tab,
    /// cargo or git work; one at a time.
    task: Option<Job>,
    /// The running title.
    game: Option<Job>,
    build: Outcome,
    tests: Outcome,
    bench: Outcome,
    sampler: sysinfo::Sampler,
    sample: sysinfo::Sample,
    sampled: Instant,
    history: Vec<(f64, f64)>,
    frames: frames::Follower,
    message: Option<(Color32, String)>,
    git_files: Vec<(String, String, bool)>,
    git_log: Vec<String>,
    git_branch: String,
    git_head: String,
    commit_msg: String,
    audit: Pending<Vec<String>>,
    audit_done: Option<Vec<String>>,
    upstream: Pending<Result<Vec<String>, String>>,
    upstream_done: Option<Result<Vec<String>, String>>,
    push_confirm: bool,
    push_typed: String,
    console_filter: String,
    exported: Option<String>,
    cache_dir: Option<PathBuf>,
}

impl App {
    fn new(cc: &eframe::CreationContext<'_>) -> App {
        let mut visuals = egui::Visuals::dark();
        visuals.selection.bg_fill = Color32::from_rgb(40, 80, 140);
        cc.egui_ctx.set_visuals(visuals);
        let s = settings::Settings::load();
        let mut app = App {
            tab: Tab::Dashboard,
            task: None,
            game: None,
            build: Outcome::default(),
            tests: Outcome::default(),
            bench: Outcome::default(),
            sampler: sysinfo::Sampler::new(),
            sample: Default::default(),
            sampled: Instant::now() - Duration::from_secs(5),
            history: Vec::new(),
            frames: frames::Follower::default(),
            message: None,
            git_files: Vec::new(),
            git_log: Vec::new(),
            git_branch: String::new(),
            git_head: String::new(),
            commit_msg: String::new(),
            audit: Default::default(),
            audit_done: None,
            upstream: Default::default(),
            upstream_done: None,
            push_confirm: false,
            push_typed: String::new(),
            console_filter: String::new(),
            exported: None,
            cache_dir: victory_core::config::Config::load().ok().map(|c| c.cache_dir),
            s,
        };
        app.refresh_git();
        app
    }

    fn victory_exe(&self) -> PathBuf {
        // The `diag` profile: the build Belobug's measurements are of.
        self.s.repo.join("target").join("diag").join("victory.exe")
    }

    fn say(&mut self, color: Color32, text: impl Into<String>) {
        self.message = Some((color, text.into()));
    }

    fn busy(&self) -> bool {
        self.task.as_ref().is_some_and(Job::running)
    }

    fn start_task(&mut self, name: &str, program: &str, args: &[&str]) {
        if self.busy() {
            self.say(WARN, "another task is still running");
            return;
        }
        let args: Vec<String> = args.iter().map(|a| a.to_string()).collect();
        self.task = Some(Job::start(name, &self.s.repo, program, &args, &[]));
        self.say(ACCENT, format!("{name} started"));
    }

    fn refresh_git(&mut self) {
        let repo = self.s.repo.clone();
        self.git_branch = git::branch(&repo);
        self.git_head = git::head(&repo);
        self.git_log = git::history(&repo, 20);
        let old: std::collections::HashMap<String, bool> = self.git_files.iter().map(|(_, p, on)| (p.clone(), *on)).collect();
        self.git_files = git::changes(&repo).into_iter().map(|(st, p)| {
            let on = old.get(&p).copied().unwrap_or(false);
            (st, p, on)
        }).collect();
    }

    /// Notes what a finished task gave, once.
    fn collect(&mut self) {
        let Some(job) = &self.task else { return };
        let st = job.state();
        if st == State::Running {
            return;
        }
        let lines = job.lines();
        let name = job.name.clone();
        let outcome = Outcome { state: Some(st.clone()), at: Some(Instant::now()), summary: String::new() };
        match name.as_str() {
            "Build" if self.build.at.is_none_or(|t| t.elapsed().as_secs() > 1) && self.build.state.as_ref() != Some(&st) || self.build.state.is_none() => {
                let errors = lines.iter().filter(|l| l.starts_with("error")).count();
                self.build = Outcome { summary: if st == State::Succeeded { "release build succeeded".into() } else { format!("build failed: {errors} error(s); see the console") }, ..outcome };
            }
            "Run tests" if self.tests.state.is_none() || self.tests.summary.is_empty() || self.tests.at.is_some_and(|t| t.elapsed().as_secs() > 1) => {
                let (p, f, i, n) = jobs::test_totals(&lines);
                let crashed = matches!(st, State::Failed(Some(c)) if c as u32 >= 0xC000_0000);
                self.tests = Outcome {
                    summary: format!(
                        "{p} passed, {f} failed, {i} ignored in {n} test programs{}",
                        if crashed { " - a test program crashed (see the console); its tests are not counted" } else { "" }
                    ),
                    ..outcome
                };
            }
            "Benchmark" => {
                let found: Vec<&String> = lines.iter().filter(|l| l.contains(" ns") || l.contains(" ms") || l.contains("MB out/s")).collect();
                self.bench = Outcome { summary: format!("{} result lines (see Diagnostics > console)", found.len()), ..outcome };
            }
            _ => {}
        }
        if name == "Build" || name == "Run tests" || name == "Benchmark" {
            // Keep the job for its console; mark it collected by renaming.
            if let Some(j) = &mut self.task {
                j.name = format!("{name} (finished)");
            }
            let ok = st == State::Succeeded;
            self.say(if ok { GOOD } else { BAD }, format!("{name}: {}", if ok { "finished" } else { "failed - see Diagnostics for the errors" }));
        }
    }

    fn launch(&mut self) {
        if self.game.as_ref().is_some_and(Job::running) {
            self.say(WARN, "the title is already running");
            return;
        }
        let exe = self.victory_exe();
        if !exe.is_file() {
            self.say(BAD, "victory.exe is not built yet: press Build first");
            return;
        }
        if !std::path::Path::new(&self.s.game_exe).is_file() {
            self.say(BAD, "choose the title's executable first (Play > Browse)");
            return;
        }
        self.frames = frames::Follower::default();
        let mut args = vec!["run".to_owned(), self.s.game_exe.clone(), "--execute".into()];
        if let Some(console) = Profile::parse(&self.s.profile).unwrap_or(Profile::Play).console() {
            args.extend(["--console".into(), console.to_owned()]);
        }
        let extra: Vec<String> = self.s.game_args.split_whitespace().map(str::to_owned).collect();
        if !extra.is_empty() {
            args.push("--".into());
            args.extend(extra);
        }
        let env = self.s.launch_env();
        self.game = Some(Job::start("Title", &self.s.repo, &exe.display().to_string(), &args, &env));
        self.say(ACCENT, format!("title started with the {} profile", self.s.profile));
        self.tab = Tab::Diagnostics;
    }

    fn tick(&mut self) {
        if self.sampled.elapsed() >= Duration::from_secs(1) {
            self.sample = self.sampler.sample();
            self.sampled = Instant::now();
            if let (Some(c), Some(g)) = (self.sample.cpu_percent, self.sample.gpu_percent) {
                self.history.push((c, g));
                if self.history.len() > 120 {
                    self.history.remove(0);
                }
            }
            if let Some(g) = &self.game {
                self.frames.poll(g);
            }
        }
        self.collect();
        if let Some(r) = self.audit.lock().unwrap().take() {
            self.audit_done = Some(r);
        }
        if let Some(r) = self.upstream.lock().unwrap().take() {
            self.upstream_done = Some(r);
        }
    }
}

fn opt(v: Option<f64>, unit: &str, digits: usize) -> String {
    v.map_or("unavailable".into(), |v| format!("{v:.digits$}{unit}"))
}

fn state_text(st: &Option<State>) -> (Color32, String) {
    match st {
        None => (Color32::GRAY, "not run in this session".into()),
        Some(State::Running) => (ACCENT, "running".into()),
        Some(State::Succeeded) => (GOOD, "succeeded".into()),
        Some(State::Failed(c)) => (BAD, format!("failed (exit {})", c.map_or("none".into(), |c| format!("0x{:08X}", c as u32)))),
        Some(State::NotStarted(e)) => (BAD, format!("could not start: {e}")),
    }
}

/// A task's name and state, its progress bar when it prints one
/// ([`progress`]), how long it has run, and its last line.
fn task_status(ui: &mut egui::Ui, t: &Job) {
    let state = t.state();
    let lines = t.lines();
    let elapsed = t.began.elapsed().as_secs_f64();
    let (c, s) = state_text(&Some(state.clone()));
    let running = matches!(state, jobs::State::Running);
    let time = if running { format!("running for {}", progress::duration(elapsed)) } else { s.to_string() };
    ui.label(RichText::new(format!("{}: {time}", t.name)).color(c).strong());
    if let Some(p) = progress::latest(&lines) {
        let text = match (p.total, p.left_s(elapsed)) {
            (Some(total), Some(left)) if running => format!("{} of {total} {}, about {} left", p.done, p.what, progress::duration(left)),
            (Some(total), _) => format!("{} of {total} {}", p.done, p.what),
            (None, _) => format!("{} {} so far", p.done, p.what),
        };
        match p.fraction() {
            Some(f) => {
                ui.add(egui::ProgressBar::new(f).text(text).show_percentage());
            }
            None => {
                ui.add(egui::ProgressBar::new(0.0).text(text).animate(running));
            }
        }
    } else if running {
        ui.add(egui::ProgressBar::new(0.0).text("starting...").animate(true));
    }
    match state {
        jobs::State::Succeeded => {
            ui.label(RichText::new("Finished.").color(GOOD));
        }
        jobs::State::Failed(_) | jobs::State::NotStarted(_) => {
            ui.label(RichText::new("Did not finish: its last lines are in Diagnostics.").color(BAD));
        }
        jobs::State::Running => {}
    }
    if let Some(l) = lines.iter().rev().find(|l| !l.starts_with("[exit code")) {
        ui.label(RichText::new(l).small().monospace());
    }
}

/// A line graph of `values` in a box `h` high, scaled to `max`.
fn graph(ui: &mut egui::Ui, values: &[f64], max: f64, h: f32, color: Color32, marks: &[(f64, &str)]) {
    let w = ui.available_width();
    let (rect, _) = ui.allocate_exact_size(egui::vec2(w, h), egui::Sense::hover());
    let p = ui.painter();
    p.rect_filled(rect, 4.0, Color32::from_gray(22));
    for (y, label) in marks {
        let yy = rect.bottom() - (*y / max).min(1.0) as f32 * rect.height();
        p.line_segment([egui::pos2(rect.left(), yy), egui::pos2(rect.right(), yy)], egui::Stroke::new(1.0_f32, Color32::from_gray(60)));
        p.text(egui::pos2(rect.left() + 4.0, yy - 2.0), egui::Align2::LEFT_BOTTOM, *label, egui::FontId::proportional(11.0), Color32::from_gray(140));
    }
    if values.len() < 2 {
        p.text(rect.center(), egui::Align2::CENTER_CENTER, "no data yet", egui::FontId::proportional(13.0), Color32::GRAY);
        return;
    }
    let n = values.len() as f32 - 1.0;
    let points: Vec<egui::Pos2> = values
        .iter()
        .enumerate()
        .map(|(i, v)| egui::pos2(rect.left() + rect.width() * i as f32 / n, rect.bottom() - (v / max).clamp(0.0, 1.0) as f32 * rect.height()))
        .collect();
    p.add(egui::Shape::line(points, egui::Stroke::new(1.5_f32, color)));
}

impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.tick();
        ctx.request_repaint_after(Duration::from_millis(250));

        egui::TopBottomPanel::top("top").show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.heading(RichText::new("Mandy").color(ACCENT).strong());
                ui.label(format!("v{}", env!("CARGO_PKG_VERSION")));
                ui.separator();
                ui.label(format!("branch {}", self.git_branch));
                ui.separator();
                for (t, name) in [(Tab::Dashboard, "Dashboard"), (Tab::Play, "Play"), (Tab::Settings, "Settings"), (Tab::Diagnostics, "Diagnostics"), (Tab::Git, "Git")] {
                    ui.selectable_value(&mut self.tab, t, name);
                }
            });
            if let Some((c, m)) = &self.message {
                ui.label(RichText::new(m).color(*c));
            }
        });

        egui::CentralPanel::default().show(ctx, |ui| match self.tab {
            Tab::Dashboard => self.dashboard(ui),
            Tab::Play => self.play(ui),
            Tab::Settings => self.settings_tab(ui),
            Tab::Diagnostics => self.diagnostics(ui),
            Tab::Git => self.git_tab(ui, ctx),
        });
    }

    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        let _ = self.s.save();
    }
}

impl App {
    fn dashboard(&mut self, ui: &mut egui::Ui) {
        ui.columns(2, |cols| {
            let ui = &mut cols[0];
            ui.heading("Project");
            egui::Grid::new("proj").num_columns(2).spacing([16.0, 6.0]).show(ui, |ui| {
                ui.label("Version");
                ui.label(format!("{} ({})", env!("CARGO_PKG_VERSION"), self.git_head));
                ui.end_row();
                ui.label("Branch");
                ui.label(&self.git_branch);
                ui.end_row();
                for (name, o) in [("Build", self.build.clone()), ("Tests", self.tests.clone()), ("Benchmark", self.bench.clone())] {
                    let (c, t) = state_text(&o.state);
                    ui.label(name);
                    ui.vertical(|ui| {
                        ui.label(RichText::new(t).color(c));
                        if !o.summary.is_empty() {
                            ui.label(RichText::new(&o.summary).small());
                        }
                    });
                    ui.end_row();
                }
                ui.label("victory.exe");
                ui.label(if self.victory_exe().is_file() { "built" } else { "not built" });
                ui.end_row();
            });
            ui.add_space(10.0);
            ui.heading("Actions");
            ui.horizontal_wrapped(|ui| {
                let busy = self.busy();
                if ui.add_enabled(!busy, egui::Button::new("Build")).on_hover_text("cargo build --locked --profile diag -p victory --features diagnostics").clicked() {
                    self.start_task("Build", "cargo", &["build", "--locked", "--profile", "diag", "-p", "victory", "--features", "diagnostics"]);
                }
                if ui.add_enabled(!busy, egui::Button::new("Run Tests")).on_hover_text("cargo test --workspace --release (several minutes)").clicked() {
                    self.start_task("Run tests", "cargo", &["test", "--workspace", "--release"]);
                }
                if ui.add_enabled(!busy, egui::Button::new("Benchmark")).on_hover_text("The synthetic benchmarks: indirect draw barriers and copy compares on the card, BCPack decode").clicked() {
                    self.start_task(
                        "Benchmark",
                        "cmd",
                        &["/C", "cargo test --release -p victory-gpu --lib -- --ignored --nocapture bench_indirect_draw_barriers card_compare_costs && cargo test --release -p victory-formats --lib -- --ignored --nocapture bcpack_throughput"],
                    );
                }
                if ui.add_enabled(self.task.is_some() && busy, egui::Button::new("Cancel task")).clicked()
                    && let Some(t) = &self.task
                {
                    t.stop();
                }
                if ui.button("Launch Game").clicked() {
                    self.launch();
                }
                let running = self.game.as_ref().is_some_and(Job::running);
                if ui.add_enabled(running, egui::Button::new(RichText::new("Stop").color(BAD))).clicked()
                    && let Some(g) = &self.game
                {
                    g.stop();
                    self.say(WARN, "title stopped");
                }
            });
            if let Some(t) = &self.task {
                task_status(ui, t);
            }

            let ui = &mut cols[1];
            ui.heading("Machine");
            let s = &self.sample;
            egui::Grid::new("mach").num_columns(2).spacing([16.0, 6.0]).show(ui, |ui| {
                ui.label("CPU");
                ui.label(opt(s.cpu_percent, " %", 0));
                ui.end_row();
                ui.label("RAM");
                ui.label(format!("{} of {}", opt(s.ram_used_gb, " GB", 1), opt(s.ram_total_gb, " GB", 1)));
                ui.end_row();
                ui.label("GPU");
                ui.label(format!("{} ({})", opt(s.gpu_percent, " % (3D)", 0), s.gpu_name.clone().unwrap_or_else(|| "unknown".into())));
                ui.end_row();
                ui.label("VRAM");
                ui.label(format!("{} of {}", opt(s.vram_used_gb, " GB", 1), opt(s.vram_total_gb, " GB", 1)));
                ui.end_row();
                ui.label("Title");
                let fps = self.frames.last().map(|r| format!("{:.1} FPS, {:.0} ms a frame", r.fps, r.frame_ms));
                ui.label(fps.unwrap_or_else(|| if self.game.as_ref().is_some_and(Job::running) { "running; waiting for the first frame reading".into() } else { "not running".into() }));
                ui.end_row();
            });
            ui.add_space(6.0);
            ui.label("CPU (blue) and GPU (green), last two minutes");
            let cpu: Vec<f64> = self.history.iter().map(|h| h.0).collect();
            let gpu: Vec<f64> = self.history.iter().map(|h| h.1).collect();
            graph(ui, &cpu, 100.0, 70.0, ACCENT, &[(50.0, "50 %")]);
            graph(ui, &gpu, 100.0, 70.0, GOOD, &[(50.0, "50 %")]);
        });
    }

    fn play(&mut self, ui: &mut egui::Ui) {
        ui.heading("Title");
        ui.label("Only software you own and are authorised to run.");
        ui.horizontal(|ui| {
            ui.label("Executable");
            ui.add(egui::TextEdit::singleline(&mut self.s.game_exe).desired_width(520.0));
            if ui.button("Browse...").clicked()
                && let Some(p) = dialog::open("Choose the title's executable", &[("Programs", "*.exe")])
            {
                self.s.game_exe = p;
                let _ = self.s.save();
            }
        });
        ui.horizontal(|ui| {
            ui.label("Arguments");
            ui.add(egui::TextEdit::singleline(&mut self.s.game_args).desired_width(520.0).hint_text("passed to the title after --"));
        });
        ui.add_space(8.0);
        ui.heading("Profile");
        for p in Profile::ALL {
            let (label, note) = match p {
                Profile::Play => ("Play", "scripts/play.ps1's settings: the set Belobug's measured runs were made with."),
                Profile::Fast => ("Fast (Series S)", "Play in the Series S profile (1080p, less video memory) with 1.5 GB of the title's buffers on the card. Softer than Series X. Not yet measured here."),
                Profile::RayTracing => ("Ray-traced light", "Play plus the title's own ray-traced indirect light: lit interiors, about a third slower (README)."),
                Profile::Safe => ("Safe", "play.ps1 -Safe: without the newer, faster ways. The first thing to try when something is wrong."),
                Profile::Stable => ("Layer defaults", "Every switch off. Much slower; for finding which switch causes a fault."),
            };
            ui.horizontal(|ui| {
                ui.radio_value(&mut self.s.profile, p.name().to_owned(), RichText::new(label).strong());
                ui.label(RichText::new(note).small());
            });
        }
        ui.add_space(8.0);
        ui.horizontal(|ui| {
            if ui.button(RichText::new("Launch Game").strong()).clicked() {
                let _ = self.s.save();
                self.launch();
            }
            let running = self.game.as_ref().is_some_and(Job::running);
            if ui.add_enabled(running, egui::Button::new(RichText::new("Stop").color(BAD))).clicked()
                && let Some(g) = &self.game
            {
                g.stop();
            }
            let game_dir = std::path::Path::new(&self.s.game_exe).parent().map(|p| p.display().to_string());
            let can_warm = !self.busy() && self.victory_exe().is_file() && game_dir.is_some();
            if ui
                .add_enabled(can_warm, egui::Button::new("Translate All Shaders"))
                .on_hover_text("victory shaders <the title's folder>: every shader translated into the cache ahead of the run (minutes, once per version of the translator; can be stopped and goes on later)")
                .clicked()
                && let Some(dir) = game_dir
            {
                let exe = self.victory_exe().display().to_string();
                self.start_task("Translate all shaders", &exe, &["shaders", &dir]);
            }
            let can_pipelines = !self.busy() && self.victory_exe().is_file();
            if ui
                .add_enabled(can_pipelines, egui::Button::new("Warm Pipelines"))
                .on_hover_text("victory warm --used: the pipelines earlier runs drew with, made ahead for a new driver or build (runs with VICTORY_PIPELINE_BOOK=1 write them down)")
                .clicked()
            {
                let exe = self.victory_exe().display().to_string();
                self.start_task("Warm pipelines", &exe, &["warm", "--used"]);
            }
        });
        // (the shader and pipeline tasks are started here: their progress is shown here too)
        if let Some(t) = &self.task {
            task_status(ui, t);
        }
        if let Some(g) = &self.game {
            let (c, s) = state_text(&Some(g.state()));
            ui.label(RichText::new(format!("Title: {s}")).color(c));
        }
    }

    fn settings_tab(&mut self, ui: &mut egui::Ui) {
        egui::ScrollArea::vertical().show(ui, |ui| {
            ui.heading("Switches");
            ui.label("Each is read by the layer at start. A switch set here overrides the profile; \"profile\" leaves it to the profile.");
            if ui.button("Restore defaults").clicked() {
                self.s.overrides.clear();
                self.s.profile = "play".into();
                let _ = self.s.save();
                self.say(GOOD, "defaults restored: play profile, no overrides");
            }
            let profile = Profile::parse(&self.s.profile).unwrap_or(Profile::Play);
            let in_profile: Vec<&str> = profile.switches().iter().map(|s| s.var).collect();
            egui::Grid::new("switches").num_columns(4).striped(true).spacing([12.0, 4.0]).show(ui, |ui| {
                ui.strong("Switch");
                ui.strong("This launch");
                ui.strong("Override");
                ui.strong("What it does");
                ui.end_row();
                for sw in profiles::all_switches() {
                    let mut label = RichText::new(sw.var).monospace();
                    if sw.experimental {
                        label = label.color(WARN);
                    }
                    ui.label(label);
                    let o = self.s.overrides.get(sw.var).cloned();
                    let now = match &o {
                        Some(v) if v.is_empty() => "off".to_owned(),
                        Some(v) => v.clone(),
                        None if in_profile.contains(&sw.var) => format!("{} (profile)", sw.value),
                        None => "off".into(),
                    };
                    ui.label(now);
                    let mut choice = match &o {
                        None => 0,
                        Some(v) if v.is_empty() => 2,
                        Some(_) => 1,
                    };
                    let before = choice;
                    egui::ComboBox::from_id_salt(sw.var).width(90.0).selected_text(["profile", "on", "off"][choice]).show_ui(ui, |ui| {
                        ui.selectable_value(&mut choice, 0, "profile");
                        ui.selectable_value(&mut choice, 1, "on");
                        ui.selectable_value(&mut choice, 2, "off");
                    });
                    if choice != before {
                        match choice {
                            0 => {
                                self.s.overrides.remove(sw.var);
                            }
                            1 => {
                                self.s.overrides.insert(sw.var.into(), sw.value.into());
                            }
                            _ => {
                                self.s.overrides.insert(sw.var.into(), String::new());
                            }
                        }
                        let _ = self.s.save();
                    }
                    ui.label(RichText::new(format!("{}{}", if sw.experimental { "EXPERIMENTAL. " } else { "" }, sw.note)).small());
                    ui.end_row();
                }
            });

            ui.add_space(12.0);
            ui.heading("Graphics and debugging");
            ui.label("Only what the layer reads. Debug options slow a run down and are for finding faults.");
            let extra: [(&str, &str, &str, bool); 12] = [
                ("VICTORY_WAVE_LDS_LOOSE", "1", "Last resort for console wave64 programs NVIDIA would refuse: run them approximately instead of not at all (a value shared inside a branch may be read stale).", true),
                ("VICTORY_WAVE_WHY", "1", "Debugging: logs why a program keeps 64-lane waves (NVIDIA refuses those).", false),
                ("VICTORY_PORT_CONSTANTS", "1", "Renderer patch 1: the title's constants heap read by the card where the title writes it (README). Off by default while tested.", true),
                ("VICTORY_PORT_DESCRIPTORS", "1", "Renderer patch 2: descriptor-table stores report their slots instead of a scan a frame (README). Off by default while tested.", true),
                ("VICTORY_PORT_CBV", "1", "Renderer patch 5: shaders read constant buffers in place (README: -4.7 ms). Off by default while tested.", true),
                ("VICTORY_PORT_GUARD", "1", "Renderer patch 6: the write guards' lock shared, with per-block claims (README: -2 ms). Off by default while tested.", true),
                ("VICTORY_PORT_JOURNAL", "1", "Renderer patch 7: the heaps' records asked at the title's submit (README: -1 to -3 ms). Off by default while tested.", true),
                ("VICTORY_FRAME_INTERVAL_MS", "16.667", "Lets the title go above 30 fps; its clock stays right (README).", false),
                ("VICTORY_OUTPUT", "1920x1080", "The title renders to 1080p itself: about 16 ms less card time, a softer picture (README).", false),
                ("VICTORY_PANEL", "1", "The layer's own panel over the picture: frame times and their parts.", false),
                ("VICTORY_D3D12_DEBUG", "1", "Debugging: Direct3D's debug layer (much slower).", false),
                ("VICTORY_GPU_TIMING", "1", "Debugging: each list timed on its own, slow ones logged.", false),
            ];
            egui::Grid::new("gfx").num_columns(3).striped(true).show(ui, |ui| {
                for (var, value, note, exp) in extra {
                    let mut on = self.s.overrides.get(var).is_some_and(|v| !v.is_empty());
                    let mut label = RichText::new(format!("{var}={value}")).monospace();
                    if exp {
                        label = label.color(WARN);
                    }
                    if ui.checkbox(&mut on, label).changed() {
                        if on {
                            self.s.overrides.insert(var.into(), value.into());
                        } else {
                            self.s.overrides.remove(var);
                        }
                        let _ = self.s.save();
                    }
                    ui.label(RichText::new(format!("{}{note}", if exp { "EXPERIMENTAL. " } else { "" })).small());
                    ui.end_row();
                }
            });
            ui.label(RichText::new("Resolution: the title picks its own from the console profile; VICTORY_OUTPUT above makes it render to 1080p.").small());

            ui.add_space(12.0);
            ui.heading("Shader cache");
            let cache = self.cache_dir.clone();
            match cache {
                Some(dir) => {
                    ui.label(format!("{}", dir.display()));
                    ui.horizontal(|ui| {
                        if ui.button("Open folder").clicked() {
                            let _ = std::process::Command::new("explorer").arg(&dir).spawn();
                        }
                        if ui.button(RichText::new("Clear shader cache").color(WARN)).on_hover_text("Deletes the translated shaders: the next run translates them again (slow)").clicked() {
                            let shaders = dir.join("shaders");
                            match std::fs::remove_dir_all(&shaders) {
                                Ok(()) => self.say(GOOD, format!("cleared {}", shaders.display())),
                                Err(e) => self.say(BAD, format!("could not clear {}: {e}", shaders.display())),
                            }
                        }
                    });
                }
                None => {
                    ui.label("the configuration could not be read (victory.toml)");
                }
            }
        });
    }

    fn diagnostics(&mut self, ui: &mut egui::Ui) {
        let last = self.frames.last().cloned();
        ui.horizontal(|ui| {
            match &last {
                Some(r) => {
                    let c = if r.fps >= 55.0 { GOOD } else if r.fps >= 28.0 { WARN } else { BAD };
                    ui.label(RichText::new(format!("{:.1} FPS", r.fps)).size(28.0).color(c).strong());
                    ui.label(format!("{:.1} ms a frame (the layer's summary line)", r.frame_ms));
                }
                None => {
                    ui.label(RichText::new("no frame readings").size(20.0).color(Color32::GRAY));
                    ui.label("they appear a few seconds after the title presents its first frames");
                }
            }
        });
        ui.label("Frame time per summary line, ms (lines: 16.7 = 60 FPS, 33.3 = 30 FPS)");
        let recent = self.frames.recent_frames(300);
        let max = recent.iter().copied().fold(50.0, f64::max).min(1000.0);
        graph(ui, &recent, max, 120.0, ACCENT, &[(16.7, "60 FPS"), (33.3, "30 FPS")]);
        let s = &self.sample;
        ui.label(format!(
            "Title RAM {} | RAM {} of {} | VRAM {} of {} | GPU {} | CPU {}",
            opt(s.game_ram_gb, " GB", 2),
            opt(s.ram_used_gb, " GB", 1),
            opt(s.ram_total_gb, " GB", 1),
            opt(s.vram_used_gb, " GB", 1),
            opt(s.vram_total_gb, " GB", 1),
            opt(s.gpu_percent, " %", 0),
            opt(s.cpu_percent, " %", 0)
        ));
        ui.separator();
        let source: Vec<(&str, Vec<String>)> = [("Title", self.game.as_ref()), ("Task", self.task.as_ref())]
            .into_iter()
            .filter_map(|(n, j)| j.map(|j| (n, j.lines())))
            .collect();
        let errors: Vec<String> = source
            .iter()
            .flat_map(|(n, l)| {
                l.iter()
                    .filter(|x| x.starts_with("error") || x.contains("panicked") || x.contains(" ERROR ") || x.contains("FAILED") || x.contains("[exit code") && !x.contains("[exit code 0 "))
                    .map(move |x| format!("{n}: {x}"))
            })
            .collect();
        ui.horizontal(|ui| {
            ui.label(RichText::new(format!("Errors: {}", errors.len())).color(if errors.is_empty() { GOOD } else { BAD }));
            ui.separator();
            ui.label("Filter");
            ui.add(egui::TextEdit::singleline(&mut self.console_filter).desired_width(200.0));
            let compact = ui
                .button("Export diagnostic log...")
                .on_hover_text("Small enough to share: repeated lines cut to their first five and their last; every warning, error and frame-rate line kept")
                .clicked();
            let full = ui.button("Export full log...").on_hover_text("Every line the title and the task printed (megabytes after a few minutes)").clicked();
            if (compact || full)
                && let Some(path) = dialog::save("Save the diagnostic log", &[("Text", "*.txt")], if full { "victory-diagnostics-full.txt" } else { "victory-diagnostics.txt" })
            {
                let mut text = String::from(if full { "Mandy diagnostic log (full)\n" } else { "Mandy diagnostic log (compact)\n" });
                text += &format!(
                    "branch {} at {}\nprofile {} (console {}); switches: {}\n",
                    self.git_branch,
                    self.git_head,
                    self.s.profile,
                    Profile::parse(&self.s.profile).and_then(Profile::console).unwrap_or("series-x"),
                    self.s.launch_env().iter().map(|(k, v)| format!("{k}={v}")).collect::<Vec<_>>().join(" ")
                );
                for (n, l) in &source {
                    let body = if full { l.iter().map(|x| export::plain(x)).collect::<Vec<_>>().join("\n") } else { export::compact(l) };
                    text += &format!("\n==== {n} ====\n{body}\n");
                }
                text += &format!("\n==== frame rate ====\n{}\n", self.frames.text());
                match std::fs::write(&path, text) {
                    Ok(()) => self.exported = Some(format!("saved {path}: check it for anything private before sharing it")),
                    Err(e) => self.exported = Some(format!("could not save {path}: {e}")),
                }
            }
        });
        if let Some(e) = &self.exported {
            ui.label(RichText::new(e).small());
        }
        if !errors.is_empty() {
            egui::ScrollArea::vertical().id_salt("errors").max_height(90.0).show(ui, |ui| {
                for e in errors.iter().rev().take(50) {
                    ui.label(RichText::new(e).color(BAD).monospace().small());
                }
            });
        }
        ui.separator();
        egui::ScrollArea::vertical().id_salt("console").stick_to_bottom(true).auto_shrink([false, false]).show(ui, |ui| {
            let f = self.console_filter.to_lowercase();
            for (n, lines) in &source {
                ui.label(RichText::new(format!("---- {n} ----")).color(ACCENT));
                let shown: Vec<&String> = lines.iter().filter(|l| f.is_empty() || l.to_lowercase().contains(&f)).collect();
                for l in &shown[shown.len().saturating_sub(2000)..] {
                    ui.label(RichText::new(l.as_str()).monospace().small());
                }
            }
        });
    }

    fn git_tab(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        ui.horizontal(|ui| {
            ui.heading(format!("Branch {}", self.git_branch));
            if ui.button("Refresh").clicked() {
                self.refresh_git();
            }
            if ui.button("Check upstream").on_hover_text("Fetches Belobug/VIctory-Belobug-AMD and lists its commits this branch does not have. Integrating them stays a reviewed, manual step.").clicked() {
                let (repo, out) = (self.s.repo.clone(), self.upstream.clone());
                self.upstream_done = None;
                std::thread::spawn(move || {
                    *out.lock().unwrap() = Some(git::upstream_news(&repo));
                });
                self.say(ACCENT, "fetching upstream...");
            }
        });
        if let Some(r) = &self.upstream_done {
            match r {
                Ok(list) if list.is_empty() => {
                    ui.label(RichText::new("upstream has nothing newer than the integrated commit").color(GOOD));
                }
                Ok(list) => {
                    ui.label(RichText::new(format!("{} upstream commits not integrated yet (review them before integrating, as PROGRESS.md describes):", list.len())).color(WARN));
                    egui::ScrollArea::vertical().id_salt("up").max_height(100.0).show(ui, |ui| {
                        for l in list {
                            ui.label(RichText::new(l).small().monospace());
                        }
                    });
                }
                Err(e) => {
                    ui.label(RichText::new(format!("upstream check failed: {e}")).color(BAD));
                }
            }
        }
        ui.columns(2, |cols| {
            let ui = &mut cols[0];
            ui.strong("Changed files (tick the ones to commit)");
            egui::ScrollArea::vertical().id_salt("files").max_height(260.0).show(ui, |ui| {
                if self.git_files.is_empty() {
                    ui.label("no changes");
                }
                for (st, path, on) in &mut self.git_files {
                    ui.checkbox(on, RichText::new(format!("{st:>2} {path}")).monospace());
                }
            });
            ui.label("Commit message");
            ui.add(egui::TextEdit::multiline(&mut self.commit_msg).desired_rows(3).desired_width(f32::INFINITY));
            let chosen: Vec<String> = self.git_files.iter().filter(|f| f.2).map(|f| f.1.clone()).collect();
            ui.horizontal(|ui| {
                if ui.add_enabled(!chosen.is_empty(), egui::Button::new("Audit")).on_hover_text("Checks the chosen files for logs, dumps, game files, secrets and personal details, and runs the asset policy").clicked() {
                    let (repo, out, files) = (self.s.repo.clone(), self.audit.clone(), chosen.clone());
                    self.audit_done = None;
                    std::thread::spawn(move || {
                        *out.lock().unwrap() = Some(git::audit(&repo, &files));
                    });
                    self.say(ACCENT, "auditing...");
                }
                let clean = self.audit_done.as_ref().is_some_and(Vec::is_empty);
                if ui.add_enabled(clean && !chosen.is_empty() && !self.commit_msg.trim().is_empty(), egui::Button::new("Commit")).on_hover_text("Needs a clean audit of these files and a message").clicked() {
                    match git::commit(&self.s.repo, &chosen, &self.commit_msg) {
                        Ok(_) => {
                            self.say(GOOD, "committed");
                            self.commit_msg.clear();
                            self.audit_done = None;
                        }
                        Err(e) => self.say(BAD, e),
                    }
                    self.refresh_git();
                }
                if ui.button("Push to GitHub...").clicked() {
                    self.push_confirm = true;
                    self.push_typed.clear();
                }
            });
            if let Some(a) = &self.audit_done {
                if a.is_empty() {
                    ui.label(RichText::new("audit: nothing private or proprietary found in the chosen files").color(GOOD));
                } else {
                    for p in a {
                        ui.label(RichText::new(format!("blocked: {p}")).color(BAD).small());
                    }
                }
            }

            let ui = &mut cols[1];
            ui.strong("History");
            egui::ScrollArea::vertical().id_salt("hist").show(ui, |ui| {
                for l in &self.git_log {
                    ui.label(RichText::new(l).monospace().small());
                }
            });
        });

        if self.push_confirm {
            let branch = self.git_branch.clone();
            egui::Window::new("Push to GitHub").collapsible(false).resizable(false).anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0]).show(ctx, |ui| {
                ui.label(format!("This publishes the commits of branch {branch} to the origin remote (git push -u origin {branch})."));
                if branch == "main" {
                    ui.label(RichText::new("This is main: pushing it changes the published default branch.").color(WARN));
                }
                let uncommitted = self.git_files.len();
                if uncommitted > 0 {
                    ui.label(format!("{uncommitted} changed files are not committed and will not be pushed."));
                }
                ui.label("Type the branch name to confirm:");
                ui.text_edit_singleline(&mut self.push_typed);
                ui.horizontal(|ui| {
                    if ui.add_enabled(self.push_typed.trim() == branch && !self.busy(), egui::Button::new(RichText::new("Push").color(WARN))).clicked() {
                        self.start_task("Push", "git", &["push", "-u", "origin", &branch]);
                        self.push_confirm = false;
                        self.tab = Tab::Diagnostics;
                    }
                    if ui.button("Cancel").clicked() {
                        self.push_confirm = false;
                    }
                });
            });
        }
    }
}

fn main() -> eframe::Result<()> {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default().with_inner_size([1280.0, 820.0]).with_min_inner_size([900.0, 600.0]).with_title("Mandy"),
        ..Default::default()
    };
    eframe::run_native("Mandy", options, Box::new(|cc| Ok(Box::new(App::new(cc)))))
}
