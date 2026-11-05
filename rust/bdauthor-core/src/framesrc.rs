//! Images d'une video pour les fenetres d'apercu (editeur de chapitres, selecteur d'image du widget « Image de la video »,
//! apercu des sous-titres) : sondage de la video, extraction d'images en arriere-plan avec memoire, vignettes d'une frise.
//! Rien ici ne depend de l'interface : les fenetres ne font que dessiner ce que ce module leur rend.
use crate::ffmpeg::{extract_frame_rgba, extract_thumbnail_rgba, probe, Tools};
use crate::palette::RgbaImage;
use std::collections::{HashMap, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{channel, Receiver, Sender, TryRecvError};
use std::sync::Arc;

/// Ce qu'une fenetre doit savoir de la video : duree, cadence et taille des images affichees.
#[derive(Clone, Debug, PartialEq)]
pub struct VideoMeta {
    pub duration: f64,
    pub fps: f64,
    /// taille des images decodees pour l'affichage (largeur demandee, hauteur selon les proportions de la video)
    pub size: (u32, u32),
}

/// Hauteur (paire) d'une image de largeur `w` qui garde les proportions `src`.
pub fn scaled_height(src: (u32, u32), w: u32) -> u32 {
    ((w as f64 * src.1.max(1) as f64 / src.0.max(1) as f64 / 2.0).round() as u32 * 2).max(2)
}

/// Sonde la video dans un fil separe ; les images affichees feront `view_w` de large.
pub fn probe_async(file: &Path, view_w: u32) -> Receiver<Result<VideoMeta, String>> {
    let (tx, rx) = channel();
    let file = file.to_path_buf();
    std::thread::spawn(move || {
        let r = Tools::locate().map_err(|e| e.to_string()).and_then(|t| probe(&t, &file).map_err(|e| e.to_string())).map(|i| VideoMeta {
            duration: i.duration_s,
            fps: if i.fps() > 1.0 { i.fps() } else { 25.0 },
            size: (view_w, scaled_height((i.width, i.height), view_w)),
        });
        let _ = tx.send(r);
    });
    rx
}

/// Comment l'image est ramenee a la taille demandee.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Fit {
    /// mise a l'echelle simple (la taille demandee suit les proportions de la video)
    Stretch,
    /// l'image remplit la taille demandee, l'excedent est rogne (fond 16/9 d'une video d'autres proportions)
    Cover,
}

/// Image demandee : numero choisi par l'appelant (numero d'image, milliseconde...) et taille.
pub type FrameKey = (i64, (u32, u32));

struct Req {
    key: FrameKey,
    t: f64,
}

/// Extraction d'images exactes en arriere-plan. Une seule extraction a la fois : quand plusieurs demandes s'accumulent
/// (defilement rapide), seule la plus recente est traitee. Les images recues sont gardees en memoire (les plus anciennes sont
/// oubliees au-dela de `cap`) ; un echec est retenu et n'est pas redemande.
pub struct FrameLoader {
    tx: Sender<Req>,
    rx: Receiver<(FrameKey, Result<RgbaImage, String>)>,
    cache: HashMap<FrameKey, Arc<RgbaImage>>,
    order: VecDeque<FrameKey>,
    bad: HashMap<FrameKey, String>,
    requested: Option<FrameKey>,
    cap: usize,
}

impl FrameLoader {
    pub fn start(file: &Path, fit: Fit, cap: usize) -> FrameLoader {
        let (tx, req_rx) = channel::<Req>();
        let (res_tx, rx) = channel();
        let src: PathBuf = file.to_path_buf();
        std::thread::spawn(move || {
            let tools = Tools::locate();
            while let Ok(mut r) = req_rx.recv() {
                while let Ok(m) = req_rx.try_recv() { r = m; }
                let out = match &tools {
                    Ok(tl) => match fit {
                        Fit::Stretch => extract_frame_rgba(tl, &src, r.t, r.key.1, true),
                        Fit::Cover => extract_thumbnail_rgba(tl, &src, r.key.1, Some(r.t), 0.0),
                    }
                    .map_err(|e| e.to_string()),
                    Err(e) => Err(e.to_string()),
                };
                if res_tx.send((r.key, out)).is_err() { break; }
            }
        });
        FrameLoader { tx, rx, cache: HashMap::new(), order: VecDeque::new(), bad: HashMap::new(), requested: None, cap: cap.max(1) }
    }

    /// Image en memoire.
    pub fn get(&self, key: i64, size: (u32, u32)) -> Option<Arc<RgbaImage>> { self.cache.get(&(key, size)).cloned() }

    /// Message d'erreur si l'extraction de cette image a echoue.
    pub fn error(&self, key: i64, size: (u32, u32)) -> Option<&str> { self.bad.get(&(key, size)).map(|s| s.as_str()) }

    /// Demande l'image `key` prise a l'instant `t` (secondes) ; sans effet si elle est deja en memoire, deja demandee ou en echec.
    /// Rend vrai si l'image est disponible tout de suite.
    pub fn want(&mut self, key: i64, t: f64, size: (u32, u32)) -> bool {
        let k = (key, size);
        if self.cache.contains_key(&k) { return true; }
        if self.bad.contains_key(&k) || self.requested == Some(k) { return false; }
        if self.tx.send(Req { key: k, t }).is_ok() { self.requested = Some(k); }
        false
    }

    /// Recupere les images terminees. Rend vrai si quelque chose est arrive (l'affichage doit etre rafraichi).
    pub fn poll(&mut self) -> bool {
        let mut got = false;
        while let Ok((k, r)) = self.rx.try_recv() {
            got = true;
            match r {
                Ok(img) => {
                    if self.cache.insert(k, Arc::new(img)).is_none() { self.order.push_back(k); }
                    while self.order.len() > self.cap { if let Some(o) = self.order.pop_front() { self.cache.remove(&o); } }
                }
                Err(e) => { self.bad.insert(k, e); }
            }
            if self.requested == Some(k) { self.requested = None; }
        }
        got
    }

    /// Une extraction est en cours.
    pub fn busy(&self) -> bool { self.requested.is_some() }
}

/// Vignettes regulieres d'une video (frise), calculees en arriere-plan, du debut a la fin.
pub struct Strip {
    rx: Option<Receiver<(usize, RgbaImage)>>,
    pub n: usize,
}

impl Strip {
    /// `n` vignettes de taille `size`, prises au milieu de `n` tranches egales de `duration`.
    pub fn start(file: &Path, duration: f64, n: usize, size: (u32, u32)) -> Strip {
        let (tx, rx) = channel();
        let src = file.to_path_buf();
        std::thread::spawn(move || {
            let Ok(tools) = Tools::locate() else { return };
            for i in 0..n {
                let t = (i as f64 + 0.5) * duration / n as f64;
                if let Ok(img) = extract_frame_rgba(&tools, &src, t, size, false) {
                    if tx.send((i, img)).is_err() { return; }
                }
            }
        });
        Strip { rx: Some(rx), n }
    }

    /// Vignettes arrivees depuis le dernier appel, et vrai tant que d'autres peuvent arriver.
    pub fn poll(&mut self) -> (Vec<(usize, RgbaImage)>, bool) {
        let mut new = Vec::new();
        let Some(rx) = &self.rx else { return (new, false) };
        loop {
            match rx.try_recv() {
                Ok(v) => new.push(v),
                Err(TryRecvError::Empty) => return (new, true),
                Err(TryRecvError::Disconnected) => { self.rx = None; return (new, false); }
            }
        }
    }
}
