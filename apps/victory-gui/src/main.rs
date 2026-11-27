//! Mandy: one window to play an Xbox Series title on a Windows PC from your
//! own copy. Pick the game and a graphics quality; the rest of the launch is
//! worked out from the PC's hardware ([`optimal`]).
//!
//! It drives the layer's own launcher (`scripts\play-nvidia.ps1`) and cargo
//! as child processes; nothing of the layer runs inside the app.

#![cfg_attr(not(test), windows_subsystem = "windows")]

mod dialog;
mod export;
mod frames;
mod jobs;
mod optimal;
mod progress;
mod settings;
mod sysinfo;

use std::time::{Duration, Instant};

use eframe::egui::{self, Color32, RichText};
use jobs::{Job, State};
use optimal::Graphics;

const ACCENT: Color32 = Color32::from_rgb(110, 170, 255);
const GOOD: Color32 = Color32::from_rgb(110, 200, 120);
const BAD: Color32 = Color32::from_rgb(235, 100, 90);
const WARN: Color32 = Color32::from_rgb(230, 180, 80);

struct App {
    s: settings::Settings,
    /// This PC, for the launch [`optimal::plan`] picks.
    hardware: optimal::Hardware,
    /// A build or a download; one at a time.
    task: Option<Job>,
    /// The running game.
    game: Option<Job>,
    sampler: sysinfo::Sampler,
    sample: sysinfo::Sample,
    sampled: Instant,
    frames: frames::Follower,
    message: Option<(Color32, String)>,
    /// Layer processes found running before a launch (from earlier ones).
    strays: Vec<u32>,
    console_filter: String,
    exported: Option<String>,
}

impl App {
    fn new(cc: &eframe::CreationContext<'_>) -> App {
        let mut visuals = egui::Visuals::dark();
        visuals.selection.bg_fill = Color32::from_rgb(40, 80, 140);
        cc.egui_ctx.set_visuals(visuals);
        App {
            s: settings::Settings::load(),
            hardware: optimal::Hardware::here(),
            task: None,
            game: None,
            sampler: sysinfo::Sampler::new(),
            sample: Default::default(),
            sampled: Instant::now() - Duration::from_secs(5),
            frames: frames::Follower::default(),
            message: None,
            strays: Vec::new(),
            console_filter: String::new(),
            exported: None,
        }
    }

    fn say(&mut self, color: Color32, text: impl Into<String>) {
        self.message = Some((color, text.into()));
    }

    fn busy(&self) -> bool {
        self.task.as_ref().is_some_and(Job::running)
    }

    fn playing(&self) -> bool {
        self.game.as_ref().is_some_and(Job::running)
    }

    fn graphics(&self) -> Graphics {
        Graphics::parse(&self.s.graphics)
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

    /// The game through the layer's launcher, with the graphics preset and
    /// the hardware's switches.
    fn launch(&mut self) {
        if self.playing() {
            self.say(WARN, "the game is already running");
            return;
        }
        // Two games at once (or a preload left behind) fight over the
        // processor and the memory: none is started over them.
        self.strays = jobs::layers_running();
        if !self.strays.is_empty() {
            self.say(BAD, format!("{} layer process(es) from an earlier launch are still running: end them first", self.strays.len()));
            return;
        }
        if !self.s.layer_exe().is_file() {
            self.say(BAD, "Mandy is not built yet: press Build first");
            return;
        }
        if !std::path::Path::new(&self.s.game_exe).is_file() {
            self.say(BAD, "choose the game's executable first (Browse)");
            return;
        }
        let plan = optimal::plan(self.graphics(), &self.hardware);
        // The switches the launcher takes only from its settings file.
        let file = self.s.repo.join("victory-settings.txt");
        let text = std::fs::read_to_string(&file).unwrap_or_default();
        if let Err(e) = std::fs::write(&file, optimal::apply(&text, &plan)) {
            self.say(BAD, format!("{}: {e}", file.display()));
            return;
        }
        let launcher = self.s.launcher().display().to_string();
        let mut args: Vec<String> =
            ["-NoProfile", "-ExecutionPolicy", "Bypass", "-File", &launcher, "-Exe", &self.s.game_exe].iter().map(|a| a.to_string()).collect();
        args.extend(plan.args.iter().cloned());
        self.frames = frames::Follower::default();
        self.game = Some(Job::start("Game", &self.s.repo, "powershell", &args, &[]));
        self.say(ACCENT, format!("started: {}", plan.graphics.label()));
    }

    fn tick(&mut self) {
        if self.sampled.elapsed() >= Duration::from_secs(1) {
            self.sample = self.sampler.sample();
            self.sampled = Instant::now();
            if let Some(g) = &self.game {
                self.frames.poll(g);
            }
        }
        if let Some(t) = &self.task
            && !t.running()
            && !t.name.ends_with("(finished)")
        {
            let ok = t.state() == State::Succeeded;
            let name = t.name.clone();
            if let Some(t) = &mut self.task {
                t.name = format!("{name} (finished)");
            }
            self.say(if ok { GOOD } else { BAD }, format!("{name}: {}", if ok { "done" } else { "failed: see the log below" }));
        }
    }
}

fn opt(v: Option<f64>, unit: &str, digits: usize) -> String {
    v.map_or("unavailable".into(), |v| format!("{v:.digits$}{unit}"))
}

/// A task's name and state, its progress bar when it prints one
/// ([`progress`]), how long it has run, and its last line.
fn task_status(ui: &mut egui::Ui, t: &Job) {
    let state = t.state();
    let lines = t.lines();
    let elapsed = t.began.elapsed().as_secs_f64();
    let running = state == State::Running;
    let (c, s) = match &state {
        State::Running => (ACCENT, format!("running for {}", progress::duration(elapsed))),
        State::Succeeded => (GOOD, "done".to_owned()),
        State::Failed(c) => (BAD, format!("failed (exit {})", c.map_or("none".into(), |c| format!("0x{:08X}", c as u32)))),
        State::NotStarted(e) => (BAD, format!("could not start: {e}")),
    };
    ui.label(RichText::new(format!("{}: {s}", t.name.trim_end_matches(" (finished)"))).color(c).strong());
    if let Some(p) = progress::latest(&lines) {
        let text = match (p.total, p.left_s(elapsed)) {
            (Some(total), Some(left)) if running => format!("{} of {total} {}, about {} left", p.done, p.what, progress::duration(left)),
            (Some(total), _) => format!("{} of {total} {}", p.done, p.what),
            (None, _) => format!("{} {} so far", p.done, p.what),
        };
        ui.add(egui::ProgressBar::new(p.fraction().unwrap_or(0.0)).text(text).animate(running && p.fraction().is_none()));
    } else if running {
        ui.add(egui::ProgressBar::new(0.0).text("working...").animate(true));
    }
    if let Some(l) = lines.iter().rev().find(|l| !l.starts_with("[exit code")) {
        ui.label(RichText::new(export::plain(l)).small().monospace());
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
        p.text(rect.center(), egui::Align2::CENTER_CENTER, "the frame rate shows here once the game draws", egui::FontId::proportional(13.0), Color32::GRAY);
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
        egui::CentralPanel::default().show(ctx, |ui| {
            egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| self.screen(ui));
        });
    }

    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        let _ = self.s.save();
    }
}

impl App {
    fn screen(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.heading(RichText::new("Mandy").color(ACCENT).strong().size(30.0));
            ui.label(format!("v{}", env!("CARGO_PKG_VERSION")));
        });
        ui.label("Plays your own copy of the game. No game files are part of Mandy.");
        if let Some((c, m)) = &self.message {
            ui.label(RichText::new(m).color(*c).strong());
        }
        ui.add_space(10.0);

        // The game.
        ui.horizontal(|ui| {
            ui.label(RichText::new("Game").strong());
            ui.add(egui::TextEdit::singleline(&mut self.s.game_exe).desired_width(520.0).hint_text("game_scarlett_master.exe"));
            if ui.button("Browse...").clicked()
                && let Some(p) = dialog::open("Choose the game's executable", &[("Programs", "*.exe")])
            {
                self.s.game_exe = p;
                let _ = self.s.save();
            }
        });
        ui.add_space(10.0);

        // Graphics quality.
        ui.label(RichText::new("Graphics quality").strong());
        let before = self.s.graphics.clone();
        for g in Graphics::ALL {
            ui.horizontal(|ui| {
                ui.radio_value(&mut self.s.graphics, g.name().to_owned(), RichText::new(g.label()).strong());
                ui.label(RichText::new(g.note()).small());
            });
        }
        if self.s.graphics != before {
            let _ = self.s.save();
        }
        let plan = optimal::plan(self.graphics(), &self.hardware);
        let hw = &self.hardware;
        ui.label(
            RichText::new(format!(
                "For {} ({:.0} GB) and {} processor threads: {}{}{}.",
                if hw.gpu_name.is_empty() { "this card" } else { &hw.gpu_name },
                hw.vram_gb,
                hw.threads,
                plan.graphics.label(),
                if hw.frame_generation() { ", frame generation ready (F1 in game)" } else { ", all of the card's memory for textures" },
                if plan.set.iter().any(|(k, _)| k == "VICTORY_THIN_THREADS") { ", helper threads for this processor" } else { "" },
            ))
            .small()
            .color(Color32::GRAY),
        );
        ui.add_space(12.0);

        // Play.
        let built = self.s.layer_exe().is_file();
        ui.horizontal(|ui| {
            let play = egui::Button::new(RichText::new("  Play  ").size(22.0).strong()).fill(Color32::from_rgb(40, 90, 50));
            if ui.add_enabled(built && !self.playing(), play).clicked() {
                let _ = self.s.save();
                self.launch();
            }
            if ui.add_enabled(self.playing(), egui::Button::new(RichText::new("Stop").size(18.0).color(BAD))).clicked()
                && let Some(g) = &self.game
            {
                g.stop();
                self.say(WARN, "stopped");
            }
            ui.separator();
            let build = if built { "Rebuild" } else { "Build Mandy" };
            if ui
                .add_enabled(!self.busy() && !self.playing(), egui::Button::new(build))
                .on_hover_text("cargo build --release --locked -p victory (a few minutes)")
                .clicked()
            {
                self.start_task(build, "cargo", &["build", "--release", "--locked", "-p", "victory"]);
            }
            if !self.s.dlss_files() {
                let script = self.s.repo.join("scripts").join("get-nvidia-sdks.ps1").display().to_string();
                if ui
                    .add_enabled(!self.busy(), egui::Button::new("Get NVIDIA DLSS files"))
                    .on_hover_text("NVIDIA's Streamline and DLSS files (about 70 MB) from NVIDIA's own GitHub; NVIDIA's licences, never part of Mandy")
                    .clicked()
                {
                    self.start_task("Get NVIDIA DLSS files", "powershell", &["-NoProfile", "-ExecutionPolicy", "Bypass", "-File", &script]);
                }
            }
        });
        if !self.strays.is_empty() {
            ui.horizontal(|ui| {
                ui.label(RichText::new(format!("{} layer process(es) from an earlier launch are still running.", self.strays.len())).color(BAD));
                if ui.button(RichText::new("End them").color(BAD)).clicked() {
                    let ended = jobs::end_processes(&self.strays);
                    self.strays = jobs::layers_running();
                    self.say(if self.strays.is_empty() { GOOD } else { WARN }, format!("ended {ended}; {} still running", self.strays.len()));
                }
            });
        }
        if let Some(t) = &self.task {
            task_status(ui, t);
        }
        ui.add_space(12.0);

        // While playing.
        let last = self.frames.last().cloned();
        ui.horizontal(|ui| match &last {
            Some(r) => {
                let c = if r.fps >= 55.0 { GOOD } else if r.fps >= 28.0 { WARN } else { BAD };
                ui.label(RichText::new(format!("{:.1} fps", r.fps)).size(28.0).color(c).strong());
                ui.label(format!("{:.1} ms a frame", r.frame_ms));
            }
            None => {
                ui.label(RichText::new(if self.playing() { "starting..." } else { "not playing" }).size(20.0).color(Color32::GRAY));
            }
        });
        let recent = self.frames.recent_frames(300);
        let max = recent.iter().copied().fold(50.0, f64::max).min(1000.0);
        graph(ui, &recent, max, 100.0, ACCENT, &[(16.7, "60 fps"), (33.3, "30 fps")]);
        let s = &self.sample;
        ui.label(
            RichText::new(format!(
                "RAM {} of {}  |  video memory {} of {}  |  graphics card {}  |  processor {}",
                opt(s.ram_used_gb, " GB", 1),
                opt(s.ram_total_gb, " GB", 1),
                opt(s.vram_used_gb, " GB", 1),
                opt(s.vram_total_gb, " GB", 1),
                opt(s.gpu_percent, " %", 0),
                opt(s.cpu_percent, " %", 0)
            ))
            .small(),
        );
        ui.add_space(8.0);
        egui::CollapsingHeader::new("Log").show(ui, |ui| self.log(ui));
    }

    fn log(&mut self, ui: &mut egui::Ui) {
        let source: Vec<(&str, Vec<String>)> = [("Game", self.game.as_ref()), ("Task", self.task.as_ref())]
            .into_iter()
            .filter_map(|(n, j)| j.map(|j| (n, j.lines())))
            .collect();
        ui.horizontal(|ui| {
            let compact = ui
                .button("Export diagnostic log...")
                .on_hover_text("Small enough to share: repeated lines cut to their first five and their last; every warning, error and frame-rate line kept")
                .clicked();
            let full = ui.button("Export full log...").on_hover_text("Every line the game and the task printed").clicked();
            if ui.button("Open the layer's logs").on_hover_text("%LOCALAPPDATA%\\victory\\logs: the launcher's own log of every run").clicked()
                && let Some(base) = std::env::var_os("LOCALAPPDATA")
            {
                let _ = std::process::Command::new("explorer").arg(std::path::PathBuf::from(base).join("victory").join("logs")).spawn();
            }
            ui.label("Filter");
            ui.add(egui::TextEdit::singleline(&mut self.console_filter).desired_width(160.0));
            if (compact || full)
                && let Some(path) = dialog::save("Save the diagnostic log", &[("Text", "*.txt")], if full { "mandy-log-full.txt" } else { "mandy-log.txt" })
            {
                let plan = optimal::plan(self.graphics(), &self.hardware);
                let mut text = String::from(if full { "Mandy diagnostic log (full)\n" } else { "Mandy diagnostic log (compact)\n" });
                text += &format!(
                    "{} for {} ({:.0} GB, {} threads): {}; settings file {}\n",
                    plan.graphics.label(),
                    self.hardware.gpu_name,
                    self.hardware.vram_gb,
                    self.hardware.threads,
                    plan.args.join(" "),
                    plan.set.iter().map(|(k, v)| format!("{k}={v}")).collect::<Vec<_>>().join(" ")
                );
                for (n, l) in &source {
                    let body = if full { l.iter().map(|x| export::plain(x)).collect::<Vec<_>>().join("\n") } else { export::compact(l) };
                    text += &format!("\n==== {n} ====\n{body}\n");
                }
                text += &format!("\n==== frame rate ====\n{}\n", self.frames.text());
                self.exported = Some(match std::fs::write(&path, text) {
                    Ok(()) => format!("saved {path}: check it for anything private before sharing it"),
                    Err(e) => format!("could not save {path}: {e}"),
                });
            }
        });
        if let Some(e) = &self.exported {
            ui.label(RichText::new(e).small());
        }
        egui::ScrollArea::vertical().id_salt("console").stick_to_bottom(true).max_height(320.0).auto_shrink([false, true]).show(ui, |ui| {
            let f = self.console_filter.to_lowercase();
            for (n, lines) in &source {
                ui.label(RichText::new(format!("---- {n} ----")).color(ACCENT));
                let shown: Vec<&String> = lines.iter().filter(|l| f.is_empty() || l.to_lowercase().contains(&f)).collect();
                for l in &shown[shown.len().saturating_sub(2000)..] {
                    ui.label(RichText::new(export::plain(l)).monospace().small());
                }
            }
        });
    }
}

fn main() -> eframe::Result<()> {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default().with_title("Mandy").with_inner_size([960.0, 760.0]).with_min_inner_size([720.0, 520.0]),
        ..Default::default()
    };
    eframe::run_native("Mandy", options, Box::new(|cc| Ok(Box::new(App::new(cc)))))
}
