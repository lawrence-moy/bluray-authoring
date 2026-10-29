//! Disque multi-titres a partir d'un fichier de projet JSON (equivalent de `project.py`).
//!
//! Format du projet : voir `examples/projet_exemple.json` et le README. Structure produite : videos = clips/playlists 0..N-1
//! (titres 1..N), fond de menu = clip N (image fixe, sans audio), playlist de menu N (fond + menu IG), clip IG 80002 ;
//! pagination au-dela de 12 elements (voir `paginate`), menus de chapitres, cache d'encodage.
//! Le dessin (vignettes, libelles, fond) est delegue a un `Renderer` : le crate `bdauthor-render` en fournit un complet.
use crate::check::{check_bdmv, Report};
use crate::style::ButtonStyle;
use crate::design::{nav_applicable, Align, Designs, Item, Kind, MenuType, NavAction, PageDesign, PageMusic, PageVisual, Target};
use crate::disc::write_bdmv;
use crate::ffmpeg::{self, EncodeOpts, Error, Tools};
use crate::film::{make_film_m2ts, make_still_m2ts};
use crate::ig::py_round;
use crate::json::{self, Value};
use crate::menu::{build_menu_clip, required_ics_pts, MenuButton, MenuOptions};
use crate::mobj::{Command, MovieObject, MovieObjectBdmv, GOTO_INSN, JUMP_TITLE_INSN, MOVE_RI_INSN, NOP_INSN, PLAY_PL_INSN};
use crate::nav::{make_index, make_movie_objects_paged, make_playlist, make_playlist_multi, plan_navigation, AudioStn, PlaylistParams, VideoStn};
use crate::palette::RgbaImage;
use crate::tsclip::{make_clip_clpi, make_clip_clpi_pg, py_floordiv, scan_clip, ClipScan, FRAME_RATES};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;
use std::sync::Arc;

pub const MAX_PER_PAGE: usize = 12; // elements par page de menu (au-dela : pagination)
pub const MAX_VIDEOS: usize = 99;
pub const MAX_CHAPTERS: usize = 99;

type Result<T> = std::result::Result<T, Error>;
fn bad<T>(m: impl Into<String>) -> Result<T> { Err(Error::Format(m.into())) }

/// Dessin des elements de menu. Les images sont en RGBA 8 bits.
///
/// Les boutons sont rendus en trois etats (normal, selectionne, active) selon un `ButtonStyle` deja a l'echelle du menu. Chaque image
/// d'etat mesure (w + 2·mx) x (h + 2·my), ou (mx, my) = `style.margins(w, h)` (place du halo et de l'agrandissement), l'element
/// occupant le centre. Un etat identique a un autre peut etre le meme `Arc` (l'image n'est alors stockee qu'une fois).
/// Une image d'une autre taille est recentree par le constructeur.
pub trait Renderer {
    /// Bouton « vignette + libelle » (`h_thumb` = hauteur de la vignette, le reste porte le libelle).
    fn thumb_button(&self, thumb: &RgbaImage, label: &str, w: u32, h_total: u32, h_thumb: u32, style: &ButtonStyle) -> [Arc<RgbaImage>; 3];
    /// Bouton uniquement textuel (« Tout lire », « Retour »...).
    fn text_button(&self, label: &str, w: u32, h: u32, style: &ButtonStyle) -> [Arc<RgbaImage>; 3];
    /// Fond de menu par defaut avec le titre.
    fn background(&self, title: &str, size: (u32, u32)) -> RgbaImage;
    /// Fond d'une page personnalisee : image `base` (ou degrade par defaut, sans titre) + textes et images, en pixels du fond.
    fn compose_background(&self, base: Option<&Path>, size: (u32, u32), decor: &[Decor]) -> std::result::Result<RgbaImage, String>;
    /// Calque transparent de `size` portant les textes et images `decor` (pose sur un fond video). Par defaut : calque vide.
    fn compose_layer(&self, size: (u32, u32), decor: &[Decor]) -> std::result::Result<RgbaImage, String> {
        let _ = decor;
        Ok(RgbaImage::new(size.0, size.1))
    }
    /// Pose `decor` (coordonnees relatives a l'image du bouton) par-dessus les etats d'un bouton : textes et images
    /// places au-dessus de ce bouton dans l'ordre de superposition. Par defaut, les etats sont rendus tels quels.
    fn overlay(&self, states: [Arc<RgbaImage>; 3], decor: &[Decor]) -> [Arc<RgbaImage>; 3] {
        let _ = decor;
        states
    }
    /// Texte ou image cliquable : `decor` est deja place dans un cadre de `w` x `h` (coordonnees relatives). Par defaut : un bouton sans libelle.
    fn decor_button(&self, decor: &Decor, w: u32, h: u32, style: &ButtonStyle) -> [Arc<RgbaImage>; 3] {
        let _ = decor;
        self.text_button("", w, h, style)
    }
    /// Bouton dessine par l'utilisateur : images (chemins complets) des etats normal, selectionne, active ; un etat sans image est
    /// derive de l'image normale avec les effets du style. Erreur si ce moteur ne sait pas charger d'images.
    fn skin_button(&self, skin: &[Option<PathBuf>; 3], w: u32, h: u32, style: &ButtonStyle) -> std::result::Result<[Arc<RgbaImage>; 3], String> {
        let _ = (skin, w, h, style);
        Err("images d'etats non gerees par ce moteur de dessin".into())
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

/// Arrete la construction sur des erreurs de mise en page : chaque erreur est ecrite dans le journal, puis l'ensemble est renvoye.
fn layout_failure<T>(errs: &[String], log: &mut dyn FnMut(&str)) -> Result<T> {
    for e in errs { log(&format!("ERREUR de mise en page : {e}")); }
    bad(format!("{} erreur(s) de mise en page, a corriger dans l'editeur : {}", errs.len(), errs.join(" | ")))
}

/// Mise en page personnalisee de la page si elle existe (la construction s'arrete si elle contient une erreur), sinon la grille
/// automatique. Retourne (mise en page, personnalisee ?).
fn resolve_design(ov: Option<&PageDesign>, expected: usize, menu: MenuType, page: usize, pages: usize, what: &str, log: &mut dyn FnMut(&str)) -> Result<(PageDesign, bool)> {
    if let Some(d) = ov {
        let probs = d.validate(expected, menu, page, pages);
        let errs: Vec<String> = probs.iter().filter(|x| x.error).map(|x| format!("{what} : {}", x.msg)).collect();
        if !errs.is_empty() {
            return layout_failure(&errs, log);
        }
        for w in probs.iter().filter(|x| !x.error) { log(&format!("{what} : {}", w.msg)); }
        return Ok((d.clone(), true));
    }
    let d = match menu {
        MenuType::Main => PageDesign::default_main(expected, page, pages),
        MenuType::Chapters => PageDesign::default_chapter(expected, page, pages),
    };
    Ok((d.map_err(Error::Format)?, false))
}

/// Image `img` centree dans un cadre transparent de `w` x `h` (rognee si plus grande).
fn recenter(img: &RgbaImage, w: u32, h: u32) -> RgbaImage {
    if img.width == w && img.height == h {
        return img.clone();
    }
    let mut out = RgbaImage::new(w, h);
    let (dx, dy) = ((w as i64 - img.width as i64) / 2, (h as i64 - img.height as i64) / 2);
    for y in 0..img.height as i64 {
        for x in 0..img.width as i64 {
            let (tx, ty) = (x + dx, y + dy);
            if tx >= 0 && ty >= 0 && tx < w as i64 && ty < h as i64 {
                out.set(tx as u32, ty as u32, img.pixel(x as u32, y as u32));
            }
        }
    }
    out
}

/// Morceau (x0, y0, w, h) d'une image.
fn sub_image(img: &RgbaImage, x0: u32, y0: u32, w: u32, h: u32) -> RgbaImage {
    let mut out = RgbaImage::new(w, h);
    for y in 0..h {
        for x in 0..w {
            out.set(x, y, img.pixel(x0 + x, y0 + y));
        }
    }
    out
}

/// Applique `f` a chaque etat en conservant le partage des etats identiques ; retourne aussi le nombre d'images distinctes.
fn map_states(st: [Arc<RgbaImage>; 3], f: &dyn Fn(&RgbaImage) -> RgbaImage) -> ([Arc<RgbaImage>; 3], u64) {
    let n = Arc::new(f(&st[0]));
    let s = if Arc::ptr_eq(&st[1], &st[0]) { n.clone() } else { Arc::new(f(&st[1])) };
    let a = if Arc::ptr_eq(&st[2], &st[1]) { s.clone() } else if Arc::ptr_eq(&st[2], &st[0]) { n.clone() } else { Arc::new(f(&st[2])) };
    let distinct = 1 + (!Arc::ptr_eq(&s, &n)) as u64 + (!Arc::ptr_eq(&a, &s) && !Arc::ptr_eq(&a, &n)) as u64;
    ([n, s, a], distinct)
}

/// Ce dont `page_buttons` a besoin en plus de la page elle-meme.
struct ButtonEnv<'a> {
    tools: &'a Tools,
    renderer: &'a dyn Renderer,
    /// style des boutons du projet
    style: &'a ButtonStyle,
    /// chemin complet d'un fichier du projet
    abs: &'a dyn Fn(&str) -> PathBuf,
    /// dessin d'un texte ou d'une image a une echelle donnee (textes et images cliquables)
    to_decor: &'a dyn Fn(&Item, f64) -> Option<Decor>,
    /// une entree par video du projet (vignettes montrant une video choisie)
    videos: &'a [SlotSpec],
    /// echelle du plan 1920 vers le menu, taille du menu
    s: f64,
    plane: (u32, u32),
}

/// Boutons (et surface d'objets graphiques de chacun) d'une page d'apres sa mise en page.
/// `decor` : textes et images non cliquables a l'echelle du menu ; `slots` : elements de la page.
#[allow(clippy::too_many_arguments)]
fn page_buttons(e: &ButtonEnv, design: &PageDesign, assignment: &[usize], decor: &[(usize, Decor)], menu: MenuType, page: usize, pages: usize, slots: &[SlotSpec], target: &dyn Fn(NavAction, Option<Target>) -> u32) -> Result<(Vec<MenuButton>, Vec<u64>)> {
    let (renderer, s) = (e.renderer, e.s);
    let (mut buttons, mut pix, mut k) = (Vec::new(), Vec::new(), 0usize);
    for (idx, it) in design.items.iter().enumerate() {
        let (x, y, w, h) = it.scaled(s);
        let st = it.look.style.as_ref().unwrap_or(e.style).scaled(s);
        let (mx, my) = st.margins(w, h);
        let (bx, by, bw, bh) = (x - mx, y - my, w + 2 * mx, h + 2 * my);
        // textes et images places au-dessus de ce bouton dans l'ordre de superposition : poses sur ses images
        let above: Vec<Decor> = decor.iter().filter(|(i, d)| *i > idx && d.intersects((bx, by, bw, bh))).map(|(_, d)| d.shifted(-bx, -by)).collect();
        let skin = &it.look.skin;
        let skinned = !skin.normal.is_empty() && !matches!(it.kind, Kind::Slot { .. });
        let skin_states = || -> Result<[Arc<RgbaImage>; 3]> {
            let one = |p: &str| if p.is_empty() { None } else { Some((e.abs)(p)) };
            renderer.skin_button(&[one(&skin.normal), one(&skin.selected), one(&skin.activated)], w as u32, h as u32, &st).map_err(Error::Format)
        };
        let (states, go, commands) = match &it.kind {
            Kind::Slot { thumb_h, video, link, .. } => {
                let sp = match video {
                    Some(Some(v)) => e.videos.get(*v).ok_or_else(|| Error::Format(format!("vignette : la video {} n'existe pas", v + 1)))?,
                    Some(None) => continue, // vignette sans video : absente du disque
                    None => {
                        let a = assignment.get(k).copied().unwrap_or(usize::MAX);
                        k += 1;
                        match slots.get(a) { Some(sp) => sp, None => continue }
                    }
                };
                let th = py_round(*thumb_h as f64 * s) as i32;
                let thumb = ffmpeg::extract_thumbnail_rgba(e.tools, &sp.source, (w as u32, th as u32), sp.at, sp.duration)?;
                let states = renderer.thumb_button(&thumb, &sp.label, w as u32, h as u32, th as u32, &st);
                // destination choisie : remplace le comportement habituel de la vignette
                match link {
                    Some(t) => (states, target(NavAction::Goto, Some(*t)), vec![]),
                    None => (states, sp.go, sp.commands.clone()),
                }
            }
            Kind::Nav { action, text, target: tg } if nav_applicable(*action, menu, page, pages) => {
                let states = if skinned { skin_states()? } else { renderer.text_button(text, w as u32, h as u32, &st) };
                (states, target(*action, *tg), vec![])
            }
            Kind::Text { .. } | Kind::Image { .. } if it.link().is_some() => {
                let states = if skinned {
                    skin_states()?
                } else {
                    let Some(d) = (e.to_decor)(it, s) else { continue };
                    renderer.decor_button(&d.shifted(-x, -y), w as u32, h as u32, &st)
                };
                (states, target(NavAction::Goto, it.link()), vec![])
            }
            _ => continue,
        };
        let states = if above.is_empty() { states } else { renderer.overlay(states, &above) };
        // taille attendue, puis rognage aux bords du menu (le halo ne doit pas deborder du plan graphique)
        let (l, t) = ((-bx).max(0), (-by).max(0));
        let (r, b) = ((bx + bw - e.plane.0 as i32).max(0), (by + bh - e.plane.1 as i32).max(0));
        let (cw, ch) = ((bw - l - r).max(1) as u32, (bh - t - b).max(1) as u32);
        let (states, distinct) = map_states(states, &|im| {
            let full = recenter(im, bw.max(1) as u32, bh.max(1) as u32);
            if (l, t, r, b) == (0, 0, 0, 0) { full } else { sub_image(&full, l as u32, t as u32, cw, ch) }
        });
        let [nrm, sel, act] = states;
        let mut btn = MenuButton::new("", go, bx + l, by + t, vec![nrm, sel, act]);
        btn.commands = commands;
        buttons.push(btn);
        pix.push(distinct * cw as u64 * ch as u64);
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

/// Fond anime (video) et/ou sonore (musique) des menus du projet.
struct MenuBg {
    video: Option<PathBuf>,
    /// garder le son de la video de fond (a defaut de musique)
    video_audio: bool,
    music: Option<PathBuf>,
    /// duree des fonds animes ou sonores (s)
    seconds: f64,
    lang: [u8; 3],
    cache: Option<PathBuf>,
}

impl MenuBg {
    fn animated(&self) -> bool { self.video.is_some() || self.music.is_some() }
}

/// Identite d'un fichier pour les cles de cache : chemin, taille, date de modification.
fn file_id(p: &Path) -> String {
    let md = std::fs::metadata(p).ok();
    let mtime = md.as_ref().and_then(|m| m.modified().ok()).and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map_or(0, |d| d.as_secs());
    format!("{}|{}|{mtime}", p.display(), md.map_or(0, |m| m.len()))
}

/// Clip de fond numero `num` : image fixe `image` (fond fixe sans son : comme avant), ou fond anime/sonore (video du projet, ou
/// `image` accompagnee de la musique), avec le calque `layer` (textes et images de la page) pose sur la video.
#[allow(clippy::too_many_arguments)]
fn menu_bg_clip(env: &BuildEnv, mb: &MenuBg, image: Option<&Path>, layer: Option<&RgbaImage>, tmp: &Path, name: &str, fond_s: f64, size: (u32, u32), fps: &str, opts: &EncodeOpts, files: &mut BTreeMap<String, Vec<u8>>, num: u32) -> Result<ClipScan> {
    if !mb.animated() {
        let img = image.ok_or_else(|| Error::Format("fond de menu : image manquante".into()))?;
        return still_clip(env.tools, env.cancel, img, fond_s, size, fps, opts, files, num);
    }
    let layer_png = match layer {
        Some(l) if l.data.chunks_exact(4).any(|p| p[3] > 0) => {
            let f = tmp.join(format!("{name}_calque.png"));
            let bytes = crate::png::write_png(l);
            std::fs::write(&f, &bytes)?;
            Some((f, fnv1a(&bytes)))
        }
        _ => None,
    };
    let visual = match &mb.video {
        Some(v) => ffmpeg::BgVisual::Video(v, mb.video_audio),
        None => ffmpeg::BgVisual::Image(image.ok_or_else(|| Error::Format("fond de menu : image manquante".into()))?),
    };
    let vis_id = match visual { ffmpeg::BgVisual::Video(v, a) => format!("{}|{a}", file_id(v)), ffmpeg::BgVisual::Image(i) => format!("{:x}", fnv1a(&std::fs::read(i)?)) };
    let key = format!(
        "{:016x}",
        fnv1a(format!("fond|{vis_id}|{:?}|{}|{size:?}|{fps}|{:.3}|{}|{}|{}|{}", layer_png.as_ref().map(|l| l.1), mb.music.as_deref().map(file_id).unwrap_or_default(),
                      mb.seconds, opts.crf, opts.preset, opts.audio_bitrate, String::from_utf8_lossy(&mb.lang)).as_bytes())
    );
    let cache_file = mb.cache.as_ref().map(|d| d.join(format!("fond_{key}.m2ts")));
    let m2ts = match cache_file.as_ref().and_then(|c| std::fs::read(c).ok()).filter(|b| !b.is_empty() && b.len() % 192 == 0) {
        Some(b) => b,
        None => {
            let (m, _) = crate::film::make_menu_bg_m2ts(env.tools, visual, layer_png.as_ref().map(|l| l.0.as_path()), mb.music.as_deref(), mb.seconds, size, fps, mb.lang, opts, None, env.cancel)?;
            if let Some(c) = &cache_file {
                std::fs::create_dir_all(c.parent().unwrap_or(Path::new(".")))?;
                let t = c.with_extension("m2ts.tmp");
                std::fs::write(&t, &m)?;
                std::fs::rename(&t, c)?;
            }
            m
        }
    };
    let sc = scan_clip(&m2ts)?;
    files.insert(format!("CLIPINF/{num:05}.clpi"), make_clip_clpi(&m2ts, mb.lang, 0, None, "0100")?);
    files.insert(format!("STREAM/{num:05}.m2ts"), m2ts);
    Ok(sc)
}

/// Fond propre a une page personnalisee : compose (base + textes + images), ou, avec un fond video, calque des textes et images
/// pose sur la video ; puis encode.
#[allow(clippy::too_many_arguments)]
fn own_background(env: &BuildEnv, mb: &MenuBg, base: &Option<PathBuf>, size: (u32, u32), decor: &[Decor], tmp: &Path, name: &str, seconds: f64, fps: &str, opts: &EncodeOpts, files: &mut BTreeMap<String, Vec<u8>>, num: u32) -> Result<ClipScan> {
    if mb.video.is_some() {
        let layer = env.renderer.compose_layer(size, decor).map_err(Error::Format)?;
        return menu_bg_clip(env, mb, None, Some(&layer), tmp, name, seconds, size, fps, opts, files, num);
    }
    let img = env.renderer.compose_background(base.as_deref(), size, decor).map_err(Error::Format)?;
    let f = tmp.join(name);
    std::fs::write(&f, crate::png::write_png(&img))?;
    menu_bg_clip(env, mb, Some(&f), None, tmp, name, seconds, size, fps, opts, files, num)
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

fn cache_key(path: &Path, lang: [u8; 3], o: &EncodeOpts, info: &ffmpeg::VideoInfo) -> Result<String> {
    let md = std::fs::metadata(path)?;
    let mtime = md.modified().ok().and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map_or(0, |d| d.as_secs());
    let abs = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    let mut s = format!("{}|{}|{}|{}|{}|{}|{}|{}|{}|v1", abs.display(), md.len(), mtime, String::from_utf8_lossy(&lang), o.crf, o.maxrate, o.keyint, o.audio_bitrate, o.preset);
    // cibles apparues apres la premiere version (PAL, entrelace) : cle differente, pour ne pas reprendre un ancien encodage ;
    // les autres cibles gardent leur cle (pas de reencodage inutile)
    let tg = ffmpeg::pick_target_scan(info.width, info.height, info.fps(), info.interlaced().then(|| info.top_first()));
    if !matches!(tg.label, "1080p" | "720p59.94" | "720p50" | "720p59.94 (cadence doublee)" | "720p24") {
        s.push_str(&format!("|cible={}", tg.label));
    }
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
    // mises en page du menu principal et pages libres verifiees avant tout encodage (les menus de chapitres, dont le nombre de
    // chapitres depend des videos encodees, sont verifies a leur construction)
    if let Some(v) = proj.get("mise_en_page") {
        let ds = Designs::from_value(v).map_err(|e| Error::Format(format!("mise_en_page : {e}")))?;
        let mp = paginate(n, par_page)?;
        let tm = mp.len() + ds.free.len();
        let mut errs = Vec::new();
        for (&pg, d) in &ds.main {
            let Some(items) = mp.get(pg) else { continue };
            errs.extend(d.validate(items.len(), MenuType::Main, pg, tm).into_iter().filter(|x| x.error).map(|x| format!("menu principal (page {}) : {}", pg + 1, x.msg)));
        }
        for (f, d) in ds.free.iter().enumerate() {
            errs.extend(d.validate_free(mp.len() + f, tm, n).into_iter().filter(|x| x.error).map(|x| format!("page libre {} : {}", f + 1, x.msg)));
        }
        if !errs.is_empty() {
            return layout_failure(&errs, &mut *env.log);
        }
    }
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
                let f = d.join(format!("{}.m2ts", cache_key(&src, lang, &eopts, &info)?));
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
                    let f = d.join(format!("{}.m2ts", cache_key(&src, lang, &eopts, &info)?));
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
    let mut plan = plan_navigation(n, total_main, &ch_counts);
    // titres ajoutes apres ceux du plan : la page 1 quand l'accueil est une autre page (le titre 0, « Top Menu », mene alors a
    // l'accueil), et « Tout lire » du menu principal s'il est utilise
    let home = designs.home;
    if matches!(home, Some(Target::Video(_))) { return bad("page d'accueil : choisissez une page du menu, pas une video"); }
    let custom_home = home.map_or(false, |h| h != Target::Main(0));
    let base_titles = plan.total_titles;
    let page1_title = if custom_home {
        plan.total_titles += 1;
        plan.main_titles[0] = plan.total_titles;
        Some(plan.total_titles)
    } else {
        None
    };
    let uses_play_all = designs.main.values().chain(designs.free.iter()).any(|d| d.items.iter().any(|it| matches!(it.kind, Kind::Nav { action: NavAction::PlayAll, .. })));
    let play_all_title = if uses_play_all { plan.total_titles += 1; Some(plan.total_titles) } else { None };
    // titre visé par un bouton « Aller a » ; None si la page n'existe pas
    let goto = |t: Target| -> Option<u32> {
        match t {
            Target::Main(p) => plan.main_titles.get(p).copied(),
            Target::Chapters(v, q) => plan.chapters.get(&v).and_then(|c| c.pages.get(q).copied()),
            Target::Video(v) => (v < n).then_some(v as u16 + 1),
        }
        .map(|x| x as u32)
    };
    for d in designs.main.values().chain(designs.chapters.values()).chain(designs.free.iter()) {
        for it in &d.items {
            if let Kind::Nav { action: NavAction::Goto, text, target } = &it.kind {
                if target.and_then(&goto).is_none() { return bad(format!("bouton « {text} » : destination inexistante ou non choisie")); }
            }
            if let Some(t) = it.link() {
                if goto(t).is_none() { return bad("texte ou image cliquable : destination inexistante (page ou video supprimee ?)"); }
            }
        }
    }
    let home_title = match home {
        Some(h) if custom_home => Some(goto(h).ok_or_else(|| Error::Format("page d'accueil inexistante (page supprimee ?)".into()))?),
        _ => None,
    };
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
    // fond anime (video) et musique des menus
    let bg_video: Option<PathBuf> = menu_cfg.get("fond_video").and_then(Value::as_str).filter(|f| !f.is_empty()).map(|f| p(f));
    let music: Option<PathBuf> = menu_cfg.get("musique").and_then(Value::as_str).filter(|f| !f.is_empty()).map(|f| p(f));
    let mut mb = MenuBg { video: bg_video.clone(), video_audio: false, music: music.clone(), seconds: fond_s, lang, cache: cache_dir.clone() };
    if let Some(v) = &bg_video {
        let info = ffmpeg::probe(env.tools, v)?;
        mb.video_audio = info.has_audio && menu_cfg.get("son_fond_video").and_then(Value::as_bool).unwrap_or(false);
        mb.seconds = info.duration_s.max(1.0);
    } else if let Some(m) = &music {
        mb.seconds = ffmpeg::probe_duration(env.tools, m)?.max(1.0);
    }
    let animated = mb.animated();
    // fond propre a une page (si sa mise en page en definit un) : (fond anime/sonore, image de base)
    let tools_ = env.tools;
    let page_bg = |d: &PageDesign| -> Result<Option<(MenuBg, Option<PathBuf>)>> {
        if d.bg.is_default() { return Ok(None); }
        let (video, video_audio, base) = match &d.bg.visual {
            PageVisual::Project => (mb.video.clone(), mb.video_audio, custom_fond.clone()),
            PageVisual::Image(f) => (None, false, Some(p(f))),
            PageVisual::Video(f, a) => (Some(p(f)), *a, None),
        };
        let music = match &d.bg.music { PageMusic::Project => mb.music.clone(), PageMusic::Silent => None, PageMusic::File(f) => Some(p(f)) };
        let (mut video_audio, mut seconds) = (video_audio, fond_s);
        if let Some(v) = &video {
            let info = ffmpeg::probe(tools_, v)?;
            video_audio &= info.has_audio;
            seconds = info.duration_s.max(1.0);
        } else if let Some(m) = &music {
            seconds = ffmpeg::probe_duration(tools_, m)?.max(1.0);
        }
        Ok(Some((MenuBg { video, video_audio, music, seconds, lang, cache: cache_dir.clone() }, base)))
    };
    // fond commun a toutes les pages sans decor propre : image du projet, video du projet
    let common_visual = custom_fond.is_some() || bg_video.is_some();
    let sd = size.0 as f64 / 1920.0; // echelle des textes/images poses sur le fond (qui a la taille de la video)
    // (rang dans l'ordre de superposition, decor) : le rang sert a poser sur les boutons ce qui est au-dessus d'eux
    // dessin d'un texte ou d'une image a l'echelle `k`
    let to_decor = |it: &Item, k: f64| -> Option<Decor> {
        let (x, y, w, h) = it.scaled(k);
        match &it.kind {
            Kind::Text { text, size, color, align, font, .. } => {
                let font = if font.is_empty() { String::new() } else { let c = p(font); if c.is_file() { c.to_string_lossy().into_owned() } else { font.clone() } };
                Some(Decor::Text { text: text.clone(), x, y, w, h, size: py_round(*size as f64 * k) as i32, color: *color, align: *align, font })
            }
            Kind::Image { path, .. } => Some(Decor::Image { path: p(path), x, y, w, h }),
            _ => None,
        }
    };
    // textes et images non cliquables (les cliquables sont des boutons), a l'echelle `k`
    let scale_decor = |d: &PageDesign, k: f64| -> Vec<(usize, Decor)> {
        d.items.iter().enumerate().filter(|(_, it)| it.link().is_none()).filter_map(|(rank, it)| to_decor(it, k).map(|x| (rank, x))).collect()
    };
    // `looping` : fond anime ou sonore (le menu tourne alors toujours en boucle)
    // clips de fond animes ou sonores (pour le resume de la construction)
    let anim_clips: std::cell::RefCell<BTreeSet<u32>> = Default::default();
    let add_menu_page = |files: &mut BTreeMap<String, Vec<u8>>, buttons: &[MenuButton], pix: &[u64], ig_num: u32, pl_num: u32, bg_num: u32, bgscan: &ClipScan, label: &str, looping: bool| -> Result<MenuStats> {
        let pts = required_ics_pts(pix, (menu_size.0 as u64, menu_size.1 as u64), 42956, 9000);
        let clip = build_menu_clip(buttons, &MenuOptions { ics_pts: pts, video_size: menu_size, ui_model: 0, rgb_colors: 63, frame_rate_code: FRAME_RATES.iter().find(|r| r.0 == bgscan.fps).map_or(1, |r| r.2) }).map_err(Error::Format)?;
        if clip.m2ts.len() > 6 * 1024 * 1024 {
            return bad(format!("{label} trop lourd ({:.1} Mo) : reduire le nombre d'elements par page (menu.par_page)", clip.m2ts.len() as f64 / 1e6));
        }
        if looping { anim_clips.borrow_mut().insert(bg_num); }
        let stats = MenuStats { pts, bytes: clip.m2ts.len(), objects: clip.menu.ods.len(), palette: clip.menu.pds.entries.len() };
        files.insert(format!("STREAM/{ig_num:05}.m2ts"), clip.m2ts);
        files.insert(format!("CLIPINF/{ig_num:05}.clpi"), clip.clpi);
        files.insert(format!("PLAYLIST/{pl_num:05}.mpls"), make_playlist(&PlaylistParams {
            clip: format!("{bg_num:05}"), in_time: (bgscan.start90 / 2) as u32, out_time: (bgscan.end90 / 2) as u32,
            video: VideoStn { pid: bgscan.video.0, coding: bgscan.video.1, fmt_rate: bgscan.video_code },
            audio: bgscan.audio_codes.first().map(|&(pid, coding, fmt_rate)| AudioStn { pid, coding, fmt_rate, lang }),
            with_menu: true, menu_clip: format!("{ig_num:05}"), menu_in: (pts / 2) as u32, menu_out: (pts / 2 + 1501) as u32,
            // 2 = arret infini sur la derniere image : le menu ne redemarre pas ; un fond anime ou sonore tourne toujours en boucle
            still_mode: if boucle || looping { 0 } else { 2 },
            ..Default::default()
        }));
        Ok(stats)
    };

    // 2. fond de menu commun (clip image fixe, sans audio)
    let is_svg = |f: &Path| f.extension().and_then(|e| e.to_str()).map_or(false, |e| e.eq_ignore_ascii_case("svg") || e.eq_ignore_ascii_case("svgz"));
    let bg_png: PathBuf = match &custom_fond {
        // ffmpeg ne lit pas le SVG : le moteur de dessin le convertit
        Some(f) if is_svg(f) => {
            let png = tmp.0.join("fond_svg.png");
            std::fs::write(&png, crate::png::write_png(&env.renderer.compose_background(Some(f), size, &[]).map_err(Error::Format)?))?;
            png
        }
        Some(f) => f.clone(),
        None => {
            let f = tmp.0.join("fond.png");
            std::fs::write(&f, crate::png::write_png(&env.renderer.background(&title, size)))?;
            f
        }
    };
    (env.log)("encodage du fond de menu ...");
    let fps = fps_str(first.fps)?;
    let bg_sc = menu_bg_clip(env, &mb, Some(&bg_png), None, &tmp.0, "fond", fond_s, size, fps, &still_opts, &mut files, n as u32)?;
    (env.progress)(0.9);

    // vignette montrant une video choisie (sur n'importe quelle page) : comme une vignette du menu principal
    let mut video_specs = Vec::with_capacity(n);
    for (i, v) in vids.iter().enumerate() {
        video_specs.push(SlotSpec {
            label: vtitle(v), source: p(&vfile(v)?), at: v.get("vignette_s").and_then(Value::as_f64), duration: durations[i], commands: vec![],
            go: plan.chapters.get(&i).map_or(i as u32 + 1, |c| c.pages[0] as u32),
        });
    }

    let benv = ButtonEnv {
        tools: env.tools, renderer: env.renderer, style: &designs.style, abs: &p, to_decor: &to_decor, videos: &video_specs, s: sc_,
        plane: (menu_size.0, menu_size.1),
    };
    let weight_note = |pix: &[u64], what: &str| -> Option<String> {
        let total: u64 = pix.iter().sum();
        (total > crate::style::DECODED_LIMIT).then(|| format!("avertissement : {what} : images de boutons de {:.1} Mo, au-dela des {:.0} Mo qu'un lecteur garantit (allegez halos, agrandissements, etats actives ou nombre de boutons)", total as f64 / 1048576.0, crate::style::DECODED_LIMIT as f64 / 1048576.0))
    };

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
            NavAction::PlayAll => play_all_title.map_or(0, u32::from),
            _ => 0,
        };
        let decor_r = scale_decor(&design, sc_);
        let (buttons, pix) = page_buttons(&benv, &design, &design.slot_assignment(slots.len()), &decor_r, MenuType::Main, pg, total_main, &slots, &target)?;
        if let Some(m) = weight_note(&pix, &what) { (env.log)(&m); }
        let decor: Vec<Decor> = scale_decor(&design, sd).into_iter().map(|(_, d)| d).collect();
        let pbg = if custom { page_bg(&design)? } else { None };
        let (bg_num, pscan, looping) = if let Some((pmb, pbase)) = &pbg {
            let num = next_bg;
            next_bg += 1;
            (num, own_background(env, pmb, pbase, size, &decor, &tmp.0, &format!("fond_p{pg}.png"), fond_s, fps, &still_opts, &mut files, num)?, pmb.animated())
        } else if custom && !(common_visual && decor.is_empty()) {
            let num = next_bg;
            next_bg += 1;
            let sc = own_background(env, &mb, &custom_fond, size, &decor, &tmp.0, &format!("fond_p{pg}.png"), fond_s, fps, &still_opts, &mut files, num)?;
            (num, sc, animated)
        } else {
            (n as u32, bg_sc.clone(), animated)
        };
        let ig_num = if pg == 0 { 80002 } else { let k = next_ig; next_ig += 1; k };
        let res = add_menu_page(&mut files, &buttons, &pix, ig_num, main_pls[pg], bg_num, &pscan, &what, looping)?;
        if pg == 0 { first_page = Some(res); }
    }
    // pages libres du menu principal : vignettes de n'importe quelle video, fond propre a chaque page
    for (f, design) in free_pages.iter().enumerate() {
        let pg = main_pg.len() + f;
        let what = format!("menu principal, page libre {}", f + 1);
        let slots: Vec<SlotSpec> = Vec::new(); // page libre : toutes ses vignettes montrent une video choisie
        let target = |a: NavAction, t: Option<Target>| match a {
            NavAction::Prev => plan.main_titles[pg.saturating_sub(1)] as u32,
            NavAction::Next => plan.main_titles[(pg + 1).min(total_main - 1)] as u32,
            NavAction::Goto => t.and_then(&goto).unwrap_or(0),
            NavAction::PlayAll => play_all_title.map_or(0, u32::from),
            _ => 0,
        };
        let decor_r = scale_decor(design, sc_);
        let identity: Vec<usize> = (0..slots.len()).collect();
        let (buttons, pix) = page_buttons(&benv, design, &identity, &decor_r, MenuType::Main, pg, total_main, &slots, &target)?;
        if let Some(m) = weight_note(&pix, &what) { (env.log)(&m); }
        let decor: Vec<Decor> = scale_decor(design, sd).into_iter().map(|(_, d)| d).collect();
        for l in design.validate_free(pg, total_main, n).iter().filter(|x| !x.error) { (env.log)(&format!("{what} : {}", l.msg)); }
        let pbg = page_bg(design)?;
        let (bg_num, pscan, looping) = if let Some((pmb, pbase)) = &pbg {
            let num = next_bg;
            next_bg += 1;
            (num, own_background(env, pmb, pbase, size, &decor, &tmp.0, &format!("fond_libre{f}.png"), fond_s, fps, &still_opts, &mut files, num)?, pmb.animated())
        } else if common_visual && decor.is_empty() {
            (n as u32, bg_sc.clone(), animated)
        } else {
            let num = next_bg;
            next_bg += 1;
            let sc = own_background(env, &mb, &custom_fond, size, &decor, &tmp.0, &format!("fond_libre{f}.png"), fond_s, fps, &still_opts, &mut files, num)?;
            (num, sc, animated)
        };
        let ig_num = { let k = next_ig; next_ig += 1; k };
        add_menu_page(&mut files, &buttons, &pix, ig_num, main_pls[pg], bg_num, &pscan, &what, looping)?;
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
        let mut shared: Option<(u32, ClipScan)> = if common_visual { Some((n as u32, bg_sc.clone())) } else { None };
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
            let decor_r = scale_decor(&design, sc_);
            let (cbuttons, pix) = page_buttons(&benv, &design, &design.slot_assignment(slots.len()), &decor_r, MenuType::Chapters, q, npg, &slots, &target)?;
            if let Some(m) = weight_note(&pix, &what) { (env.log)(&m); }
            let decor: Vec<Decor> = scale_decor(&design, sd).into_iter().map(|(_, d)| d).collect();
            let pbg = if custom { page_bg(&design)? } else { None };
            let (bg_num, pscan, looping) = if let Some((pmb, pbase)) = &pbg {
                let num = next_bg;
                next_bg += 1;
                (num, own_background(env, pmb, pbase, size, &decor, &tmp.0, &format!("fond_ch{k}_{q}.png"), fond_s, fps, &still_opts, &mut files, num)?, pmb.animated())
            } else if custom && !(common_visual && decor.is_empty()) {
                let num = next_bg;
                next_bg += 1;
                let sc = own_background(env, &mb, &custom_fond, size, &decor, &tmp.0, &format!("fond_ch{k}_{q}.png"), fond_s, fps, &still_opts, &mut files, num)?;
                (num, sc, animated)
            } else {
                if shared.is_none() {
                    let num = next_bg;
                    next_bg += 1;
                    let cbg = tmp.0.join(format!("fond_ch{k}.png"));
                    std::fs::write(&cbg, crate::png::write_png(&env.renderer.background(&format!("{} - chapitres", vtitle(&vids[i])), size)))?;
                    shared = Some((num, menu_bg_clip(env, &mb, Some(&cbg), None, &tmp.0, &format!("fond_ch{k}"), fond_s, size, fps, &still_opts, &mut files, num)?));
                }
                let (num, sc) = shared.clone().unwrap();
                (num, sc, animated)
            };
            add_menu_page(&mut files, &cbuttons, &pix, ig_num, pl_num, bg_num, &pscan, &what, looping)?;
        }
        ch_pls.insert(i, pls);
    }

    // 5. playlists des videos et navigation
    let mut video_params = Vec::new();
    for (i, sc) in scans.iter().enumerate() {
        let Some(&(pid_a, type_a, code_a)) = sc.audio_codes.first() else { return bad(format!("la video {} n'a pas de piste audio exploitable", i + 1)) };
        let pp = PlaylistParams {
            clip: format!("{i:05}"), in_time: (sc.start90 / 2) as u32, out_time: (sc.end90 / 2) as u32,
            video: VideoStn { pid: sc.video.0, coding: sc.video.1, fmt_rate: sc.video_code },
            audio: Some(AudioStn { pid: pid_a, coding: type_a, fmt_rate: code_a, lang }),
            with_menu: false, pg: pg_info[i].clone(), marks: if chapters[i].is_empty() { None } else { Some(chapters[i].iter().map(|c| c.mark45).collect()) },
            ..Default::default()
        };
        files.insert(format!("PLAYLIST/{i:05}.mpls"), make_playlist(&pp));
        video_params.push(pp);
    }
    // « Tout lire » : toutes les videos a la suite
    let play_all_pl = if play_all_title.is_some() {
        let pl = next_pl;
        next_pl += 1;
        files.insert(format!("PLAYLIST/{pl:05}.mpls"), make_playlist_multi(&video_params));
        Some(pl)
    } else {
        None
    };
    // sequence d'ouverture : une video jouee a l'insertion du disque, avant l'accueil
    let intro_pl = match menu_cfg.get("intro").and_then(Value::as_str).filter(|f| !f.is_empty()) {
        None => None,
        Some(f) => {
            let src = p(f);
            (env.log)(&format!("sequence d'ouverture {f} ..."));
            let info = ffmpeg::probe(env.tools, &src)?;
            let cache_file = match &cache_dir { Some(d) => Some(d.join(format!("{}.m2ts", cache_key(&src, lang, &eopts, &info)?))), None => None };
            let m2ts = match cache_file.as_ref().and_then(|c| std::fs::read(c).ok()).filter(|b| !b.is_empty() && b.len() % 192 == 0) {
                Some(b) => b,
                None => {
                    let (m, _) = make_film_m2ts(env.tools, &src, lang, &eopts, None, &mut |_| {}, env.cancel)?;
                    if let Some(c) = &cache_file {
                        std::fs::create_dir_all(c.parent().unwrap_or(Path::new(".")))?;
                        let t = c.with_extension("m2ts.tmp");
                        std::fs::write(&t, &m)?;
                        std::fs::rename(&t, c)?;
                    }
                    m
                }
            };
            let num = next_bg;
            next_bg += 1;
            let sc = scan_clip(&m2ts)?;
            files.insert(format!("CLIPINF/{num:05}.clpi"), make_clip_clpi(&m2ts, lang, 0, None, "0100")?);
            files.insert(format!("STREAM/{num:05}.m2ts"), m2ts);
            let (pid_a, type_a, code_a) = *sc.audio_codes.first().ok_or_else(|| Error::Format("sequence d'ouverture sans audio".into()))?;
            let pl = next_pl;
            next_pl += 1;
            files.insert(format!("PLAYLIST/{pl:05}.mpls"), make_playlist(&PlaylistParams {
                clip: format!("{num:05}"), in_time: (sc.start90 / 2) as u32, out_time: (sc.end90 / 2) as u32,
                video: VideoStn { pid: sc.video.0, coding: sc.video.1, fmt_rate: sc.video_code },
                audio: Some(AudioStn { pid: pid_a, coding: type_a, fmt_rate: code_a, lang }), with_menu: false, ..Default::default()
            }));
            Some(pl)
        }
    };
    let _ = (next_pl, next_bg);
    let anim = anim_clips.borrow().clone();
    if !anim.is_empty() {
        let bytes: usize = anim.iter().filter_map(|num| files.get(&format!("STREAM/{num:05}.m2ts"))).map(Vec::len).sum();
        (env.log)(&format!("fonds de menu animes ou sonores : {} clip(s), {:.0} Mo au total", anim.len(), bytes as f64 / 1e6));
    }
    let main_pls16: Vec<u16> = main_pls.iter().map(|&x| x as u16).collect();
    let mo = make_movie_objects_paged(n, &main_pls16, &plan.main_titles, &ch_pls, &plan.chapters, &video_page, retour_ch);
    let title_objs: Vec<u16> = (1..=plan.total_titles).collect();
    if page1_title.is_none() && play_all_pl.is_none() && intro_pl.is_none() {
        files.insert("MovieObject.bdmv".into(), mo);
        files.insert("index.bdmv".into(), make_index(0, 0, &title_objs, b"bdauthor"));
    } else {
        // objets des titres ajoutes (meme ordre que leur numero), puis objet d'ouverture
        let mut m = MovieObjectBdmv::parse(&mo)?;
        if m.objects.len() != base_titles as usize + 1 { return bad("objets de navigation : numerotation inattendue"); }
        let jump = |t: u32| Command::new(JUMP_TITLE_INSN, t, 0);
        if page1_title.is_some() {
            // la page 1 recoit son propre titre ; l'objet 0 (Top Menu) mene a l'accueil
            m.objects.push(MovieObject::new(vec![
                Command::new(PLAY_PL_INSN, main_pls[0], 0), Command::new(NOP_INSN, 0, 0), Command::new(NOP_INSN, 0, 0), Command::new(NOP_INSN, 0, 0),
                Command::new(GOTO_INSN, 0, 0),
            ]));
            m.objects[0] = MovieObject::new(vec![jump(home_title.unwrap_or(0))]);
        }
        if let Some(pl) = play_all_pl {
            // toutes les videos puis retour a l'accueil (titre 0 = Top Menu)
            m.objects.push(MovieObject::new(vec![Command::new(PLAY_PL_INSN, pl, 0), jump(0)]));
        }
        let fp = match intro_pl {
            Some(pl) => {
                m.objects.push(MovieObject::new(vec![Command::new(PLAY_PL_INSN, pl, 0), jump(0)]));
                (m.objects.len() - 1) as u16
            }
            None => 0,
        };
        files.insert("MovieObject.bdmv".into(), m.pack());
        files.insert("index.bdmv".into(), make_index(fp, 0, &title_objs, b"bdauthor"));
    }
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
