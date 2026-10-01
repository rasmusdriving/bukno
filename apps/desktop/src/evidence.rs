//! Measurement hooks for synthetic scenario runs.
//!
//! Off unless `BUKNO_AUTOPILOT` is set. Each autopilot drives the real
//! window, writes its numbers to `BUKNO_EVIDENCE_DIR`, then closes the app:
//!
//! - `screenshot`: saves the rendered window as `screen.png`.
//! - `scroll`: jumps and then scrolls smoothly through the whole chat while a
//!   reply streams, recording per-frame times in `metrics.json`.
//! - `idle` and `orb`: count frames and process CPU time over a quiet window,
//!   with nothing animating (`idle`) or only the working orb (`orb`).

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use egui::{Event, ViewportCommand};
use serde_json::json;

use crate::theme::Theme;
use crate::transcript::TranscriptView;
use crate::transcript::document::Document;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Autopilot {
    Screenshot,
    Scroll,
    Idle,
    Orb,
}

#[derive(Default)]
struct Phase {
    name: &'static str,
    ui_ms: Vec<f32>,
    interval_ms: Vec<f32>,
    laid_out: Vec<usize>,
}

pub struct Evidence {
    pilot: Option<Autopilot>,
    dir: PathBuf,
    settle: Duration,
    measure: Duration,
    launched: Instant,
    frame_started: Option<Instant>,
    last_frame: Option<Instant>,
    frames: Arc<AtomicU64>,
    phases: Vec<Phase>,
    phase_started: Option<Instant>,
    direction: f32,
    screenshot_requested: bool,
    quiet_started: bool,
    done: bool,
}

impl Evidence {
    pub fn from_env() -> Self {
        let pilot = match std::env::var("BUKNO_AUTOPILOT").ok().as_deref() {
            Some("screenshot") => Some(Autopilot::Screenshot),
            Some("scroll") => Some(Autopilot::Scroll),
            Some("idle") => Some(Autopilot::Idle),
            Some("orb") => Some(Autopilot::Orb),
            _ => None,
        };
        let secs = |name: &str, default: f64| {
            Duration::from_secs_f64(std::env::var(name).ok().and_then(|v| v.parse().ok()).unwrap_or(default))
        };
        Self {
            pilot,
            dir: std::env::var_os("BUKNO_EVIDENCE_DIR").map_or_else(|| PathBuf::from("."), PathBuf::from),
            settle: secs("BUKNO_SETTLE_SECS", 2.0),
            measure: secs("BUKNO_MEASURE_SECS", 10.0),
            launched: Instant::now(),
            frame_started: None,
            last_frame: None,
            frames: Arc::default(),
            phases: Vec::new(),
            phase_started: None,
            direction: 1.0,
            screenshot_requested: false,
            quiet_started: false,
            done: false,
        }
    }

    pub fn active(&self) -> bool {
        self.pilot.is_some()
    }

    pub fn frame_start(&mut self, ctx: &egui::Context) {
        self.frames.fetch_add(1, Ordering::Relaxed);
        let now = Instant::now();
        if let (Some(last), Some(phase)) = (self.last_frame, self.phases.last_mut()) {
            phase.interval_ms.push((now - last).as_secs_f32() * 1000.0);
        }
        self.last_frame = Some(now);
        self.frame_started = Some(now);
        if self.pilot == Some(Autopilot::Screenshot) {
            let shot = ctx.input(|i| {
                i.events.iter().find_map(|e| match e {
                    Event::Screenshot { image, .. } => Some(image.clone()),
                    _ => None,
                })
            });
            if let Some(image) = shot {
                self.save_png(&image);
                self.finish(ctx, json!({ "screenshot": "screen.png", "size": image.size, "pixels_per_point": ctx.pixels_per_point() }));
            }
        }
    }

    pub fn frame_end(&mut self, ctx: &egui::Context, transcript: &mut TranscriptView, doc: &Document, theme: &Theme) {
        let Some(pilot) = self.pilot else { return };
        if self.done {
            return;
        }
        if let (Some(start), Some(phase)) = (self.frame_started, self.phases.last_mut()) {
            phase.ui_ms.push(start.elapsed().as_secs_f32() * 1000.0);
            phase.laid_out.push(transcript.stats.laid_out);
        }
        let since_launch = self.launched.elapsed();
        if since_launch < self.settle {
            if matches!(pilot, Autopilot::Screenshot | Autopilot::Scroll) {
                ctx.request_repaint();
            } else {
                // Idle and orb runs must not add frames of their own; wake once after settling.
                ctx.request_repaint_after(self.settle - since_launch);
            }
            return;
        }
        match pilot {
            Autopilot::Screenshot => {
                if !self.screenshot_requested {
                    self.screenshot_requested = true;
                    ctx.send_viewport_cmd(ViewportCommand::Screenshot(egui::UserData::default()));
                }
                ctx.request_repaint();
            }
            Autopilot::Scroll => self.scroll(ctx, transcript, doc, theme),
            Autopilot::Idle | Autopilot::Orb => self.quiet(ctx, pilot),
        }
    }

    fn scroll(&mut self, ctx: &egui::Context, transcript: &mut TranscriptView, doc: &Document, theme: &Theme) {
        let now = Instant::now();
        let phase_len = self.measure / 2;
        let step = match self.phases.len() {
            0 => {
                self.phases.push(Phase { name: "jump", ..Default::default() });
                self.phase_started = Some(now);
                transcript.scroll_to_block(theme, 0);
                return ctx.request_repaint();
            }
            1 if now - self.phase_started.unwrap() > phase_len => {
                self.phases.push(Phase { name: "smooth", ..Default::default() });
                self.phase_started = Some(now);
                0.0
            }
            1 => 1_200.0,
            2 if now - self.phase_started.unwrap() > phase_len => {
                let report = self.scroll_report(doc);
                return self.finish(ctx, report);
            }
            _ => 40.0,
        };
        // Sweep down and back up: the jump phase lays out a new screen every frame.
        if (self.direction > 0.0 && transcript.is_following()) || (self.direction < 0.0 && transcript.offset() <= 0.0) {
            self.direction = -self.direction;
        }
        transcript.scroll_by(step * self.direction);
        ctx.request_repaint();
    }

    fn scroll_report(&self, doc: &Document) -> serde_json::Value {
        let phases: Vec<_> = self
            .phases
            .iter()
            .map(|p| {
                json!({
                    "phase": p.name,
                    "frames": p.ui_ms.len(),
                    "ui_ms": summary(&p.ui_ms),
                    "frame_interval_ms": summary(&p.interval_ms),
                    "blocks_laid_out_per_frame": summary(&p.laid_out.iter().map(|&n| n as f32).collect::<Vec<_>>()),
                })
            })
            .collect();
        json!({
            "workload": "scroll the synthetic chat while a reply streams",
            "messages": doc.messages.len(),
            "blocks": doc.blocks.len(),
            "phases": phases,
            "note": "ui_ms is CPU time spent building the frame on the UI thread; frame_interval_ms is the time between frame starts (includes vsync and GPU)."
        })
    }

    fn quiet(&mut self, ctx: &egui::Context, pilot: Autopilot) {
        if self.quiet_started {
            return;
        }
        self.quiet_started = true;
        let frames = self.frames.clone();
        let measure = self.measure;
        let dir = self.dir.clone();
        let ctx = ctx.clone();
        let start_frames = frames.load(Ordering::Relaxed);
        let start_cpu = process_cpu_seconds();
        std::thread::spawn(move || {
            let started = Instant::now();
            std::thread::sleep(measure);
            let wall = started.elapsed().as_secs_f64();
            let counted = frames.load(Ordering::Relaxed) - start_frames;
            let cpu = process_cpu_seconds().zip(start_cpu).map(|(end, start)| end - start);
            let report = json!({
                "workload": match pilot { Autopilot::Orb => "working orb only, nothing else changing", _ => "idle window, visible, no run" },
                "seconds": wall,
                "frames": counted,
                "frames_per_second": counted as f64 / wall,
                "process_cpu_seconds": cpu,
                "process_cpu_percent_of_one_core": cpu.map(|c| c / wall * 100.0),
                "pid": std::process::id(),
            });
            write(&dir, "metrics.json", &report);
            ctx.send_viewport_cmd(ViewportCommand::Close);
            ctx.request_repaint();
        });
    }

    fn finish(&mut self, ctx: &egui::Context, report: serde_json::Value) {
        self.done = true;
        write(&self.dir, "metrics.json", &report);
        // BUKNO_LINGER_SECS keeps the window open and idle afterwards, so
        // memory can be sampled once the workload has stopped.
        let linger = std::env::var("BUKNO_LINGER_SECS").ok().and_then(|v| v.parse::<f64>().ok()).unwrap_or(0.0);
        if linger > 0.0 {
            let ctx = ctx.clone();
            std::thread::spawn(move || {
                std::thread::sleep(Duration::from_secs_f64(linger));
                ctx.send_viewport_cmd(ViewportCommand::Close);
                ctx.request_repaint();
            });
        } else {
            ctx.send_viewport_cmd(ViewportCommand::Close);
        }
    }

    fn save_png(&self, image: &egui::ColorImage) {
        let path = self.dir.join("screen.png");
        let Ok(file) = std::fs::File::create(&path) else { return };
        let mut encoder = png::Encoder::new(std::io::BufWriter::new(file), image.size[0] as u32, image.size[1] as u32);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        if let Ok(mut writer) = encoder.write_header() {
            let _ = writer.write_image_data(image.as_raw());
        }
    }
}

fn write(dir: &std::path::Path, name: &str, value: &serde_json::Value) {
    let _ = std::fs::create_dir_all(dir);
    let _ = std::fs::write(dir.join(name), serde_json::to_string_pretty(value).unwrap_or_default());
}

fn summary(values: &[f32]) -> serde_json::Value {
    if values.is_empty() {
        return serde_json::Value::Null;
    }
    let mut v = values.to_vec();
    v.sort_by(f32::total_cmp);
    let pct = |p: f32| v[((v.len() - 1) as f32 * p).round() as usize];
    json!({ "p50": pct(0.5), "p95": pct(0.95), "p99": pct(0.99), "max": v[v.len() - 1], "count": v.len() })
}

/// User plus system CPU time of this process, all threads included.
#[cfg(unix)]
fn process_cpu_seconds() -> Option<f64> {
    let mut usage = std::mem::MaybeUninit::<libc::rusage>::uninit();
    // SAFETY: getrusage writes a full rusage struct for RUSAGE_SELF.
    let usage = unsafe {
        if libc::getrusage(libc::RUSAGE_SELF, usage.as_mut_ptr()) != 0 {
            return None;
        }
        usage.assume_init()
    };
    let t = |tv: libc::timeval| tv.tv_sec as f64 + tv.tv_usec as f64 / 1e6;
    Some(t(usage.ru_utime) + t(usage.ru_stime))
}

#[cfg(not(unix))]
fn process_cpu_seconds() -> Option<f64> {
    None
}
