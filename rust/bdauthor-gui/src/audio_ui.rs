//! Panneau « Pistes audio » d'une video : pistes de la video ou fichiers audio separes, chacune avec sa langue du projet.
use crate::i18n::{tr, trf};
use crate::subs_ui::language_for_code;
use bdauthor_core::audio::{AudioFormat, AudioSource, AC3_RATES, AC3_SURROUND_KBPS, AudioTrackCfg, LpcmChannels, LPCM_RATES, MAX_AUDIO_TRACKS};
use crate::player::AudioOut;
use bdauthor_core::ffmpeg::{audio_peaks_range, audio_peaks_with, list_audio_streams, probe_duration, AudioStreamInfo, Tools};
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
        // frequence et canaux de la piste source (piste integree a la video, si la video a deja ete examinee)
        let src_stream = match &t.source {
            AudioSource::Embedded(n) => probes.get(&v.file).and_then(|r| r.as_ref().ok()).and_then(|s| s.iter().find(|x| x.index == *n)).map(|x| (x.sample_rate, x.channels)),
            AudioSource::File(_) => None,
        };
        let src_rate = src_stream.map(|s| s.0).filter(|r| *r > 0);
        let src_ch = src_stream.map(|s| s.1).filter(|c| *c > 0);
        ui.push_id(("audio", k), |ui| {
            let what = match &t.source {
                AudioSource::File(f) => Path::new(f).file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| f.clone()),
                AudioSource::Embedded(n) => trf!("embedded track no. {}", n + 1),
            };
            // ligne de la piste : titre a gauche ; a droite, la langue, les reglages, l'ecoute et le retrait
            let open_id = ui.id().with("reglages_ouverts");
            let mut open: bool = ui.data_mut(|d| d.get_persisted(open_id)).unwrap_or(false);
            let hl = ui.painter().add(egui::Shape::Noop);
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
            // ligne survolee : fond en surbrillance (dessine sous les boutons)
            if ui.rect_contains_pointer(row.response.rect) { ui.painter().set(hl, egui::Shape::rect_filled(row.response.rect.expand2(egui::vec2(3.0, 2.0)), 4.0, ui.visuals().widgets.hovered.weak_bg_fill)); }
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
                    ui.label(tr("Offset +/- (ms):"));
                    changed |= ui.add(egui::DragValue::new(&mut t.offset_ms).speed(10.0).range(-600_000..=600_000).suffix(" ms")).changed();
                });
                ui.horizontal(|ui| {
                    ui.label(tr("Convert to format:"));
                    egui::ComboBox::from_id_salt("format_piste_audio").selected_text(tr(t.format.label())).show_ui(ui, |ui| {
                        for f in AudioFormat::ALL {
                            if ui.selectable_label(t.format == f, tr(f.label())).clicked() {
                                t.format = f;
                                if !f.is_encoded_ac3() { t.bitrate = 0; }
                                if !f.is_lpcm() { t.sample_rate = 0; t.lpcm_channels = LpcmChannels::Source; }
                                changed = true;
                            }
                        }
                    }).response.on_hover_text(tr("Dolby Digital is compact. The copy keeps the source's own Dolby Digital track untouched (it must be 48 kHz, 5.1 at most, 640 kb/s at most). The DTS copy keeps a DTS, DTS-HD High Resolution or DTS-HD Master Audio track untouched (48 kHz): every player plays at least its DTS core. LPCM is lossless but heavy: about 4.6 Mb/s for 5.1 in 16 bits, 6.9 Mb/s in 24 bits; the video bitrate is lowered accordingly."));
                });
                if t.format.is_encoded_ac3() {
                    ui.horizontal(|ui| {
                        ui.label(tr("Bitrate:"));
                        let default = if t.format == AudioFormat::Ac3Surround { format!("{} kb/s ({})", AC3_SURROUND_KBPS, tr("default")) } else { tr("project default").to_string() };
                        let cur = if t.bitrate == 0 { default.clone() } else { format!("{} kb/s", t.bitrate) };
                        egui::ComboBox::from_id_salt("debit_piste_audio").selected_text(cur).show_ui(ui, |ui| {
                            if ui.selectable_label(t.bitrate == 0, &default).clicked() { t.bitrate = 0; changed = true; }
                            for r in AC3_RATES {
                                if ui.selectable_label(t.bitrate == r, format!("{r} kb/s")).clicked() { t.bitrate = r; changed = true; }
                            }
                        }).response.on_hover_text(tr("Bitrate of this Dolby Digital track. A higher rate is a little better and takes more room on the disc."));
                    });
                }
                if t.format == AudioFormat::Ac3Surround {
                    ui.label(egui::RichText::new(tr("Sources with more than 6 channels are mixed down to 5.1; a stereo source would only fill the front channels.")).weak());
                }
                if t.format.is_lpcm() {
                    ui.horizontal(|ui| {
                        ui.label(tr("Sample rate:"));
                        let khz = |r: u32| format!("{} kHz", r / 1000);
                        egui::ComboBox::from_id_salt("frequence_piste_audio").selected_text(khz(t.rate())).show_ui(ui, |ui| {
                            for r in LPCM_RATES {
                                if ui.selectable_label(t.rate() == r, khz(r)).clicked() { t.sample_rate = if r == 48_000 { 0 } else { r }; changed = true; }
                            }
                        }).response.on_hover_text(tr("96 kHz keeps the high frequencies of a high-resolution source (music, concert) and doubles the size of the track. With a 48 kHz source it gains nothing."));
                        if let Some(r) = src_rate { ui.label(egui::RichText::new(trf!("(source: {} Hz)", r)).weak()); }
                    });
                    ui.horizontal(|ui| {
                        ui.label(tr("Channels:"));
                        let stereo = matches!(t.lpcm_channels, LpcmChannels::Stereo | LpcmChannels::StereoDpl2);
                        let label = |c: LpcmChannels| match c {
                            LpcmChannels::Source => tr("as the source"),
                            LpcmChannels::Max51 => tr("5.1 at most"),
                            _ => tr("stereo"),
                        };
                        egui::ComboBox::from_id_salt("canaux_piste_audio").selected_text(label(t.lpcm_channels)).show_ui(ui, |ui| {
                            for c in [LpcmChannels::Source, LpcmChannels::Max51, LpcmChannels::Stereo] {
                                let on = if c == LpcmChannels::Stereo { stereo } else { t.lpcm_channels == c };
                                if ui.selectable_label(on, label(c)).clicked() && !on { t.lpcm_channels = c; changed = true; }
                            }
                        }).response.on_hover_text(tr("Lighter track: a 7.1 source is mixed into 5.1 (the back channels go into the surround ones), or everything into stereo (centre and surrounds added to left and right, subwoofer kept). A source that already has fewer channels is left as it is. A stereo mix is about 5 dB quieter on the dialogues than the original."));
                        if stereo {
                            let mut dpl = t.lpcm_channels == LpcmChannels::StereoDpl2;
                            if ui.checkbox(&mut dpl, tr("Dolby Pro Logic II")).on_hover_text(tr("Ordinary stereo, from which a home-cinema amplifier can rebuild a surround effect.")).changed() {
                                t.lpcm_channels = if dpl { LpcmChannels::StereoDpl2 } else { LpcmChannels::Stereo };
                                changed = true;
                            }
                        }
                        if let Some(c) = src_ch {
                            let out = t.lpcm_out_channels(c);
                            let mbps = format!("{:.1}", t.kbps(c, 0, 0) as f64 / 1000.0);
                            ui.label(egui::RichText::new(trf!("(source: {} channels → {}, {} Mb/s)", c, out, crate::i18n::num(mbps))).weak());
                        }
                    });
                    if let Some(r) = src_rate.filter(|r| t.rate() == 96_000 && *r < 96_000) {
                        ui.colored_label(egui::Color32::from_rgb(230, 170, 70), trf!("The source is at {} Hz: 96 kHz makes the track heavier without improving it.", r));
                    }
                    ui.colored_label(egui::Color32::from_rgb(230, 170, 70), tr("Lossless audio: not every player accepts it; test the disc on yours."));
                }
                if t.format == AudioFormat::DtsCopy {
                    ui.colored_label(egui::Color32::from_rgb(230, 170, 70), tr("DTS-HD is written as on commercial discs (DTS core plus HD extension); test the disc on your player."));
                }
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
    /// avancement de la lecture de la forme d'onde (0 a 1)
    Progress(f32),
    Peaks(Vec<f32>),
    Failed(String),
}

/// Formes d'onde deja calculees pendant la session (fichier, piste, taille et date du fichier) : rouvrir la meme piste est immediat.
type PeaksKey = (PathBuf, Option<u32>, u64, Option<std::time::SystemTime>);
static PEAKS_CACHE: std::sync::OnceLock<std::sync::Mutex<HashMap<PeaksKey, (f64, Vec<f32>)>>> = std::sync::OnceLock::new();

fn peaks_key(file: &Path, stream: Option<u32>) -> Option<PeaksKey> {
    let m = std::fs::metadata(file).ok()?;
    Some((file.to_path_buf(), stream, m.len(), m.modified().ok()))
}

fn peaks_cache() -> std::sync::MutexGuard<'static, HashMap<PeaksKey, (f64, Vec<f32>)>> {
    PEAKS_CACHE.get_or_init(Default::default).lock().unwrap_or_else(|e| e.into_inner())
}

/// Enveloppe fine d'une portion de la piste, calculee pour le zoom.
struct Detail {
    start: f64,
    len: f64,
    peaks: Vec<f32>,
}

/// Portion visible la plus courte (s).
const MIN_VIEW: f64 = 1.0;
/// Nombre de valeurs de l'enveloppe de la piste entiere.
const FULL_BINS: usize = 2000;

/// Lecture en cours de la fenetre d'ecoute.
struct Playing {
    out: AudioOut,
    /// position de depart (s) et moment du depart
    from: f64,
    t0: Instant,
    paused: bool,
}

/// Fenetre d'ecoute d'une piste audio : forme d'onde sur une frise, lecture / pause / arret, clic ou glisser pour se deplacer.
/// Le son passe par la sortie audio de l'application (comme les apercus video) : la position affichee est celle du son entendu.
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
    /// avancement du calcul de la forme d'onde (0 a 1)
    progress: f32,
    rx: Option<Receiver<Loaded>>,
    error: Option<String>,
    tools: Option<Tools>,
    play: Option<Playing>,
    /// position quand rien ne joue
    pos: f64,
    /// position visee pendant un glisser sur la frise
    scrub: Option<f64>,
    /// portion visible de la frise (debut, duree en s) ; duree 0 : toute la piste
    view: (f64, f64),
    /// enveloppe fine de la portion zoomee, calcul en cours, portion voulue (debut, duree, valeurs) et depuis quand
    detail: Option<Detail>,
    detail_rx: Option<Receiver<Option<Detail>>>,
    detail_want: Option<((f64, f64, usize), Instant)>,
    /// position au dernier affichage (suivi de la lecture quand on est zoome)
    last_cur: f64,
    /// niveaux du son joue
    vu: crate::vumeter::VuMeter,
}

/// Heure avec les dixiemes si `tenths`.
fn clock_fine(t: f64, tenths: bool) -> String {
    if !tenths { return clock(t); }
    let d = (t.max(0.0) * 10.0).round() as u64;
    let s = d / 10;
    if s >= 3600 { format!("{}:{:02}:{:02}.{}", s / 3600, s / 60 % 60, s % 60, d % 10) } else { format!("{}:{:02}.{}", s / 60, s % 60, d % 10) }
}

/// Duree lisible : « 4.2 s », « 3 min 20 s ».
fn span(t: f64) -> String {
    if t < 10.0 { format!("{:.1} s", t) } else if t < 60.0 { format!("{:.0} s", t) } else if t < 3600.0 {
        format!("{} min {:02} s", (t / 60.0) as u64, (t % 60.0) as u64)
    } else {
        format!("{} h {:02} min", (t / 3600.0) as u64, (t / 60.0 % 60.0) as u64)
    }
}

fn clock(t: f64) -> String {
    let s = t.max(0.0).round() as u64;
    if s >= 3600 { format!("{}:{:02}:{:02}", s / 3600, s / 60 % 60, s % 60) } else { format!("{}:{:02}", s / 60, s % 60) }
}

impl AudioPreview {
    /// `file` : fichier audio, ou video dont on ecoute la piste audio numero `stream`.
    pub fn new(video: usize, track: usize, title: String, file: &Path, stream: Option<u32>) -> AudioPreview {
        let key = peaks_key(file, stream);
        let mut me = AudioPreview {
            video, track, title, file: file.to_path_buf(), stream, duration: 0.0, peaks: Vec::new(), progress: 0.0, rx: None, error: None,
            tools: None, play: None, pos: 0.0, scrub: None, view: (0.0, 0.0), detail: None, detail_rx: None, detail_want: None, last_cur: 0.0,
            vu: Default::default(),
        };
        if let Some((d, p)) = key.as_ref().and_then(|k| peaks_cache().get(k).cloned()) {
            (me.duration, me.peaks, me.progress) = (d, p, 1.0);
            return me;
        }
        let (tx, rx) = channel();
        let src = file.to_path_buf();
        std::thread::spawn(move || {
            let tools = match Tools::locate() { Ok(t) => t, Err(e) => { let _ = tx.send(Loaded::Failed(e.to_string())); return; } };
            let dur = match probe_duration(&tools, &src) { Ok(d) => d, Err(e) => { let _ = tx.send(Loaded::Failed(e.to_string())); return; } };
            let _ = tx.send(Loaded::Duration(dur));
            // avancement par pour-cent ; fenetre fermee (envoi impossible) : le calcul s'arrete
            let mut last = -1i32;
            let mut report = |f: f32| {
                let pct = (f * 100.0) as i32;
                if pct == last { return true; }
                last = pct;
                tx.send(Loaded::Progress(f)).is_ok()
            };
            let r = audio_peaks_with(&tools, &src, stream, dur, FULL_BINS, &mut report);
            match r {
                Ok(p) => {
                    if let Some(k) = key { peaks_cache().insert(k, (dur, p.clone())); }
                    let _ = tx.send(Loaded::Peaks(p));
                }
                Err(bdauthor_core::ffmpeg::Error::Cancelled) => {}
                Err(e) => { let _ = tx.send(Loaded::Failed(e.to_string())); }
            }
        });
        me.rx = Some(rx);
        me
    }

    fn poll(&mut self) {
        let Some(rx) = &self.rx else { return };
        let mut done = false;
        while let Ok(m) = rx.try_recv() {
            match m {
                Loaded::Duration(d) => self.duration = d,
                Loaded::Progress(f) => self.progress = f,
                Loaded::Peaks(p) => { self.peaks = p; done = true; }
                Loaded::Failed(e) => { self.error = Some(e); done = true; }
            }
        }
        if done { self.rx = None; }
    }

    /// Position actuelle (s) : celle du son entendu.
    fn current(&self) -> f64 {
        match &self.play {
            Some(p) => (p.from + p.out.position()).min(self.duration.max(p.from)),
            None => self.pos,
        }
    }

    /// Arrete le son en gardant la position.
    fn stop(&mut self) {
        let cur = self.current();
        if self.play.take().is_some() { self.pos = cur; }
    }

    fn start_at(&mut self, t: f64) {
        self.stop();
        if self.tools.is_none() {
            match Tools::locate() { Ok(t) => self.tools = Some(t), Err(e) => { self.error = Some(e.to_string()); return; } }
        }
        let Some(tools) = &self.tools else { return };
        match AudioOut::start(tools, &self.file, self.stream, t) {
            Ok(out) => { self.play = Some(Playing { out, from: t, t0: Instant::now(), paused: false }); self.pos = t; self.error = None; }
            Err(e) => self.error = Some(e.to_string()),
        }
    }

    /// Lecture / pause : la pause fige la sortie audio (reprise immediate) ; si la sortie ne sait pas, le son s'arrete et
    /// repartira de la meme position.
    fn toggle(&mut self) {
        match &mut self.play {
            Some(p) if !p.paused => {
                if p.out.pause() { p.paused = true; } else { self.stop(); }
            }
            Some(p) => {
                if p.out.resume() { p.paused = false; } else { let t = self.current(); self.start_at(t); }
            }
            None => {
                let from = if self.pos >= self.duration - 0.1 { 0.0 } else { self.pos };
                self.start_at(from);
            }
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

    /// Portion visible (debut, duree) ; toute la piste quand on n'est pas zoome.
    fn visible(&self) -> (f64, f64) {
        let dur = self.duration;
        let (s, l) = self.view;
        if l <= 0.0 || l >= dur { (0.0, dur) } else { (s.clamp(0.0, dur - l), l) }
    }

    /// Change la portion visible (bornee a la piste, au moins `MIN_VIEW`).
    fn set_view(&mut self, start: f64, len: f64) {
        let dur = self.duration;
        if dur <= 0.0 { return; }
        let len = len.clamp(MIN_VIEW.min(dur), dur);
        self.view = if len >= dur { (0.0, 0.0) } else { (start.clamp(0.0, dur - len), len) };
    }

    /// Zoom d'un facteur `f` (< 1 : on agrandit) en gardant l'instant `anchor` au meme endroit.
    fn zoom_at(&mut self, f: f64, anchor: f64) {
        let (s, l) = self.visible();
        let nl = l * f;
        self.set_view(anchor - (anchor - s) * f, nl);
    }

    /// Enveloppe fine : demandee quand l'enveloppe de la piste entiere est trop grossiere pour la portion visible (`cols`
    /// colonnes), calculee un peu apres le dernier changement de zoom, sur la portion visible elargie de moitie de chaque cote
    /// (un petit deplacement ne la refait pas).
    fn update_detail(&mut self, cols: usize) {
        if let Some(rx) = &self.detail_rx {
            match rx.try_recv() {
                Ok(d) => { if d.is_some() { self.detail = d; } self.detail_rx = None; }
                Err(std::sync::mpsc::TryRecvError::Disconnected) => self.detail_rx = None,
                Err(_) => {}
            }
        }
        let (vs, vl) = self.visible();
        let dur = self.duration;
        if dur <= 0.0 || vl >= dur { self.detail_want = None; return; }
        let col = vl / cols.max(1) as f64;
        let full = if self.peaks.is_empty() { f64::INFINITY } else { dur / self.peaks.len() as f64 };
        if col >= full * 0.75 { self.detail_want = None; return; }
        let fine_enough = self.detail.as_ref().is_some_and(|d| {
            d.start <= vs + 1e-6 && d.start + d.len >= vs + vl - 1e-6 && d.len / d.peaks.len().max(1) as f64 <= col * 1.5
        });
        if fine_enough { self.detail_want = None; return; }
        let start = (vs - vl * 0.5).max(0.0);
        let len = (vs + vl * 1.5).min(dur) - start;
        let bins = ((len / col) as usize * 2).clamp(1, 40_000);
        let want = (start, len, bins);
        if self.detail_want.as_ref().map(|w| w.0) != Some(want) { self.detail_want = Some((want, Instant::now())); }
        let Some((want, since)) = self.detail_want else { return };
        if self.detail_rx.is_some() || since.elapsed().as_millis() < 150 { return; }
        let Ok(tools) = Tools::locate() else { return };
        let (tx, rx) = channel();
        let (src, stream) = (self.file.clone(), self.stream);
        std::thread::spawn(move || {
            let r = audio_peaks_range(&tools, &src, stream, want.0, want.1, want.2).ok().map(|p| Detail { start: want.0, len: want.1, peaks: p });
            let _ = tx.send(r);
        });
        self.detail_rx = Some(rx);
        self.detail_want = None;
    }

    /// Plus forte valeur de l'enveloppe entre `t0` et `t1` : enveloppe fine si elle couvre l'intervalle, sinon celle de la piste.
    fn peak(&self, t0: f64, t1: f64) -> Option<f32> {
        let max_of = |p: &[f32], start: f64, len: f64| -> f32 {
            let n = p.len();
            let i0 = (((t0 - start) / len * n as f64).floor().max(0.0) as usize).min(n - 1);
            let i1 = (((t1 - start) / len * n as f64).ceil() as usize).clamp(i0 + 1, n);
            p[i0..i1].iter().cloned().fold(0.0_f32, f32::max)
        };
        if let Some(d) = &self.detail {
            if !d.peaks.is_empty() && d.start <= t0 && d.start + d.len >= t1 && d.len / d.peaks.len() as f64 <= (t1 - t0) * 1.5 {
                return Some(max_of(&d.peaks, d.start, d.len));
            }
        }
        if self.peaks.is_empty() || self.duration <= 0.0 { return None; }
        Some(max_of(&self.peaks, 0.0, self.duration))
    }

    fn ui(&mut self, ui: &mut egui::Ui) {
        self.poll();
        // fin de la piste : retour au debut ; sortie audio muette (peripherique en defaut) : arret avec un message
        if self.play.as_ref().is_some_and(|p| p.out.finished()) {
            self.play = None;
            self.pos = 0.0;
        } else if self.play.as_ref().is_some_and(|p| !p.paused && !p.out.started() && p.t0.elapsed().as_millis() > 1500) {
            self.play = None;
            self.error = Some(tr("the audio output does not respond").to_string());
        }
        let dur = self.duration;
        let running = self.play.as_ref().is_some_and(|p| !p.paused);
        let cur = self.scrub.unwrap_or_else(|| self.current());
        // zoome et en lecture : la frise suit le son quand il sort de la portion visible
        let (vs, vl) = self.visible();
        if running && self.scrub.is_none() && vl < dur && self.last_cur >= vs && self.last_cur <= vs + vl && cur > vs + vl {
            self.set_view(cur - vl * 0.05, vl);
        }
        self.last_cur = cur;
        let (mut toggle, mut rewind) = (false, false);
        let mut zoom: Option<f64> = None;
        let zoomed = self.visible().1 < dur;
        ui.horizontal(|ui| {
            if ui.add_enabled(dur > 0.0, egui::Button::new(if running { tr("⏸ Pause") } else { tr("▶ Play") })).clicked() { toggle = true; }
            if ui.add_enabled(self.play.is_some() || self.pos > 0.0, egui::Button::new(tr("⏹ Stop"))).on_hover_text(tr("Stop and go back to the start")).clicked() { rewind = true; }
            ui.monospace(format!("{} / {}", clock_fine(cur, zoomed && self.visible().1 < 60.0), clock(dur)));
            ui.separator();
            if ui.add_enabled(dur > 0.0, egui::Button::new("🔍+")).on_hover_text(tr("Zoom in (mouse wheel on the waveform)")).clicked() { zoom = Some(0.5); }
            if ui.add_enabled(zoomed, egui::Button::new("🔍−")).on_hover_text(tr("Zoom out")).clicked() { zoom = Some(2.0); }
            if ui.add_enabled(zoomed, egui::Button::new(tr("Whole track"))).clicked() { zoom = Some(f64::INFINITY); }
            if zoomed {
                ui.label(egui::RichText::new(trf!("{} visible", span(self.visible().1))).weak())
                    .on_hover_text(tr("Shift + wheel, or horizontal scroll: move along the track"));
            }
            if self.detail_rx.is_some() || self.detail_want.is_some() { ui.spinner(); }
            if self.rx.is_some() && self.error.is_none() {
                let pct = (self.progress * 100.0).round() as u32;
                ui.add(egui::ProgressBar::new(self.progress).desired_width(260.0).animate(true).text(format!("{} {} %", tr("Reading the waveform…"), pct)))
                    .on_hover_text(tr("The waveform is read from the whole track; you can already play it."));
            }
        });
        if let Some(e) = &self.error { ui.colored_label(egui::Color32::from_rgb(230, 100, 90), e); }
        // vumetre : cretes du son tel qu'il part vers la sortie audio (pistes multicanal reduites aux canaux de la sortie)
        let peaks = self.play.as_ref().map(|p| p.out.levels()).unwrap_or_default();
        self.vu.update(&peaks);
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new(tr("Level (dBFS)")).weak())
                .on_hover_text(tr("Peak level of the sound sent to the audio output, in decibels below full scale. The thin mark holds the peak; the red light shows saturation. A multichannel track is shown with the channels of your audio output."));
            let w = (ui.available_width() - 4.0).min(560.0);
            self.vu.ui(ui, w);
        });
        // boutons : zoom autour de la position de lecture si elle est visible, sinon autour du milieu
        if let Some(f) = zoom {
            if f.is_infinite() { self.view = (0.0, 0.0); } else {
                let (vs, vl) = self.visible();
                let anchor = if cur >= vs && cur <= vs + vl { cur } else { vs + vl / 2.0 };
                self.zoom_at(f, anchor);
            }
        }

        // frise : forme d'onde, zone deja jouee en couleur, regle des temps
        let h = (ui.available_height() - 4.0).clamp(80.0, 220.0);
        let (rect, resp) = ui.allocate_exact_size(egui::vec2(ui.available_width(), h), egui::Sense::click_and_drag());
        // molette : zoom autour du pointeur ; Maj + molette ou defilement horizontal : deplacement
        if resp.hovered() && dur > 0.0 {
            let (d, shift) = ui.input(|i| (i.smooth_scroll_delta, i.modifiers.shift));
            let (vs, vl) = self.visible();
            let (dx, dy) = if shift && d.x == 0.0 { (d.y, 0.0) } else { (d.x, d.y) };
            if dy != 0.0 {
                let anchor = ui.input(|i| i.pointer.hover_pos()).map_or(vs + vl / 2.0, |p| vs + (((p.x - rect.left()) / rect.width()).clamp(0.0, 1.0) as f64) * vl);
                self.zoom_at((-dy as f64 / 200.0).exp(), anchor);
            }
            if dx != 0.0 && vl < dur { self.set_view(vs - dx as f64 / rect.width() as f64 * vl, vl); }
            if dx != 0.0 || dy != 0.0 { ui.input_mut(|i| i.smooth_scroll_delta = egui::Vec2::ZERO); }
        }
        let cols = (rect.width() / 2.0).max(1.0) as usize;
        self.update_detail(cols);
        let (vs, vl) = self.visible();
        let vis = ui.visuals().clone();
        let painter = ui.painter_at(rect);
        painter.rect_filled(rect, 3.0, vis.extreme_bg_color);
        let ruler = 18.0;
        let wave = egui::Rect::from_min_max(rect.min, egui::pos2(rect.right(), rect.bottom() - ruler));
        let x_of = |t: f64| rect.left() + ((t - vs) / vl.max(1e-6)) as f32 * rect.width();
        if dur > 0.0 {
            // regle : graduation d'au moins 70 px
            let px_per_s = rect.width() as f64 / vl.max(1e-6);
            let step = [0.1, 0.2, 0.5, 1.0, 2.0, 5.0, 10.0, 15.0, 30.0, 60.0, 120.0, 300.0, 600.0, 900.0, 1800.0, 3600.0]
                .into_iter().find(|s| s * px_per_s >= 70.0).unwrap_or(7200.0);
            let mut t = (vs / step).ceil() * step;
            while t <= vs + vl + 1e-9 {
                let x = x_of(t);
                painter.line_segment([egui::pos2(x, rect.bottom() - ruler), egui::pos2(x, rect.bottom() - ruler + 5.0)], egui::Stroke::new(1.0_f32, vis.weak_text_color()));
                painter.text(egui::pos2(x + 3.0, rect.bottom() - 2.0), egui::Align2::LEFT_BOTTOM, clock_fine(t, step < 1.0), egui::FontId::proportional(11.0), vis.weak_text_color());
                t += step;
            }
        }
        // hauteur : relative au plus fort de la piste entiere (le zoom ne change pas l'echelle)
        let norm = if self.peaks.is_empty() {
            self.detail.as_ref().map_or(0.001, |d| d.peaks.iter().cloned().fold(0.001_f32, f32::max))
        } else {
            self.peaks.iter().cloned().fold(0.001_f32, f32::max)
        };
        if dur > 0.0 {
            let mid = wave.center().y;
            let played = x_of(cur);
            for c in 0..cols {
                let (t0, t1) = (vs + c as f64 * vl / cols as f64, vs + (c + 1) as f64 * vl / cols as f64);
                let Some(p) = self.peak(t0, t1) else { break };
                let x = rect.left() + (c as f32 + 0.5) * rect.width() / cols as f32;
                let half = ((p / norm).min(1.0).sqrt() * (wave.height() / 2.0 - 2.0)).max(1.0);
                let col = if x <= played { vis.selection.bg_fill } else { vis.widgets.inactive.fg_stroke.color.gamma_multiply(0.6) };
                painter.line_segment([egui::pos2(x, mid - half), egui::pos2(x, mid + half)], egui::Stroke::new(1.5_f32, col));
            }
            if cur >= vs && cur <= vs + vl {
                let x = x_of(cur);
                painter.line_segment([egui::pos2(x, rect.top()), egui::pos2(x, rect.bottom() - ruler)], egui::Stroke::new(2.0_f32, vis.selection.stroke.color));
            }
        }
        // zoome : petite barre en haut qui situe la portion visible dans la piste
        if dur > 0.0 && vl < dur {
            let bar = egui::Rect::from_min_size(rect.min + egui::vec2(0.0, 1.0), egui::vec2(rect.width(), 3.0));
            painter.rect_filled(bar, 1.0, vis.widgets.inactive.bg_fill);
            let a = rect.left() + (vs / dur) as f32 * rect.width();
            let b = (rect.left() + ((vs + vl) / dur) as f32 * rect.width()).max(a + 3.0);
            painter.rect_filled(egui::Rect::from_min_max(egui::pos2(a, bar.top()), egui::pos2(b, bar.bottom())), 1.0, vis.selection.stroke.color);
        }
        // clic ou glisser : se deplacer (la lecture repart au relachement)
        let at = |p: egui::Pos2| vs + (((p.x - rect.left()) / rect.width()).clamp(0.0, 1.0) as f64) * vl;
        if dur > 0.0 {
            if resp.dragged() { if let Some(p) = resp.interact_pointer_pos() { self.scrub = Some(at(p)); } }
            if resp.drag_stopped() || resp.clicked() {
                let t = resp.interact_pointer_pos().map(at).or(self.scrub).unwrap_or(self.pos);
                self.scrub = None;
                // en lecture : le son repart de la; en pause ou a l'arret : la position change, Lecture repartira de la
                if running { self.start_at(t); } else { self.play = None; self.pos = t; }
            }
        }
        if toggle { self.toggle(); }
        if rewind { self.play = None; self.pos = 0.0; }
        if running || self.vu.active() || self.rx.is_some() || self.detail_rx.is_some() || self.detail_want.is_some() { ui.ctx().request_repaint(); }
    }
}
