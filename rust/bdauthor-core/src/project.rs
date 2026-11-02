//! Disque multi-titres a partir d'un fichier de projet JSON (equivalent de `project.py`).
//!
//! Format du projet : voir `examples/projet_exemple.json` et le README. Structure produite : videos = clips/playlists 0..N-1
//! (titres 1..N), fond de menu = clip N (image fixe, sans audio), playlist de menu N (fond + menu IG), clip IG 80002 ;
//! pagination au-dela de 12 elements (voir `paginate`), menus de chapitres, cache d'encodage.
//! Le dessin (vignettes, libelles, fond) est delegue a un `Renderer` : le crate `bdauthor-render` en fournit un complet.
use crate::check::{check_bdmv, Report};
use crate::style::ButtonStyle;
use crate::design::{nav_applicable, Align, Designs, Item, Kind, NavAction, PageDesign, PageMusic, PageVisual, SlotReturn, Target};
use crate::disc::write_bdmv;
use crate::ffmpeg::{self, EncodeOpts, Error, Tools};
use crate::film::{make_film_m2ts, make_still_m2ts};
use crate::ig::py_round;
use crate::json::{self, Value};
use crate::menu::{build_menu_clip, required_ics_pts, MenuButton, MenuOptions};
use crate::mobj::{Command, MovieObject, MovieObjectBdmv, GOTO_INSN, JUMP_TITLE_INSN, JUMP_TITLE_REG_INSN, MOVE_RI_INSN, MOVE_RR_INSN, NOP_INSN, PLAY_PL_INSN, PLAY_PL_PM_INSN};
use crate::convert::{generate_pages, GenVideo};
use crate::nav::{make_index, make_playlist, make_playlist_multi, AudioStn, PlaylistParams, VideoStn};
use crate::palette::RgbaImage;
use crate::tsclip::{make_clip_clpi, make_clip_clpi_tracks, py_floordiv, scan_clip, ClipScan, FRAME_RATES};
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

/// Registre ou une vignette (ou un lien vers une video) range le titre auquel revenir a la fin de la video.
const R_RETURN: u32 = 1;

/// « m:ss »
fn mmss(t: f64) -> String {
    let s = t.round().max(0.0) as i64;
    format!("{}:{:02}", s / 60, s % 60)
}

/// Repere (45 kHz) d'un instant `t` (secondes depuis le debut) d'une video : debut exact pour 0, sinon image d'acces qui precede.
fn snap_mark(sc: &ClipScan, t: f64) -> u32 {
    if t <= 0.0 {
        return (sc.start90 / 2) as u32;
    }
    let target = sc.start90 as i64 + py_round(t * 90000.0) as i64;
    let best = sc.eps.iter().map(|e| e.0).filter(|&p| p as i64 <= target).max().or_else(|| sc.eps.iter().map(|e| e.0).min()).unwrap_or(sc.start90);
    (best / 2) as u32
}

struct SlotSpec { label: String, source: PathBuf, at: Option<f64>, duration: f64, commands: Vec<Command>, go: u32 }

/// Arrete la construction sur des erreurs de mise en page : chaque erreur est ecrite dans le journal, puis l'ensemble est renvoye.
fn layout_failure<T>(errs: &[String], log: &mut dyn FnMut(&str)) -> Result<T> {
    for e in errs { log(&format!("ERREUR de mise en page : {e}")); }
    bad(format!("{} erreur(s) de mise en page, a corriger dans l'editeur : {}", errs.len(), errs.join(" | ")))
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
    /// contenu et commandes d'une vignette montrant une video choisie (video ou chapitre)
    slot_spec: &'a dyn Fn(&Item) -> Option<SlotSpec>,
    /// echelle du plan 1920 vers le menu, taille du menu
    s: f64,
    plane: (u32, u32),
    /// identifiants des langues de sous-titres du projet, dans l'ordre : le rang est le numero de piste
    langs: &'a [u32],
    /// sons des menus : fichier (tel qu'ecrit dans le style) -> identifiant dans sound.bdmv
    sounds: &'a BTreeMap<String, u8>,
}

/// Boutons (et surface d'objets graphiques de chacun) de la page `page` sur `pages` d'apres sa mise en page.
/// `decor` : textes et images non cliquables a l'echelle du menu.
fn page_buttons(e: &ButtonEnv, design: &PageDesign, decor: &[(usize, Decor)], page: usize, pages: usize, target: &dyn Fn(NavAction, Option<Target>) -> (u32, Vec<Command>)) -> Result<(Vec<MenuButton>, Vec<u64>)> {
    let (renderer, s) = (e.renderer, e.s);
    let (mut buttons, mut pix) = (Vec::new(), Vec::new());
    let mut item_nav: Vec<(u32, crate::design::NavLinks)> = Vec::new();
    for (idx, it) in design.items.iter().enumerate() {
        let (x, y, w, h) = it.scaled(s);
        let st = it.look.style.as_ref().unwrap_or(e.style).scaled(s);
        let (mx, my) = st.margins(w, h);
        let (bx, by, bw, bh) = (x - mx, y - my, w + 2 * mx, h + 2 * my);
        // textes et images places au-dessus de ce bouton dans l'ordre de superposition : poses sur ses images
        let above: Vec<Decor> = decor.iter().filter(|(i, d)| *i > idx && d.intersects((bx, by, bw, bh))).map(|(_, d)| d.shifted(-bx, -by)).collect();
        let skin = &it.look.skin;
        let skinned = skin.has_normal() && !matches!(it.kind, Kind::Slot { .. });
        // dessin des etats pour un style donne (appele une fois par image du halo anime)
        type Draw<'d> = Box<dyn Fn(&ButtonStyle) -> Result<[Arc<RgbaImage>; 3]> + 'd>;
        let skin_draw = || -> Draw<'_> {
            let one = |p: &str| if p.is_empty() { None } else { Some((e.abs)(p)) };
            let paths = [one(skin.normal_file()), one(&skin.selected), one(&skin.activated)];
            Box::new(move |st: &ButtonStyle| renderer.skin_button(&paths, w as u32, h as u32, st).map_err(Error::Format))
        };
        let (draw, go, commands): (Draw<'_>, u32, Vec<Command>) = match &it.kind {
            Kind::Slot { thumb_h, video, .. } => {
                // vignette sans video : absente du disque
                let Some(v) = video else { continue };
                let sp = (e.slot_spec)(it).ok_or_else(|| Error::Format(format!("vignette : la video {} n'existe pas", v + 1)))?;
                let th = py_round(*thumb_h as f64 * s) as i32;
                let thumb = ffmpeg::extract_thumbnail_rgba(e.tools, &sp.source, (w as u32, th as u32), sp.at, sp.duration)?;
                let label = sp.label.clone();
                let draw: Draw<'_> = Box::new(move |st: &ButtonStyle| Ok(renderer.thumb_button(&thumb, &label, w as u32, h as u32, th as u32, st)));
                // destination choisie ou lecture : deja dans `slot_spec`
                (draw, sp.go, sp.commands.clone())
            }
            Kind::Nav { action, text, target: tg } if nav_applicable(*action, page, pages) => {
                let draw: Draw<'_> = if skinned { skin_draw() } else {
                    let text = text.clone();
                    Box::new(move |st: &ButtonStyle| Ok(renderer.text_button(&text, w as u32, h as u32, st)))
                };
                let (g, c) = target(*action, *tg);
                (draw, g, c)
            }
            Kind::Text { .. } | Kind::Image { .. } if it.link().is_some() => {
                let draw: Draw<'_> = if skinned {
                    skin_draw()
                } else {
                    let Some(d) = (e.to_decor)(it, s) else { continue };
                    let d = d.shifted(-x, -y);
                    Box::new(move |st: &ButtonStyle| Ok(renderer.decor_button(&d, w as u32, h as u32, st)))
                };
                let (g, c) = target(NavAction::Goto, it.link());
                (draw, g, c)
            }
            _ => continue,
        };
        let mut states = draw(&st)?;
        let mut frames: Vec<Arc<RgbaImage>> = Vec::new();
        if skinned && skin.anim_frames() > 0 {
            // animation fournie par l'auteur : chaque image remplace l'image « selectionne » (effets du style appliques)
            let one = |p: &str| if p.is_empty() { None } else { Some((e.abs)(p)) };
            let mut all = Vec::new();
            for f in &skin.sel_frames {
                let paths = [one(skin.normal_file()), one(f), one(&skin.activated)];
                all.push(renderer.skin_button(&paths, w as u32, h as u32, &st).map_err(|m| Error::Format(format!("animation ({f}) : {m}")))?[1].clone());
            }
            states[1] = all.remove(0);
            frames = all;
        } else {
            // halo anime : images suivantes de l'etat selectionne, halo de plus en plus petit puis de nouveau grand
            for i in 1..st.pulse_frames() {
                frames.push(draw(&st.pulse_style(i))?[1].clone());
            }
        }
        // etat normal anime (images de l'auteur) : chaque image passe par les effets de l'etat normal (assombrissement)
        let mut nframes: Vec<Arc<RgbaImage>> = Vec::new();
        if skinned && skin.normal_anim_frames() >= 2 {
            let one = |p: &str| if p.is_empty() { None } else { Some((e.abs)(p)) };
            let mut all = Vec::new();
            for f in &skin.normal_frames {
                let paths = [one(f), one(&skin.selected), one(&skin.activated)];
                all.push(renderer.skin_button(&paths, w as u32, h as u32, &st).map_err(|m| Error::Format(format!("animation ({f}) : {m}")))?[0].clone());
            }
            // la premiere image est deja l'etat normal ; les suivantes forment l'animation
            nframes = all.into_iter().skip(1).collect();
        }
        let cover = |im: Arc<RgbaImage>| if above.is_empty() { im } else { renderer.overlay([im.clone(), im.clone(), im], &above)[0].clone() };
        let states = if above.is_empty() { states } else { renderer.overlay(states, &above) };
        let frames: Vec<Arc<RgbaImage>> = frames.into_iter().map(cover).collect();
        let nframes: Vec<Arc<RgbaImage>> = nframes.into_iter().map(cover).collect();
        // taille attendue, puis rognage aux bords du menu (le halo ne doit pas deborder du plan graphique)
        let (l, t) = ((-bx).max(0), (-by).max(0));
        let (r, b) = ((bx + bw - e.plane.0 as i32).max(0), (by + bh - e.plane.1 as i32).max(0));
        let (cw, ch) = ((bw - l - r).max(1) as u32, (bh - t - b).max(1) as u32);
        let fit = |im: &RgbaImage| {
            let full = recenter(im, bw.max(1) as u32, bh.max(1) as u32);
            if (l, t, r, b) == (0, 0, 0, 0) { full } else { sub_image(&full, l as u32, t as u32, cw, ch) }
        };
        let (states, distinct) = map_states(states, &fit);
        let frames: Vec<Arc<RgbaImage>> = frames.iter().map(|im| Arc::new(fit(im))).collect();
        let nframes: Vec<Arc<RgbaImage>> = nframes.iter().map(|im| Arc::new(fit(im))).collect();
        // une animation a son propre exemplaire de l'image de depart (objets consecutifs)
        let distinct = if frames.is_empty() { distinct } else { distinct + frames.len() as u64 + 1 };
        let distinct = if nframes.is_empty() { distinct } else { distinct + nframes.len() as u64 + 1 };
        let [nrm, sel, act] = states;
        let mut btn = MenuButton::new("", go, bx + l, by + t, vec![nrm, sel, act]);
        // sons du bouton (les siens, sinon ceux du style du projet ; independants de l'apparence)
        let snd = |f: &str| if f.is_empty() { 0xFF } else { e.sounds.get(f).copied().unwrap_or(0xFF) };
        let (s_sel, s_act) = it.look.sounds(e.style);
        btn.sound_select = snd(s_sel);
        btn.sound_activate = snd(s_act);
        btn.sel_frames = frames;
        btn.normal_once = !nframes.is_empty() && skin.normal_once;
        btn.normal_frames = nframes;
        btn.sel_once = skinned && skin.anim_frames() > 1 && skin.sel_once;
        // sous-titres choisis par le bouton : regles avant ses autres commandes
        let mut commands = commands;
        if it.sub != crate::design::SubChoice::Keep && commands.is_empty() { commands.push(Command::new(JUMP_TITLE_INSN, go, 0)); }
        match it.sub {
            crate::design::SubChoice::Keep => {}
            crate::design::SubChoice::Off => commands.insert(0, crate::mobj::set_pg_stream(None)),
            crate::design::SubChoice::Lang(id) => {
                let rank = e.langs.iter().position(|l| *l == id).ok_or_else(|| Error::Format("sous-titres d'un bouton : la langue choisie n'existe plus dans la liste des langues".into()))?;
                commands.insert(0, crate::mobj::set_pg_stream(Some(rank)));
            }
        }
        btn.commands = commands;
        buttons.push(btn);
        item_nav.push((it.id, it.nav));
        pix.push(distinct * cw as u64 * ch as u64);
    }
    // voisins choisis a la main (les liens vers un element qui n'est pas un bouton du disque restent automatiques)
    let pos: std::collections::HashMap<u32, usize> = item_nav.iter().enumerate().filter(|(_, (id, _))| *id != 0).map(|(k, (id, _))| (*id, k)).collect();
    for (k, (_, nav)) in item_nav.iter().enumerate() {
        for d in 0..4 {
            buttons[k].nav_set[d] = match nav.get(d) {
                crate::design::NavLink::Auto => None,
                crate::design::NavLink::Stay => Some(k as u16),
                crate::design::NavLink::Item(id) => pos.get(&id).map(|p| *p as u16),
            };
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

/// Repartit `count` elements en pages equilibrees (pages de depart). Une seule page si count <= par_page (12 par defaut) ; sinon
/// pages de `par_page` (8 par defaut).
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
/// Resume des reglages audio d'une video pour la cle du cache d'encodage.
fn audio_signature(tracks: &[crate::audio::AudioTrackCfg], langs: &[[u8; 3]]) -> String {
    let mut s = String::new();
    for t in tracks {
        match &t.source {
            crate::audio::AudioSource::Embedded(n) => s.push_str(&format!("e{n}")),
            crate::audio::AudioSource::File(f) => {
                let md = std::fs::metadata(f).ok();
                let mtime = md.as_ref().and_then(|m| m.modified().ok()).and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map_or(0, |d| d.as_secs());
                s.push_str(&format!("f{}:{}:{}", f, md.map_or(0, |m| m.len()), mtime));
            }
        }
        s.push_str(&format!("@{};", t.offset_ms));
    }
    for l in langs { s.push_str(&String::from_utf8_lossy(l)); s.push(','); }
    s
}

fn fnv1a(data: &[u8]) -> u64 {
    data.iter().fold(0xcbf2_9ce4_8422_2325u64, |h, &b| (h ^ b as u64).wrapping_mul(0x0000_0100_0000_01b3))
}

fn cache_key(path: &Path, lang: [u8; 3], o: &EncodeOpts, info: &ffmpeg::VideoInfo) -> Result<String> {
    cache_key_with(path, lang, o, info, "")
}

/// `extra` : reglages audio propres a la video (pistes, langues) ; vide : meme cle qu'avant leur apparition.
fn cache_key_with(path: &Path, lang: [u8; 3], o: &EncodeOpts, info: &ffmpeg::VideoInfo, extra: &str) -> Result<String> {
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
    if !extra.is_empty() { s.push_str(&format!("|audio={extra}")); }
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

/// Pistes de sous-titres d'une video, rangees par numero de piste : `cfg[k]` est la piste k (`None` : piste vide, gardee pour que
/// chaque langue ait le meme numero dans toutes les videos) avec son code de langue.
type SubSlot = (Option<crate::subtitle::SubTrackCfg>, [u8; 3]);

/// Ajoute les pistes de sous-titres `cfg` a un clip encode (remuxage). Retourne le nouveau clip et les flux PG declares (PID, langue).
/// `keep_empty` : garde aussi les pistes sans replique affichable (numerotation par langues) ; sinon elles sont ignorees.
#[allow(clippy::too_many_arguments)]
fn add_subtitles(
    env: &mut BuildEnv, src: &Path, base: &Path, cfg: &[SubSlot], keep_empty: bool, audio_langs: &[[u8; 3]], tmp: &Path, m2ts: &[u8], num: usize, video_fps: f64,
) -> Result<(Vec<u8>, Vec<(u16, [u8; 3])>)> {
    use crate::subtitle::{load_cues, shift, SubSource};
    let abs = |f: &str| -> PathBuf { let q = Path::new(f); if q.is_absolute() { q.to_path_buf() } else { base.join(q) } };
    let mut specs = Vec::new();
    for (k, (t, tl)) in cfg.iter().enumerate() {
        let Some(t) = t else {
            specs.push(crate::film::SubSpec { lang: *tl, cues: vec![], style: crate::subtitle::SubStyle::default() });
            continue;
        };
        if let Some(m) = t.problems().into_iter().next() { return bad(format!("video {num}, sous-titres {} : {m}", k + 1)); }
        let cues = match &t.source {
            SubSource::File(f) => crate::subtitle::load_cues_fps(&abs(f), t.fps.or(crate::subtitle::plausible_fps(video_fps))).map_err(Error::Format)?,
            SubSource::Embedded(idx) => {
                let dst = tmp.join(format!("sous_titres_{num}_{idx}.srt"));
                ffmpeg::extract_subtitle_srt(env.tools, src, *idx, &dst)?;
                load_cues(&dst).map_err(Error::Format)?
            }
        };
        let cues = if t.strip_notes { crate::subtitle::strip_notes(&cues) } else { cues };
        let mut style = t.style.clone();
        if !style.font.is_empty() {
            let c = abs(&style.font);
            if c.is_file() { style.font = c.to_string_lossy().into_owned(); }
        }
        specs.push(crate::film::SubSpec { lang: *tl, cues: shift(&cues, t.offset_ms), style });
    }
    let enc = crate::film::load_encoded_m2ts(m2ts)?;
    let tracks = crate::film::encode_sub_tracks(&enc, &specs, env.renderer)?;
    let mut kept = Vec::new();
    for (k, t) in tracks.into_iter().enumerate() {
        if t.specs.is_empty() && cfg[k].0.is_some() {
            (env.log)(&format!("   sous-titres {} : aucune replique affichable{}", k + 1, if keep_empty { " (piste vide)" } else { ", piste ignoree" }));
        }
        if !t.specs.is_empty() {
            kept.push(t);
        } else if keep_empty {
            kept.push(crate::film::empty_sub_track(&enc, t.lang)?);
        }
    }
    let n_sets: usize = kept.iter().map(|t| t.specs.len()).sum();
    let (out, _) = crate::film::mux_encoded_tracks(&enc, audio_langs, &kept)?;
    (env.log)(&format!("   {} piste(s) de sous-titres ajoutee(s) ({} segments)", kept.len(), n_sets));
    Ok((out, kept.iter().enumerate().map(|(k, t)| (crate::pgs::PG_PID0 + k as u16, t.lang)).collect()))
}

/// Pistes de sous-titres de chaque video quand le projet a une liste de langues : la piste k de chaque video est la langue de rang k
/// de la liste (piste vide si la video n'a pas cette langue), jusqu'a la derniere langue utilisee par une video.
/// Retourne aussi le nombre de pistes ainsi creees dans chaque video (0 : aucune video n'a de sous-titres).
fn slot_tracks(langs: &[crate::subtitle::Language], all: &[Vec<crate::subtitle::SubTrackCfg>]) -> Result<(Vec<Vec<SubSlot>>, usize)> {
    let mut used = 0usize;
    for (i, tracks) in all.iter().enumerate() {
        for (k, t) in tracks.iter().enumerate() {
            let Some(rank) = langs.iter().position(|l| l.id == t.lang_id) else {
                return bad(format!("video {}, sous-titres {} : langue a choisir dans la liste des langues du projet", i + 1, k + 1));
            };
            if tracks[..k].iter().any(|o| o.lang_id == t.lang_id) {
                return bad(format!("video {}, sous-titres {} : la langue « {} » est deja utilisee par une autre piste de cette video", i + 1, k + 1, langs[rank].name));
            }
            used = used.max(rank + 1);
        }
    }
    let out = all.iter().map(|tracks| {
        (0..used).map(|rank| (tracks.iter().find(|t| t.lang_id == langs[rank].id).cloned(), langs[rank].code_bytes())).collect()
    }).collect();
    Ok((out, used))
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
    let title = proj.get("titre").and_then(Value::as_str).unwrap_or("Menu").to_string();
    let boucle = menu_cfg.get("boucle").and_then(Value::as_bool).unwrap_or(false);
    let fond_s = menu_cfg.get("fond_secondes").and_then(Value::as_f64).unwrap_or(5.0);
    let p = |f: &str| -> PathBuf { let q = Path::new(f); if q.is_absolute() { q.to_path_buf() } else { base.join(q) } };
    let vfile = |v: &Value| -> Result<String> { v.get("fichier").and_then(Value::as_str).map(String::from).ok_or_else(|| Error::Format("video sans champ « fichier »".into())) };
    let vtitle = |v: &Value| -> String {
        v.get("titre").and_then(Value::as_str).filter(|s| !s.is_empty()).map(String::from).unwrap_or_else(|| {
            Path::new(v.get("fichier").and_then(Value::as_str).unwrap_or("")).file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default()
        })
    };
    // pages du menu verifiees avant tout encodage (sauf les instants des chapitres, compares plus tard a la duree reelle des videos)
    if let Some(v) = proj.get("mise_en_page") {
        let ds = Designs::from_value(v).map_err(|e| Error::Format(format!("mise_en_page : {e}")))?;
        let np = ds.free.len();
        let mut errs = Vec::new();
        for (k, d) in ds.free.iter().enumerate() {
            errs.extend(d.validate(k, np, n).into_iter().filter(|x| x.error).map(|x| format!("page {} : {}", k + 1, x.msg)));
            for pr in d.check_targets(&|t| match t { Target::Main(q) => q < np, Target::Video(v) => v < n, Target::Stay(e) | Target::Audio(_, e) => e.map_or(true, |q| q < np) }) {
                errs.push(format!("page {} : {}", k + 1, pr.msg));
            }
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

    // langues de sous-titres du projet : le rang d'une langue est son numero de piste dans toutes les videos
    let languages = crate::subtitle::languages_from_value(proj.get("langues")).map_err(Error::Format)?;
    if languages.len() > crate::film::MAX_SUB_TRACKS {
        return bad(format!("{} langues de sous-titres : {} au maximum", languages.len(), crate::film::MAX_SUB_TRACKS));
    }
    let lang_ids: Vec<u32> = languages.iter().map(|l| l.id).collect();
    let all_subs: Vec<Vec<crate::subtitle::SubTrackCfg>> = vids.iter().map(|v| crate::subtitle::tracks_from_value(v.get("soustitres")).map_err(Error::Format)).collect::<Result<_>>()?;
    let (slots, n_slots) = if languages.is_empty() { (Vec::new(), 0) } else { slot_tracks(&languages, &all_subs)? };
    // pistes audio : meme regle de numerotation par langue (les langues audio viennent de la meme liste)
    let all_audio: Vec<Vec<crate::audio::AudioTrackCfg>> = vids.iter().map(|v| crate::audio::tracks_from_value(v.get("pistes_audio")).map_err(Error::Format)).collect::<Result<_>>()?;
    if languages.is_empty() && all_audio.iter().any(|t| !t.is_empty()) {
        return bad("pistes audio configurees mais aucune langue dans la liste des langues du projet (menu Edition > Langues des sous-titres)");
    }
    let audio_plan = crate::audio::plan_audio(&languages, lang_s, &all_audio).map_err(Error::Format)?;
    let mut audio_langs: Vec<Vec<[u8; 3]>> = Vec::new();

    // 1. videos
    for (i, v) in vids.iter().enumerate() {
        let name = vfile(v)?;
        let src = p(&name);
        (env.log)(&format!("[{}/{n}] encodage de {name} ...", i + 1));
        let info = ffmpeg::probe(env.tools, &src)?;
        durations.push(info.duration_s);
        // pistes audio de cette video (chemins complets) et leurs langues
        let (a_tracks, a_langs): (Vec<crate::audio::AudioTrackCfg>, Vec<[u8; 3]>) = match &audio_plan {
            Some(pl) => {
                let mut t = pl.videos[i].tracks.clone();
                for x in &mut t {
                    if let crate::audio::AudioSource::File(f) = &mut x.source {
                        let full = p(f);
                        if !full.is_file() { return bad(format!("video {}, piste audio : fichier introuvable ({f})", i + 1)); }
                        *f = full.to_string_lossy().into_owned();
                    }
                }
                (t, pl.videos[i].langs.clone())
            }
            None => (Vec::new(), vec![lang]),
        };
        let a_sig = if audio_plan.is_some() { audio_signature(&a_tracks, &a_langs) } else { String::new() };
        audio_langs.push(a_langs.clone());
        let cached = match &cache_dir {
            Some(d) => {
                let f = d.join(format!("{}.m2ts", cache_key_with(&src, lang, &eopts, &info, &a_sig)?));
                std::fs::read(&f).ok().filter(|b| !b.is_empty() && b.len() % 192 == 0).map(|b| (b, f))
            }
            None => None,
        };
        let m2ts = match cached {
            Some((b, _)) => { (env.log)("   (deja encodee : cache)"); b }
            None => {
                let (lo, hi) = (i as f64 / n as f64 * 0.85, (i + 1) as f64 / n as f64 * 0.85);
                let progress = &mut *env.progress;
                let (m, st) = crate::film::make_film_m2ts_tracks(env.tools, &src, &a_langs, &a_tracks, &eopts, None, &mut |f| progress(lo + (hi - lo) * f), env.cancel)?;
                (env.log)(&format!("   {} : {} images, {} trames audio", st.cible, st.images, st.trames_audio));
                if let Some(d) = &cache_dir {
                    std::fs::create_dir_all(d)?;
                    let f = d.join(format!("{}.m2ts", cache_key_with(&src, lang, &eopts, &info, &a_sig)?));
                    let t = f.with_extension("m2ts.tmp");
                    std::fs::write(&t, &m)?;
                    std::fs::rename(&t, &f)?;
                }
                m
            }
        };
        // sous-titres PGS : ajoutes par remuxage du clip encode (le cache ne contient pas les sous-titres)
        let (m2ts, pg) = if n_slots > 0 {
            // avec une liste de langues, toutes les videos ont les memes pistes (vides au besoin) : un meme numero = une meme langue
            add_subtitles(env, &src, &base, &slots[i], true, &a_langs, &tmp.0, &m2ts, i + 1, info.fps())?
        } else if all_subs[i].is_empty() {
            (m2ts, Vec::new())
        } else {
            let cfg: Vec<SubSlot> = all_subs[i].iter().enumerate().map(|(k, t)| {
                let tl: [u8; 3] = t.lang.as_bytes().try_into().map_err(|_| Error::Format(format!("video {}, sous-titres {} : langue invalide", i + 1, k + 1)))?;
                Ok((Some(t.clone()), tl))
            }).collect::<Result<_>>()?;
            add_subtitles(env, &src, &base, &cfg, false, &a_langs, &tmp.0, &m2ts, i + 1, info.fps())?
        };
        let sc = scan_clip(&m2ts)?;
        let pg_langs: Vec<[u8; 3]> = pg.iter().map(|x| x.1).collect();
        files.insert(format!("CLIPINF/{i:05}.clpi"), make_clip_clpi_tracks(&m2ts, &a_langs, 0, None, "0100", &pg_langs)?);
        pg_info.push(pg);
        files.insert(format!("STREAM/{i:05}.m2ts"), m2ts);
        scans.push(sc);
    }
    (env.progress)(0.86);
    // 2. pages du menu ; un projet sans page recoit les pages de depart (grille des videos, pages de chapitres)
    let mut video_chapters: Vec<Vec<Chapter>> = Vec::new();
    for (v, sc) in vids.iter().zip(&scans) {
        video_chapters.push(parse_chapters(v, sc)?);
    }
    let mut designs = match proj.get("mise_en_page") {
        Some(v) => Designs::from_value(v).map_err(|e| Error::Format(format!("mise_en_page : {e}")))?,
        None => Designs::default(),
    };
    if designs.free.is_empty() {
        let gv: Vec<GenVideo> = vids.iter().zip(&video_chapters)
            .map(|(v, ch)| GenVideo { title: vtitle(v), chapters: ch.iter().map(|c| (c.t_rel, c.name.clone())).collect() })
            .collect();
        designs.free = generate_pages(&gv, &title, &designs.style).map_err(Error::Format)?.free;
        (env.log)(&format!("aucune page de menu : {} page(s) de depart generee(s)", designs.free.len()));
    }
    let pages: Vec<PageDesign> = designs.free.clone();
    // sons des menus : un identifiant par fichier distinct, dans l'ordre de rencontre (style du projet puis elements)
    let sound_ids: BTreeMap<String, u8> = {
        let mut order: Vec<String> = Vec::new();
        let mut add = |f: &str| if !f.is_empty() && !order.iter().any(|x| x == f) { order.push(f.to_string()); };
        for d in &pages {
            for it in d.items.iter().filter(|it| it.is_button()) {
                let (a, b) = it.look.sounds(&designs.style);
                add(a);
                add(b);
            }
        }
        if order.len() > crate::sound::MAX_SOUNDS { return bad(format!("trop de sons de menu differents ({}, {} au plus)", order.len(), crate::sound::MAX_SOUNDS)); }
        order.into_iter().enumerate().map(|(k, f)| (f, k as u8)).collect()
    };
    let np = pages.len();
    if np == 0 {
        return bad("aucune page de menu : ajoutez au moins une page dans l'editeur");
    }
    {
        let mut errs = Vec::new();
        for (k, d) in pages.iter().enumerate() {
            for pr in d.validate(k, np, n) {
                if pr.error { errs.push(format!("page {} : {}", k + 1, pr.msg)); } else { (env.log)(&format!("page {} : {}", k + 1, pr.msg)); }
            }
            for pr in d.check_targets(&|t| match t { Target::Main(q) => q < np, Target::Video(v) => v < n, Target::Stay(e) | Target::Audio(_, e) => e.map_or(true, |q| q < np) }) {
                errs.push(format!("page {} : {}", k + 1, pr.msg));
            }
        }
        if !errs.is_empty() {
            return layout_failure(&errs, &mut *env.log);
        }
    }

    // 3. reperes de chapitre de chaque video : ses chapitres (touches chapitre suivant / precedent) et les departs des vignettes
    //    de chapitre ; chaque repere est cale sur l'image d'acces qui precede
    let mut marks: Vec<Vec<u32>> = Vec::with_capacity(n);
    for (i, sc) in scans.iter().enumerate() {
        let dur = (sc.end90 - sc.start90) as f64 / 90000.0;
        let mut set: BTreeSet<u32> = video_chapters[i].iter().map(|c| c.mark45).collect();
        for (k, d) in pages.iter().enumerate() {
            for it in &d.items {
                if let (Some(v), Some(t)) = (it.fixed_video(), it.chapter_start()) {
                    if v != i { continue; }
                    if t >= dur {
                        return bad(format!("page {} : vignette de chapitre a {} au-dela de la fin de la video {} ({})", k + 1, mmss(t), i + 1, mmss(dur)));
                    }
                    set.insert(snap_mark(sc, t));
                }
            }
        }
        if !set.is_empty() { set.insert((sc.start90 / 2) as u32); }
        if set.len() > MAX_CHAPTERS { return bad(format!("video {} : {} reperes de chapitre, {MAX_CHAPTERS} au maximum", i + 1, set.len())); }
        marks.push(set.into_iter().collect());
    }
    let mark_index = |v: usize, t: f64| -> u32 { let m = snap_mark(&scans[v], t); marks[v].iter().position(|&x| x == m).unwrap_or(0) as u32 };

    // 4. titres : videos 1..n, pages n+1..n+P, puis « lecture depuis un repere » des videos qui en ont besoin, puis « Tout lire ».
    //    Le registre R1 garde le titre ou revenir a la fin d'une video (0 = Top Menu, donc l'accueil).
    let page_title = |k: usize| (n + 1 + k) as u32;
    let mut t_next = (n + np) as u32;
    let mut pfm: BTreeMap<usize, u32> = BTreeMap::new();
    for v in 0..n {
        if pages.iter().any(|d| d.items.iter().any(|it| it.fixed_video() == Some(v) && it.chapter_start().is_some())) {
            t_next += 1;
            pfm.insert(v, t_next);
        }
    }
    let uses_play_all = pages.iter().any(|d| d.items.iter().any(|it| matches!(it.kind, Kind::Nav { action: NavAction::PlayAll, .. })));
    let play_all_title = if uses_play_all { t_next += 1; Some(t_next) } else { None };
    let total_titles = t_next;
    let home_idx = match designs.home {
        None => 0,
        Some(Target::Main(q)) if q < np => q,
        Some(_) => return bad("page d'accueil inexistante (page supprimee ?)"),
    };
    let set_ret = |r: u32| Command::new(MOVE_RI_INSN, R_RETURN, r);
    // destination d'un lien : (titre, commandes) ; `origin` : titre ou revenir apres une video
    let goto = |t: Target, origin: u32| -> (u32, Vec<Command>) {
        match t {
            Target::Video(v) => (0, vec![set_ret(origin), Command::new(JUMP_TITLE_INSN, v as u32 + 1, 0)]),
            Target::Main(q) => (page_title(q.min(np - 1)), vec![]),
            // aucune navigation : une commande vide (sans elle, le bouton menerait au Top Menu)
            // reglage des sous-titres (commande ajoutee par `page_buttons`) puis page de sortie, ou aucune navigation
            // (une commande vide : sans elle, le bouton menerait au Top Menu)
            Target::Stay(e) => match e {
                Some(q) => { let t = page_title(q.min(np - 1)); (t, vec![Command::new(JUMP_TITLE_INSN, t, 0)]) }
                None => (0, vec![Command::new(NOP_INSN, 0, 0)]),
            },
            // langue audio : numero de piste = rang parmi les langues ayant de l'audio (voir `audio::plan_audio`) ;
            // sans piste correspondante (signale par la validation), le bouton ne change rien
            Target::Audio(id, e) => {
                let rank = audio_plan.as_ref().and_then(|pl| pl.slots.iter().position(|s| *s == id));
                let mut c: Vec<Command> = rank.map(crate::mobj::set_audio_stream).into_iter().collect();
                match e {
                    Some(q) => { let t = page_title(q.min(np - 1)); c.push(Command::new(JUMP_TITLE_INSN, t, 0)); (t, c) }
                    None => { c.push(Command::new(NOP_INSN, 0, 0)); (0, c) }
                }
            }
        }
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
    let add_menu_page = |files: &mut BTreeMap<String, Vec<u8>>, buttons: &[MenuButton], pix: &[u64], ig_num: u32, pl_num: u32, bg_num: u32, bgscan: &ClipScan, label: &str, looping: bool, anim_fps: u32| -> Result<MenuStats> {
        let pts = required_ics_pts(pix, (menu_size.0 as u64, menu_size.1 as u64), 42956, 9000);
        let clip = build_menu_clip(buttons, &MenuOptions { ics_pts: pts, video_size: menu_size, ui_model: 0, rgb_colors: 63, frame_rate_code: FRAME_RATES.iter().find(|r| r.0 == bgscan.fps).map_or(1, |r| r.2),
            anim_fps: if anim_fps == 0 { crate::design::DEFAULT_ANIM_FPS } else { anim_fps } as f64 }).map_err(Error::Format)?;
        if clip.m2ts.len() > 6 * 1024 * 1024 {
            return bad(format!("{label} trop lourd ({:.1} Mo) : reduire le nombre d'elements de la page ", clip.m2ts.len() as f64 / 1e6));
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


    let weight_note = |pix: &[u64], what: &str| -> Option<String> {
        let total: u64 = pix.iter().sum();
        (total > crate::style::DECODED_LIMIT).then(|| format!("avertissement : {what} : images de boutons de {:.1} Mo, au-dela des {:.0} Mo qu'un lecteur garantit (allegez halos, agrandissements, etats actives ou nombre de boutons)", total as f64 / 1048576.0, crate::style::DECODED_LIMIT as f64 / 1048576.0))
    };

    // 5. une page de menu par page generique : titre n+1+k, playlist n+k, clip de menu 80002+k
    let mut next_pl = (n + np) as u32;
    let mut next_bg = (n + 1) as u32;
    let mut first_page: Option<MenuStats> = None;
    for (k, d) in pages.iter().enumerate() {
        let what = format!("page {}{}", k + 1, if d.name.is_empty() { String::new() } else { format!(" ({})", d.name) });
        let origin = page_title(k);
        // vignettes : video depuis le debut ou depuis un repere, ou destination choisie ; retour selon la vignette
        let slot_spec = |it: &Item| -> Option<SlotSpec> {
            let Kind::Slot { video: Some(v), link, start, label, thumb_at, ret, .. } = &it.kind else { return None };
            let v = *v;
            let vj = vids.get(v)?;
            let back = match ret { SlotReturn::Origin => origin, SlotReturn::Home => 0, SlotReturn::Page(q) if *q < np => page_title(*q), SlotReturn::Page(_) => origin };
            let (go, commands) = match (link, start) {
                (Some(t), _) => goto(*t, back),
                (None, Some(t)) => (0, vec![set_ret(back), Command::new(MOVE_RI_INSN, 0, mark_index(v, *t)), Command::new(JUMP_TITLE_INSN, *pfm.get(&v)?, 0)]),
                (None, None) => (0, vec![set_ret(back), Command::new(JUMP_TITLE_INSN, v as u32 + 1, 0)]),
            };
            let label = if !label.is_empty() { label.clone() } else if let Some(t) = start { mmss(*t) } else { vtitle(vj) };
            let at = thumb_at.or(*start).or_else(|| vj.get("vignette_s").and_then(Value::as_f64));
            Some(SlotSpec { label, source: p(&vfile(vj).ok()?), at, duration: durations[v], commands, go })
        };
        let target = |a: NavAction, t: Option<Target>| -> (u32, Vec<Command>) {
            match a {
                NavAction::Prev => (page_title(k.saturating_sub(1)), vec![]),
                NavAction::Next => (page_title((k + 1).min(np - 1)), vec![]),
                NavAction::PlayAll => (0, vec![set_ret(origin), Command::new(JUMP_TITLE_INSN, play_all_title.unwrap_or(0), 0)]),
                NavAction::Goto => t.map(|t| goto(t, origin)).unwrap_or((page_title(home_idx), vec![])),
            }
        };
        let benv = ButtonEnv {
            tools: env.tools, renderer: env.renderer, style: &designs.style, abs: &p, to_decor: &to_decor, slot_spec: &slot_spec, s: sc_,
            plane: (menu_size.0, menu_size.1), langs: &lang_ids, sounds: &sound_ids,
        };
        let decor_r = scale_decor(d, sc_);
        let (buttons, pix) = page_buttons(&benv, d, &decor_r, k, np, &target)?;
        if buttons.is_empty() { return bad(format!("{what} : aucun bouton (le lecteur ne pourrait pas en sortir)")); }
        if let Some(m) = weight_note(&pix, &what) { (env.log)(&m); }
        let decor: Vec<Decor> = scale_decor(d, sd).into_iter().map(|(_, d)| d).collect();
        let pbg = page_bg(d)?;
        let (bg_num, pscan, looping) = if let Some((pmb, pbase)) = &pbg {
            let num = next_bg;
            next_bg += 1;
            (num, own_background(env, pmb, pbase, size, &decor, &tmp.0, &format!("fond_page{k}.png"), fond_s, fps, &still_opts, &mut files, num)?, pmb.animated())
        } else if decor.is_empty() {
            (n as u32, bg_sc.clone(), animated)
        } else {
            let num = next_bg;
            next_bg += 1;
            (num, own_background(env, &mb, &custom_fond, size, &decor, &tmp.0, &format!("fond_page{k}.png"), fond_s, fps, &still_opts, &mut files, num)?, animated)
        };
        let res = add_menu_page(&mut files, &buttons, &pix, 80002 + k as u32, (n + k) as u32, bg_num, &pscan, &what, looping, d.anim_fps)?;
        if k == 0 { first_page = Some(res); }
    }
    let first_page = first_page.expect("au moins une page");
    (env.progress)(0.95);

    // 6. playlists des videos (avec leurs reperes), « Tout lire », sequence d'ouverture, navigation
    let mut video_params = Vec::new();
    for (i, sc) in scans.iter().enumerate() {
        if sc.audio_codes.is_empty() { return bad(format!("la video {} n'a pas de piste audio exploitable", i + 1)); }
        let stns: Vec<AudioStn> = sc.audio_codes.iter().enumerate().map(|(k, &(pid, coding, fmt_rate))| {
            AudioStn { pid, coding, fmt_rate, lang: audio_langs[i].get(k).or(audio_langs[i].last()).copied().unwrap_or(lang) }
        }).collect();
        let pp = PlaylistParams {
            clip: format!("{i:05}"), in_time: (sc.start90 / 2) as u32, out_time: (sc.end90 / 2) as u32,
            video: VideoStn { pid: sc.video.0, coding: sc.video.1, fmt_rate: sc.video_code },
            audio: Some(stns[0]), more_audio: stns[1..].to_vec(),
            with_menu: false, pg: pg_info[i].clone(), marks: if marks[i].is_empty() { None } else { Some(marks[i].clone()) },
            ..Default::default()
        };
        files.insert(format!("PLAYLIST/{i:05}.mpls"), make_playlist(&pp));
        video_params.push(pp);
    }
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
    let jump_back = Command::new(JUMP_TITLE_REG_INSN, R_RETURN, 0);
    let mut objs = vec![MovieObject::new(vec![Command::new(JUMP_TITLE_INSN, page_title(home_idx), 0)])]; // 0 : Top Menu -> accueil
    for v in 0..n {
        objs.push(MovieObject::new(vec![Command::new(PLAY_PL_INSN, v as u32, 0), jump_back]));
    }
    for k in 0..np {
        objs.push(MovieObject::new(vec![
            Command::new(PLAY_PL_INSN, (n + k) as u32, 0), Command::new(NOP_INSN, 0, 0), Command::new(NOP_INSN, 0, 0), Command::new(NOP_INSN, 0, 0),
            Command::new(GOTO_INSN, 0, 0),
        ]));
    }
    for &v in pfm.keys() {
        objs.push(MovieObject::new(vec![
            Command::new(MOVE_RR_INSN, 10, 0), Command::new(MOVE_RI_INSN, 0, 0), Command::new(PLAY_PL_PM_INSN, v as u32, 10), jump_back,
        ]));
    }
    if let Some(pl) = play_all_pl {
        objs.push(MovieObject::new(vec![Command::new(PLAY_PL_INSN, pl, 0), jump_back]));
    }
    if objs.len() != total_titles as usize + 1 { return bad("objets de navigation : numerotation inattendue"); }
    let fp = match intro_pl {
        Some(pl) => {
            objs.push(MovieObject::new(vec![Command::new(PLAY_PL_INSN, pl, 0), Command::new(JUMP_TITLE_INSN, 0, 0)]));
            (objs.len() - 1) as u16
        }
        None => 0,
    };
    files.insert("MovieObject.bdmv".into(), MovieObjectBdmv { version: "0100".into(), objects: objs }.pack());
    let title_objs: Vec<u16> = (1..=total_titles as u16).collect();
    files.insert("index.bdmv".into(), make_index(fp, 0, &title_objs, b"bdauthor"));
    // sons des menus (AUXDATA/sound.bdmv)
    if !sound_ids.is_empty() {
        let mut by_id: Vec<(&String, u8)> = sound_ids.iter().map(|(f, k)| (f, *k)).collect();
        by_id.sort_by_key(|x| x.1);
        let mut sounds = Vec::new();
        for (f, _) in by_id {
            let path = p(f);
            if !path.is_file() { return bad(format!("son de menu introuvable : {f}")); }
            sounds.push(ffmpeg::decode_menu_sound(env.tools, &path)?);
        }
        let data = crate::sound::make_sound_bdmv(&sounds);
        (env.log)(&format!("sons des menus : {} son(s), {:.1} s en tout, {:.0} Ko", sounds.len(), sounds.iter().map(|s| s.seconds()).sum::<f64>(), data.len() as f64 / 1024.0));
        files.insert("AUXDATA/sound.bdmv".into(), data);
    }

    write_bdmv(out_dir, &files)?;
    let report = check_bdmv(&out_dir.join("BDMV"));
    (env.progress)(1.0);
    Ok(BuildInfo {
        videos: n, main_pages: np,
        chapters: marks.iter().enumerate().filter(|(_, m)| m.len() > 1).map(|(i, m)| (i + 1, m.len())).collect(),
        format: fmt, menu_size, ics_pts_s: first_page.pts as f64 / 90000.0, menu_bytes: first_page.bytes,
        menu_objects: first_page.objects, palette_entries: first_page.palette,
        total_stream_bytes: files.iter().filter(|(k, _)| k.starts_with("STREAM")).map(|(_, v)| v.len()).sum(),
        durations_s: scans.iter().map(|sc| ((sc.end90 - sc.start90) as f64 / 90000.0 * 100.0).round() / 100.0).collect(),
        report,
    })
}
