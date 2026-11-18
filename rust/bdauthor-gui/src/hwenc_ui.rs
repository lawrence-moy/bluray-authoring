//! Reglages : encodeur video (x264 ou materiel) et detection de l'acceleration materielle, lancee seulement par un bouton.
use crate::i18n::{tr, trf};
use bdauthor_core::ffmpeg::Tools;
use bdauthor_core::hwenc::{detect, Detection, Status, VideoEncoder};
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
            for e in [VideoEncoder::X264, VideoEncoder::Qsv, VideoEncoder::Nvenc, VideoEncoder::Amf] {
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
    crate::help(ui, tr("A hardware encoder is much faster than x264, with a slightly lower quality at the same bitrate. Each video it encodes is checked: if the stream is not Blu-ray compliant, the video is encoded again by x264. The “fill the disc” mode always uses x264 (two passes)."));
    changed
}
