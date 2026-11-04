//! bdauthor : creation de disques Blu-ray (menus HDMV) a partir de vos videos. Interface egui/eframe.
#![cfg_attr(all(not(debug_assertions), windows), windows_subsystem = "windows")]

mod i18n;
mod editor_ui;
mod audio_ui;
mod subs_ui;
mod style_ui;
mod sim_ui;
mod iso_ui;
mod cover_ui;
mod chapters_ui;
mod frame_ui;

use crate::i18n::{tr, trf};
use bdauthor_core::check::Kind;
use bdauthor_core::editor::{History, PageRef};
use bdauthor_core::palette::RgbaImage;
use bdauthor_core::ffmpeg::Tools;
use bdauthor_core::model::{ChapterEntry, ProjectModel, VideoEntry, PRESETS};
use bdauthor_core::iso::{finish_build, temp_build_dir, IsoKeep};
use bdauthor_core::project::{build_project, BuildEnv};
use bdauthor_render::FontRenderer;
use eframe::egui;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{channel, Receiver};
use std::sync::Arc;

enum Msg {
    Log(String),
    Progress(f64),
    /// (succes, resume)
    Done(Result<(bool, String), String>),
}

/// Reglages de l'application, enregistres dans un petit fichier texte (cle=valeur) du dossier de configuration de l'utilisateur.
#[derive(Clone, PartialEq)]
struct Config {
    /// Dossier de ffmpeg/ffprobe (ou chemin de ffmpeg) ; vide = recherche automatique.
    ffmpeg_path: String,
    /// Pendant un glissement, espace les rendus de l'apercu selon la duree du dernier rendu (sinon : a chaque image).
    adaptive_preview: bool,
    /// sauvegarde automatique du projet (fichier a cote, jamais le projet lui-meme) toutes les `autosave_min` minutes
    autosave: bool,
    autosave_min: u32,
    /// langue de l'interface (code : en, fr, ou un fichier du dossier `langues`)
    lang: String,
}

/// Ecrit `text` dans un fichier temporaire voisin puis le renomme : un plantage ou un disque plein en cours d'ecriture ne laisse jamais un fichier tronque.
fn write_atomic(path: &Path, text: &str) -> std::io::Result<()> {
    let mut tmp = path.as_os_str().to_owned();
    tmp.push(".tmp");
    let tmp = PathBuf::from(tmp);
    std::fs::write(&tmp, text)?;
    std::fs::rename(&tmp, path).map_err(|e| { let _ = std::fs::remove_file(&tmp); e })
}

/// Fichier de sauvegarde automatique : `<projet>.autosave.json` a cote du projet ; un projet jamais enregistre utilise le dossier de configuration.
fn autosave_path(project: Option<&Path>) -> Option<PathBuf> {
    match project {
        Some(p) => {
            let stem = p.file_stem()?.to_string_lossy().into_owned();
            Some(p.with_file_name(format!("{stem}.autosave.json")))
        }
        None => Config::path().map(|c| c.with_file_name("recuperation.autosave.json")),
    }
}

/// « il y a 3 min », « il y a 2 h », « il y a 4 j » pour un fichier modifie a `t`.
fn age_text(t: std::time::SystemTime) -> String {
    let s = std::time::SystemTime::now().duration_since(t).map(|d| d.as_secs()).unwrap_or(0);
    if s < 90 { tr("less than 2 minutes ago").into() } else if s < 5400 { trf!("{} min ago", s / 60) } else if s < 172800 { trf!("{} h ago", s / 3600) } else { trf!("{} days ago", s / 86400) }
}

/// Fenetre reelle du systeme (viewport secondaire) : elle peut sortir du cadre de la fenetre principale, passer sur un autre ecran et se
/// redimensionner. `open` repasse a faux a la fermeture. Si le systeme ne gere pas les fenetres multiples, c'est une fenetre flottante classique.
pub(crate) fn os_window(ctx: &egui::Context, id: &str, title: &str, size: [f32; 2], open: &mut bool, add: &mut dyn FnMut(&mut egui::Ui)) {
    os_window_panels(ctx, id, title, size, open, None, &mut |ui, _| add(ui));
}

/// Comme `os_window`, avec en plus une zone du bas dont la hauteur se regle en glissant son bord (`bottom` : hauteur initiale ; `add` est
/// alors appele deux fois, d'abord pour la zone du bas (`true`), puis pour le contenu principal (`false`), qui n'a pas de defilement propre).
pub(crate) fn os_window_panels(ctx: &egui::Context, id: &str, title: &str, size: [f32; 2], open: &mut bool, bottom: Option<f32>, add: &mut dyn FnMut(&mut egui::Ui, bool)) {
    let builder = egui::ViewportBuilder::default().with_title(title).with_inner_size(size).with_min_inner_size([320.0, 200.0]);
    ctx.show_viewport_immediate(egui::ViewportId::from_hash_of(id), builder, |ctx, class| {
        if class == egui::ViewportClass::Embedded {
            let mut still = true;
            egui::Window::new(title).open(&mut still).default_width(size[0]).show(ctx, |ui| {
                if bottom.is_some() { add(ui, true); ui.separator(); }
                add(ui, false);
            });
            if !still { *open = false; }
        } else {
            match bottom {
                Some(h) => {
                    egui::TopBottomPanel::bottom(egui::Id::new((id, "bas"))).resizable(true).default_height(h).height_range(60.0..=900.0).show(ctx, |ui| {
                        egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| add(ui, true));
                    });
                    egui::CentralPanel::default().show(ctx, |ui| add(ui, false));
                }
                None => { egui::CentralPanel::default().show(ctx, |ui| { egui::ScrollArea::both().auto_shrink([false, false]).show(ui, |ui| add(ui, false)); }); }
            }
            if ctx.input(|i| i.viewport().close_requested()) { *open = false; }
        }
    });
}

/// Bouton « ? » : le texte d'explication s'affiche au survol (il prend ainsi moins de place qu'un paragraphe dans les panneaux).
pub(crate) fn help(ui: &mut egui::Ui, text: &str) {
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(18.0, 18.0), egui::Sense::click());
    let vis = *ui.style().interact(&resp);
    ui.painter().circle(rect.center(), 8.0, vis.bg_fill, egui::Stroke::new(1.0_f32, vis.fg_stroke.color));
    ui.painter().text(rect.center(), egui::Align2::CENTER_CENTER, "?", egui::FontId::proportional(12.0), vis.fg_stroke.color);
    let _ = resp.on_hover_cursor(egui::CursorIcon::Help).on_hover_ui(|ui| {
        ui.set_max_width(380.0);
        ui.label(text);
    });
}

/// Petit bouton « supprimer » dessine (une croix) : le glyphe ✕ n'est pas dans la police par defaut.
pub(crate) fn delete_button(ui: &mut egui::Ui, tip: &str) -> bool {
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(20.0, 20.0), egui::Sense::click());
    let vis = *ui.style().interact(&resp);
    let p = ui.painter();
    p.rect(rect, 3.0, vis.bg_fill, vis.bg_stroke);
    let r = rect.shrink(6.0);
    let line = egui::Stroke::new(1.8_f32, vis.fg_stroke.color);
    p.line_segment([r.left_top(), r.right_bottom()], line);
    p.line_segment([r.right_top(), r.left_bottom()], line);
    resp.on_hover_text(tip).clicked()
}

/// Charge du glisser-deposer de la liste des videos (rang de la video).
#[derive(Clone, Copy)]
struct VideoDrag(usize);

impl Config {
    fn path() -> Option<PathBuf> {
        let base = std::env::var_os("APPDATA").map(PathBuf::from).or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))?;
        Some(base.join("bdauthor").join("config.txt"))
    }
    fn load() -> Config {
        let mut c = Config { adaptive_preview: true, ffmpeg_path: String::new(), autosave: true, autosave_min: 2, lang: "fr".to_string() };
        if let Some(t) = Config::path().and_then(|p| std::fs::read_to_string(p).ok()) {
            for l in t.lines() {
                match l.split_once('=') {
                    Some(("adaptive_preview", v)) => c.adaptive_preview = v.trim() != "false",
                    Some(("ffmpeg_path", v)) => c.ffmpeg_path = v.trim().to_string(),
                    Some(("autosave", v)) => c.autosave = v.trim() != "false",
                    Some(("autosave_min", v)) => c.autosave_min = v.trim().parse().unwrap_or(2).clamp(1, 60),
                    Some(("lang", v)) => c.lang = v.trim().to_string(),
                    _ => {}
                }
            }
        }
        Tools::set_custom_path(&c.ffmpeg_path);
        crate::i18n::set_language(&c.lang);
        c
    }
    fn save(&self) {
        if let Some(p) = Config::path() {
            if let Some(d) = p.parent() {
                let _ = std::fs::create_dir_all(d);
            }
            let _ = std::fs::write(p, format!("adaptive_preview={}\nffmpeg_path={}\nautosave={}\nautosave_min={}\nlang={}\n", self.adaptive_preview, self.ffmpeg_path, self.autosave, self.autosave_min, self.lang));
        }
    }
}

#[derive(PartialEq, Clone, Copy)]
enum Mode {
    Projet,
    MiseEnPage,
    Jaquette,
}

struct App {
    mode: Mode,
    // editeur de mise en page
    cur: Option<PageRef>,
    history: History,
    t0: std::time::Instant,
    last_preview: std::time::Instant,
    /// duree du dernier rendu d'apercu (ms)
    render_ms: f32,
    cfg: Config,
    show_config: bool,
    /// fenetre « Projet » (onglets General, Style des boutons, Langues)
    show_project: bool,
    /// onglet affiche de la fenetre Projet : 0 General, 1 Style des boutons, 2 Langues
    project_tab: u8,
    /// element montre selectionne dans l'apercu courant
    preview_sel: Option<usize>,
    /// etat « active » montre dans l'apercu courant
    preview_act: bool,
    /// image du halo anime montree dans l'apercu (simulation)
    preview_pulse: usize,
    /// pas des animations de l'etat normal montre dans l'apercu (+ 1 ; 0 : images fixes)
    preview_nstep: usize,
    /// apercu de l'animation personnalisee de l'element selectionne (hors simulation)
    anim_preview: bool,
    /// depart de l'animation de l'apercu : (element selectionne, apercu actif, simulation) et instant ; relancee a chaque nouvelle selection
    anim_key: (Option<usize>, bool, bool, bool),
    anim_t0: std::time::Instant,
    /// simulation de la telecommande en cours
    sim: Option<sim_ui::Sim>,
    sel_item: Option<usize>,
    /// autres elements selectionnes avec `sel_item` (Maj+clic) : deplaces et alignes ensemble
    extra_sel: Vec<usize>,
    /// presse-papiers des widgets (copier / couper / coller, y compris d'une page a l'autre)
    clipboard: Vec<bdauthor_core::design::Item>,
    clip_from: Option<PageRef>,
    /// le presse-papiers vient d'un « couper » : les widgets collés gardent leur nom
    clip_cut: bool,
    /// onglet Jaquette : element selectionne, texture de l'apercu, compteurs de modification / de rendu
    cover_sel: Option<usize>,
    /// elements selectionnes en plus de cover_sel (selection multiple)
    cover_extra: Vec<usize>,
    /// rectangle de selection en cours : point de depart (mm) et selection de base
    cover_marquee: Option<((f64, f64), Vec<usize>)>,
    /// presse-papiers des elements de jaquette (partage jaquette / etiquette)
    cover_clip: Vec<bdauthor_core::cover::CoverItem>,
    cover_paste_n: usize,
    /// aimantation et zone de securite
    cover_snap: bool,
    cover_safe: bool,
    /// sauvegarde automatique : dernier essai, derniere reussite, sauvegarde proposee a la restauration
    autosave_t: std::time::Instant,
    autosave_ok: Option<std::time::Instant>,
    restore_offer: Option<PathBuf>,
    /// video dont le retrait attend la confirmation de l'utilisateur
    confirm_remove: Option<usize>,
    /// fenetre « Quitter sans enregistrer ? » ; `quit_ok` : la fermeture est confirmee
    confirm_quit: bool,
    quit_ok: bool,
    show_about: bool,
    recovery_checked: bool,
    /// fenetre d'apercu du rendu d'une piste de sous-titres
    sub_preview: Option<subs_ui::SubPreview>,
    /// editeur de chapitres ouvert (une video a la fois) et demande d'ouverture (rang de la video)
    chapter_editor: Option<chapters_ui::ChapterEditor>,
    chapter_request: Option<usize>,
    /// selecteur d'image d'un widget « Image de la video » : (page, element)
    frame_picker: Option<frame_ui::FramePicker>,
    frame_request: Option<(usize, usize)>,
    /// images entieres des videos (widget « Image de la video »), par (fichier, instant en ms)
    frames: HashMap<(String, i64), Arc<RgbaImage>>,
    cover_tex: Option<egui::TextureHandle>,
    cover_gen: u64,
    cover_seen: u64,
    /// 0 : jaquette, 1 : etiquette de disque
    cover_page: u8,
    /// calque actif (recoit les nouveaux elements)
    cover_layer: usize,
    /// vue de la jaquette : zoom (1 = feuille entiere), decalage, deplacement en cours, regles en pouces, finesse (dpi) de l'apercu
    cover_zoom: f32,
    cover_pan: egui::Vec2,
    cover_panning: bool,
    cover_inch: bool,
    /// export PNG rogne au format fini
    cover_png_trim: bool,
    cover_tex_dpi: u32,
    /// calques replies dans la liste, calque en cours de renommage
    cover_closed: HashSet<usize>,
    cover_layer_edit: Option<(usize, String)>,
    /// historique annuler / refaire de (jaquette, etiquette)
    cover_hist: Option<bdauthor_core::cover::EditHistory<(bdauthor_core::cover::Cover, bdauthor_core::cover::DiscLabel)>>,
    /// redimensionnement / deplacement a la souris en cours
    cover_drag: Option<cover_ui::CoverDrag>,
    /// export PDF en arriere-plan : message final
    cover_export_rx: Option<Receiver<String>>,
    /// valeurs des champs automatiques utilisees pour l'apercu
    cover_vars_seen: std::collections::BTreeMap<String, String>,
    /// vue de l'apercu : zoom (1 = page entiere), decalage en pixels, page a laquelle elle se rapporte, deplacement en cours
    view_zoom: f32,
    view_pan: egui::Vec2,
    view_page: Option<PageRef>,
    view_panning: bool,
    paste_n: i32,
    /// rectangle de selection en cours (depart sur le plan, selection de depart si Maj)
    marquee: Option<((f32, f32), Vec<usize>)>,
    /// calque ou vont les nouveaux elements (identifiant ; 0 : celui de l'element selectionne, sinon le premier plan)
    active_layer: u32,
    /// calque en cours de renommage : (page, identifiant, nom saisi)
    layer_edit: Option<(usize, u32, String)>,
    /// pages depliees (true) ou repliees (false) dans l'arborescence ; absente : repliee
    tree_pages: HashMap<usize, bool>,
    /// calques replies : (page, identifiant)
    tree_layers_closed: HashSet<(usize, u32)>,
    /// page en cours de renommage : (indice, nom saisi)
    page_edit: Option<(usize, String)>,
    drag: Option<editor_ui::DragState>,
    snap: bool,
    /// aimantation aux bords et centres de l'ecran et des autres elements (Alt : desactivee le temps du geste)
    guide_snap: bool,
    edit_gen: u64,
    preview_gen: u64,
    preview_tex: Option<egui::TextureHandle>,
    base_cache: Option<(PathBuf, RgbaImage)>,
    durations: HashMap<String, f64>,
    /// videos dont la duree n'a pas pu etre lue (fichier absent ou illisible)
    probe_failed: HashSet<String>,
    /// (hauteur, cadence) de chaque video sondee
    video_sizes: HashMap<String, (u32, f64)>,
    sub_probes: subs_ui::Probes,
    audio_probes: audio_ui::AudioProbes,
    thumbs: HashMap<(String, i64), Arc<RgbaImage>>,
    requested: HashSet<String>,
    worker: Option<editor_ui::Worker>,
    renderer: Result<FontRenderer, String>,
    model: ProjectModel,
    path: Option<PathBuf>,
    dirty: bool,
    sel: Option<usize>,
    log: Vec<String>,
    progress: f32,
    rx: Option<Receiver<Msg>>,
    cancel: Arc<AtomicBool>,
    result: Option<(bool, String)>,
    notice: String,
}

impl Default for App {
    fn default() -> Self {
        App {
            mode: Mode::Projet, cur: None, history: History::default(), t0: std::time::Instant::now(), last_preview: std::time::Instant::now(), render_ms: 0.0, cfg: Config::load(), show_config: false, show_project: false, project_tab: 0, preview_sel: None, preview_act: false, preview_pulse: 0, preview_nstep: 0, anim_preview: false, anim_key: (None, false, false, false), anim_t0: std::time::Instant::now(), sim: None, sel_item: None, extra_sel: Vec::new(), clipboard: Vec::new(), clip_from: None, clip_cut: false, cover_sel: None, cover_extra: Vec::new(), cover_marquee: None, cover_clip: Vec::new(), cover_paste_n: 0, cover_snap: true, cover_safe: false, autosave_t: std::time::Instant::now(), autosave_ok: None, restore_offer: None, confirm_remove: None, confirm_quit: false, quit_ok: false, show_about: false, recovery_checked: false, sub_preview: None, chapter_editor: None, chapter_request: None, frame_picker: None, frame_request: None, frames: HashMap::new(), cover_tex: None, cover_gen: 1, cover_seen: 0, cover_page: 0, cover_layer: 0, cover_zoom: 1.0, cover_pan: egui::Vec2::ZERO, cover_panning: false, cover_inch: false, cover_png_trim: false, cover_tex_dpi: 0, cover_closed: HashSet::new(), cover_layer_edit: None, cover_hist: None, cover_drag: None, cover_export_rx: None, cover_vars_seen: Default::default(), view_zoom: 1.0, view_pan: egui::Vec2::ZERO, view_page: None, view_panning: false, paste_n: 0, marquee: None, active_layer: 0, layer_edit: None, page_edit: None, tree_pages: HashMap::new(), tree_layers_closed: HashSet::new(), drag: None, snap: true, guide_snap: true, edit_gen: 1, preview_gen: 0, preview_tex: None, base_cache: None,
            durations: HashMap::new(), probe_failed: HashSet::new(), video_sizes: HashMap::new(), sub_probes: HashMap::new(), audio_probes: HashMap::new(), thumbs: HashMap::new(), requested: HashSet::new(), worker: None, renderer: FontRenderer::new(),
            model: ProjectModel::default(), path: None, dirty: false, sel: None, log: vec![], progress: 0.0, rx: None, cancel: Arc::new(AtomicBool::new(false)), result: None, notice: String::new() }
    }
}

fn clean(p: &Path) -> String {
    p.to_string_lossy().replace('\\', "/")
}

/// Duree d'une video : « m:ss », ou « h:mm:ss » a partir d'une heure.
fn duration_text(t: f64) -> String {
    let s = t.round().max(0.0) as u64;
    if s >= 3600 { format!("{}:{:02}:{:02}", s / 3600, s / 60 % 60, s % 60) } else { format!("{}:{:02}", s / 60, s % 60) }
}

impl App {
    fn running(&self) -> bool {
        self.rx.is_some()
    }

    fn new_project(&mut self) {
        let worker = self.worker.take();
        *self = App::default();
        self.worker = worker;
    }

    fn reset_editor(&mut self) {
        self.cur = None;
        self.sel_item = None;
        self.extra_sel.clear();
        self.drag = None;
        self.cover_hist = None;
        self.cover_sel = None;
        self.cover_extra.clear();
        self.cover_gen += 1;
        self.edit_gen += 1;
        self.base_cache = None;
        self.history.reset(&self.model.designs);
    }

    /// Sauvegarde automatique : seulement si le projet a change, hors geste a la souris ; ne touche jamais au fichier du projet.
    fn autosave_tick(&mut self, ctx: &egui::Context) {
        if !self.cfg.autosave { return; }
        let every = std::time::Duration::from_secs(self.cfg.autosave_min.max(1) as u64 * 60);
        ctx.request_repaint_after(std::time::Duration::from_secs(30));
        if !self.dirty || self.autosave_t.elapsed() < every || ctx.input(|i| i.pointer.any_down()) { return; }
        self.autosave_t = std::time::Instant::now();
        let Some(p) = autosave_path(self.path.as_deref()) else { return };
        if let Some(d) = p.parent() { let _ = std::fs::create_dir_all(d); }
        match write_atomic(&p, &self.model.to_json()) {
            Ok(()) => self.autosave_ok = Some(std::time::Instant::now()),
            Err(e) => self.notice = trf!("Auto-save failed: {}", e),
        }
    }

    /// Une sauvegarde automatique plus recente que le projet (ou, sans projet, le fichier de recuperation) est proposee a la restauration.
    fn check_recovery(&mut self) {
        let Some(a) = autosave_path(self.path.as_deref()) else { return };
        let Ok(am) = std::fs::metadata(&a).and_then(|m| m.modified()) else { return };
        let newer = match &self.path {
            Some(p) => std::fs::metadata(p).and_then(|m| m.modified()).map(|pm| am > pm).unwrap_or(true),
            None => true,
        };
        if newer { self.restore_offer = Some(a); }
    }

    fn restore_autosave(&mut self, a: &Path) {
        match std::fs::read_to_string(a).map_err(|e| e.to_string()).and_then(|t| ProjectModel::from_json(&t)) {
            Ok(m) => {
                self.model = m;
                self.dirty = true;
                self.sel = if self.model.videos.is_empty() { None } else { Some(0) };
                self.reset_editor();
                self.notice = tr("Auto-save restored: save the project to keep it.").into();
            }
            Err(e) => self.notice = trf!("Auto-save unreadable: {}", e),
        }
    }

    fn open(&mut self) {
        let Some(p) = rfd::FileDialog::new().add_filter(tr("bdauthor project"), &["json"]).pick_file() else { return };
        match std::fs::read_to_string(&p).map_err(|e| e.to_string()).and_then(|t| ProjectModel::from_json(&t)) {
            Ok(m) => {
                self.model = m;
                self.path = Some(p);
                self.dirty = false;
                self.sel = if self.model.videos.is_empty() { None } else { Some(0) };
                self.reset_editor();
                self.notice = tr("Project opened.").into();
                self.restore_offer = None;
                self.check_recovery();
            }
            Err(e) => self.notice = trf!("Cannot open the project: {}", e),
        }
    }

    fn save(&mut self, as_new: bool) -> bool {
        let target = if as_new || self.path.is_none() {
            match rfd::FileDialog::new().add_filter(tr("bdauthor project"), &["json"]).set_file_name("projet.json").save_file() {
                Some(p) => p,
                None => return false,
            }
        } else {
            self.path.clone().unwrap()
        };
        match write_atomic(&target, &self.model.to_json()) {
            Ok(()) => {
                // la sauvegarde automatique n'a plus lieu d'etre : le projet est a jour
                for q in [autosave_path(self.path.as_deref()), autosave_path(Some(target.as_path())), autosave_path(None)].into_iter().flatten() { let _ = std::fs::remove_file(q); }
                self.autosave_ok = None;
                self.restore_offer = None;
                self.path = Some(target);
                self.dirty = false;
                self.notice = tr("Project saved.").into();
                true
            }
            Err(e) => {
                self.notice = trf!("Cannot save: {}", e);
                false
            }
        }
    }

    fn start_build(&mut self, ctx: &egui::Context) {
        if self.running() {
            return;
        }
        self.result = None;
        if !self.save(false) {
            return;
        }
        let project = self.path.clone().unwrap();
        let base = project.parent().map(Path::to_path_buf).unwrap_or_default();
        let problems = self.model.validate(&base);
        if !problems.is_empty() {
            self.result = Some((false, trf!("Fix this first:\n- {}", problems.join("\n- "))));
            return;
        }
        // destination : dossier, image, ou les deux (reglages de l'onglet « Génération ISO »)
        let iso_cfg = self.model.iso.clone();
        let title = self.model.title.clone();
        let (out, iso_path) = match iso_cfg.keep {
            IsoKeep::Folder | IsoKeep::Both => {
                let Some(out) = rfd::FileDialog::new().set_title(tr("Disc output folder")).pick_folder() else { return };
                let iso = (iso_cfg.keep == IsoKeep::Both).then(|| iso_cfg.iso_path(&base, &out));
                (out, iso)
            }
            IsoKeep::Iso => {
                let iso = if iso_cfg.file.trim().is_empty() {
                    let name = format!("{}.iso", title.chars().map(|c| if r#"\/:*?"<>|"#.contains(c) { '_' } else { c }).collect::<String>().trim());
                    let Some(f) = rfd::FileDialog::new().set_title(tr("Disc ISO image")).add_filter(tr("ISO image"), &["iso"]).set_file_name(name).save_file() else { return };
                    f
                } else {
                    iso_cfg.iso_path(&base, &base)
                };
                match temp_build_dir(&iso) {
                    Ok(d) => (d, Some(iso)),
                    Err(e) => { self.result = Some((false, trf!("Failed: {}", e))); return; }
                }
            }
        };
        self.log.clear();
        self.progress = 0.0;
        self.cancel = Arc::new(AtomicBool::new(false));
        let (tx, rx) = channel();
        self.rx = Some(rx);
        let (cancel, ctx) = (self.cancel.clone(), ctx.clone());
        std::thread::spawn(move || {
            let send = |m: Msg| {
                let _ = tx.send(m);
                ctx.request_repaint();
            };
            let run = || -> Result<(bool, String), String> {
                let tools = Tools::locate().map_err(|e| e.to_string())?;
                let renderer = FontRenderer::new()?;
                let mut log = |m: &str| send(Msg::Log(m.to_string()));
                let mut prog = |f: f64| send(Msg::Progress(f));
                let mut env = BuildEnv { tools: &tools, renderer: &renderer, cancel: &cancel, log: &mut log, progress: &mut prog };
                let built = build_project(&project, &out, &mut env).map_err(|e| e.to_string());
                // image seule : le dossier de construction temporaire ne doit pas rester apres un echec
                if built.is_err() && iso_cfg.keep == IsoKeep::Iso { let _ = std::fs::remove_dir_all(&out); }
                let info = built?;
                let mut s = trf!("{} video(s), {} menu page(s), format {}x{} at {} fps, {} MB of stream.\n", info.videos, info.main_pages, info.format.0, info.format.1, info.format.2, format!("{:.1}", info.total_stream_bytes as f64 / 1e6));
                if iso_cfg.keep != IsoKeep::Iso { s += &trf!("Disc written to {}\n", out.display()); }
                if let Some(iso) = &iso_path {
                    let r = finish_build(&out, iso, &iso_cfg, &title, &cancel, env.log, env.progress).map_err(|e| e.to_string());
                    if r.is_err() && iso_cfg.keep == IsoKeep::Iso { let _ = std::fs::remove_dir_all(&out); }
                    s += &r.map_err(|e| trf!("ISO image: {}", e))?;
                    s.push('\n');
                }
                for (k, m) in &info.report.items {
                    match k {
                        Kind::Error => s += &trf!("ERROR: {}\n", m),
                        Kind::Warn => s += &trf!("Warning: {}\n", m),
                        Kind::Info => {}
                    }
                }
                s += if info.report.ok() { tr("Verification: no errors.") } else { tr("Verification: ERRORS detected.") };
                Ok((info.report.ok(), s))
            };
            send(Msg::Done(run()));
        });
    }

    fn poll(&mut self) {
        let mut done = false;
        if let Some(rx) = &self.rx {
            while let Ok(m) = rx.try_recv() {
                match m {
                    Msg::Log(l) => self.log.push(l),
                    Msg::Progress(p) => self.progress = p as f32,
                    Msg::Done(r) => {
                        self.result = Some(match r {
                            Ok(x) => x,
                            Err(e) => (false, if e.contains("annulee") { tr("Build cancelled.").to_string() } else { trf!("Failed: {}", e) }),
                        });
                        done = true;
                    }
                }
            }
        }
        if done {
            self.rx = None;
        }
    }

    fn add_videos(&mut self) {
        let Some(files) = rfd::FileDialog::new()
            .add_filter(tr("Videos"), &["mp4", "mkv", "mov", "avi", "m2ts", "mts", "ts", "webm", "wmv", "flv", "m4v", "mpg", "mpeg"])
            .pick_files() else { return };
        for f in files {
            self.model.videos.push(VideoEntry::new(&clean(&f)));
        }
        if self.sel.is_none() && !self.model.videos.is_empty() {
            self.sel = Some(0);
        }
        self.dirty = true;
    }
}

impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.poll();
        if self.worker.is_none() {
            self.worker = Some(editor_ui::Worker::start(ctx.clone()));
        }
        self.poll_worker();
        self.history_step(ctx);
        if !self.recovery_checked { self.recovery_checked = true; self.check_recovery(); }
        self.autosave_tick(ctx);
        if self.running() {
            ctx.request_repaint_after(std::time::Duration::from_millis(200));
        }
        let busy = self.running();
        // fermeture de la fenetre (croix ou Fichier > Quitter) : confirmation si le projet n'est pas enregistre ou si une construction tourne
        if ctx.input(|i| i.viewport().close_requested()) && !self.quit_ok && (self.dirty || busy) {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            self.confirm_quit = true;
        }
        if self.confirm_quit {
            let mut choice: Option<u8> = None;
            egui::Window::new(tr("Quit bdauthor?")).collapsible(false).resizable(false).anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO).show(ctx, |ui| {
                if self.dirty { ui.label(tr("The project has unsaved changes.")); }
                if busy { ui.label(tr("A disc build is in progress: it will be stopped.")); }
                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    if self.dirty && ui.button(tr("Save and quit")).clicked() { choice = Some(2); }
                    if ui.button(tr("Quit without saving")).clicked() { choice = Some(1); }
                    if ui.button(tr("Cancel")).clicked() { choice = Some(0); }
                });
            });
            if ctx.input(|i| i.key_pressed(egui::Key::Escape)) { choice = Some(0); }
            match choice {
                Some(2) => { if self.save(false) { self.quit_ok = true; ctx.send_viewport_cmd(egui::ViewportCommand::Close); } self.confirm_quit = false; }
                Some(1) => { self.cancel.store(true, Ordering::Relaxed); self.quit_ok = true; ctx.send_viewport_cmd(egui::ViewportCommand::Close); self.confirm_quit = false; }
                Some(_) => self.confirm_quit = false,
                None => {}
            }
        }
        if self.show_about {
            let mut open = true;
            egui::Window::new(tr("About bdauthor")).open(&mut open).collapsible(false).resizable(false).anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO).show(ctx, |ui| {
                ui.heading("bdauthor");
                ui.label(trf!("Version {}", env!("CARGO_PKG_VERSION")));
                ui.add_space(4.0);
                ui.label(tr("Blu-ray disc authoring: videos, chapters, interactive menus (HDMV), subtitles, audio tracks, cover and ISO image."));
                ui.add_space(4.0);
                ui.label(egui::RichText::new(tr("Uses ffmpeg (LGPL / GPL, run as a separate program), egui / eframe, rfd, image, ab_glyph and resvg (MIT / Apache-2.0 licenses).")).weak());
                ui.add_space(4.0);
                if let Some(p) = Config::path() { ui.label(egui::RichText::new(trf!("Settings: {}", p.display())).small().weak()); }
                if let Some(d) = crate::i18n::user_dir() { ui.label(egui::RichText::new(trf!("Additional languages: {}", d.display())).small().weak()); }
                ui.add_space(6.0);
                if ui.button(tr("Close")).clicked() { self.show_about = false; }
            });
            if !open { self.show_about = false; }
        }
        // vignettes liees a un chapitre : elles suivent l'instant du chapitre (et se detachent si le chapitre disparait)
        if self.model.sync_chapters() { self.dirty = true; self.bump(); }
        // durees des videos (affichees dans la liste) : sondees une fois, en tache de fond
        self.request_durations();
        if let Some(a) = self.restore_offer.clone() {
            let when = std::fs::metadata(&a).and_then(|m| m.modified()).map(age_text).unwrap_or_default();
            let mut done = false;
            egui::Window::new(tr("Project recovery")).collapsible(false).resizable(false).anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0]).show(ctx, |ui| {
                ui.label(trf!("An auto-save more recent than the project was found ({}). The program may have stopped before saving.", when));
                ui.label(egui::RichText::new(clean(&a)).weak());
                ui.horizontal(|ui| {
                    if ui.button(tr("Restore")).clicked() { self.restore_autosave(&a); done = true; }
                    if ui.button(tr("Ignore")).on_hover_text(tr("Keeps the backup file; it will be offered again at next opening")).clicked() { done = true; }
                    if ui.button(tr("Delete the backup")).clicked() { let _ = std::fs::remove_file(&a); done = true; }
                });
            });
            if done { self.restore_offer = None; }
        }
        if let (Some(pv), Ok(r)) = (self.sub_preview.as_mut(), &self.renderer) {
            let base = self.path.as_ref().and_then(|p| p.parent()).map(Path::to_path_buf).unwrap_or_default();
            let (ch, keep) = match self.model.videos.get_mut(pv.video) {
                Some(v) => subs_ui::preview_window(ctx, pv, v, r, &base),
                None => (false, false),
            };
            if ch { self.dirty = true; }
            if !keep { self.sub_preview = None; }
        }
        if let Some(vi) = self.chapter_request.take() {
            let base = self.path.as_ref().and_then(|p| p.parent()).map(Path::to_path_buf).unwrap_or_default();
            self.chapter_editor = chapters_ui::ChapterEditor::new(&self.model, vi, &base);
        }
        let res = self.chapter_editor.as_mut().map(|ed| chapters_ui::window(ctx, ed, &mut self.model));
        if let Some((ch, keep)) = res {
            if ch { self.dirty = true; self.bump(); }
            if !keep { self.chapter_editor = None; }
        }
        if let Some((page, item)) = self.frame_request.take() {
            let base = self.path.as_ref().and_then(|p| p.parent()).map(Path::to_path_buf).unwrap_or_default();
            self.frame_picker = frame_ui::FramePicker::new(&self.model, page, item, &base);
        }
        let res = self.frame_picker.as_mut().map(|fp| frame_ui::window(ctx, fp, &mut self.model));
        if let Some((ch, keep)) = res {
            if ch { self.dirty = true; self.bump(); }
            if !keep { self.frame_picker = None; }
        }
        let mut open = self.show_config;
        // une fenetre systeme existe tant qu'on l'appelle : seulement quand elle est ouverte
        if open { os_window(ctx, "configuration", tr("Settings"), [640.0, 480.0], &mut open, &mut |ui| {
            let before = self.cfg.clone();
            ui.horizontal(|ui| {
                ui.label("Langue / Language :");
                let langs = crate::i18n::available();
                let cur = langs.iter().find(|(c, _)| *c == self.cfg.lang).map_or_else(|| self.cfg.lang.clone(), |(_, n)| n.clone());
                egui::ComboBox::from_id_salt("interface_language").selected_text(cur).show_ui(ui, |ui| {
                    for (c, n) in &langs {
                        if ui.selectable_label(self.cfg.lang == *c, n).clicked() { self.cfg.lang = c.clone(); }
                    }
                });
                if ui.button(tr("Create a translation…")).on_hover_text(tr("Saves a file with all the texts of the interface, to translate into another language. Put the translated file in the “langues” folder next to the program: the language then appears in this list.")).clicked() {
                    let mut dlg = rfd::FileDialog::new().add_filter("JSON", &["json"]).set_file_name("langue.json");
                    if let Some(d) = crate::i18n::user_dir() { let _ = std::fs::create_dir_all(&d); dlg = dlg.set_directory(d); }
                    if let Some(f) = dlg.save_file() {
                        let name = f.file_stem().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                        let _ = std::fs::write(f, crate::i18n::template(&name));
                    }
                }
            });
            ui.separator();
            ui.checkbox(&mut self.cfg.adaptive_preview, tr("Adjust preview refresh while dragging"));
            ui.label(trf!("Last preview render: {} ms", format!("{:.1}", self.render_ms)));
            crate::help(ui, tr("On: the preview is redrawn at the pace your machine can sustain (the frame always follows the mouse without delay). Off: the preview is redrawn on every frame, which is smoother on a fast machine but may slow down on a busy page."));
            ui.separator();
            ui.horizontal(|ui| {
                ui.checkbox(&mut self.cfg.autosave, tr("Automatic project backup every"));
                ui.add_enabled(self.cfg.autosave, egui::DragValue::new(&mut self.cfg.autosave_min).range(1..=60).suffix(" min"));
            });
            crate::help(ui, tr("Written to a file next to the project (name.autosave.json), only if the project has changed; the project itself is only modified by “Save”. Offered for restoration after an unexpected stop."));
            ui.separator();
            ui.label(tr("ffmpeg path (folder containing ffmpeg and ffprobe, or ffmpeg itself):"));
            ui.horizontal(|ui| {
                ui.add(egui::TextEdit::singleline(&mut self.cfg.ffmpeg_path).hint_text(tr("(automatic: next to the program, then PATH)")).desired_width(320.0));
                if ui.button(tr("Folder…")).clicked() {
                    if let Some(d) = rfd::FileDialog::new().set_title(tr("ffmpeg folder")).pick_folder() { self.cfg.ffmpeg_path = clean(&d); }
                }
                if ui.button(tr("File…")).clicked() {
                    if let Some(f) = rfd::FileDialog::new().set_title("ffmpeg").pick_file() { self.cfg.ffmpeg_path = clean(&f); }
                }
                if !self.cfg.ffmpeg_path.is_empty() && ui.button(tr("Auto")).clicked() { self.cfg.ffmpeg_path.clear(); }
            });
            // la detection lance ffmpeg : refaite seulement quand le chemin change (resultat gardé en memoire d'interface)
            let key = egui::Id::new("ffmpeg_status");
            let cached: Option<(String, Option<String>)> = ui.data(|d| d.get_temp(key));
            let found = match cached {
                Some((p, r)) if p == self.cfg.ffmpeg_path => r,
                _ => {
                    Tools::set_custom_path(&self.cfg.ffmpeg_path);
                    let r = Tools::locate().ok().map(|t| t.ffmpeg.display().to_string());
                    ui.data_mut(|d| d.insert_temp(key, (self.cfg.ffmpeg_path.clone(), r.clone())));
                    r
                }
            };
            match found {
                Some(p) => { ui.label(egui::RichText::new(trf!("Used: {}", p)).weak()); }
                None => { ui.colored_label(egui::Color32::from_rgb(220, 90, 90), tr("ffmpeg/ffprobe not found with this setting.")); }
            }
            ui.separator();
            if ui.checkbox(&mut self.model.cache, tr("Reuse already-encoded videos (cache)")).changed() { self.dirty = true; }
            ui.label(egui::RichText::new(tr("Setting saved with the project.")).weak());
            if self.cfg.lang != before.lang {
                crate::i18n::set_language(&self.cfg.lang);
                // les menus et listes deroulantes gardent la taille de leur premiere ouverture : oubliee pour s'adapter aux nouveaux textes
                ui.ctx().memory_mut(|m| m.reset_areas());
            }
            if self.cfg != before { Tools::set_custom_path(&self.cfg.ffmpeg_path); self.cfg.save(); }
        }); }
        self.show_config = open;

        // fenetre du projet (Edition > Projet) : onglets General (disque, encodage), Style des boutons, Langues
        if self.show_project {
            let mut open = true;
            let mut changed = false;
            let mut style_changed = false;
            let mut pick_sound: Option<u8> = None;
            let mut pick_iso = false;
            let (bgv, mus) = (self.model.bg_video.clone(), self.model.music.clone());
            let bg_note = self.bg_note(&bgv, &mus);
            let tab = &mut self.project_tab;
            let m = &mut self.model;
            os_window(ctx, "projet", tr("Project"), [720.0, 600.0], &mut open, &mut |ui| {
                ui.horizontal(|ui| {
                    ui.selectable_value(tab, 0, tr("General"));
                    ui.selectable_value(tab, 1, tr("Button style"));
                    ui.selectable_value(tab, 4, tr("Page style"));
                    ui.selectable_value(tab, 2, tr("Subtitle languages"));
                    ui.selectable_value(tab, 3, tr("ISO generation"));
                });
                ui.separator();
                ui.add_enabled_ui(!busy, |ui| match *tab {
                    1 => {
                        egui::ScrollArea::vertical().max_height(560.0).show(ui, |ui| {
                            style_changed = style_ui::style_ui(ui, &mut m.designs.style, "projet", &mut pick_sound);
                        });
                        crate::help(ui, tr("Applies to all buttons of the project, except those with their own appearance (inspector)."));
                    }
                    4 => {
                        egui::ScrollArea::vertical().max_height(560.0).show(ui, |ui| {
                            ui.heading(tr("Menu background and music"));
                            ui.horizontal(|ui| {
                                ui.label(tr("Background image:"));
                                changed |= ui.add(egui::TextEdit::singleline(&mut m.bg).hint_text(tr("(automatically generated background)"))).changed();
                                if ui.button(tr("Choose…")).clicked() {
                                    if let Some(f) = rfd::FileDialog::new().add_filter(tr("Images"), &["png", "jpg", "jpeg", "bmp", "svg", "svgz"]).pick_file() {
                                        m.bg = clean(&f);
                                        changed = true;
                                    }
                                }
                                if ui.button(tr("No image")).clicked() { m.bg.clear(); changed = true; }
                            });
                            ui.horizontal(|ui| {
                                ui.label(tr("Background duration:"));
                                changed |= ui.add(egui::DragValue::new(&mut m.bg_seconds).speed(0.5).range(1.0..=600.0).suffix(" s")).changed();
                            });
                            changed |= file_row(ui, tr("Background video (animated):"), &mut m.bg_video, tr("(none)"), tr("Videos"), VIDEO_EXTS);
                            if !m.bg_video.is_empty() {
                                changed |= ui.checkbox(&mut m.bg_video_audio, tr("Keep the background video's sound (if there is no music)")).changed();
                            }
                            changed |= file_row(ui, tr("Menu music:"), &mut m.music, tr("(none)"), tr("Audio"), &["mp3", "wav", "flac", "m4a", "aac", "ogg", "opus", "ac3", "wma"]);
                            if !m.bg_video.is_empty() || !m.music.is_empty() {
                                crate::help(ui, tr("The background loops: it lasts as long as the background video or, with no video, the music. Texts and images of custom pages are placed over the video."));
                                if let Some(n) = &bg_note { ui.label(egui::RichText::new(n.as_str()).weak()); }
                            }
                            changed |= ui.checkbox(&mut m.loop_menu, tr("Loop the background (otherwise: stop on the last frame)")).changed();
                            crate::help(ui, tr("Project defaults: each Layout page can have its own background, music and animations."));
                        });
                    }
                    2 => {
                        egui::ScrollArea::vertical().max_height(560.0).show(ui, |ui| {
                            changed |= subs_ui::languages_ui(ui, m);
                        });
                    }
                    3 => {
                        egui::ScrollArea::vertical().max_height(560.0).show(ui, |ui| {
                            let title = m.title.clone();
                            changed |= iso_ui::iso_ui(ui, &mut m.iso, &title, &mut pick_iso);
                        });
                    }
                    _ => {
                        ui.heading(tr("Disc"));
                        ui.horizontal(|ui| {
                            ui.label(tr("Disc title:"));
                            changed |= ui.text_edit_singleline(&mut m.title).changed();
                        });
                        ui.horizontal(|ui| {
                            ui.label(tr("Default audio language (3 letters):")).on_hover_text(tr("Language of the audio track of videos whose audio tracks are not chosen"));
                            changed |= ui.add(egui::TextEdit::singleline(&mut m.lang).desired_width(50.0)).changed();
                        });
                        changed |= file_row(ui, tr("Opening sequence:"), &mut m.intro, tr("(none: the menu opens directly)"), tr("Videos"), VIDEO_EXTS);
                        changed |= ui.checkbox(&mut m.menu_1080, tr("Menu in 1920x1080 (otherwise: video size)")).changed();
                        ui.separator();
                        ui.heading(tr("Encoding"));
                        ui.horizontal(|ui| {
                            ui.label(tr("x264 quality (CRF, lower = better):"));
                            changed |= ui.add(egui::Slider::new(&mut m.crf, 0..=51)).changed();
                        });
                        egui::ComboBox::from_label(tr("Encoding preset (slower = more compact)"))
                            .selected_text(m.preset.clone())
                            .show_ui(ui, |ui| {
                                for p in PRESETS {
                                    changed |= ui.selectable_value(&mut m.preset, p.to_string(), p).changed();
                                }
                            });
                    }
                });
            });
            if let Some(k) = pick_sound {
                if let Some(f) = rfd::FileDialog::new().add_filter(tr("Sounds"), &style_ui::SOUND_EXTS).pick_file() {
                    let rel = self.relative(&f);
                    let st = &mut self.model.designs.style;
                    if k == 1 { st.sound_select = rel; } else { st.sound_activate = rel; }
                    style_changed = true;
                }
            }
            if pick_iso {
                let name = format!("{}.iso", self.model.title.chars().map(|c| if r#"\/:*?"<>|"#.contains(c) { '_' } else { c }).collect::<String>().trim());
                if let Some(f) = rfd::FileDialog::new().set_title(tr("Disc ISO image")).add_filter(tr("ISO image"), &["iso"]).set_file_name(name).save_file() {
                    self.model.iso.file = self.relative(&f);
                    changed = true;
                }
            }
            // style : l'apercu de la mise en page est redessine
            if style_changed { self.bump(); }
            if changed { self.dirty = true; }
            self.show_project = open;
        }

        egui::TopBottomPanel::top("barre").show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.menu_button(tr("File"), |ui| {
                    ui.add_enabled_ui(!busy, |ui| {
                        if ui.button(tr("New project")).clicked() { self.new_project(); ui.close_menu(); }
                        if ui.button(tr("Open project…")).clicked() { ui.close_menu(); self.open(); }
                        if ui.button(tr("Save")).clicked() { ui.close_menu(); self.save(false); }
                        if ui.button(tr("Save as…")).clicked() { ui.close_menu(); self.save(true); }
                    });
                    ui.separator();
                    if ui.button(tr("Settings…")).clicked() { self.show_config = true; ui.close_menu(); }
                    ui.separator();
                    if ui.button(tr("Quit")).clicked() { ui.close_menu(); ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close); }
                });
                // annuler / retablir : modifications de la mise en page
                ui.menu_button(tr("Edit"), |ui| {
                    let edit = !busy && self.mode == Mode::MiseEnPage;
                    if ui.add_enabled(edit && self.history.can_undo(), egui::Button::new(tr("↶ Undo")).shortcut_text("Ctrl+Z")).clicked() { self.undo_action(); ui.close_menu(); }
                    if ui.add_enabled(edit && self.history.can_redo(), egui::Button::new(tr("↷ Redo")).shortcut_text("Ctrl+Y")).clicked() { self.redo_action(); ui.close_menu(); }
                    if self.mode != Mode::MiseEnPage {
                        ui.label(egui::RichText::new(tr("Applies to the “Layout” tab")).weak());
                    }
                    ui.separator();
                    if ui.button(tr("Project…")).on_hover_text(tr("Disc and encoding, button style, languages, ISO generation")).clicked() {
                        self.show_project = true;
                        ui.close_menu();
                    }
                });
                ui.menu_button(tr("About"), |ui| {
                    if ui.button(tr("About bdauthor…")).clicked() { self.show_about = true; ui.close_menu(); }
                });
                ui.separator();
                ui.selectable_value(&mut self.mode, Mode::Projet, tr("Videos"));
                ui.selectable_value(&mut self.mode, Mode::MiseEnPage, tr("Layout"));
                ui.selectable_value(&mut self.mode, Mode::Jaquette, tr("Cover"));
                ui.separator();
                let name = self.path.as_ref().map(|p| clean(p)).unwrap_or_else(|| tr("(project not saved)").into());
                ui.label(format!("{}{}", name, if self.dirty { " *" } else { "" }));
                if let (true, Some(t)) = (self.dirty, self.autosave_ok) {
                    ui.label(egui::RichText::new(trf!("(auto-save {} min ago)", t.elapsed().as_secs() / 60)).weak());
                }
            });
            if !self.notice.is_empty() {
                ui.label(egui::RichText::new(&self.notice).weak());
            }
        });

        // zone de construction : masquee dans l'onglet Jaquette (sauf pendant une construction, pour garder la progression et « Annuler »)
        if self.mode != Mode::Jaquette || busy { egui::TopBottomPanel::bottom("construction").default_height(160.0).min_height(60.0).resizable(true).show(ctx, |ui| {
            ui.horizontal(|ui| {
                if busy {
                    if ui.button(tr("Cancel")).clicked() { self.cancel.store(true, Ordering::Relaxed); }
                    ui.add(egui::ProgressBar::new(self.progress).show_percentage().desired_width(300.0));
                } else if ui.add_enabled(!self.model.videos.is_empty(), egui::Button::new(tr("Build the disc…"))).clicked() {
                    self.start_build(ctx);
                }
            });
            if let Some((ok, text)) = &self.result {
                let col = if *ok { egui::Color32::from_rgb(90, 200, 120) } else { egui::Color32::from_rgb(230, 100, 90) };
                ui.colored_label(col, text);
            }
            egui::ScrollArea::vertical().stick_to_bottom(true).auto_shrink([false, false]).show(ui, |ui| {
                for l in &self.log {
                    ui.monospace(l);
                }
            });
        }); }

        if self.mode == Mode::Jaquette {
            self.cover_screen(ctx, busy);
            return;
        }
        let layout_mode = self.mode == Mode::MiseEnPage;
        if layout_mode {
            // panneau de gauche : arborescence en haut, details de l'element selectionne en dessous (separation reglable)
            let h = ctx.screen_rect().height();
            egui::SidePanel::left("pages").resizable(true).default_width(400.0).show(ctx, |ui| {
                egui::TopBottomPanel::bottom("inspecteur").resizable(true).default_height(h * 0.45).min_height(120.0).show_inside(ui, |ui| {
                    egui::ScrollArea::vertical().id_salt("defil_inspecteur").auto_shrink([false, false]).show(ui, |ui| {
                        ui.add_enabled_ui(!busy, |ui| self.inspector_ui(ui));
                    });
                });
                self.page_list_ui(ui);
            });
            egui::CentralPanel::default().show(ctx, |ui| {
                ui.add_enabled_ui(!busy, |ui| self.layout_central(ui, ctx));
            });
            return;
        }
        egui::SidePanel::left("videos").resizable(true).default_width(320.0).show(ctx, |ui| {
            ui.heading(tr("Videos"));
            ui.add_enabled_ui(!busy, |ui| {
                if ui.button(tr("Add videos…")).clicked() { self.add_videos(); }
            });
            ui.separator();
            // (rang, Some(destination)) : deplacement par glisser-deposer ; (rang, None) : retrait
            let mut op: Option<(usize, Option<usize>)> = None;
            let mut reload: Option<String> = None;
            let mut ask: Option<usize> = None;
            let mut rows: Vec<egui::Rect> = Vec::new();
            let list = egui::ScrollArea::vertical().id_salt("liste_videos").auto_shrink([false, false]).show(ui, |ui| {
                for (i, v) in self.model.videos.iter().enumerate() {
                    let label = if v.title.is_empty() { Path::new(&v.file).file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default() } else { v.title.clone() };
                    let r = ui.horizontal(|ui| {
                        if busy {
                            ui.label(egui::RichText::new("☰").weak());
                        } else {
                            let _ = ui.dnd_drag_source(egui::Id::new(("video", i)), VideoDrag(i), |ui| ui.label(egui::RichText::new("☰").strong()))
                                .response.on_hover_text(tr("Drag to change the order of the videos"));
                        }
                        // les boutons gardent leur place : le nom est raccourci pour tenir dans la largeur du panneau
                        let max_chars = ((ui.available_width() - 62.0).max(60.0) / 7.0) as usize;
                        let full = match self.durations.get(&v.file) {
                            Some(d) => format!("{:>2}. {} ({})", i + 1, label, duration_text(*d)),
                            None if self.probe_failed.contains(&v.file) => trf!("{}. {} (unknown)", format!("{:>2}", i + 1), label),
                            None => format!("{:>2}. {}", i + 1, label),
                        };
                        let shown = if full.chars().count() > max_chars { format!("{}…", full.chars().take(max_chars.saturating_sub(1)).collect::<String>()) } else { full.clone() };
                        if ui.selectable_label(self.sel == Some(i), shown).on_hover_text(format!("{full}\n{}", v.file)).clicked() { self.sel = Some(i); }
                        ui.add_enabled_ui(!busy, |ui| {
                            if delete_button(ui, tr("Remove this video")) { ask = Some(i); }
                            if ui.small_button("↻").on_hover_text(tr("Reload this video (re-read the duration, tracks and thumbnails)")).clicked() { reload = Some(v.file.clone()); }
                        });
                    }).response;
                    rows.push(r.rect);
                }
            });
            if let Some(i) = ask { self.confirm_remove = Some(i); }
            if let Some(f) = reload {
                self.durations.remove(&f);
                self.probe_failed.remove(&f);
                self.video_sizes.remove(&f);
                self.sub_probes.remove(&f);
                self.audio_probes.remove(&f);
                self.thumbs.retain(|(k, _), _| *k != f);
                self.frames.retain(|(k, _), _| *k != f);
                let pp = format!("P|{f}");
                let tp = format!("T|{f}|");
                let fp = format!("F|{f}|");
                self.requested.retain(|k| *k != pp && !k.starts_with(&tp) && !k.starts_with(&fp));
                self.edit_gen += 1;
            }
            // glisser-deposer (meme mecanique que la liste des pages) : une ligne bleue montre la place d'arrivee
            let area = list.inner_rect;
            if let (Some(drag), Some(pos)) = (egui::DragAndDrop::payload::<VideoDrag>(ctx), ctx.pointer_hover_pos()) {
                if area.expand(8.0).contains(pos) && !rows.is_empty() {
                    let r = rows.iter().filter(|rect| rect.center().y < pos.y).count();
                    let y = if r < rows.len() { rows[r].top() } else { rows[rows.len() - 1].bottom() };
                    ui.painter().hline(area.x_range(), y, egui::Stroke::new(2.5_f32, egui::Color32::from_rgb(90, 170, 255)));
                    if ui.input(|i| i.pointer.any_released()) {
                        egui::DragAndDrop::clear_payload(ctx);
                        let from = drag.0;
                        let to = if r > from { r - 1 } else { r };
                        if to != from { op = Some((from, Some(to))); }
                    }
                }
            }
            // confirmation avant de retirer une video
            if let Some(i) = self.confirm_remove {
                match self.model.videos.get(i) {
                    None => self.confirm_remove = None,
                    Some(v) => {
                        let name = if v.title.is_empty() { Path::new(&v.file).file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default() } else { v.title.clone() };
                        let (n_ch, n_sub, n_aud) = (v.chapters.len(), v.subs.len(), v.audio.len());
                        let mut answer: Option<bool> = None;
                        egui::Window::new(tr("Remove the video?")).collapsible(false).resizable(false).anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0]).show(ctx, |ui| {
                            ui.label(trf!("“{}” will be removed from the project.", name));
                            if n_ch + n_sub + n_aud > 0 {
                                ui.label(trf!("Its settings will be lost: {} chapter(s), {} subtitle track(s), {} audio track(s).", n_ch, n_sub, n_aud));
                            }
                            ui.label(egui::RichText::new(tr("The video file is not deleted from disk.")).weak());
                            ui.add_space(6.0);
                            ui.horizontal(|ui| {
                                if ui.button(tr("Remove")).clicked() { answer = Some(true); }
                                if ui.button(tr("Cancel")).clicked() { answer = Some(false); }
                            });
                        });
                        if ctx.input(|i| i.key_pressed(egui::Key::Escape)) { answer = Some(false); }
                        match answer {
                            Some(true) => { op = Some((i, None)); self.confirm_remove = None; }
                            Some(false) => self.confirm_remove = None,
                            None => {}
                        }
                    }
                }
            }
            if let Some((i, dest)) = op {
                match dest {
                    Some(to) => {
                        // echanges successifs : les vignettes et liens des pages suivent leur video
                        if to > i { for k in i..to { self.model.swap_videos(k, k + 1); } } else { for k in (to..i).rev() { self.model.swap_videos(k, k + 1); } }
                        // la selection suit la video deplacee (ou celle decalee par le deplacement)
                        self.sel = self.sel.map(|s| if s == i { to } else if i < s && s <= to { s - 1 } else if to <= s && s < i { s + 1 } else { s });
                    }
                    None => {
                        self.model.remove_video(i);
                        self.sel = if self.model.videos.is_empty() { None } else { Some(i.min(self.model.videos.len() - 1)) };
                    }
                }
                self.dirty = true;
                self.reset_editor();
            }
        });

        egui::CentralPanel::default().show(ctx, |ui| {
            egui::ScrollArea::vertical().show(ui, |ui| {
                ui.add_enabled_ui(!busy, |ui| {
                    let mut changed = false;
                    let sub_base = self.path.as_ref().and_then(|p| p.parent()).map(Path::to_path_buf).unwrap_or_default();
                    // vignettes liees a chaque chapitre de la video selectionnee : (identifiant, nombre)
                    let chapter_uses: Vec<(u32, usize)> = self.sel.and_then(|i| self.model.videos.get(i).map(|v| v.chapters.iter().map(|c| (c.id, self.model.designs.chapter_uses(i, c.id))).collect::<Vec<_>>())).unwrap_or_default();
                    // duree de la video selectionnee (si connue) : borne des instants de chapitre et de vignette
                    let max_t: f64 = self.sel.and_then(|i| self.model.videos.get(i)).and_then(|v| self.durations.get(&v.file).copied()).filter(|d| *d > 0.0).unwrap_or(1.0e6);
                    if let Some(v) = self.sel.and_then(|i| self.model.videos.get_mut(i)) {
                        ui.heading(tr("Selected video"));
                        ui.label(egui::RichText::new(&v.file).weak());
                        ui.horizontal(|ui| {
                            ui.label(tr("Title in the menu:"));
                            changed |= ui.text_edit_singleline(&mut v.title).changed();
                        });
                        ui.horizontal(|ui| {
                            let mut on = v.thumb_s.is_some();
                            if ui.checkbox(&mut on, tr("Alternative thumbnail at time:")).on_hover_text(tr("Unchecked: image taken automatically. Checked (or by changing the value): image taken at this moment of the video.")).changed() {
                                v.thumb_s = if on { Some(v.thumb_s.unwrap_or(1.0)) } else { None };
                                changed = true;
                            }
                            // la zone de saisie est toujours visible ; la modifier active l'option
                            let mut t = v.thumb_s.unwrap_or(1.0);
                            if ui.add(egui::DragValue::new(&mut t).speed(0.5).range(0.0..=max_t).suffix(" s")).changed() {
                                v.thumb_s = Some(t);
                                changed = true;
                            }
                        });
                        ui.separator();
                        ui.label(tr("Chapter marks (next / previous keys of the remote control, chapter thumbnails, start pages):"));
                        if v.auto_s.is_some() {
                            // ancienne option (repères automatiques) : plus proposée, mais un projet existant peut encore l'utiliser
                            ui.horizontal(|ui| {
                                ui.colored_label(egui::Color32::from_rgb(230, 170, 60), tr("This project uses old automatic marks."));
                                if ui.small_button(tr("Remove them")).clicked() { v.auto_s = None; changed = true; }
                            });
                        }
                        crate::help(ui, tr("A chapter thumbnail (page) can be linked to one of them, or create one."));
                        let mut del = None;
                        for (k, c) in v.chapters.iter_mut().enumerate() {
                            ui.horizontal(|ui| {
                                changed |= ui.add(egui::DragValue::new(&mut c.t).speed(1.0).range(0.0..=max_t).suffix(" s")).changed();
                                if max_t < 1.0e6 { ui.label(egui::RichText::new(format!("/ {}", duration_text(max_t))).weak()).on_hover_text(tr("Total duration of the video")); }
                                changed |= ui.add(egui::TextEdit::singleline(&mut c.title).hint_text(tr("name (optional)"))).changed();
                                if let Some(n) = chapter_uses.iter().find(|u| u.0 == c.id).map(|u| u.1).filter(|n| *n > 0) {
                                    ui.label(egui::RichText::new(trf!("{} thumbnail{}", n, if n > 1 { "s" } else { "" })).weak()).on_hover_text(tr("Page thumbnails linked to this chapter: they follow its time. If you remove the chapter, they keep their time."));
                                }
                                if ui.small_button(tr("Remove")).clicked() { del = Some(k); }
                            });
                        }
                        if let Some(k) = del { v.chapters.remove(k); changed = true; }
                        if ui.button(tr("Add a chapter")).clicked() {
                            let t = v.chapters.iter().map(|c| c.t).fold(f64::NEG_INFINITY, f64::max);
                            let t = if t.is_finite() { (t + 60.0).min(max_t) } else { 0.0 };
                            let id = v.next_chapter_id();
                            v.chapters.push(ChapterEntry { id, t, title: String::new() });
                            changed = true;
                        }
                        if ui.button(tr("Chapter editor…")).on_hover_text(tr("Opens a window with the video image by image, playback and a timeline, to place the chapters and choose the thumbnail images.")).clicked() { self.chapter_request = self.sel; }
                        ui.separator();
                        changed |= audio_ui::panel(ui, v, &mut self.model.languages, &mut self.audio_probes, &sub_base);
                        ui.separator();
                        let mut want_preview = None;
                        // cadence de la video (sondee) : sert de cadence par defaut aux sous-titres a temps en images
                        let auto_fps = self.video_sizes.get(&v.file).and_then(|x| bdauthor_core::subtitle::plausible_fps(x.1));
                        changed |= subs_ui::panel(ui, v, &mut self.model.languages, &mut self.sub_probes, &sub_base, &mut want_preview, auto_fps);
                        if let (Some(k), Some(vi)) = (want_preview, self.sel) {
                            let file = if Path::new(&v.file).is_absolute() { PathBuf::from(&v.file) } else { sub_base.join(&v.file) };
                            if let Some(t) = v.subs.get(k) { self.sub_preview = Some(subs_ui::SubPreview::new(vi, k, t, &file, &sub_base, auto_fps)); }
                        }
                        ui.separator();
                    } else {
                        ui.label(tr("Add videos then select one to set its title, thumbnail and chapters."));
                        ui.separator();
                    }

                    // fond, musique et ouverture des menus : fenetre Edition > Projet (onglets « Style des pages » et « Général »)
                    // « vignettes par page generee » retire de l'interface : seules les pages personnalisees sont utilisees (la valeur
                    // eventuelle du fichier reste lue pour la premiere conversion d'un ancien projet)
                    if changed {
                        self.dirty = true;
                    }
                });
            });
        });
    }
}

const VIDEO_EXTS: &[&str] = &["mp4", "mkv", "mov", "avi", "m2ts", "mts", "ts", "webm", "wmv", "flv", "m4v", "mpg", "mpeg"];

/// Ligne « libelle : [chemin] Choisir… Aucun » ; retourne vrai si le chemin a change.
fn file_row(ui: &mut egui::Ui, label: &str, path: &mut String, hint: &str, filter: &str, exts: &[&str]) -> bool {
    let mut changed = false;
    ui.horizontal(|ui| {
        ui.label(label);
        changed |= ui.add(egui::TextEdit::singleline(path).hint_text(hint)).changed();
        if ui.button(tr("Choose…")).clicked() {
            if let Some(f) = rfd::FileDialog::new().add_filter(filter, exts).pick_file() {
                *path = clean(&f);
                changed = true;
            }
        }
        if !path.is_empty() && ui.button(tr("None")).clicked() {
            path.clear();
            changed = true;
        }
    });
    changed
}

fn main() -> eframe::Result<()> {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default().with_inner_size([1150.0, 760.0]).with_title(tr("bdauthor — Blu-ray discs")),
        ..Default::default()
    };
    eframe::run_native("bdauthor", options, Box::new(|_cc| Ok(Box::new(App::default()))))
}
