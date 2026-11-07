//! Panneau « Pistes audio » d'une video : pistes de la video ou fichiers audio separes, chacune avec sa langue du projet.
use crate::i18n::{tr, trf};
use crate::subs_ui::language_for_code;
use bdauthor_core::audio::{AudioSource, AudioTrackCfg, MAX_AUDIO_TRACKS};
use bdauthor_core::ffmpeg::{audio_peaks, list_audio_streams, probe_duration, AudioPlayback, AudioStreamInfo, Tools};
use bdauthor_core::model::VideoEntry;
use bdauthor_core::subtitle::Language;
use eframe::egui;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{channel, Receiver};
use std::time::Instant;

/// Pistes audio deja recherchees, par fichier video (ou le message d'erreur de la recherche).
pub type AudioProbes = HashMap<String, Result<Vec<AudioStreamInfo>, String>>;

fn clean(p: &Path) -> String {
    let s = p.to_string_lossy().into_owned();
    s.strip_prefix(r"\\?\").map(String::from).unwrap_or(s)
}

fn abs(base: &Path, f: &str) -> PathBuf {
    let q = Path::new(f);
    if q.is_absolute() { q.to_path_buf() } else { base.join(q) }
}

/// Piste audio deplacee par glisser-deposer (rang dans la liste de la video).
#[derive(Clone, Copy)]
struct AudioRow(usize);

/// Retourne vrai si quelque chose a change. `langs` : langues du projet (le rang d'une langue audio est son numero de piste).
pub fn panel(ui: &mut egui::Ui, v: &mut VideoEntry, langs: &mut Vec<Language>, probes: &mut AudioProbes, base: &Path, listen: &mut Option<usize>) -> bool {
    let mut changed = false;
    ui.label(tr("Audio tracks (selectable with the remote control):"));
    if v.audio.is_empty() {
        ui.label(egui::RichText::new(tr("No track chosen: the video's first audio track is used, in the default audio language (Edit > Project…).")).weak());
    }
    let mut del = None;
    let mut rows: Vec<egui::Rect> = Vec::new();
    for (k, t) in v.audio.iter_mut().enumerate() {
        ui.push_id(("audio", k), |ui| {
            let what = match &t.source {
                AudioSource::File(f) => Path::new(f).file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| f.clone()),
                AudioSource::Embedded(n) => trf!("embedded track no. {}", n + 1),
            };
            // ligne de la piste : titre a gauche ; a droite, la langue, les reglages, l'ecoute et le retrait
            let open_id = ui.id().with("reglages_ouverts");
            let mut open: bool = ui.data_mut(|d| d.get_persisted(open_id)).unwrap_or(false);
            let row = ui.horizontal(|ui| {
                let _ = ui.dnd_drag_source(egui::Id::new(("piste_audio", k)), AudioRow(k), |ui| ui.label(egui::RichText::new("☰").strong().size(17.0)))
                    .response.on_hover_text(tr("Drag to change the order of the audio tracks (track 1 is the one at the top)"));
                ui.label(egui::RichText::new("🎵").size(17.0));
                ui.add(egui::Label::new(format!("{} — {}", k + 1, what)).truncate());
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.button(egui::RichText::new("🗑").size(17.0)).on_hover_text(tr("Remove this track")).clicked() { del = Some(k); }
                    if ui.button(egui::RichText::new("🔊").size(17.0)).on_hover_text(tr("Listen to this track: opens a window with a timeline and a play button")).clicked() { *listen = Some(k); }
                    if ui.button(egui::RichText::new("✏").size(17.0)).on_hover_text(tr("Edit this track's settings")).clicked() { open = !open; }
                    let cur = langs.iter().find(|l| l.id == t.lang_id).map(|l| l.name.clone());
                    egui::ComboBox::from_id_salt("langue_piste_audio").selected_text(cur.clone().unwrap_or_else(|| tr("(to choose)").to_string())).show_ui(ui, |ui| {
                        for l in langs.iter() {
                            if ui.selectable_label(t.lang_id == l.id, &l.name).clicked() { t.lang_id = l.id; t.lang = l.code.clone(); changed = true; }
                        }
                    }).response.on_hover_text(tr("Language of this track"));
                    if cur.is_none() { ui.colored_label(egui::Color32::from_rgb(230, 100, 90), tr("to choose")); }
                });
            });
            rows.push(row.response.rect);
            ui.data_mut(|d| d.insert_persisted(open_id, open));
            if open { ui.indent("reglages_piste", |ui| {
                if let AudioSource::File(f) = &mut t.source {
                    ui.horizontal(|ui| {
                        ui.label(tr("File:"));
                        changed |= ui.text_edit_singleline(f).changed();
                        if ui.button(tr("Choose…")).clicked() {
                            if let Some(p) = rfd::FileDialog::new().add_filter("Audio", &["wav", "flac", "mp3", "ac3", "eac3", "dts", "aac", "m4a", "ogg", "opus", "mka"]).pick_file() {
                                *f = clean(&p);
                                changed = true;
                            }
                        }
                    });
                    if !f.trim().is_empty() && !abs(base, f).is_file() {
                        ui.colored_label(egui::Color32::from_rgb(230, 100, 90), tr("File not found."));
                    }
                }
                ui.horizontal(|ui| {
                    ui.label(tr("Offset:"));
                    changed |= ui.add(egui::DragValue::new(&mut t.offset_ms).speed(10.0).range(-600_000..=600_000).suffix(" ms")).changed();
                });
            }); }
        });
    }
    // glisser-deposer : la piste se range a l'endroit indique par le trait bleu
    if let (Some(from), Some(pos), Some(first), Some(last)) = (egui::DragAndDrop::payload::<AudioRow>(ui.ctx()), ui.ctx().pointer_hover_pos(), rows.first(), rows.last()) {
        let from = from.0;
        let area = first.union(*last).expand(8.0);
        if area.contains(pos) && from < v.audio.len() {
            let to = rows.iter().filter(|r| r.center().y < pos.y).count();
            let y = if to < rows.len() { rows[to].top() - 1.0 } else { last.bottom() + 1.0 };
            ui.painter().hline(area.x_range(), y, egui::Stroke::new(2.5_f32, egui::Color32::from_rgb(90, 170, 255)));
            if ui.input(|i| i.pointer.any_released()) {
                let to = if to > from { to - 1 } else { to };
                if to != from { let t = v.audio.remove(from); v.audio.insert(to, t); changed = true; }
            }
        }
    }
    if let Some(k) = del { v.audio.remove(k); changed = true; }

    let full = v.audio.len() >= MAX_AUDIO_TRACKS;
    ui.add_enabled_ui(!full, |ui| {
        ui.horizontal(|ui| {
            if ui.button(tr("Add an audio file…")).clicked() {
                if let Some(p) = rfd::FileDialog::new().add_filter("Audio", &["wav", "flac", "mp3", "ac3", "eac3", "dts", "aac", "m4a", "ogg", "opus", "mka"]).pick_file() {
                    let mut t = AudioTrackCfg::from_file(&clean(&p), "fra");
                    // premiere langue du projet : le francais par defaut ; ensuite, a choisir
                    if langs.is_empty() { t.lang_id = language_for_code(langs, "fra"); }
                    v.audio.push(t);
                    changed = true;
                }
            }
            if ui.button(tr("Audio tracks of the video…")).clicked() {
                let r = match Tools::locate() {
                    Ok(t) => list_audio_streams(&t, &abs(base, &v.file)).map_err(|e| e.to_string()),
                    Err(e) => Err(e.to_string()),
                };
                probes.insert(v.file.clone(), r);
            }
        });
    });
    if full {
        ui.label(egui::RichText::new(trf!("{} tracks maximum per video.", MAX_AUDIO_TRACKS)).weak());
    }
    match probes.get(&v.file) {
        Some(Err(e)) => { ui.colored_label(egui::Color32::from_rgb(230, 100, 90), e.as_str()); }
        Some(Ok(list)) if list.is_empty() => { ui.label(egui::RichText::new(tr("No audio track in this video.")).weak()); }
        Some(Ok(list)) => {
            for s in list {
                ui.horizontal(|ui| {
                    let name = trf!("Track {} : {} {} {} {}", s.index + 1, s.codec, if s.lang.is_empty() { "?" } else { &s.lang }, if s.channels > 0 { trf!("{} channels", s.channels) } else { String::new() }, s.title);
                    ui.label(name);
                    let used = v.audio.iter().any(|t| t.source == AudioSource::Embedded(s.index));
                    if ui.add_enabled(!used && !full, egui::Button::new(tr("Add"))).clicked() {
                        let lang = if s.lang.len() == 3 { s.lang.as_str() } else { "und" };
                        let mut t = AudioTrackCfg::embedded(s.index, lang);
                        // la langue annoncee par la piste est ajoutee a la liste du projet si elle n'y est pas
                        t.lang_id = language_for_code(langs, lang);
                        v.audio.push(t);
                        changed = true;
                    }
                });
            }
        }
        None => {}
    }
    crate::help(ui, tr("Each video has its own audio languages, in the order of this list (drag ☰ to change it: track 1 is at the top). A menu button “audio language” remembers the chosen language: each video then starts on its track in that language, or on its own track if it has none (“Play all” follows the tracks of the first video). Tracks are converted to stereo AC-3. The offset corrects an advance or a delay."));
    changed
}

/// Message du fil de lecture de l'enveloppe.
enum Loaded {
    Duration(f64),
    Peaks(Vec<f32>),
    Failed(String),
}

/// Fenetre d'ecoute d'une piste audio : forme d'onde sur une frise, bouton lecture / arret, clic ou glisser pour se deplacer.
pub struct AudioPreview {
    #[allow(dead_code)]
    pub video: usize,
    #[allow(dead_code)]
    pub track: usize,
    title: String,
    file: PathBuf,
    /// numero de la piste audio du fichier (piste integree a la video) ; `None` pour un fichier audio
    stream: Option<u32>,
    duration: f64,
    peaks: Vec<f32>,
    rx: Option<Receiver<Loaded>>,
    error: Option<String>,
    tools: Option<Tools>,
    /// lecture en cours : processus, instant du depart, position de depart (s)
    play: Option<(AudioPlayback, Instant, f64)>,
    /// position quand rien ne joue
    pos: f64,
    /// position visee pendant un glisser sur la frise
    scrub: Option<f64>,
}

fn clock(t: f64) -> String {
    let s = t.max(0.0).round() as u64;
    if s >= 3600 { format!("{}:{:02}:{:02}", s / 3600, s / 60 % 60, s % 60) } else { format!("{}:{:02}", s / 60, s % 60) }
}

impl AudioPreview {
    /// `file` : fichier audio, ou video dont on ecoute la piste audio numero `stream`.
    pub fn new(video: usize, track: usize, title: String, file: &Path, stream: Option<u32>) -> AudioPreview {
        let (tx, rx) = channel();
        let src = file.to_path_buf();
        std::thread::spawn(move || {
            let tools = match Tools::locate() { Ok(t) => t, Err(e) => { let _ = tx.send(Loaded::Failed(e.to_string())); return; } };
            let dur = match probe_duration(&tools, &src) { Ok(d) => d, Err(e) => { let _ = tx.send(Loaded::Failed(e.to_string())); return; } };
            let _ = tx.send(Loaded::Duration(dur));
            match audio_peaks(&tools, &src, stream, dur, 2000) {
                Ok(p) => { let _ = tx.send(Loaded::Peaks(p)); }
                Err(e) => { let _ = tx.send(Loaded::Failed(e.to_string())); }
            }
        });
        AudioPreview { video, track, title, file: file.to_path_buf(), stream, duration: 0.0, peaks: Vec::new(), rx: Some(rx), error: None, tools: None, play: None, pos: 0.0, scrub: None }
    }

    fn poll(&mut self) {
        let Some(rx) = &self.rx else { return };
        let mut done = false;
        while let Ok(m) = rx.try_recv() {
            match m {
                Loaded::Duration(d) => self.duration = d,
                Loaded::Peaks(p) => { self.peaks = p; done = true; }
                Loaded::Failed(e) => { self.error = Some(e); done = true; }
            }
        }
        if done { self.rx = None; }
    }

    /// Position actuelle (s).
    fn current(&self) -> f64 {
        match &self.play {
            Some((_, t0, from)) => (from + t0.elapsed().as_secs_f64()).min(self.duration),
            None => self.pos,
        }
    }

    fn stop(&mut self) {
        let cur = self.current();
        if let Some((mut pb, _, _)) = self.play.take() { pb.stop(); self.pos = cur; }
    }

    fn start_at(&mut self, t: f64) {
        self.stop();
        if self.tools.is_none() {
            match Tools::locate() { Ok(t) => self.tools = Some(t), Err(e) => { self.error = Some(e.to_string()); return; } }
        }
        let Some(tools) = &self.tools else { return };
        match AudioPlayback::start(tools, &self.file, self.stream, t) {
            Ok(pb) => { self.play = Some((pb, Instant::now(), t)); self.pos = t; self.error = None; }
            Err(e) => self.error = Some(e.to_string()),
        }
    }

    /// Affiche la fenetre ; rend faux quand on la ferme.
    pub fn window(&mut self, ctx: &egui::Context) -> bool {
        let mut open = true;
        let title = trf!("Listen: {}", self.title);
        crate::os_window(ctx, "ecoute_piste_audio", &title, [900.0, 280.0], &mut open, &mut |ui| self.ui(ui));
        if !open { self.stop(); }
        open
    }

    fn ui(&mut self, ui: &mut egui::Ui) {
        self.poll();
        // fin de la piste : retour au debut
        if self.play.as_mut().map_or(false, |(pb, _, _)| pb.finished()) {
            self.play = None;
            self.pos = 0.0;
        }
        let dur = self.duration;
        let playing = self.play.is_some();
        let cur = self.scrub.unwrap_or_else(|| self.current());
        let mut toggle = false;
        ui.horizontal(|ui| {
            if ui.add_enabled(dur > 0.0, egui::Button::new(if playing { tr("⏹ Stop") } else { tr("▶ Play") })).clicked() { toggle = true; }
            ui.monospace(format!("{} / {}", clock(cur), clock(dur)));
            if self.rx.is_some() && self.error.is_none() { ui.spinner(); ui.label(egui::RichText::new(tr("Reading the waveform…")).weak()); }
        });
        if let Some(e) = &self.error { ui.colored_label(egui::Color32::from_rgb(230, 100, 90), e); }

        // frise : forme d'onde, zone deja jouee en couleur, regle des temps
        let h = (ui.available_height() - 4.0).clamp(80.0, 220.0);
        let (rect, resp) = ui.allocate_exact_size(egui::vec2(ui.available_width(), h), egui::Sense::click_and_drag());
        let vis = ui.visuals().clone();
        let painter = ui.painter_at(rect);
        painter.rect_filled(rect, 3.0, vis.extreme_bg_color);
        let ruler = 18.0;
        let wave = egui::Rect::from_min_max(rect.min, egui::pos2(rect.right(), rect.bottom() - ruler));
        let x_of = |t: f64| rect.left() + (t / dur.max(1e-6)) as f32 * rect.width();
        if dur > 0.0 {
            // regle : graduation d'au moins 70 px
            let px_per_s = rect.width() as f64 / dur;
            let step = [1.0, 2.0, 5.0, 10.0, 15.0, 30.0, 60.0, 120.0, 300.0, 600.0, 900.0, 1800.0, 3600.0].into_iter().find(|s| s * px_per_s >= 70.0).unwrap_or(7200.0);
            let mut t = 0.0;
            while t <= dur {
                let x = x_of(t);
                painter.line_segment([egui::pos2(x, rect.bottom() - ruler), egui::pos2(x, rect.bottom() - ruler + 5.0)], egui::Stroke::new(1.0_f32, vis.weak_text_color()));
                painter.text(egui::pos2(x + 3.0, rect.bottom() - 2.0), egui::Align2::LEFT_BOTTOM, clock(t), egui::FontId::proportional(11.0), vis.weak_text_color());
                t += step;
            }
        }
        if !self.peaks.is_empty() {
            let norm = self.peaks.iter().cloned().fold(0.001_f32, f32::max);
            let mid = wave.center().y;
            let played = x_of(cur);
            let n = self.peaks.len();
            let cols = (rect.width() / 2.0).max(1.0) as usize;
            for c in 0..cols {
                let x = rect.left() + (c as f32 + 0.5) * rect.width() / cols as f32;
                // plus forte valeur des tranches couvertes par cette colonne
                let (i0, i1) = (c * n / cols, ((c + 1) * n / cols).max(c * n / cols + 1).min(n));
                let p = self.peaks[i0..i1].iter().cloned().fold(0.0_f32, f32::max) / norm;
                let half = (p.sqrt() * (wave.height() / 2.0 - 2.0)).max(1.0);
                let col = if x <= played { vis.selection.bg_fill } else { vis.widgets.inactive.fg_stroke.color.gamma_multiply(0.6) };
                painter.line_segment([egui::pos2(x, mid - half), egui::pos2(x, mid + half)], egui::Stroke::new(1.5_f32, col));
            }
        }
        if dur > 0.0 {
            let x = x_of(cur);
            painter.line_segment([egui::pos2(x, rect.top()), egui::pos2(x, rect.bottom() - ruler)], egui::Stroke::new(2.0_f32, vis.selection.stroke.color));
        }
        // clic ou glisser : se deplacer (la lecture repart au relachement)
        let at = |p: egui::Pos2| (((p.x - rect.left()) / rect.width()).clamp(0.0, 1.0) as f64) * dur;
        if dur > 0.0 {
            if resp.dragged() { if let Some(p) = resp.interact_pointer_pos() { self.scrub = Some(at(p)); } }
            if resp.drag_stopped() || resp.clicked() {
                let t = resp.interact_pointer_pos().map(at).or(self.scrub).unwrap_or(self.pos);
                self.scrub = None;
                if self.play.is_some() { self.start_at(t); } else { self.pos = t; }
            }
        }
        if toggle {
            if playing { self.stop(); } else { let from = if self.pos >= dur - 0.1 { 0.0 } else { self.pos }; self.start_at(from); }
        }
        if self.play.is_some() || self.rx.is_some() { ui.ctx().request_repaint(); }
    }
}
