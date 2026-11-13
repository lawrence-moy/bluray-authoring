//! Lecteur commun des apercus video (choix d'une image, chapitres, sous-titres) : images calees sur le son, joue par la sortie
//! audio de l'application (cpal). Le son donne le temps ; sans son (muet, piste absente, pas de sortie audio), le temps ecoule.
use crate::i18n::{tr, trf};
use bdauthor_core::ffmpeg::{list_audio_streams, FrameStream, Tools};
use bdauthor_core::palette::RgbaImage;
use bdauthor_core::playback::{PcmFeed, PcmSource, PlayClock};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use eframe::egui;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicI32, Ordering};
use std::sync::mpsc::TryRecvError;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// Son des apercus : -1 muet, sinon numero de la piste audio (0 = la premiere). Commun a toutes les fenetres, pour la session.
static SOUND: AtomicI32 = AtomicI32::new(0);

fn sound_choice() -> Option<u32> {
    let v = SOUND.load(Ordering::Relaxed);
    (v >= 0).then_some(v as u32)
}

/// Son en cours : flux decode par ffmpeg et sortie audio qui le joue.
struct AudioOut {
    feed: PcmFeed,
    _stream: cpal::Stream,
}

impl AudioOut {
    fn start(tools: &Tools, file: &Path, track: u32, t: f64) -> Result<AudioOut, String> {
        let host = cpal::default_host();
        let dev = host.default_output_device().ok_or_else(|| tr("no audio output found").to_string())?;
        let sup = dev.default_output_config().map_err(|e| e.to_string())?;
        let (rate, ch) = (sup.sample_rate().0, sup.channels());
        let feed = PcmFeed::start(tools, file, Some(track), t, rate, ch as u32).map_err(|e| e.to_string())?;
        let cfg: cpal::StreamConfig = sup.config();
        let src = feed.source();
        let stream = match sup.sample_format() {
            cpal::SampleFormat::F32 => build::<f32>(&dev, &cfg, src),
            cpal::SampleFormat::I16 => build::<i16>(&dev, &cfg, src),
            cpal::SampleFormat::U16 => build::<u16>(&dev, &cfg, src),
            cpal::SampleFormat::I32 => build::<i32>(&dev, &cfg, src),
            f => return Err(trf!("audio output format not supported ({})", format!("{f:?}"))),
        }
        .map_err(|e| e.to_string())?;
        stream.play().map_err(|e| e.to_string())?;
        Ok(AudioOut { feed, _stream: stream })
    }
}

fn build<T>(dev: &cpal::Device, cfg: &cpal::StreamConfig, src: PcmSource) -> Result<cpal::Stream, cpal::BuildStreamError>
where
    T: cpal::SizedSample + cpal::FromSample<f32>,
{
    let mut tmp: Vec<f32> = Vec::new();
    dev.build_output_stream(
        cfg,
        move |data: &mut [T], info: &cpal::OutputCallbackInfo| {
            let ts = info.timestamp();
            if let Some(d) = ts.playback.duration_since(&ts.callback) { src.set_latency(d); }
            tmp.resize(data.len(), 0.0);
            src.fill(&mut tmp);
            for (o, s) in data.iter_mut().zip(tmp.iter()) { *o = <T as cpal::Sample>::from_sample(*s); }
        },
        |e| eprintln!("sortie audio : {e}"),
        None,
    )
}

/// Une image de la lecture et son instant dans la video (secondes).
pub struct Shown {
    pub img: RgbaImage,
    pub t: f64,
}

/// Lecture d'une video avec son. Arretee a la destruction.
pub struct Player {
    file: PathBuf,
    size: (u32, u32),
    cover: bool,
    fps: f64,
    frames: FrameStream,
    /// instant de la premiere image du flux d'images, et images recues depuis
    f_start: f64,
    got: u64,
    pending: Option<RgbaImage>,
    frames_done: bool,
    last_frame_t: f64,
    last_resync: Instant,
    audio: Option<AudioOut>,
    audio_t0: Instant,
    track: Option<u32>,
    clock: PlayClock,
    note: Option<String>,
}

impl Player {
    /// Lit `file` a partir de `t` secondes, en images RGBA de `size` (`cover` : l'image remplit `size`, l'excedent est rogne).
    pub fn start(file: &Path, t: f64, size: (u32, u32), cover: bool, fps: f64) -> Result<Player, String> {
        let tools = Tools::locate().map_err(|e| e.to_string())?;
        let frames = FrameStream::start_paced(&tools, file, t, size, cover, false).map_err(|e| e.to_string())?;
        let mut p = Player {
            file: file.to_path_buf(), size, cover, fps: if fps > 1.0 { fps } else { 25.0 },
            frames, f_start: t, got: 0, pending: None, frames_done: false, last_frame_t: t, last_resync: Instant::now(),
            audio: None, audio_t0: Instant::now(), track: None, clock: PlayClock::new(t), note: None,
        };
        p.open_audio(&tools, t);
        Ok(p)
    }

    fn open_audio(&mut self, tools: &Tools, t: f64) {
        self.audio = None;
        self.track = sound_choice();
        self.note = None;
        if let Some(n) = self.track {
            match AudioOut::start(tools, &self.file, n, t) {
                Ok(a) => self.audio = Some(a),
                Err(e) => self.note = Some(trf!("without sound: {}", e)),
            }
        }
        self.clock.restart_audio(t);
        self.audio_t0 = Instant::now();
    }

    /// Position du son en cours, `None` s'il n'y en a pas (ou plus).
    fn audio_pos(&mut self) -> Option<f64> {
        let (finished, started, pos) = match &self.audio { Some(a) => (a.feed.finished(), a.feed.started(), a.feed.position()), None => return None };
        if finished {
            self.audio = None;
            return None;
        }
        // une sortie qui ne tire aucun son (peripherique en defaut) ne doit pas bloquer l'image
        if !started && self.audio_t0.elapsed() > Duration::from_millis(1500) {
            self.audio = None;
            self.note = Some(tr("without sound: the audio output does not respond").to_string());
            return None;
        }
        Some(pos)
    }

    /// A appeler a chaque image de l'interface : la derniere image due (s'il y en a une nouvelle) et vrai a la fin de la video.
    pub fn poll(&mut self) -> (Option<Shown>, bool) {
        // son change (ici ou dans une autre fenetre) : le nouveau son reprend a l'instant affiche
        if sound_choice() != self.track {
            if let Ok(tools) = Tools::locate() {
                let t = self.clock.last();
                self.open_audio(&tools, t);
            }
        }
        let pos = self.audio_pos();
        let now = self.clock.time(pos, Instant::now());
        let half = 0.5 / self.fps;
        let mut shown = None;
        loop {
            if self.pending.is_none() {
                match self.frames.rx.try_recv() {
                    Ok(img) => { self.got += 1; self.pending = Some(img); }
                    Err(TryRecvError::Empty) => break,
                    Err(TryRecvError::Disconnected) => { self.frames_done = true; break; }
                }
            }
            let ft = self.f_start + (self.got - 1) as f64 / self.fps;
            if ft > now + half { break; }
            self.last_frame_t = ft;
            shown = self.pending.take().map(|img| Shown { img, t: ft });
        }
        // images en retard (decodage plus lent que la lecture) : l'image reprend a l'instant du son
        if !self.frames_done && self.pending.is_none() && now - self.last_frame_t > 0.5 && self.last_resync.elapsed() > Duration::from_secs(2) {
            if let Ok(tools) = Tools::locate() {
                let at = now + 0.2;
                if let Ok(fs) = FrameStream::start_paced(&tools, &self.file, at, self.size, self.cover, false) {
                    (self.frames, self.f_start, self.got, self.last_frame_t) = (fs, at, 0, at);
                }
            }
            self.last_resync = Instant::now();
        }
        (shown, self.frames_done && self.pending.is_none())
    }

    /// Instant de la lecture (secondes).
    pub fn time(&self) -> f64 { self.clock.last() }

    /// Pourquoi la lecture est sans son, le cas echeant.
    pub fn note(&self) -> Option<&str> { self.note.as_deref() }
}

/// Liste des pistes audio d'un fichier, lue en arriere-plan (une fois par fichier et par session).
fn audio_tracks(ctx: &egui::Context, file: &Path) -> Option<Vec<String>> {
    let id = egui::Id::new(("pistes_son_apercu", file));
    let slot: Option<Arc<Mutex<Option<Vec<String>>>>> = ctx.data(|d| d.get_temp(id));
    let slot = slot.unwrap_or_else(|| {
        let s = Arc::new(Mutex::new(None));
        let (s2, f, c2) = (s.clone(), file.to_path_buf(), ctx.clone());
        std::thread::spawn(move || {
            let v = Tools::locate().ok().and_then(|t| list_audio_streams(&t, &f).ok()).unwrap_or_default();
            let labels: Vec<String> = v.iter().map(|a| {
                let mut l = trf!("Track {}", a.index + 1);
                for part in [a.lang.as_str(), a.codec.as_str()] { if !part.is_empty() && part != "und" { l.push_str(" · "); l.push_str(part); } }
                if a.channels > 0 { l.push_str(&format!(" · {} ch", a.channels)); }
                if !a.title.is_empty() { l.push_str(&format!(" · {}", a.title)); }
                l
            }).collect();
            *s2.lock().unwrap() = Some(labels);
            c2.request_repaint();
        });
        ctx.data_mut(|d| d.insert_temp(id, s.clone()));
        s
    });
    let v = slot.lock().unwrap().clone();
    v
}

/// Choix du son des apercus (muet ou une piste de `file`), a placer a cote du bouton de lecture ; le choix vaut pour toutes les
/// fenetres et s'applique tout de suite a la lecture en cours.
pub fn sound_menu(ui: &mut egui::Ui, file: &Path, player: Option<&Player>) {
    let tracks = audio_tracks(ui.ctx(), file);
    let cur = SOUND.load(Ordering::Relaxed);
    let name = |k: i32| -> String {
        if k < 0 { return tr("Mute").to_string(); }
        tracks.as_ref().and_then(|t| t.get(k as usize).cloned()).unwrap_or_else(|| trf!("Track {}", k + 1))
    };
    ui.label("🔊");
    egui::ComboBox::from_id_salt(("son_apercu", file)).selected_text(name(cur)).width(170.0).show_ui(ui, |ui| {
        if ui.selectable_label(cur < 0, tr("Mute")).clicked() { SOUND.store(-1, Ordering::Relaxed); }
        let n = tracks.as_ref().map_or(1, |t| t.len());
        for k in 0..n as i32 {
            if ui.selectable_label(cur == k, name(k)).clicked() { SOUND.store(k, Ordering::Relaxed); }
        }
    })
    .response
    .on_hover_text(tr("Sound of the preview: mute, or one of the video's audio tracks. The image follows the sound."));
    if tracks.as_ref().is_some_and(|t| t.is_empty()) {
        ui.label(egui::RichText::new(tr("(no audio track)")).small().weak());
    } else if let Some(n) = player.and_then(|p| p.note()) {
        ui.label(egui::RichText::new(n).small().color(egui::Color32::from_rgb(230, 170, 70)));
    }
}
