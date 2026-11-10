//! Images d'une video pour les fenetres d'apercu (editeur de chapitres, selecteur d'image du widget « Image de la video »,
//! apercu des sous-titres) : sondage de la video, extraction d'images en arriere-plan avec memoire, vignettes d'une frise.
//! Rien ici ne depend de l'interface : les fenetres ne font que dessiner ce que ce module leur rend.
use crate::ffmpeg::{extract_frame_rgba, extract_frames_rgba, extract_thumbnail_rgba, probe, stream_range_keyframes, Tools};
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

/// Nombre de vignettes de la frise d'une video entiere (le meme pour toutes les fenetres : une frise calculee sert a toutes).
pub const STRIP_N: usize = 48;
/// Largeur des vignettes de la frise.
pub const STRIP_THUMB_W: u32 = 160;
/// Largeur des images d'apercu des fenetres (la frise prend les proportions de ces images).
pub const VIEW_W: u32 = 960;
/// Taille maximale du cache disque des frises : au-dela, les plus anciennes sont effacees.
const STRIP_CACHE_MAX: u64 = 300 << 20;
/// Vignettes extraites par un meme ffmpeg.
const STRIP_BATCH: usize = 6;
const STRIP_MAGIC: &[u8; 8] = b"BDASTRP1";

/// Taille des vignettes de la frise pour des images d'apercu de taille `view`.
pub fn strip_thumb_size(view: (u32, u32)) -> (u32, u32) { (STRIP_THUMB_W, scaled_height(view, STRIP_THUMB_W)) }

/// Dossier du cache choisi dans les reglages (rien : emplacement par defaut).
static CACHE_DIR: std::sync::Mutex<Option<PathBuf>> = std::sync::Mutex::new(None);

/// Choisit le dossier du cache des frises ; vide : emplacement par defaut (`default_strip_cache_dir`).
pub fn set_strip_cache_dir(dir: &str) {
    let d = dir.trim();
    if let Ok(mut c) = CACHE_DIR.lock() { *c = if d.is_empty() { None } else { Some(PathBuf::from(d)) }; }
}

/// Dossier du cache des frises : celui des reglages, sinon l'emplacement par defaut.
pub fn strip_cache_dir() -> Option<PathBuf> {
    if let Some(d) = CACHE_DIR.lock().ok().and_then(|c| c.clone()) { return Some(d); }
    default_strip_cache_dir()
}

/// Les frises du cache : nombre de fichiers et taille totale (octets).
pub fn strip_cache_usage() -> (usize, u64) {
    let Some(rd) = strip_cache_dir().and_then(|d| std::fs::read_dir(d).ok()) else { return (0, 0) };
    rd.flatten().filter(|e| e.path().extension().is_some_and(|x| x == "frise"))
        .fold((0, 0), |(n, b), e| (n + 1, b + e.metadata().map_or(0, |m| m.len())))
}

/// Efface les frises du cache (seulement elles : le dossier peut contenir autre chose). Rend le nombre de fichiers effaces.
pub fn clear_strip_cache() -> usize {
    let Some(rd) = strip_cache_dir().and_then(|d| std::fs::read_dir(d).ok()) else { return 0 };
    rd.flatten().filter(|e| e.path().extension().is_some_and(|x| x == "frise")).filter(|e| std::fs::remove_file(e.path()).is_ok()).count()
}

/// Emplacement par defaut du cache des frises : `BDAUTHOR_CACHE_DIR`, sinon le cache de l'utilisateur (LOCALAPPDATA, XDG_CACHE_HOME, ~/.cache).
pub fn default_strip_cache_dir() -> Option<PathBuf> {
    if let Some(d) = std::env::var_os("BDAUTHOR_CACHE_DIR") { return Some(PathBuf::from(d).join("frises")); }
    let base = std::env::var_os("LOCALAPPDATA").map(PathBuf::from)
        .or_else(|| std::env::var_os("XDG_CACHE_HOME").map(PathBuf::from))
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".cache")))?;
    Some(base.join("bdauthor").join("frises"))
}

fn fnv64(b: &[u8]) -> u64 {
    b.iter().fold(0xcbf29ce484222325u64, |h, &x| (h ^ x as u64).wrapping_mul(0x100000001b3))
}

/// Fichier du cache pour cette frise, et sa cle (chemin, taille et date de la video, plage, nombre et taille des vignettes) :
/// une video modifiee ou remplacee change de cle.
fn strip_cache_entry(src: &Path, from: f64, to: f64, n: usize, size: (u32, u32), exact: bool) -> Option<(PathBuf, String)> {
    let dir = strip_cache_dir()?;
    let m = std::fs::metadata(src).ok()?;
    let mt = m.modified().ok()?.duration_since(std::time::UNIX_EPOCH).ok()?.as_nanos();
    let abs = std::fs::canonicalize(src).unwrap_or_else(|_| src.to_path_buf());
    let key = format!("{}|{}|{}|{:x}|{:x}|{}|{}x{}|{}", abs.display(), m.len(), mt, from.to_bits(), to.to_bits(), n, size.0, size.1, exact);
    Some((dir.join(format!("{:016x}.frise", fnv64(key.as_bytes()))), key))
}

fn read_u32(b: &[u8], p: &mut usize) -> Option<u32> {
    let v = u32::from_le_bytes(b.get(*p..*p + 4)?.try_into().ok()?);
    *p += 4;
    Some(v)
}

/// Vignettes deja enregistrees (une case vide par vignette manquante) ; rien d'utilisable : tout est vide.
fn load_strip_cache(path: &Path, key: &str, n: usize, size: (u32, u32)) -> Vec<Option<RgbaImage>> {
    let mut out = vec![None; n];
    let Ok(b) = std::fs::read(path) else { return out };
    let parse = || -> Option<Vec<Option<RgbaImage>>> {
        if b.get(..8)? != STRIP_MAGIC { return None; }
        let mut p = 8;
        let kl = read_u32(&b, &mut p)? as usize;
        if b.get(p..p + kl)? != key.as_bytes() { return None; }
        p += kl;
        if read_u32(&b, &mut p)? as usize != n || read_u32(&b, &mut p)? != size.0 || read_u32(&b, &mut p)? != size.1 { return None; }
        let len = size.0 as usize * size.1 as usize * 4;
        let mut v = vec![None; n];
        for slot in v.iter_mut() {
            let here = *b.get(p)?;
            p += 1;
            if here == 1 {
                *slot = Some(RgbaImage { width: size.0, height: size.1, data: b.get(p..p + len)?.to_vec() });
                p += len;
            }
        }
        Some(v)
    };
    if let Some(v) = parse() { out = v; }
    let _ = std::fs::File::options().append(true).open(path).and_then(|f| f.set_modified(std::time::SystemTime::now()));
    out
}

/// Enregistre les vignettes (ecriture atomique), puis ramene le cache sous sa taille maximale.
fn save_strip_cache(path: &Path, key: &str, size: (u32, u32), imgs: &[Option<RgbaImage>]) {
    let Some(dir) = path.parent() else { return };
    if std::fs::create_dir_all(dir).is_err() { return; }
    let len = size.0 as usize * size.1 as usize * 4;
    let mut b = Vec::with_capacity(32 + key.len() + imgs.len() * (len + 1));
    b.extend_from_slice(STRIP_MAGIC);
    b.extend_from_slice(&(key.len() as u32).to_le_bytes());
    b.extend_from_slice(key.as_bytes());
    for v in [imgs.len() as u32, size.0, size.1] { b.extend_from_slice(&v.to_le_bytes()); }
    for im in imgs {
        match im {
            Some(im) if im.data.len() == len => { b.push(1); b.extend_from_slice(&im.data); }
            _ => b.push(0),
        }
    }
    let tmp = path.with_extension(format!("tmp{}", std::process::id()));
    if std::fs::write(&tmp, &b).is_err() || std::fs::rename(&tmp, path).is_err() { let _ = std::fs::remove_file(&tmp); return; }
    // les plus anciennes (dernier usage) sont effacees au-dela de la taille maximale
    let Ok(rd) = std::fs::read_dir(dir) else { return };
    let mut files: Vec<(std::time::SystemTime, u64, PathBuf)> = rd.flatten()
        .filter(|e| e.path().extension().is_some_and(|x| x == "frise"))
        .filter_map(|e| { let m = e.metadata().ok()?; Some((m.modified().ok()?, m.len(), e.path())) })
        .collect();
    let mut total: u64 = files.iter().map(|f| f.1).sum();
    files.sort_by_key(|f| f.0);
    for (_, l, f) in files {
        if total <= STRIP_CACHE_MAX { break; }
        if f != path && std::fs::remove_file(&f).is_ok() { total -= l; }
    }
}

/// Calcule la frise dans le fil courant, en envoyant chaque vignette a `tx` (arret des que plus personne n'ecoute).
/// `cache` : les vignettes deja enregistrees sont reprises, et celles calculees sont enregistrees (meme si le calcul est interrompu).
#[allow(clippy::too_many_arguments)]
fn run_strip(src: PathBuf, from: f64, to: f64, n: usize, size: (u32, u32), exact: bool, order: Vec<usize>, tx: Sender<(usize, RgbaImage)>, cache: bool, workers: usize) {
    let entry = if cache { strip_cache_entry(&src, from, to, n, size, exact) } else { None };
    let have = match &entry { Some((p, k)) => load_strip_cache(p, k, n, size), None => vec![None; n] };
    for (i, im) in have.iter().enumerate() {
        if let Some(im) = im { if tx.send((i, im.clone())).is_err() { return; } }
    }
    let todo: Vec<usize> = order.into_iter().filter(|&i| have.get(i).is_some_and(|h| h.is_none())).collect();
    if todo.is_empty() { return; }
    let Ok(tools) = Tools::locate() else { return };
    let have = Arc::new(std::sync::Mutex::new(have));
    let added = Arc::new(AtomicUsize::new(0));
    let save = |have: &Arc<std::sync::Mutex<Vec<Option<RgbaImage>>>>| {
        if let (Some((p, k)), true) = (&entry, added.load(Ordering::Relaxed) > 0) {
            if let Ok(h) = have.lock() { save_strip_cache(p, k, size, &h); }
        }
    };
    // une passe unique lit toute la plage (tout le fichier pour une frise de film entier) : elle ne vaut que pour une plage courte ;
    // au-dela, des recherches directes (-ss) en parallele sont bien plus rapides
    if !exact && (to - from) <= 180.0 {
        let mut alive = true;
        let got = stream_range_keyframes(&tools, &src, from, to, n, size, |i, img| {
            if let Ok(mut h) = have.lock() { if let Some(s) = h.get_mut(i) { *s = Some(img.clone()); added.fetch_add(1, Ordering::Relaxed); } }
            alive = tx.send((i, img)).is_ok();
            alive
        });
        if !alive || matches!(got, Ok(g) if g > 0) { save(&have); return; }
    }
    // lots de quelques vignettes par ffmpeg (dans l'ordre de service), repartis entre plusieurs ffmpeg en parallele
    let batches: Arc<Vec<Vec<usize>>> = Arc::new(todo.chunks(STRIP_BATCH).map(|c| c.to_vec()).collect());
    let next = Arc::new(AtomicUsize::new(0));
    let mut handles = Vec::new();
    for _ in 0..workers.max(1) {
        let (tx, src, tools, next, batches, have, added) = (tx.clone(), src.clone(), tools.clone(), next.clone(), batches.clone(), have.clone(), added.clone());
        handles.push(std::thread::spawn(move || loop {
            let k = next.fetch_add(1, Ordering::Relaxed);
            let Some(batch) = batches.get(k) else { return };
            let time = |i: usize| from + (i as f64 + 0.5) * (to - from) / n as f64;
            let times: Vec<f64> = batch.iter().map(|&i| time(i)).collect();
            // un seul ffmpeg pour le lot ; s'il ne rend pas toutes les images, elles sont extraites une a une
            let imgs: Vec<(usize, RgbaImage)> = match extract_frames_rgba(&tools, &src, &times, size, exact) {
                Ok(v) => batch.iter().copied().zip(v).collect(),
                Err(_) => batch.iter().filter_map(|&i| extract_frame_rgba(&tools, &src, time(i), size, exact).ok().map(|im| (i, im))).collect(),
            };
            for (i, img) in imgs {
                if let Ok(mut h) = have.lock() { if let Some(s) = h.get_mut(i) { *s = Some(img.clone()); added.fetch_add(1, Ordering::Relaxed); } }
                if tx.send((i, img)).is_err() { next.store(usize::MAX / 2, Ordering::Relaxed); return; }
            }
        }));
    }
    for h in handles { let _ = h.join(); }
    save(&have);
}

fn default_workers() -> usize { std::thread::available_parallelism().map_or(2, |p| p.get()).clamp(2, 8) }

/// Calcule a l'avance, en arriere-plan et une video apres l'autre, la frise de chaque video (elle est alors lue du cache a
/// l'ouverture des fenetres). Peu de ffmpeg a la fois, pour ne pas ralentir le reste ; les frises deja en cache ne coutent rien.
pub fn prefetch_strips(files: Vec<PathBuf>) {
    std::thread::spawn(move || {
        let Ok(tools) = Tools::locate() else { return };
        for f in files {
            let Ok(i) = probe(&tools, &f) else { continue };
            if i.duration_s <= 0.0 { continue; }
            let size = strip_thumb_size((VIEW_W, scaled_height((i.width, i.height), VIEW_W)));
            let (tx, _rx) = channel();
            run_strip(f, 0.0, i.duration_s, STRIP_N, size, false, strip_order(STRIP_N, 0.0, i.duration_s, None), tx, true, 2);
        }
    });
}

impl Strip {
    /// `n` vignettes de taille `size`, prises au milieu de `n` tranches egales de `duration` (images cles, rapide). Elles sont
    /// gardees dans le cache disque : une frise deja calculee s'affiche aussitot.
    pub fn start(file: &Path, duration: f64, n: usize, size: (u32, u32)) -> Strip {
        Strip::spawn(file, 0.0, duration, n, size, false, None, true)
    }

    /// `n` vignettes prises au milieu de `n` tranches egales de la plage [`from`, `to`] (secondes). `exact` : images exactes
    /// (plus lent, pour une echelle ou deux vignettes voisines tombent sur des images cles differentes) ; sinon images cles.
    /// La frise s'arrete d'elle-meme quand on la laisse tomber.
    pub fn start_range(file: &Path, from: f64, to: f64, n: usize, size: (u32, u32), exact: bool) -> Strip {
        Strip::start_range_focus(file, from, to, n, size, exact, None)
    }

    /// Comme `start_range` (sans cache disque). Images cles d'une plage courte : une seule passe de ffmpeg ; sinon, et si cette passe
    /// ne donne rien, les vignettes sont extraites une a une par plusieurs ffmpeg en parallele, dans l'ordre de `strip_order`
    /// (`focus` : instant a servir en premier).
    pub fn start_range_focus(file: &Path, from: f64, to: f64, n: usize, size: (u32, u32), exact: bool, focus: Option<f64>) -> Strip {
        Strip::spawn(file, from, to, n, size, exact, focus, false)
    }

    #[allow(clippy::too_many_arguments)]
    fn spawn(file: &Path, from: f64, to: f64, n: usize, size: (u32, u32), exact: bool, focus: Option<f64>, cache: bool) -> Strip {
        let (tx, rx) = channel();
        let src = file.to_path_buf();
        let order = strip_order(n, from, to, focus);
        std::thread::spawn(move || run_strip(src, from, to, n, size, exact, order, tx, cache, default_workers()));
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
