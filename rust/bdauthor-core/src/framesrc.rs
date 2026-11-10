//! Images d'une video pour les fenetres d'apercu (editeur de chapitres, selecteur d'image du widget « Image de la video »,
//! apercu des sous-titres) : sondage de la video, extraction d'images en arriere-plan avec memoire, vignettes d'une frise.
//! Rien ici ne depend de l'interface : les fenetres ne font que dessiner ce que ce module leur rend.
use crate::ffmpeg::{extract_frame_rgba, extract_thumbnail_rgba, probe, stream_range_keyframes, Tools};
use crate::palette::RgbaImage;
use std::collections::{HashMap, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{channel, Receiver, Sender, TryRecvError};
use std::sync::atomic::{AtomicUsize, Ordering};
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

/// Taille a laquelle on extrait l'image entiere d'une video pour un element « Image de la video » (1920 de large, proportions de la video).
pub fn full_frame_size(tools: &Tools, src: &Path) -> crate::ffmpeg::Result<(u32, u32)> {
    let i = probe(tools, src)?;
    Ok((1920, scaled_height((i.width, i.height), 1920)))
}

/// Ecrit dans `dst` (PNG) l'image de la video `src` (de duree `duration_s`) a l'instant `fr.at`, prise a la taille `size` puis
/// recadree comme `fr.crop`. Meme code pour les menus et la jaquette.
pub fn write_video_frame_png(tools: &Tools, src: &Path, size: (u32, u32), duration_s: f64, fr: &crate::design::VideoFrame, dst: &Path) -> crate::ffmpeg::Result<()> {
    let full = extract_frame_rgba(tools, src, fr.at.min((duration_s - 0.05).max(0.0)), size, true)?;
    let (cx, cy, cw, ch) = fr.crop_px(full.width, full.height);
    let img = crate::cover::crop(&full, cx as i32, cy as i32, cw as i32, ch as i32);
    std::fs::write(dst, crate::png::write_png(&img))?;
    Ok(())
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

/// Ordre de calcul des `n` vignettes de la plage [`from`, `to`] : en partant de `focus` (la vue visible) vers les bords, ou, sans
/// `focus`, de proche en proche sur toute la plage (debut, milieu, quarts, huitiemes...) pour que la frise se precise partout a la fois.
pub fn strip_order(n: usize, from: f64, to: f64, focus: Option<f64>) -> Vec<usize> {
    if let Some(f) = focus {
        let t = |i: usize| from + (i as f64 + 0.5) * (to - from) / n.max(1) as f64;
        let mut v: Vec<usize> = (0..n).collect();
        v.sort_by(|a, b| (t(*a) - f).abs().total_cmp(&(t(*b) - f).abs()));
        return v;
    }
    let mut seen = vec![false; n];
    let mut out = Vec::with_capacity(n);
    let mut stride = n.next_power_of_two().max(1);
    loop {
        let mut i = 0;
        while i < n { if !seen[i] { seen[i] = true; out.push(i); } i += stride; }
        if stride == 1 { break; }
        stride /= 2;
    }
    out
}

impl Strip {
    /// `n` vignettes de taille `size`, prises au milieu de `n` tranches egales de `duration` (images cles, rapide).
    pub fn start(file: &Path, duration: f64, n: usize, size: (u32, u32)) -> Strip {
        Strip::start_range(file, 0.0, duration, n, size, false)
    }

    /// `n` vignettes prises au milieu de `n` tranches egales de la plage [`from`, `to`] (secondes). `exact` : images exactes
    /// (plus lent, pour une echelle ou deux vignettes voisines tombent sur des images cles differentes) ; sinon images cles.
    /// La frise s'arrete d'elle-meme quand on la laisse tomber.
    pub fn start_range(file: &Path, from: f64, to: f64, n: usize, size: (u32, u32), exact: bool) -> Strip {
        Strip::start_range_focus(file, from, to, n, size, exact, None)
    }

    /// Comme `start_range`. Images cles : toute la plage est lue en une seule passe de ffmpeg (bien plus rapide que `n` extractions) ;
    /// si cette passe ne donne rien, et pour les images exactes, les vignettes sont extraites une a une par plusieurs ffmpeg en
    /// parallele, dans l'ordre de `strip_order` (`focus` : instant a servir en premier).
    pub fn start_range_focus(file: &Path, from: f64, to: f64, n: usize, size: (u32, u32), exact: bool, focus: Option<f64>) -> Strip {
        let (tx, rx) = channel();
        let src = file.to_path_buf();
        let order = Arc::new(strip_order(n, from, to, focus));
        std::thread::spawn(move || {
            let Ok(tools) = Tools::locate() else { return };
            if !exact {
                let mut alive = true;
                let got = stream_range_keyframes(&tools, &src, from, to, n, size, |i, img| { alive = tx.send((i, img)).is_ok(); alive });
                if !alive { return; }
                if matches!(got, Ok(g) if g > 0) { return; }
            }
            let next = Arc::new(AtomicUsize::new(0));
            let workers = std::thread::available_parallelism().map_or(2, |p| p.get()).clamp(1, 4);
            let mut handles = Vec::new();
            for _ in 0..workers {
                let (tx, src, tools, next, order) = (tx.clone(), src.clone(), tools.clone(), next.clone(), order.clone());
                handles.push(std::thread::spawn(move || loop {
                    let k = next.fetch_add(1, Ordering::Relaxed);
                    let Some(&i) = order.get(k) else { return };
                    let t = from + (i as f64 + 0.5) * (to - from) / n as f64;
                    if let Ok(img) = extract_frame_rgba(&tools, &src, t, size, exact) {
                        if tx.send((i, img)).is_err() { next.store(usize::MAX / 2, Ordering::Relaxed); return; }
                    }
                }));
            }
            for h in handles { let _ = h.join(); }
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
