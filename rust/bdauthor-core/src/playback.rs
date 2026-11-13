//! Lecture synchronisee des apercus : son decode par ffmpeg pour une sortie audio de l'application, et horloge de lecture.
//!
//! Le son donne le temps : la sortie audio compte les echantillons reellement joues, et l'image affichee est celle de cet instant.
//! Sans son (muet, pas de piste, pas de sortie audio, fin de la piste), l'horloge continue sur le temps ecoule, sans saut.

use crate::ffmpeg::{no_window, Tools};
use crate::ffmpeg::Result;
use std::collections::VecDeque;
use std::io::Read;
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

struct Inner {
    buf: Mutex<VecDeque<f32>>,
    channels: usize,
    rate: u32,
    /// images d'echantillons (un echantillon par canal) donnees a la sortie
    played: AtomicU64,
    /// delai entre le remplissage et l'ecoute (microsecondes), selon la sortie audio
    latency_us: AtomicU64,
    eof: AtomicBool,
    stop: AtomicBool,
}

/// Cote « sortie audio » d'un flux : a donner au fil de la carte son. Se clone librement.
#[derive(Clone)]
pub struct PcmSource(Arc<Inner>);

impl PcmSource {
    /// Remplit `out` (echantillons entrelaces) avec la suite du son ; ce qui manque (flux en retard, fin) est du silence.
    pub fn fill(&self, out: &mut [f32]) {
        let ch = self.0.channels.max(1);
        let n = {
            let mut b = self.0.buf.lock().unwrap();
            let n = out.len().min(b.len()) / ch * ch;
            for (o, s) in out.iter_mut().zip(b.drain(..n)) { *o = s; }
            n
        };
        for o in &mut out[n..] { *o = 0.0; }
        self.0.played.fetch_add((n / ch) as u64, Ordering::Relaxed);
    }

    /// Delai de la sortie audio : ce qui vient d'etre donne sera entendu dans `d`.
    pub fn set_latency(&self, d: Duration) {
        self.0.latency_us.store(d.as_micros().min(2_000_000) as u64, Ordering::Relaxed);
    }
}

/// Son d'une piste decode par ffmpeg en echantillons flottants entrelaces, au format de la sortie audio (`rate`, `channels`).
/// Arrete a la destruction.
pub struct PcmFeed {
    child: Child,
    src: PcmSource,
}

/// Duree tamponnee en avance (secondes) : assez pour ne pas manquer, assez peu pour suivre une pause tout de suite.
const AHEAD_S: f64 = 0.4;

impl PcmFeed {
    /// `track` : numero parmi les pistes audio (0 = la premiere) ; `None` : celle que ffmpeg choisit.
    pub fn start(tools: &Tools, src: &Path, track: Option<u32>, t: f64, rate: u32, channels: u32) -> Result<PcmFeed> {
        let mut c = Command::new(&tools.ffmpeg);
        c.args(["-v", "error", "-nostdin", "-ss", &format!("{:.4}", t.max(0.0)), "-i"]).arg(src);
        if let Some(n) = track { c.args(["-map", &format!("0:a:{n}")]); }
        c.args(["-vn", "-sn", "-dn", "-ac", &channels.max(1).to_string(), "-ar", &rate.max(8000).to_string(), "-f", "f32le", "pipe:1"]);
        c.stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::null());
        no_window(&mut c);
        let mut child = c.spawn()?;
        let mut out = child.stdout.take().expect("stdout");
        let inner = Arc::new(Inner {
            buf: Mutex::new(VecDeque::new()),
            channels: channels.max(1) as usize,
            rate: rate.max(8000),
            played: AtomicU64::new(0),
            latency_us: AtomicU64::new(0),
            eof: AtomicBool::new(false),
            stop: AtomicBool::new(false),
        });
        let cap = (inner.rate as f64 * AHEAD_S) as usize * inner.channels;
        let w = inner.clone();
        std::thread::spawn(move || {
            let mut raw = vec![0u8; 16 * 1024];
            let mut rest: Vec<u8> = Vec::new();
            loop {
                if w.stop.load(Ordering::Relaxed) { break; }
                while w.buf.lock().unwrap().len() > cap {
                    if w.stop.load(Ordering::Relaxed) { return; }
                    std::thread::sleep(Duration::from_millis(5));
                }
                let n = match out.read(&mut raw) { Ok(0) | Err(_) => break, Ok(n) => n };
                rest.extend_from_slice(&raw[..n]);
                let whole = rest.len() / 4 * 4;
                {
                    let mut b = w.buf.lock().unwrap();
                    b.extend(rest[..whole].chunks_exact(4).map(|q| f32::from_le_bytes([q[0], q[1], q[2], q[3]])));
                }
                rest.drain(..whole);
            }
            w.eof.store(true, Ordering::Relaxed);
        });
        Ok(PcmFeed { child, src: PcmSource(inner) })
    }

    /// A donner au fil de la sortie audio.
    pub fn source(&self) -> PcmSource { self.src.clone() }

    /// Temps entendu depuis le debut du flux (secondes) : echantillons joues, moins le delai de la sortie.
    pub fn position(&self) -> f64 {
        let i = &self.src.0;
        let played = i.played.load(Ordering::Relaxed) as f64 / i.rate as f64;
        (played - i.latency_us.load(Ordering::Relaxed) as f64 / 1e6).max(0.0)
    }

    /// Le son a commence a sortir.
    pub fn started(&self) -> bool { self.src.0.played.load(Ordering::Relaxed) > 0 }

    /// Tout le son a ete joue (fin de la piste, ou piste absente / illisible).
    pub fn finished(&self) -> bool { self.src.0.eof.load(Ordering::Relaxed) && self.src.0.buf.lock().unwrap().is_empty() }
}

impl Drop for PcmFeed {
    fn drop(&mut self) {
        self.src.0.stop.store(true, Ordering::Relaxed);
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// Horloge d'une lecture : l'instant de la video (secondes) a montrer maintenant.
pub struct PlayClock {
    /// instant de la video ou le son en cours a commence
    origin: f64,
    /// dernier instant donne
    last: f64,
    /// sans son : (instant, moment) de depart du temps ecoule
    wall: Option<(f64, Instant)>,
}

impl PlayClock {
    pub fn new(t: f64) -> PlayClock { PlayClock { origin: t, last: t, wall: None } }

    /// L'instant a montrer. `audio` : position du son en cours depuis son debut (`PcmFeed::position`), `None` sans son ; `now` :
    /// le moment present. Le passage du son au temps ecoule (et l'inverse, apres `restart_audio`) se fait sans saut.
    pub fn time(&mut self, audio: Option<f64>, now: Instant) -> f64 {
        let t = match audio {
            Some(p) => { self.wall = None; self.origin + p }
            None => { let (t0, i0) = *self.wall.get_or_insert((self.last, now)); t0 + now.saturating_duration_since(i0).as_secs_f64() }
        };
        // jamais en arriere (le delai de la sortie audio peut varier un peu)
        self.last = t.max(self.last);
        self.last
    }

    /// Un nouveau son commence a l'instant `t` de la video (changement de piste, son retrouve).
    pub fn restart_audio(&mut self, t: f64) { self.origin = t; }

    /// Le dernier instant donne.
    pub fn last(&self) -> f64 { self.last }
}
