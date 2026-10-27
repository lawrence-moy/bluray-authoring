//! Disque multi-titres a partir d'un fichier de projet JSON (equivalent de `project.py`).
//!
//! Format du projet : voir `examples/projet_exemple.json` et le README. Structure produite : videos = clips/playlists 0..N-1
//! (titres 1..N), fond de menu = clip N (image fixe, sans audio), playlist de menu N (fond + menu IG), clip IG 80002 ;
//! pagination au-dela de 12 elements (voir `paginate`), menus de chapitres, cache d'encodage.
//! Le dessin (vignettes, libelles, fond) est delegue a un `Renderer` : le crate `bdauthor-render` en fournit un complet.
use crate::check::{check_bdmv, Report};
use crate::design::{nav_applicable, Align, Designs, Kind, MenuType, NavAction, PageDesign, Target};
use crate::disc::write_bdmv;
use crate::ffmpeg::{self, EncodeOpts, Error, Tools};
use crate::film::{make_film_m2ts, make_still_m2ts};
use crate::ig::py_round;
use crate::json::{self, Value};
use crate::menu::{build_menu_clip, required_ics_pts, MenuButton, MenuOptions};
use crate::mobj::{Command, JUMP_TITLE_INSN, MOVE_RI_INSN};
use crate::nav::{make_index, make_movie_objects_paged, make_playlist, plan_navigation, AudioStn, PlaylistParams, VideoStn};
use crate::palette::RgbaImage;
use crate::tsclip::{make_clip_clpi, make_clip_clpi_pg, py_floordiv, scan_clip, ClipScan, FRAME_RATES};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;
use std::sync::Arc;

pub const MAX_PER_PAGE: usize = 12; // elements par page de menu (au-dela : pagination)
pub const MAX_VIDEOS: usize = 99;
pub const MAX_CHAPTERS: usize = 99;

type Result<T> = std::result::Result<T, Error>;
fn bad<T>(m: impl Into<String>) -> Result<T> { Err(Error::Format(m.into())) }

/// Dessin des elements de menu. Les images sont en RGBA 8 bits.
pub trait Renderer {
    /// (normal, selectionne) d'un bouton « vignette + libelle » ; l'etat active reutilise le selectionne.
    fn thumb_button(&self, thumb: &RgbaImage, label: &str, w: u32, h_total: u32, h_thumb: u32) -> [Arc<RgbaImage>; 2];
    /// (normal, selectionne) d'un bouton uniquement textuel (« Tout lire », « Retour »...).
    fn text_button(&self, label: &str, w: u32, h: u32) -> [Arc<RgbaImage>; 2];
    /// Fond de menu par defaut avec le titre.
    fn background(&self, title: &str, size: (u32, u32)) -> RgbaImage;
    /// Fond d'une page personnalisee : image `base` (ou degrade par defaut, sans titre) + textes et images, en pixels du fond.
    fn compose_background(&self, base: Option<&Path>, size: (u32, u32), decor: &[Decor]) -> std::result::Result<RgbaImage, String>;
    /// Pose `decor` (coordonnees relatives a l'image du bouton) par-dessus les deux etats d'un bouton : textes et images
    /// places au-dessus de ce bouton dans l'ordre de superposition. Par defaut, les etats sont rendus tels quels.
    fn overlay(&self, states: [Arc<RgbaImage>; 2], decor: &[Decor]) -> [Arc<RgbaImage>; 2] {
        let _ = decor;
        states
    }
    /// Image (fond transparent) d'une replique de sous-titre : texte centre, contour compris, aux tailles du style (deja a l'echelle de
    /// la video). `max_w` : largeur disponible (le texte est reduit s'il deborde). `None` si ce moteur ne sait pas dessiner de sous-titres.
    fn subtitle(&self, text: &str, style: &crate::subtitle::SubStyle, max_w: u32) -> Option<RgbaImage> {
        let _ = (text, style, max_w);
        None
    }
}

/// Texte ou image posee sur le fond d'une page (coordonnees et tailles en pixels du fond).
#[derive(Debug, Clone, PartialEq)]
pub enum Decor {
    /// `font` : voir `design::Kind::Text` (un chemin relatif au projet est deja converti en chemin complet).
    Text { text: String, x: i32, y: i32, w: i32, h: i32, size: i32, color: [u8; 3], align: Align, font: String },
    Image { path: PathBuf, x: i32, y: i32, w: i32, h: i32 },
}

impl Decor {
    /// Rectangle (x, y, largeur, hauteur) du cadre.
    pub fn rect(&self) -> (i32, i32, i32, i32) {
        match self { Decor::Text { x, y, w, h, .. } | Decor::Image { x, y, w, h, .. } => (*x, *y, *w, *h) }
    }
    pub fn shifted(&self, dx: i32, dy: i32) -> Decor {
        let mut d = self.clone();
        match &mut d { Decor::Text { x, y, .. } | Decor::Image { x, y, .. } => { *x += dx; *y += dy; } }
        d
    }
    pub fn intersects(&self, r: (i32, i32, i32, i32)) -> bool {
        let (x, y, w, h) = self.rect();
        x < r.0 + r.2 && r.0 < x + w && y < r.1 + r.3 && r.1 < y + h
    }
}

struct SlotSpec { label: String, source: PathBuf, at: Option<f64>, duration: f64, commands: Vec<Command>, go: u32 }

/// Utilise la mise en page personnalisee si elle est valide pour cette page, sinon la grille automatique. Retourne (mise en page, personnalisee ?).
fn resolve_design(ov: Option<&PageDesign>, expected: usize, menu: MenuType, page: usize, pages: usize, what: &str, log: &mut dyn FnMut(&str)) -> Result<(PageDesign, bool)> {
    if let Some(d) = ov {
        let probs = d.validate(expected, menu, page, pages);
        let errs: Vec<&str> = probs.iter().filter(|x| x.error).map(|x| x.msg.as_str()).collect();
        if errs.is_empty() {
            for w in probs.iter().filter(|x| !x.error) { log(&format!("{what} : {}", w.msg)); }
            return Ok((d.clone(), true));
        }
        log(&format!("{what} : mise en page personnalisee ignoree ({}) ; grille automatique utilisee", errs.join(" ")));
    }
    let d = match menu {
        MenuType::Main => PageDesign::default_main(expected, page, pages),
        MenuType::Chapters => PageDesign::default_chapter(expected, page, pages),
    };
    Ok((d.map_err(Error::Format)?, false))
}

/// Boutons (et surface d'objets graphiques de chacun) d'une page d'apres sa mise en page ; `s` = echelle du plan 1920 vers le menu.
#[allow(clippy::too_many_arguments)]
fn page_buttons(tools: &Tools, renderer: &dyn Renderer, design: &PageDesign, assignment: &[usize], decor: &[(usize, Decor)], menu: MenuType, page: usize, pages: usize, slots: &[SlotSpec], target: &dyn Fn(NavAction, Option<Target>) -> u32, s: f64) -> Result<(Vec<MenuButton>, Vec<u64>)> {
    let (mut buttons, mut pix, mut k) = (Vec::new(), Vec::new(), 0usize);
    for (idx, it) in design.items.iter().enumerate() {
        let (x, y, w, h) = it.scaled(s);
        // textes et images places au-dessus de ce bouton dans l'ordre de superposition : poses sur ses images
        let above: Vec<Decor> = decor.iter().filter(|(i, d)| *i > idx && d.intersects((x, y, w, h))).map(|(_, d)| d.shifted(-x, -y)).collect();
        let over = |st: [Arc<RgbaImage>; 2]| if above.is_empty() { st } else { renderer.overlay(st, &above) };
        match &it.kind {
            Kind::Slot { thumb_h, .. } => {
                let sp = &slots[assignment[k]];
                k += 1;
                let th = py_round(*thumb_h as f64 * s) as i32;
                let thumb = ffmpeg::extract_thumbnail_rgba(tools, &sp.source, (w as u32, th as u32), sp.at, sp.duration)?;
                let [nrm, sel] = over(renderer.thumb_button(&thumb, &sp.label, w as u32, h as u32, th as u32));
                let mut b = MenuButton::new("", sp.go, x, y, vec![nrm, sel.clone(), sel]);
                b.commands = sp.commands.clone();
                buttons.push(b);
                pix.push(2 * w as u64 * h as u64);
            }
            Kind::Nav { action, text, target: tg } if nav_applicable(*action, menu, page, pages) => {
                let [nrm, sel] = over(renderer.text_button(text, w as u32, h as u32));
                buttons.push(MenuButton::new("", target(*action, *tg), x, y, vec![nrm, sel.clone(), sel]));
                pix.push(2 * w as u64 * h as u64);
            }
            _ => {}
        }
    }
    Ok((buttons, pix))
}

/// Image fixe -> clip de fond numero `num` (m2ts + clpi dans `files`).
#[allow(clippy::too_many_arguments)]
fn still_clip(tools: &Tools, cancel: &AtomicBool, png: &Path, seconds: f64, size: (u32, u32), fps: &str, opts: &EncodeOpts, files: &mut BTreeMap<String, Vec<u8>>, num: u32) -> Result<ClipScan> {
    let (m2ts, _) = make_still_m2ts(tools, png, seconds, size, fps, opts, None, cancel)?;
    let sc = scan_clip(&m2ts)?;
    files.insert(format!("CLIPINF/{num:05}.clpi"), make_clip_clpi(&m2ts, *b"und", 0, None, "0100")?);
    files.insert(format!("STREAM/{num:05}.m2ts"), m2ts);
    Ok(sc)
}

/// Fond propre a une page personnalisee : compose (base + textes + images), ecrit en PNG puis encode en clip fixe.
#[allow(clippy::too_many_arguments)]
fn own_background(env: &BuildEnv, base: &Option<PathBuf>, size: (u32, u32), decor: &[Decor], tmp: &Path, name: &str, seconds: f64, fps: &str, opts: &EncodeOpts, files: &mut BTreeMap<String, Vec<u8>>, num: u32) -> Result<ClipScan> {
    let img = env.renderer.compose_background(base.as_deref(), size, decor).map_err(Error::Format)?;
    let f = tmp.join(name);
    std::fs::write(&f, crate::png::write_png(&img))?;
    still_clip(env.tools, env.cancel, &f, seconds, size, fps, opts, files, num)
}

// ------------------------------------------------------------------ mise en page
/// (colonnes, largeur vignette, hauteur vignette, hauteur etiquette, ecart horizontal, ecart vertical) sur un plan 1920x1080
const LAYOUTS: [(usize, i32, i32, i32, i32, i32); 4] = [(3, 480, 270, 56, 60, 40), (4, 360, 202, 48, 40, 30), (4, 280, 157, 40, 40, 24), (4, 260, 146, 38, 40, 24)];

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Cell { pub x: i32, pub y: i32, pub w: i32, pub th: i32, pub h: i32 }

/// Grille de boutons pour `n` vignettes sur un plan de `size`, dans la zone libre entre le titre (y = 250) et le bas
/// (marge 60, plus `reserve` pixels pour des boutons additionnels). Premiere grille qui tient.
pub fn layout(n: usize, size: (u32, u32), reserve: i32) -> Result<Vec<Cell>> {
    if n > MAX_PER_PAGE || n < 1 {
        return bad(format!("{n} elements sur une page : 1 a {MAX_PER_PAGE}"));
    }
    let s = size.0 as f64 / 1920.0;
    let avail = 1080 - 250 - 60 - reserve;
    let mut found = None;
    for &(cols0, tw, th, lab, gx, gy) in &LAYOUTS {
        let cols = n.min(cols0);
        let rows = (n + cols - 1) / cols;
        let (w, h) = (tw, th + lab);
        let total_h = rows as i32 * h + (rows as i32 - 1) * gy;
        if total_h <= avail {
            found = Some((cols, rows, w, th, h, gx, gy, total_h));
            break;
        }
    }
    let Some((cols, rows, w, th, h, gx, gy, total_h)) = found else { return bad(format!("{n} elements ne tiennent pas dans le menu")) };
    let total_w = cols as i32 * w + (cols as i32 - 1) * gx;
    let (x0, y0) = ((1920 - total_w) / 2, 250 + (avail - total_h) / 2);
    let r = |v: i32| py_round(v as f64 * s) as i32;
    Ok((0..n)
        .map(|i| {
            let (row, c) = (i / cols, (i % cols) as i32);
            let xr = if row == rows - 1 && n % cols != 0 {
                let k = (n - (rows - 1) * cols) as i32;
                (1920 - (k * w + (k - 1) * gx)) / 2 + c * (w + gx)
            } else {
                x0 + c * (w + gx)
            };
            Cell { x: r(xr), y: r(y0 + row as i32 * (h + gy)), w: r(w), th: r(th), h: r(h) }
        })
        .collect())
}

/// Repartit `count` elements en pages equilibrees. Une seule page si count <= par_page (12 par defaut) ; sinon pages de `par_page` (8 par defaut).
pub fn paginate(count: usize, par_page: Option<usize>) -> Result<Vec<Vec<usize>>> {
    if let Some(p) = par_page {
        if !(1..=MAX_PER_PAGE).contains(&p) {
            return bad(format!("menu.par_page : 1 a {MAX_PER_PAGE}"));
        }
    }
    if count <= par_page.unwrap_or(MAX_PER_PAGE) {
        return Ok(vec![(0..count).collect()]);
    }
    let pages = (count + par_page.unwrap_or(8) - 1) / par_page.unwrap_or(8);
    let (base, extra) = (count / pages, count % pages);
    let mut out = Vec::new();
    let mut k = 0;
    for q in 0..pages {
        let size = base + usize::from(q < extra);
        out.push((k..k + size).collect());
        k += size;
    }
    Ok(out)
}

// ------------------------------------------------------------------ chapitres
#[derive(Debug, Clone, PartialEq)]
pub struct Chapter { pub mark45: u32, pub name: String, pub t_rel: f64 }

/// Reperes d'une video ; vide s'il y a moins de 2 chapitres. Chaque repere (sauf le premier) est cale sur l'image d'acces qui precede.
pub fn parse_chapters(v: &Value, sc: &ClipScan) -> Result<Vec<Chapter>> {
    let dur = (sc.end90 - sc.start90) as f64 / 90000.0;
    let auto = v.get("chapitres_auto_s").and_then(Value::as_f64).unwrap_or(0.0);
    let mut spec: Vec<(f64, Option<String>)> = Vec::new();
    if let Some(a) = v.get("chapitres").and_then(Value::as_array) {
        for c in a {
            match c {
                Value::Obj(_) => {
                    let t = c.get("t").and_then(Value::as_f64).ok_or_else(|| Error::Format("chapitre sans temps « t »".into()))?;
                    spec.push((t, c.get("titre").and_then(Value::as_str).map(String::from)));
                }
                _ => spec.push((c.as_f64().ok_or_else(|| Error::Format("chapitre : nombre de secondes attendu".into()))?, None)),
            }
        }
    }
    if spec.is_empty() && auto > 0.0 {
        let n = py_floordiv(dur, auto) as i64 + 1;
        spec = (0..n).map(|k| k as f64 * auto).filter(|&t| t < dur - auto / 2.0).map(|t| (t, None)).collect();
    }
    if spec.is_empty() {
        return Ok(vec![]);
    }
    if spec.iter().all(|(t, _)| *t > 0.0) {
        spec.insert(0, (0.0, None));
    }
    spec.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));
    let mut eps: Vec<u64> = sc.eps.iter().map(|e| e.0).collect();
    eps.sort();
    let mut out: BTreeMap<u64, Option<String>> = BTreeMap::new();
    for (t, name) in spec {
        if t >= dur {
            continue;
        }
        let m45 = if t == 0.0 {
            sc.start90 / 2
        } else {
            let target = sc.start90 as i64 + py_round(t * 90000.0) as i64;
            eps.iter().copied().filter(|&p| p as i64 <= target).max().or(eps.first().copied()).unwrap_or(sc.start90) / 2
        };
        out.entry(m45).or_insert(name);
    }
    let chapters: Vec<Chapter> = out
        .iter()
        .enumerate()
        .map(|(j, (&m, name))| Chapter {
            mark45: m as u32,
            name: name.clone().filter(|s| !s.is_empty()).unwrap_or_else(|| format!("Chapitre {}", j + 1)),
            t_rel: (m as f64 * 2.0 - sc.start90 as f64) / 90000.0,
        })
        .collect();
    if chapters.len() < 2 {
        return Ok(vec![]);
    }
    if chapters.len() > MAX_CHAPTERS {
        return bad(format!("{} chapitres : {MAX_CHAPTERS} au maximum par video", chapters.len()));
    }
    Ok(chapters)
}

// ------------------------------------------------------------------ cache d'encodage
fn fnv1a(data: &[u8]) -> u64 {
    data.iter().fold(0xcbf2_9ce4_8422_2325u64, |h, &b| (h ^ b as u64).wrapping_mul(0x0000_0100_0000_01b3))
}

fn cache_key(path: &Path, lang: [u8; 3], o: &EncodeOpts) -> Result<String> {
    let md = std::fs::metadata(path)?;
    let mtime = md.modified().ok().and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map_or(0, |d| d.as_secs());
    let abs = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    let s = format!("{}|{}|{}|{}|{}|{}|{}|{}|{}|v1", abs.display(), md.len(), mtime, String::from_utf8_lossy(&lang), o.crf, o.maxrate, o.keyint, o.audio_bitrate, o.preset);
    Ok(format!("{:016x}", fnv1a(s.as_bytes())))
}

// ------------------------------------------------------------------ construction
pub struct BuildEnv<'a> {
    pub tools: &'a Tools,
    pub renderer: &'a dyn Renderer,
    pub cancel: &'a AtomicBool,
    pub log: &'a mut dyn FnMut(&str),
    /// Avancement global 0.0..=1.0.
    pub progress: &'a mut dyn FnMut(f64),
}

#[derive(Debug)]
pub struct BuildInfo {
    pub videos: usize,
    pub main_pages: usize,
    pub chapters: BTreeMap<usize, usize>,
    pub chapter_pages: BTreeMap<usize, usize>,
    pub format: (u32, u32, f64),
    pub menu_size: (u32, u32),
    pub ics_pts_s: f64,
    pub menu_bytes: usize,
    pub menu_objects: usize,
    pub palette_entries: usize,
    pub total_stream_bytes: usize,
    pub durations_s: Vec<f64>,
    pub report: Report,
}

struct TmpDir(PathBuf);
impl Drop for TmpDir {
    fn drop(&mut self) { let _ = std::fs::remove_dir_all(&self.0); }
}

struct MenuStats { pts: u64, bytes: usize, objects: usize, palette: usize }

fn fps_str(fps: f64) -> Result<&'static str> {
    let _ = FRAME_RATES;
    Ok(match fps {
        x if x == 23.976 => "24000/1001",
        x if x == 24.0 => "24",
        x if x == 25.0 => "25",
        x if x == 29.97 => "30000/1001",
        x if x == 50.0 => "50",
        x if x == 59.94 => "60000/1001",
        x => return bad(format!("cadence {x} non geree")),
    })
}

fn get_usize(v: Option<&Value>, what: &str) -> Result<Option<usize>> {
    match v {
        None | Some(Value::Null) => Ok(None),
        Some(x) => x.as_i64().filter(|n| *n >= 0).map(|n| Some(n as usize)).ok_or_else(|| Error::Format(format!("{what} : entier positif attendu"))),
    }
}

/// Ajoute les pistes de sous-titres `cfg` a un clip encode (remuxage). Retourne le nouveau clip et les flux PG declares (PID, langue).
#[allow(clippy::too_many_arguments)]
fn add_subtitles(
    env: &mut BuildEnv, src: &Path, base: &Path, cfg: &[crate::subtitle::SubTrackCfg], lang: [u8; 3], tmp: &Path, m2ts: &[u8], num: usize,
) -> Result<(Vec<u8>, Vec<(u16, [u8; 3])>)> {
    use crate::subtitle::{load_cues, shift, SubSource};
    let abs = |f: &str| -> PathBuf { let q = Path::new(f); if q.is_absolute() { q.to_path_buf() } else { base.join(q) } };
    let mut specs = Vec::new();
    for (k, t) in cfg.iter().enumerate() {
        if let Some(m) = t.problems().into_iter().next() { return bad(format!("video {num}, sous-titres {} : {m}", k + 1)); }
        let tl: [u8; 3] = t.lang.as_bytes().try_into().map_err(|_| Error::Format(format!("video {num}, sous-titres {} : langue invalide", k + 1)))?;
        let cues = match &t.source {
            SubSource::File(f) => load_cues(&abs(f)).map_err(Error::Format)?,
            SubSource::Embedded(idx) => {
                let dst = tmp.join(format!("sous_titres_{num}_{idx}.srt"));
                ffmpeg::extract_subtitle_srt(env.tools, src, *idx, &dst)?;
                load_cues(&dst).map_err(Error::Format)?
            }
        };
        let mut style = t.style.clone();
        if !style.font.is_empty() {
            let c = abs(&style.font);
            if c.is_file() { style.font = c.to_string_lossy().into_owned(); }
        }
        specs.push(crate::film::SubSpec { lang: tl, cues: shift(&cues, t.offset_ms), style });
    }
    let enc = crate::film::load_encoded_m2ts(m2ts)?;
    let tracks = crate::film::encode_sub_tracks(&enc, &specs, env.renderer)?;
    let mut kept = Vec::new();
    for (k, t) in tracks.into_iter().enumerate() {
        if t.specs.is_empty() {
            (env.log)(&format!("   sous-titres {} : aucune replique affichable, piste ignoree", k + 1));
        } else {
            kept.push(t);
        }
    }
    let n_sets: usize = kept.iter().map(|t| t.specs.len()).sum();
    let (out, _) = crate::film::mux_encoded_subs(&enc, lang, &kept)?;
    (env.log)(&format!("   {} piste(s) de sous-titres ajoutee(s) ({} segments)", kept.len(), n_sets));
    Ok((out, kept.iter().enumerate().map(|(k, t)| (crate::pgs::PG_PID0 + k as u16, t.lang)).collect()))
}

pub fn build_project(project_path: &Path, out_dir: &Path, env: &mut BuildEnv) -> Result<BuildInfo> {
    let base = std::fs::canonicalize(project_path).unwrap_or_else(|_| project_path.to_path_buf());
    let base = base.parent().map(Path::to_path_buf).unwrap_or_default();
    let proj = json::parse(&std::fs::read_to_string(project_path)?).map_err(|e| Error::Format(e.to_string()))?;
    let vids = proj.get("videos").and_then(Value::as_array).ok_or_else(|| Error::Format("champ « videos » (liste) absent".into()))?;
    let n = vids.len();
    if !(1..=MAX_VIDEOS).contains(&n) {
        return bad(format!("1 a {MAX_VIDEOS} videos attendues, {n} donnees"));
    }
    let lang_s = proj.get("langue").and_then(Value::as_str).unwrap_or("und");
    let lang: [u8; 3] = lang_s.as_bytes().try_into().map_err(|_| Error::Format(format!("langue « {lang_s} » : 3 lettres attendues (ex. fra)")))?;
    let empty = Value::Obj(vec![]);
    let enc_cfg = proj.get("encodage").unwrap_or(&empty);
    let mut eopts = EncodeOpts::default();
    if let Some(c) = get_usize(enc_cfg.get("crf"), "encodage.crf")? { eopts.crf = c as u32; }
    if let Some(c) = get_usize(enc_cfg.get("maxrate"), "encodage.maxrate")? { eopts.maxrate = c as u32; }
    if let Some(c) = get_usize(enc_cfg.get("keyint"), "encodage.keyint")? { eopts.keyint = c as u32; }
    if let Some(s) = enc_cfg.get("preset").and_then(Value::as_str) { eopts.preset = s.to_string(); }
    if let Some(s) = enc_cfg.get("audio_bitrate").and_then(Value::as_str) { eopts.audio_bitrate = s.to_string(); }
    let menu_cfg = proj.get("menu").unwrap_or(&empty);
    let par_page = get_usize(menu_cfg.get("par_page"), "menu.par_page")?;
    let title = proj.get("titre").and_then(Value::as_str).unwrap_or("Menu").to_string();
    let boucle = menu_cfg.get("boucle").and_then(Value::as_bool).unwrap_or(false);
    let fond_s = menu_cfg.get("fond_secondes").and_then(Value::as_f64).unwrap_or(5.0);
    let retour_ch = menu_cfg.get("retour").and_then(Value::as_str).unwrap_or("chapitres") == "chapitres";
    let p = |f: &str| -> PathBuf { let q = Path::new(f); if q.is_absolute() { q.to_path_buf() } else { base.join(q) } };
    let vfile = |v: &Value| -> Result<String> { v.get("fichier").and_then(Value::as_str).map(String::from).ok_or_else(|| Error::Format("video sans champ « fichier »".into())) };
    let vtitle = |v: &Value| -> String {
        v.get("titre").and_then(Value::as_str).filter(|s| !s.is_empty()).map(String::from).unwrap_or_else(|| {
            Path::new(v.get("fichier").and_then(Value::as_str).unwrap_or("")).file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default()
        })
    };
    let cache_dir = if menu_cfg.get("cache").and_then(Value::as_bool).unwrap_or(true) { Some(base.join(".cache_bdauthor")) } else { None };

    let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0);
    let tmp = TmpDir(std::env::temp_dir().join(format!("bdauthor_build_{}_{nanos}", std::process::id())));
    std::fs::create_dir_all(&tmp.0)?;
    let mut files: BTreeMap<String, Vec<u8>> = BTreeMap::new();
    let mut scans: Vec<ClipScan> = Vec::new();
    let mut durations: Vec<f64> = Vec::new();
    let mut pg_info: Vec<Vec<(u16, [u8; 3])>> = Vec::new();

    // 1. videos
    for (i, v) in vids.iter().enumerate() {
        let name = vfile(v)?;
        let src = p(&name);
        (env.log)(&format!("[{}/{n}] encodage de {name} ...", i + 1));
        let info = ffmpeg::probe(env.tools, &src)?;
        durations.push(info.duration_s);
        let cached = match &cache_dir {
            Some(d) => {
                let f = d.join(format!("{}.m2ts", cache_key(&src, lang, &eopts)?));
                std::fs::read(&f).ok().filter(|b| !b.is_empty() && b.len() % 192 == 0).map(|b| (b, f))
            }
            None => None,
        };
        let m2ts = match cached {
            Some((b, _)) => { (env.log)("   (deja encodee : cache)"); b }
            None => {
                let (lo, hi) = (i as f64 / n as f64 * 0.85, (i + 1) as f64 / n as f64 * 0.85);
                let progress = &mut *env.progress;
                let (m, st) = make_film_m2ts(env.tools, &src, lang, &eopts, None, &mut |f| progress(lo + (hi - lo) * f), env.cancel)?;
                (env.log)(&format!("   {} : {} images, {} trames audio", st.cible, st.images, st.trames_audio));
                if let Some(d) = &cache_dir {
                    std::fs::create_dir_all(d)?;
                    let f = d.join(format!("{}.m2ts", cache_key(&src, lang, &eopts)?));
                    let t = f.with_extension("m2ts.tmp");
                    std::fs::write(&t, &m)?;
                    std::fs::rename(&t, &f)?;
                }
                m
            }
        };
        // sous-titres PGS : ajoutes par remuxage du clip encode (le cache ne contient pas les sous-titres)
        let sub_cfg = crate::subtitle::tracks_from_value(v.get("soustitres")).map_err(Error::Format)?;
        let (m2ts, pg) = if sub_cfg.is_empty() { (m2ts, Vec::new()) } else { add_subtitles(env, &src, &base, &sub_cfg, lang, &tmp.0, &m2ts, i + 1)? };
        let sc = scan_clip(&m2ts)?;
        let pg_langs: Vec<[u8; 3]> = pg.iter().map(|x| x.1).collect();
        files.insert(format!("CLIPINF/{i:05}.clpi"), make_clip_clpi_pg(&m2ts, lang, 0, None, "0100", &pg_langs)?);
        pg_info.push(pg);
        files.insert(format!("STREAM/{i:05}.m2ts"), m2ts);
        scans.push(sc);
    }
    (env.progress)(0.86);
    let mut chapters: Vec<Vec<Chapter>> = Vec::new();
    for (v, sc) in vids.iter().zip(&scans) {
        chapters.push(parse_chapters(v, sc)?);
    }
    let with_ch: Vec<usize> = (0..n).filter(|&i| !chapters[i].is_empty()).collect();
    let designs = match proj.get("mise_en_page") {
        Some(v) => Designs::from_value(v).map_err(|e| Error::Format(format!("mise_en_page : {e}")))?,
        None => Designs::default(),
    };
    let main_pg = paginate(n, par_page)?;
    // pages libres (apres les pages automatiques) : chacune doit etre valide, sinon la construction s'arrete
    let free_pages: &[PageDesign] = &designs.free;
    let total_main = main_pg.len() + free_pages.len();
    for (f, d) in free_pages.iter().enumerate() {
        let errs: Vec<String> = d.validate_free(main_pg.len() + f, total_main, n).into_iter().filter(|x| x.error).map(|x| x.msg).collect();
        if !errs.is_empty() { return bad(format!("page libre {} : {}", f + 1, errs.join(" "))); }
    }
    let mut ch_pg: BTreeMap<usize, Vec<Vec<usize>>> = BTreeMap::new();
    for &i in &with_ch {
        ch_pg.insert(i, paginate(chapters[i].len(), par_page)?);
    }
    let mut video_page = vec![0usize; n];
    for (pg, idxs) in main_pg.iter().enumerate() {
        for &i in idxs { video_page[i] = pg; }
    }
    let ch_counts: BTreeMap<usize, usize> = ch_pg.iter().map(|(&i, g)| (i, g.len())).collect();
    let plan = plan_navigation(n, total_main, &ch_counts);
    // titre visé par un bouton « Aller a » ; None si la page n'existe pas
    let goto = |t: Target| -> Option<u32> {
        match t {
            Target::Main(p) => plan.main_titles.get(p).copied(),
            Target::Chapters(v, q) => plan.chapters.get(&v).and_then(|c| c.pages.get(q).copied()),
        }
        .map(|x| x as u32)
    };
    for d in designs.main.values().chain(designs.chapters.values()).chain(designs.free.iter()) {
        for it in &d.items {
            if let Kind::Nav { action: NavAction::Goto, text, target } = &it.kind {
                if target.and_then(&goto).is_none() { return bad(format!("bouton « {text} » : page cible inexistante ou non choisie")); }
            }
        }
    }
    let first = &scans[0];
    let fmt = (first.width, first.height, first.fps);
    for (i, sc) in scans.iter().enumerate().skip(1) {
        if (sc.width, sc.height, sc.fps) != fmt {
            (env.log)(&format!("avertissement : la video {} est en {}x{} {} i/s, la premiere en {}x{} {} i/s (un changement de format peut provoquer un temps mort a la lecture)", i + 1, sc.width, sc.height, sc.fps, fmt.0, fmt.1, fmt.2));
        }
    }
    let size = (first.width, first.height);
    let menu_size = if menu_cfg.get("taille").and_then(Value::as_str) == Some("1080") { (1920, 1080) } else { size };
    let sc_ = menu_size.0 as f64 / 1920.0;

    let still_opts = EncodeOpts { crf: eopts.crf, preset: eopts.preset.clone(), ..Default::default() };
    let custom_fond: Option<PathBuf> = menu_cfg.get("fond").and_then(Value::as_str).map(|f| p(f));
    let sd = size.0 as f64 / 1920.0; // echelle des textes/images poses sur le fond (qui a la taille de la video)
    // (rang dans l'ordre de superposition, decor) : le rang sert a poser sur les boutons ce qui est au-dessus d'eux
    let scale_decor = |d: &PageDesign| -> Vec<(usize, Decor)> {
        d.items.iter().enumerate().filter_map(|(rank, it)| {
            let (x, y, w, h) = it.scaled(sd);
            match &it.kind {
                Kind::Text { text, size, color, align, font } => {
                    let font = if font.is_empty() { String::new() } else { let c = p(font); if c.is_file() { c.to_string_lossy().into_owned() } else { font.clone() } };
                    Some((rank, Decor::Text { text: text.clone(), x, y, w, h, size: py_round(*size as f64 * sd) as i32, color: *color, align: *align, font }))
                }
                Kind::Image { path } => Some((rank, Decor::Image { path: p(path), x, y, w, h })),
                _ => None,
            }
        }).collect()
    };
    let add_menu_page = |files: &mut BTreeMap<String, Vec<u8>>, buttons: &[MenuButton], pix: &[u64], ig_num: u32, pl_num: u32, bg_num: u32, bgscan: &ClipScan, label: &str| -> Result<MenuStats> {
        let pts = required_ics_pts(pix, (menu_size.0 as u64, menu_size.1 as u64), 42956, 9000);
        let clip = build_menu_clip(buttons, &MenuOptions { ics_pts: pts, video_size: menu_size, ui_model: 0, rgb_colors: 63 }).map_err(Error::Format)?;
        if clip.m2ts.len() > 6 * 1024 * 1024 {
            return bad(format!("{label} trop lourd ({:.1} Mo) : reduire le nombre d'elements par page (menu.par_page)", clip.m2ts.len() as f64 / 1e6));
        }
        let stats = MenuStats { pts, bytes: clip.m2ts.len(), objects: clip.menu.ods.len(), palette: clip.menu.pds.entries.len() };
        files.insert(format!("STREAM/{ig_num:05}.m2ts"), clip.m2ts);
        files.insert(format!("CLIPINF/{ig_num:05}.clpi"), clip.clpi);
        files.insert(format!("PLAYLIST/{pl_num:05}.mpls"), make_playlist(&PlaylistParams {
            clip: format!("{bg_num:05}"), in_time: (bgscan.start90 / 2) as u32, out_time: (bgscan.end90 / 2) as u32,
            video: VideoStn { pid: bgscan.video.0, coding: bgscan.video.1, fmt_rate: bgscan.video_code }, audio: None,
            with_menu: true, menu_clip: format!("{ig_num:05}"), menu_in: (pts / 2) as u32, menu_out: (pts / 2 + 1501) as u32,
            still_mode: if boucle { 0 } else { 2 }, // 2 = arret infini sur la derniere image : le menu ne redemarre pas
            ..Default::default()
        }));
        Ok(stats)
    };

    // 2. fond de menu commun (clip image fixe, sans audio)
    let bg_png: PathBuf = match &custom_fond {
        Some(f) => f.clone(),
        None => {
            let f = tmp.0.join("fond.png");
            std::fs::write(&f, crate::png::write_png(&env.renderer.background(&title, size)))?;
            f
        }
    };
    (env.log)("encodage du fond de menu ...");
    let fps = fps_str(first.fps)?;
    let bg_sc = still_clip(env.tools, env.cancel, &bg_png, fond_s, size, fps, &still_opts, &mut files, n as u32)?;
    (env.progress)(0.9);

    // 3. menu principal : une page par groupe de videos (page 1 = clip IG 80002, playlist N)
    let main_pls: Vec<u32> = std::iter::once(n as u32).chain((1..total_main).map(|q| (n + q) as u32)).collect();
    let mut next_pl = (n + total_main) as u32;
    let (mut next_ig, mut next_bg) = (80003u32, (n + 1) as u32);
    let mut first_page: Option<MenuStats> = None;
    for (pg, idxs) in main_pg.iter().enumerate() {
        let what = format!("menu principal, page {}", pg + 1);
        let (design, custom) = resolve_design(designs.main.get(&pg), idxs.len(), MenuType::Main, pg, total_main, &what, &mut *env.log)?;
        let mut slots = Vec::new();
        for &i in idxs {
            let v = &vids[i];
            slots.push(SlotSpec {
                label: vtitle(v), source: p(&vfile(v)?), at: v.get("vignette_s").and_then(Value::as_f64), duration: durations[i], commands: vec![],
                go: plan.chapters.get(&i).map_or(i as u32 + 1, |c| c.pages[0] as u32), // video a chapitres : le bouton ouvre le menu de chapitres
            });
        }
        let target = |a: NavAction, t: Option<Target>| match a {
            NavAction::Prev => plan.main_titles[pg.saturating_sub(1)] as u32,
            NavAction::Next => plan.main_titles[(pg + 1).min(total_main - 1)] as u32,
            NavAction::Goto => t.and_then(&goto).unwrap_or(0),
            _ => 0,
        };
        let decor_r = scale_decor(&design);
        let (buttons, pix) = page_buttons(env.tools, env.renderer, &design, &design.slot_assignment(slots.len()), &decor_r, MenuType::Main, pg, total_main, &slots, &target, sc_)?;
        let decor: Vec<Decor> = decor_r.into_iter().map(|(_, d)| d).collect();
        let (bg_num, pscan) = if custom && !(custom_fond.is_some() && decor.is_empty()) {
            let num = next_bg;
            next_bg += 1;
            let sc = own_background(env, &custom_fond, size, &decor, &tmp.0, &format!("fond_p{pg}.png"), fond_s, fps, &still_opts, &mut files, num)?;
            (num, sc)
        } else {
            (n as u32, bg_sc.clone())
        };
        let ig_num = if pg == 0 { 80002 } else { let k = next_ig; next_ig += 1; k };
        let res = add_menu_page(&mut files, &buttons, &pix, ig_num, main_pls[pg], bg_num, &pscan, &what)?;
        if pg == 0 { first_page = Some(res); }
    }
    // pages libres du menu principal : vignettes de n'importe quelle video, fond propre a chaque page
    for (f, design) in free_pages.iter().enumerate() {
        let pg = main_pg.len() + f;
        let what = format!("menu principal, page libre {}", f + 1);
        let mut slots = Vec::new();
        for it in design.items.iter() {
            if let Kind::Slot { assign: Some(i), .. } = it.kind {
                let v = &vids[i];
                slots.push(SlotSpec {
                    label: vtitle(v), source: p(&vfile(v)?), at: v.get("vignette_s").and_then(Value::as_f64), duration: durations[i], commands: vec![],
                    go: plan.chapters.get(&i).map_or(i as u32 + 1, |c| c.pages[0] as u32),
                });
            }
        }
        let target = |a: NavAction, t: Option<Target>| match a {
            NavAction::Prev => plan.main_titles[pg.saturating_sub(1)] as u32,
            NavAction::Next => plan.main_titles[(pg + 1).min(total_main - 1)] as u32,
            NavAction::Goto => t.and_then(&goto).unwrap_or(0),
            _ => 0,
        };
        let decor_r = scale_decor(design);
        let identity: Vec<usize> = (0..slots.len()).collect();
        let (buttons, pix) = page_buttons(env.tools, env.renderer, design, &identity, &decor_r, MenuType::Main, pg, total_main, &slots, &target, sc_)?;
        let decor: Vec<Decor> = decor_r.into_iter().map(|(_, d)| d).collect();
        for l in design.validate_free(pg, total_main, n).iter().filter(|x| !x.error) { (env.log)(&format!("{what} : {}", l.msg)); }
        let (bg_num, pscan) = if custom_fond.is_some() && decor.is_empty() {
            (n as u32, bg_sc.clone())
        } else {
            let num = next_bg;
            next_bg += 1;
            let sc = own_background(env, &custom_fond, size, &decor, &tmp.0, &format!("fond_libre{f}.png"), fond_s, fps, &still_opts, &mut files, num)?;
            (num, sc)
        };
        let ig_num = { let k = next_ig; next_ig += 1; k };
        add_menu_page(&mut files, &buttons, &pix, ig_num, main_pls[pg], bg_num, &pscan, &what)?;
    }
    let first_page = first_page.expect("au moins une page");
    (env.progress)(0.95);

    // 4. menus de chapitres (un fond par video si pas de fond commun ; une page par groupe de chapitres)
    let mut ch_pls: BTreeMap<usize, Vec<u16>> = BTreeMap::new();
    for (k, &i) in with_ch.iter().enumerate() {
        (env.log)(&format!("menu de chapitres de la video {} ({} chapitres, {} page(s)) ...", i + 1, chapters[i].len(), ch_pg[&i].len()));
        let npg = ch_pg[&i].len();
        let retour_t = plan.main_titles[video_page[i]] as u32;
        let titles = &plan.chapters[&i];
        let mut shared: Option<(u32, ClipScan)> = if custom_fond.is_some() { Some((n as u32, bg_sc.clone())) } else { None };
        let mut pls = Vec::new();
        for (q, idxs) in ch_pg[&i].iter().enumerate() {
            let what = format!("menu de chapitres {} (page {})", i + 1, q + 1);
            let (design, custom) = resolve_design(designs.chapters.get(&(i, q)), idxs.len(), MenuType::Chapters, q, npg, &what, &mut *env.log)?;
            let (pl_num, ig_num) = (next_pl, next_ig);
            next_pl += 1;
            next_ig += 1;
            pls.push(pl_num as u16);
            let mut slots = Vec::new();
            for &j in idxs {
                let ch = &chapters[i][j];
                let secs = py_round(ch.t_rel) as i64;
                slots.push(SlotSpec {
                    label: format!("{} ({}:{:02})", ch.name, secs / 60, secs % 60), source: p(&vfile(&vids[i])?), at: Some(ch.t_rel), duration: durations[i],
                    commands: vec![Command::new(MOVE_RI_INSN, 0, j as u32), Command::new(JUMP_TITLE_INSN, titles.play as u32, 0)], go: 0,
                });
            }
            let target = |a: NavAction, t: Option<Target>| match a {
                NavAction::Prev => titles.pages[q.saturating_sub(1)] as u32,
                NavAction::Next => titles.pages[(q + 1).min(npg - 1)] as u32,
                NavAction::PlayAll => i as u32 + 1,
                NavAction::Back => retour_t,
                NavAction::Goto => t.and_then(&goto).unwrap_or(0),
            };
            let decor_r = scale_decor(&design);
            let (cbuttons, pix) = page_buttons(env.tools, env.renderer, &design, &design.slot_assignment(slots.len()), &decor_r, MenuType::Chapters, q, npg, &slots, &target, sc_)?;
            let decor: Vec<Decor> = decor_r.into_iter().map(|(_, d)| d).collect();
            let (bg_num, pscan) = if custom && !(custom_fond.is_some() && decor.is_empty()) {
                let num = next_bg;
                next_bg += 1;
                let sc = own_background(env, &custom_fond, size, &decor, &tmp.0, &format!("fond_ch{k}_{q}.png"), fond_s, fps, &still_opts, &mut files, num)?;
                (num, sc)
            } else {
                if shared.is_none() {
                    let num = next_bg;
                    next_bg += 1;
                    let cbg = tmp.0.join(format!("fond_ch{k}.png"));
                    std::fs::write(&cbg, crate::png::write_png(&env.renderer.background(&format!("{} - chapitres", vtitle(&vids[i])), size)))?;
                    shared = Some((num, still_clip(env.tools, env.cancel, &cbg, fond_s, size, fps, &still_opts, &mut files, num)?));
                }
                let (num, sc) = shared.clone().unwrap();
                (num, sc)
            };
            add_menu_page(&mut files, &cbuttons, &pix, ig_num, pl_num, bg_num, &pscan, &what)?;
        }
        ch_pls.insert(i, pls);
    }

    // 5. playlists des videos et navigation
    for (i, sc) in scans.iter().enumerate() {
        let Some(&(pid_a, type_a, code_a)) = sc.audio_codes.first() else { return bad(format!("la video {} n'a pas de piste audio exploitable", i + 1)) };
        files.insert(format!("PLAYLIST/{i:05}.mpls"), make_playlist(&PlaylistParams {
            clip: format!("{i:05}"), in_time: (sc.start90 / 2) as u32, out_time: (sc.end90 / 2) as u32,
            video: VideoStn { pid: sc.video.0, coding: sc.video.1, fmt_rate: sc.video_code },
            audio: Some(AudioStn { pid: pid_a, coding: type_a, fmt_rate: code_a, lang }),
            with_menu: false, pg: pg_info[i].clone(), marks: if chapters[i].is_empty() { None } else { Some(chapters[i].iter().map(|c| c.mark45).collect()) },
            ..Default::default()
        }));
    }
    let main_pls16: Vec<u16> = main_pls.iter().map(|&x| x as u16).collect();
    files.insert("MovieObject.bdmv".into(), make_movie_objects_paged(n, &main_pls16, &plan.main_titles, &ch_pls, &plan.chapters, &video_page, retour_ch));
    let title_objs: Vec<u16> = (1..=plan.total_titles).collect();
    files.insert("index.bdmv".into(), make_index(0, 0, &title_objs, b"bdauthor"));
    write_bdmv(out_dir, &files)?;
    let report = check_bdmv(&out_dir.join("BDMV"));
    (env.progress)(1.0);
    Ok(BuildInfo {
        videos: n, main_pages: total_main,
        chapters: chapters.iter().enumerate().filter(|(_, c)| !c.is_empty()).map(|(i, c)| (i + 1, c.len())).collect(),
        chapter_pages: ch_pg.iter().map(|(i, g)| (i + 1, g.len())).collect(),
        format: fmt, menu_size, ics_pts_s: first_page.pts as f64 / 90000.0, menu_bytes: first_page.bytes,
        menu_objects: first_page.objects, palette_entries: first_page.palette,
        total_stream_bytes: files.iter().filter(|(k, _)| k.starts_with("STREAM")).map(|(_, v)| v.len()).sum(),
        durations_s: scans.iter().map(|sc| ((sc.end90 - sc.start90) as f64 / 90000.0 * 100.0).round() / 100.0).collect(),
        report,
    })
}
