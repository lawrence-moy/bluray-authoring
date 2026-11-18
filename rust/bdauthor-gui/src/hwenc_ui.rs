//! Reglages : encodeur video (x264 ou materiel) et detection de l'acceleration materielle, lancee seulement par un bouton.
use crate::i18n::{tr, trf};
use bdauthor_core::ffmpeg::Tools;
use bdauthor_core::bench::{measure_source, Case, CaseResult, Source};
use bdauthor_core::hwenc::{detect, Detection, Status, VideoEncoder};
use std::path::PathBuf;
use eframe::egui;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{channel, Receiver};
use std::sync::{Arc, Mutex};

enum Msg {
    Step(f32, String),
    Done(Result<Detection, String>),
}

struct Running {
    rx: Receiver<Msg>,
    cancel: Arc<AtomicBool>,
    progress: f32,
    step: String,
}

/// Detection en cours, et derniere detection de la session (avec les messages detailles, que la configuration ne garde pas).
struct State {
    running: Option<Running>,
    last: Option<Detection>,
    error: Option<String>,
}

static STATE: Mutex<State> = Mutex::new(State { running: None, last: None, error: None });

/// Encodeur a utiliser pour une generation, d'apres la configuration : (encodeur, sait coder l'entrelace).
pub fn chosen(encoder: &str, detection: &str) -> (VideoEncoder, bool) {
    let e = VideoEncoder::from_name(encoder).unwrap_or_default();
    let d = Detection::from_config(detection);
    match d.as_ref().and_then(|d| d.probe(e)) {
        Some(p) if p.usable() => (e, p.interlaced_ok()),
        // pas (ou plus) vu conforme : x264
        _ => (VideoEncoder::X264, false),
    }
}

fn status_text(s: &Status) -> (bool, String) {
    match s {
        Status::Ok => (true, tr("Blu-ray compliant").to_string()),
        Status::NotInFfmpeg => (false, tr("not included in this ffmpeg").to_string()),
        Status::Unavailable(m) if m.is_empty() => (false, tr("not available on this computer").to_string()),
        Status::Unavailable(m) => (false, trf!("not available on this computer ({})", m)),
        Status::Refused(m) if m.is_empty() => (false, tr("refused the Blu-ray settings").to_string()),
        Status::Refused(m) => (false, trf!("refused the Blu-ray settings ({})", m)),
        Status::NotCompliant(v) if v.is_empty() => (false, tr("output not Blu-ray compliant").to_string()),
        Status::NotCompliant(v) => (false, trf!("output not Blu-ray compliant: {}", v.join("; "))),
    }
}

/// Section des reglages ; rend vrai si la configuration a change.
pub fn section(ui: &mut egui::Ui, encoder: &mut String, detection: &mut String) -> bool {
    let mut changed = false;
    let mut st = STATE.lock().unwrap_or_else(|p| p.into_inner());
    // messages de la detection en cours
    let mut finished = None;
    if let Some(r) = st.running.as_mut() {
        while let Ok(m) = r.rx.try_recv() {
            match m {
                Msg::Step(f, s) => { r.progress = f; if !s.is_empty() { r.step = s; } }
                Msg::Done(res) => finished = Some(res),
            }
        }
        ui.ctx().request_repaint_after(std::time::Duration::from_millis(100));
    }
    if let Some(res) = finished {
        st.running = None;
        match res {
            Ok(d) => {
                *detection = d.to_config();
                st.last = Some(d);
                st.error = None;
                changed = true;
            }
            Err(e) => st.error = Some(e),
        }
    }
    let det = st.last.clone().or_else(|| Detection::from_config(detection));
    let usable = |e: VideoEncoder| e == VideoEncoder::X264 || det.as_ref().and_then(|d| d.probe(e)).is_some_and(|p| p.usable());
    // un encodeur qui n'est plus vu conforme : retour a x264
    let cur = VideoEncoder::from_name(encoder).unwrap_or_default();
    if !usable(cur) && st.running.is_none() && det.is_some() {
        *encoder = VideoEncoder::X264.name().to_string();
        changed = true;
    }
    let cur = VideoEncoder::from_name(encoder).unwrap_or_default();
    let speed = |e: VideoEncoder| -> String {
        let d = match det.as_ref() { Some(d) => d, None => return String::new() };
        match d.probe(e) {
            Some(p) if d.x264_fps > 0.0 && p.fps > 0.0 => format!(" (≈ {:.0}× x264)", (p.fps / d.x264_fps).max(1.0)),
            _ => String::new(),
        }
    };

    ui.horizontal(|ui| {
        ui.label(tr("Video encoder:"));
        egui::ComboBox::from_id_salt("video_encoder").selected_text(format!("{}{}", tr(cur.label()), speed(cur))).show_ui(ui, |ui| {
            for e in VideoEncoder::ALL.into_iter().filter(|e| e.on_this_system()) {
                let r = ui.add_enabled(usable(e), egui::SelectableLabel::new(cur == e, format!("{}{}", tr(e.label()), speed(e))));
                let r = if usable(e) { r } else { r.on_disabled_hover_text(tr("Not found usable by the last detection.")) };
                if r.clicked() && cur != e { *encoder = e.name().to_string(); changed = true; }
            }
        });
        let busy = st.running.is_some();
        if ui.add_enabled(!busy, egui::Button::new(tr("Detect hardware acceleration…")))
            .on_hover_text(tr("Tries each hardware encoder known to ffmpeg with the Blu-ray settings, on a few seconds of test pattern (progressive, then interlaced), and checks the stream produced. Takes from a few seconds to a minute."))
            .clicked()
        {
            let (tx, rx) = channel();
            let cancel = Arc::new(AtomicBool::new(false));
            let c2 = cancel.clone();
            let ctx = ui.ctx().clone();
            std::thread::spawn(move || {
                let res = match Tools::locate() {
                    Ok(tools) => {
                        let (tx2, ctx2) = (tx.clone(), ctx.clone());
                        detect(&tools, &mut |f, s| { let _ = tx2.send(Msg::Step(f, s.to_string())); ctx2.request_repaint(); }, &c2).map_err(|e| e.to_string())
                    }
                    Err(e) => Err(e.to_string()),
                };
                let _ = tx.send(Msg::Done(res));
                ctx.request_repaint();
            });
            st.running = Some(Running { rx, cancel, progress: 0.0, step: String::new() });
        }
    });
    if let Some(r) = &st.running {
        ui.horizontal(|ui| {
            ui.add(egui::ProgressBar::new(r.progress).desired_width(260.0).animate(true).text(r.step.clone()));
            if ui.button(tr("Cancel")).clicked() { r.cancel.store(true, Ordering::Relaxed); }
        });
    }
    if let Some(e) = &st.error {
        if !e.contains(tr("operation cancelled")) { ui.colored_label(egui::Color32::from_rgb(220, 90, 90), e); }
    }
    // resultat de la derniere detection
    if let Some(d) = &det {
        ui.indent("resultat_detection", |ui| {
            for p in &d.probes {
                let (ok, text) = status_text(&p.status);
                let line = if ok {
                    let inter = if p.interlaced_ok() { tr("interlaced: yes") } else { tr("interlaced: no") };
                    format!("✔ {} — {}{}, {}", tr(p.encoder.label()), text, speed(p.encoder), inter)
                } else {
                    format!("✖ {} — {}", tr(p.encoder.label()), text)
                };
                let rt = egui::RichText::new(line).size(12.0);
                ui.add(egui::Label::new(if ok { rt } else { rt.weak() }).wrap());
            }
        });
    } else if st.running.is_none() {
        ui.label(egui::RichText::new(tr("No detection yet: the videos are encoded by x264.")).weak());
    }
    if cur != VideoEncoder::X264 && !det.as_ref().and_then(|d| d.probe(cur)).is_some_and(|p| p.interlaced_ok()) {
        ui.label(egui::RichText::new(tr("This encoder does not code interlaced video: interlaced sources and 1080p 25 / 29.97 sources (written as 1080i on a Blu-ray) stay on x264.")).weak());
    }
    if ui.button(tr("Measure performance…")).on_hover_text(tr("Encodes an extract of a real video with x264 and with the hardware encoder (with and without decoding by the graphics chip), without writing anything, and shows the speed and the processor load.")).clicked() {
        BENCH.lock().unwrap_or_else(|p| p.into_inner()).open = true;
    }
    crate::help(ui, tr("A hardware encoder is much faster than x264, with a slightly lower quality at the same bitrate. Each video it encodes is checked: if the stream is not Blu-ray compliant, the video is encoded again by x264. The “fill the disc” mode always uses x264 (two passes)."));
    changed
}

// ---------------------------------------------------------------- mesure des performances

enum BenchMsg {
    Step(f64, String),
    Done(Result<(Vec<CaseResult>, Option<String>), String>),
}

struct BenchRun {
    rx: Receiver<BenchMsg>,
    cancel: Arc<AtomicBool>,
    progress: f64,
    step: String,
}

struct Bench {
    open: bool,
    /// video choisie (vide : la premiere du projet), duree de l'extrait (s), preset x264 (vide : celui du projet)
    file: String,
    secs: u32,
    preset: String,
    running: Option<BenchRun>,
    /// resultats, note, video et preset mesures
    results: Option<(Vec<CaseResult>, Option<String>, String, String)>,
    error: Option<String>,
}

static BENCH: Mutex<Bench> = Mutex::new(Bench { open: false, file: String::new(), secs: 20, preset: String::new(), running: None, results: None, error: None });

/// Valeurs de `Bench::file` qui designent une mire generee (sinon : chemin d'une video).
const PATTERN_24: &str = "<mire 1080p23.976>";
const PATTERN_25: &str = "<mire 1080p25>";

/// Nom affiche de la video mesuree.
fn source_name(f: &str) -> String {
    match f {
        PATTERN_24 => tr("Test pattern 1080p 23.976 (generated)").to_string(),
        PATTERN_25 => tr("Test pattern 1080p 25, written as 1080i (generated)").to_string(),
        _ => std::path::Path::new(f).file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| f.to_string()),
    }
}

fn source_of(f: &str) -> Option<Source> {
    match f {
        PATTERN_24 => Some(Source::Pattern { fps25: false }),
        PATTERN_25 => Some(Source::Pattern { fps25: true }),
        _ => std::path::Path::new(f).is_file().then(|| Source::File(PathBuf::from(f))),
    }
}

const PRESETS: [&str; 9] = ["ultrafast", "superfast", "veryfast", "faster", "fast", "medium", "slow", "slower", "veryslow"];

/// La fenetre de mesure est ouverte.
pub fn bench_open() -> bool { BENCH.lock().unwrap_or_else(|p| p.into_inner()).open }

/// Fenetre de mesure (ouverte par le bouton des reglages). `videos` : videos du projet (chemins complets) ; `preset`, `crf` :
/// reglages du projet, utilises par defaut.
pub fn bench_window(ctx: &egui::Context, encoder: &str, detection: &str, videos: &[PathBuf], preset: &str, crf: u32) {
    let mut b = BENCH.lock().unwrap_or_else(|p| p.into_inner());
    if !b.open { return; }
    if let Some(r) = b.running.as_mut() {
        let mut done = None;
        while let Ok(m) = r.rx.try_recv() {
            match m { BenchMsg::Step(f, s) => { r.progress = f; if !s.is_empty() { r.step = s; } } BenchMsg::Done(res) => done = Some(res) }
        }
        if let Some(res) = done {
            let (file, pr) = (b.file.clone(), b.preset.clone());
            b.running = None;
            match res {
                Ok((r, note)) => { b.results = Some((r, note, file, pr)); b.error = None; }
                Err(e) => { if !e.contains(tr("operation cancelled")) { b.error = Some(e); } }
            }
        }
        ctx.request_repaint_after(std::time::Duration::from_millis(100));
    }
    // par defaut : la premiere video du projet, sinon la mire
    if source_of(&b.file).is_none() {
        b.file = videos.first().map(|v| v.display().to_string()).unwrap_or_else(|| PATTERN_24.to_string());
    }
    if b.preset.is_empty() { b.preset = if preset.is_empty() { "medium".into() } else { preset.to_string() }; }
    let hw = chosen_or_detected(encoder, detection);
    let mut open = true;
    crate::os_window(ctx, "mesure_performances", tr("Measure performance"), [620.0, 430.0], &mut open, &mut |ui| {
        let busy = b.running.is_some();
        ui.add_enabled_ui(!busy, |ui| {
            egui::Grid::new("bench_settings").num_columns(2).spacing([10.0, 6.0]).show(ui, |ui| {
                ui.label(tr("Video:"));
                ui.horizontal(|ui| {
                    let cur = if b.file.is_empty() { tr("(to choose)").to_string() } else { source_name(&b.file) };
                    egui::ComboBox::from_id_salt("bench_video").selected_text(cur).width(320.0).show_ui(ui, |ui| {
                        for v in videos {
                            let f = v.display().to_string();
                            if ui.selectable_label(b.file == f, source_name(&f)).clicked() { b.file = f; }
                        }
                        if !videos.is_empty() { ui.separator(); }
                        for p in [PATTERN_24, PATTERN_25] {
                            if ui.selectable_label(b.file == p, source_name(p)).clicked() { b.file = p.to_string(); }
                        }
                    }).response.on_hover_text(tr("The test pattern is generated for the measure, with a fine grain so that it is about as hard to encode as a film. A real video of your own gives the most faithful measure."));
                    if ui.button(tr("Choose…")).clicked() {
                        if let Some(p) = rfd::FileDialog::new().pick_file() { b.file = p.display().to_string(); }
                    }
                });
                ui.end_row();
                ui.label(tr("Extract:"));
                egui::ComboBox::from_id_salt("bench_secs").selected_text(format!("{} s", b.secs)).show_ui(ui, |ui| {
                    for s in [10, 20, 30, 60] { if ui.selectable_label(b.secs == s, format!("{s} s")).clicked() { b.secs = s; } }
                }).response.on_hover_text(tr("Taken from the middle of the video. Longer gives a steadier measure (the processor of a laptop slows down when it heats up)."));
                ui.end_row();
                ui.label(tr("x264 preset:"));
                egui::ComboBox::from_id_salt("bench_preset").selected_text(b.preset.clone()).show_ui(ui, |ui| {
                    for p in PRESETS { if ui.selectable_label(b.preset == p, p).clicked() { b.preset = p.to_string(); } }
                }).response.on_hover_text(tr("Preset of the project by default; the hardware encoder uses the matching setting, as in the disc build."));
                ui.end_row();
                ui.label(tr("Hardware encoder:"));
                ui.label(match hw { Some((e, _)) => tr(e.label()).to_string(), None => tr("none detected (run the detection in the settings)").to_string() });
                ui.end_row();
            });
        });
        ui.horizontal(|ui| {
            let src = source_of(&b.file);
            let can = !busy && src.is_some();
            if ui.add_enabled(can, egui::Button::new(tr("Start the measure"))).clicked() {
                let (tx, rx) = channel();
                let cancel = Arc::new(AtomicBool::new(false));
                let (c2, ctx2) = (cancel.clone(), ctx.clone());
                let (src, secs, pr) = (src.clone().unwrap_or(Source::Pattern { fps25: false }), b.secs as f64, b.preset.clone());
                std::thread::spawn(move || {
                    let res = match Tools::locate() {
                        Ok(tools) => {
                            let (tx2, ctx3) = (tx.clone(), ctx2.clone());
                            measure_source(&tools, &src, secs, &pr, crf, hw, &mut |f, s| { let _ = tx2.send(BenchMsg::Step(f, s.to_string())); ctx3.request_repaint(); }, &c2).map_err(|e| e.to_string())
                        }
                        Err(e) => Err(e.to_string()),
                    };
                    let _ = tx.send(BenchMsg::Done(res));
                    ctx2.request_repaint();
                });
                b.running = Some(BenchRun { rx, cancel, progress: 0.0, step: String::new() });
            }
            if let Some(r) = &b.running {
                ui.add(egui::ProgressBar::new(r.progress as f32).desired_width(240.0).animate(true).text(r.step.clone()));
                if ui.button(tr("Cancel")).clicked() { r.cancel.store(true, Ordering::Relaxed); }
            }
        });
        if let Some(e) = &b.error { ui.colored_label(egui::Color32::from_rgb(220, 90, 90), e); }
        if let Some((res, note, file, pr)) = &b.results {
            ui.separator();
            ui.label(egui::RichText::new(trf!("{} — x264 preset {}", source_name(file), pr)).weak());
            egui::Grid::new("bench_results").striped(true).num_columns(5).spacing([14.0, 4.0]).show(ui, |ui| {
                for h in [tr("Configuration"), tr("Pictures/s"), tr("Speed"), tr("Processor"), tr("Time")] { ui.label(egui::RichText::new(h).strong()); }
                ui.end_row();
                for c in res {
                    ui.label(match c.case { Case::X264 => tr("x264 (software)").to_string(), _ => tr(c.case.label()).to_string() });
                    match &c.result {
                        Ok(m) => {
                            ui.monospace(format!("{:.1}", m.fps));
                            ui.monospace(format!("×{:.2}", m.speed)).on_hover_text(tr("Compared with the real duration of the video: ×1 = as long as the video lasts."));
                            ui.monospace(format!("{:.0} % ({:.1} {})", m.cpu_share * 100.0, m.cpu_cores, tr("cores"))).on_hover_text(tr("Average load of the processor during the encoding (all its cores together)."));
                            ui.monospace(format!("{:.1} s", m.seconds));
                        }
                        Err(e) => { ui.colored_label(egui::Color32::from_rgb(220, 90, 90), e); ui.label(""); ui.label(""); ui.label(""); }
                    }
                    ui.end_row();
                }
            });
            if let Some(n) = note { ui.label(egui::RichText::new(n).weak()); }
            for line in conclusions(res) { ui.label(line); }
        }
    });
    // fermer la fenetre arrete une mesure en cours
    if !open {
        if let Some(r) = &b.running { r.cancel.store(true, Ordering::Relaxed); }
        b.open = false;
    }
}

/// Encodeur materiel a mesurer : celui choisi s'il est materiel, sinon le premier reconnu conforme par la detection.
fn chosen_or_detected(encoder: &str, detection: &str) -> Option<(VideoEncoder, bool)> {
    let c = chosen(encoder, detection);
    if c.0 != VideoEncoder::X264 { return Some(c); }
    let d = Detection::from_config(detection)?;
    d.probes.iter().find(|p| p.usable()).map(|p| (p.encoder, p.interlaced_ok()))
}

/// Ce que disent les mesures, en clair.
fn conclusions(res: &[CaseResult]) -> Vec<String> {
    let get = |c: Case| res.iter().find(|r| r.case == c).and_then(|r| r.result.as_ref().ok());
    let mut out = Vec::new();
    if let (Some(x), Some(h)) = (get(Case::X264), get(Case::Hw)) {
        out.push(trf!("The hardware encoder is {}× as fast as x264.", format!("{:.1}", h.fps / x.fps.max(0.001))));
        if h.cpu_share >= 0.85 {
            out.push(tr("With the hardware encoder, the processor is almost saturated: it is what limits the speed (decoding the source and the filters).").to_string());
        } else {
            out.push(tr("With the hardware encoder, the processor is not saturated: the video chip sets the speed.").to_string());
        }
        if let Some(g) = get(Case::HwGpuDecode) {
            let gain = (g.fps / h.fps.max(0.001) - 1.0) * 100.0;
            if gain >= 10.0 {
                out.push(trf!("Decoding by the graphics chip gains {} % and frees the processor ({} % instead of {} %): worth adding to the disc build.", format!("{gain:.0}"), format!("{:.0}", g.cpu_share * 100.0), format!("{:.0}", h.cpu_share * 100.0)));
            } else if gain > -5.0 {
                out.push(trf!("Decoding by the graphics chip changes little ({} %).", format!("{gain:+.0}")));
            } else {
                out.push(trf!("Decoding by the graphics chip is slower here ({} %).", format!("{gain:+.0}")));
            }
        }
    }
    out
}
