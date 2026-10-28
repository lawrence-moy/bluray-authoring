//! bdauthor : creation de disques Blu-ray (menus HDMV) a partir de vos videos. Interface egui/eframe.
#![cfg_attr(all(not(debug_assertions), windows), windows_subsystem = "windows")]

mod editor_ui;
mod subs_ui;
mod style_ui;
mod sim_ui;

use bdauthor_core::check::Kind;
use bdauthor_core::editor::{History, PageRef};
use bdauthor_core::palette::RgbaImage;
use bdauthor_core::ffmpeg::Tools;
use bdauthor_core::model::{ChapterEntry, ProjectModel, VideoEntry, PRESETS};
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
#[derive(Clone, Copy, PartialEq)]
struct Config {
    /// Pendant un glissement, espace les rendus de l'apercu selon la duree du dernier rendu (sinon : a chaque image).
    adaptive_preview: bool,
}

impl Config {
    fn path() -> Option<PathBuf> {
        let base = std::env::var_os("APPDATA").map(PathBuf::from).or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))?;
        Some(base.join("bdauthor").join("config.txt"))
    }
    fn load() -> Config {
        let mut c = Config { adaptive_preview: true };
        if let Some(t) = Config::path().and_then(|p| std::fs::read_to_string(p).ok()) {
            for l in t.lines() {
                if let Some(("adaptive_preview", v)) = l.split_once('=') {
                    c.adaptive_preview = v.trim() != "false";
                }
            }
        }
        c
    }
    fn save(&self) {
        if let Some(p) = Config::path() {
            if let Some(d) = p.parent() {
                let _ = std::fs::create_dir_all(d);
            }
            let _ = std::fs::write(p, format!("adaptive_preview={}\n", self.adaptive_preview));
        }
    }
}

#[derive(PartialEq, Clone, Copy)]
enum Mode {
    Projet,
    MiseEnPage,
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
    /// fenetre « Style des boutons »
    show_style: bool,
    /// fenetre « Fond de cette page »
    show_page_bg: bool,
    /// element montre selectionne dans l'apercu courant
    preview_sel: Option<usize>,
    /// etat « active » montre dans l'apercu courant
    preview_act: bool,
    /// simulation de la telecommande en cours
    sim: Option<sim_ui::Sim>,
    sel_item: Option<usize>,
    drag: Option<editor_ui::DragState>,
    snap: bool,
    edit_gen: u64,
    preview_gen: u64,
    preview_tex: Option<egui::TextureHandle>,
    base_cache: Option<(PathBuf, RgbaImage)>,
    durations: HashMap<String, f64>,
    /// (hauteur, cadence) de chaque video sondee
    video_sizes: HashMap<String, (u32, f64)>,
    sub_probes: subs_ui::Probes,
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
            mode: Mode::Projet, cur: None, history: History::default(), t0: std::time::Instant::now(), last_preview: std::time::Instant::now(), render_ms: 0.0, cfg: Config::load(), show_config: false, show_style: false, show_page_bg: false, preview_sel: None, preview_act: false, sim: None, sel_item: None, drag: None, snap: true, edit_gen: 1, preview_gen: 0, preview_tex: None, base_cache: None,
            durations: HashMap::new(), video_sizes: HashMap::new(), sub_probes: HashMap::new(), thumbs: HashMap::new(), requested: HashSet::new(), worker: None, renderer: FontRenderer::new(),
            model: ProjectModel::default(), path: None, dirty: false, sel: None, log: vec![], progress: 0.0, rx: None, cancel: Arc::new(AtomicBool::new(false)), result: None, notice: String::new() }
    }
}

fn clean(p: &Path) -> String {
    p.to_string_lossy().replace('\\', "/")
}

fn mmss(t: f64) -> String {
    let s = t.round().max(0.0) as u64;
    format!("{}:{:02}", s / 60, s % 60)
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
        self.drag = None;
        self.edit_gen += 1;
        self.base_cache = None;
        self.history.reset(&self.model.designs);
    }

    fn open(&mut self) {
        let Some(p) = rfd::FileDialog::new().add_filter("Projet bdauthor", &["json"]).pick_file() else { return };
        match std::fs::read_to_string(&p).map_err(|e| e.to_string()).and_then(|t| ProjectModel::from_json(&t)) {
            Ok(m) => {
                self.model = m;
                self.path = Some(p);
                self.dirty = false;
                self.sel = if self.model.videos.is_empty() { None } else { Some(0) };
                self.reset_editor();
                self.notice = "Projet ouvert.".into();
            }
            Err(e) => self.notice = format!("Impossible d'ouvrir le projet : {e}"),
        }
    }

    fn save(&mut self, as_new: bool) -> bool {
        let target = if as_new || self.path.is_none() {
            match rfd::FileDialog::new().add_filter("Projet bdauthor", &["json"]).set_file_name("projet.json").save_file() {
                Some(p) => p,
                None => return false,
            }
        } else {
            self.path.clone().unwrap()
        };
        match std::fs::write(&target, self.model.to_json()) {
            Ok(()) => {
                self.path = Some(target);
                self.dirty = false;
                self.notice = "Projet enregistré.".into();
                true
            }
            Err(e) => {
                self.notice = format!("Enregistrement impossible : {e}");
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
            self.result = Some((false, format!("Corrigez d'abord :\n- {}", problems.join("\n- "))));
            return;
        }
        let Some(out) = rfd::FileDialog::new().set_title("Dossier de sortie du disque").pick_folder() else { return };
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
                let info = build_project(&project, &out, &mut env).map_err(|e| e.to_string())?;
                let mut s = format!(
                    "{} vidéo(s), {} page(s) de menu, format {}x{} à {} i/s, {:.1} Mo de flux.\nDisque écrit dans {}\n",
                    info.videos, info.main_pages, info.format.0, info.format.1, info.format.2, info.total_stream_bytes as f64 / 1e6, out.display()
                );
                for (k, m) in &info.report.items {
                    match k {
                        Kind::Error => s += &format!("ERREUR : {m}\n"),
                        Kind::Warn => s += &format!("Avertissement : {m}\n"),
                        Kind::Info => {}
                    }
                }
                s += if info.report.ok() { "Vérification : aucune erreur." } else { "Vérification : ERREURS détectées." };
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
                            Err(e) => (false, if e.contains("annulee") { "Construction annulée.".to_string() } else { format!("Échec : {e}") }),
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
            .add_filter("Vidéos", &["mp4", "mkv", "mov", "avi", "m2ts", "mts", "ts", "webm", "wmv", "flv", "m4v", "mpg", "mpeg"])
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
        if self.running() {
            ctx.request_repaint_after(std::time::Duration::from_millis(200));
        }
        let busy = self.running();
        let mut open = self.show_config;
        egui::Window::new("Configuration").open(&mut open).collapsible(false).resizable(false).show(ctx, |ui| {
            let before = self.cfg;
            ui.checkbox(&mut self.cfg.adaptive_preview, "Ajuster le rafraîchissement de l'aperçu pendant le déplacement");
            ui.label(format!("Dernier rendu de l'aperçu : {:.1} ms", self.render_ms));
            ui.label(egui::RichText::new("Activé : l'aperçu est redessiné au rythme que votre machine peut tenir (le cadre suit toujours la souris sans retard). Désactivé : l'aperçu est redessiné à chaque image, ce qui est plus fluide sur une machine rapide mais peut ralentir sur une page chargée.").weak());
            if self.cfg != before { self.cfg.save(); }
        });
        self.show_config = open;

        egui::TopBottomPanel::top("barre").show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.menu_button("Fichier", |ui| {
                    ui.add_enabled_ui(!busy, |ui| {
                        if ui.button("Nouveau").clicked() { self.new_project(); ui.close_menu(); }
                        if ui.button("Ouvrir…").clicked() { ui.close_menu(); self.open(); }
                        if ui.button("Enregistrer").clicked() { ui.close_menu(); self.save(false); }
                        if ui.button("Enregistrer sous…").clicked() { ui.close_menu(); self.save(true); }
                    });
                    ui.separator();
                    if ui.button("Configuration…").clicked() { self.show_config = true; ui.close_menu(); }
                });
                ui.separator();
                ui.selectable_value(&mut self.mode, Mode::Projet, "Projet");
                ui.selectable_value(&mut self.mode, Mode::MiseEnPage, "Mise en page");
                ui.separator();
                let name = self.path.as_ref().map(|p| clean(p)).unwrap_or_else(|| "(projet non enregistré)".into());
                ui.label(format!("{}{}", name, if self.dirty { " *" } else { "" }));
            });
            if !self.notice.is_empty() {
                ui.label(egui::RichText::new(&self.notice).weak());
            }
        });

        egui::TopBottomPanel::bottom("construction").min_height(160.0).resizable(true).show(ctx, |ui| {
            ui.horizontal(|ui| {
                if busy {
                    if ui.button("Annuler").clicked() { self.cancel.store(true, Ordering::Relaxed); }
                    ui.add(egui::ProgressBar::new(self.progress).show_percentage().desired_width(300.0));
                } else if ui.add_enabled(!self.model.videos.is_empty(), egui::Button::new("Construire le disque…")).clicked() {
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
        });

        let layout_mode = self.mode == Mode::MiseEnPage;
        if layout_mode {
            egui::SidePanel::left("pages").resizable(true).default_width(300.0).show(ctx, |ui| self.page_list_ui(ui));
            egui::SidePanel::right("inspecteur").resizable(true).default_width(300.0).show(ctx, |ui| {
                egui::ScrollArea::vertical().show(ui, |ui| {
                    ui.add_enabled_ui(!busy, |ui| self.inspector_ui(ui));
                });
            });
            egui::CentralPanel::default().show(ctx, |ui| {
                ui.add_enabled_ui(!busy, |ui| self.layout_central(ui, ctx));
            });
            return;
        }
        egui::SidePanel::left("videos").resizable(true).default_width(320.0).show(ctx, |ui| {
            ui.heading("Vidéos");
            ui.add_enabled_ui(!busy, |ui| {
                if ui.button("Ajouter des vidéos…").clicked() { self.add_videos(); }
            });
            ui.separator();
            let mut action: Option<(usize, i32)> = None; // (index, -1 haut / 1 bas / 0 retirer)
            egui::ScrollArea::vertical().show(ui, |ui| {
                for (i, v) in self.model.videos.iter().enumerate() {
                    let label = if v.title.is_empty() { Path::new(&v.file).file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default() } else { v.title.clone() };
                    ui.horizontal(|ui| {
                        if ui.selectable_label(self.sel == Some(i), format!("{:>2}. {}", i + 1, label)).clicked() { self.sel = Some(i); }
                        ui.add_enabled_ui(!busy, |ui| {
                            if ui.small_button("Monter").clicked() { action = Some((i, -1)); }
                            if ui.small_button("Descendre").clicked() { action = Some((i, 1)); }
                            if ui.small_button("Retirer").clicked() { action = Some((i, 0)); }
                        });
                    });
                }
            });
            if let Some((i, a)) = action {
                let n = self.model.videos.len();
                match a {
                    -1 if i > 0 => { self.model.swap_videos(i, i - 1); self.sel = Some(i - 1); }
                    1 if i + 1 < n => { self.model.swap_videos(i, i + 1); self.sel = Some(i + 1); }
                    0 => {
                        self.model.remove_video(i);
                        self.sel = if self.model.videos.is_empty() { None } else { Some(i.min(self.model.videos.len() - 1)) };
                    }
                    _ => {}
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
                    if let Some(v) = self.sel.and_then(|i| self.model.videos.get_mut(i)) {
                        ui.heading("Vidéo sélectionnée");
                        ui.label(egui::RichText::new(&v.file).weak());
                        ui.horizontal(|ui| {
                            ui.label("Titre dans le menu :");
                            changed |= ui.text_edit_singleline(&mut v.title).changed();
                        });
                        ui.horizontal(|ui| {
                            let mut on = v.thumb_s.is_some();
                            if ui.checkbox(&mut on, "Vignette prise à").changed() {
                                v.thumb_s = if on { Some(1.0) } else { None };
                                changed = true;
                            }
                            if let Some(t) = &mut v.thumb_s {
                                changed |= ui.add(egui::DragValue::new(t).speed(0.5).range(0.0..=1.0e6).suffix(" s")).changed();
                            }
                        });
                        ui.separator();
                        ui.label("Chapitres (au moins 2 pour créer un menu de chapitres) :");
                        ui.horizontal(|ui| {
                            let mut on = v.auto_s.is_some();
                            if ui.checkbox(&mut on, "Automatiques, un toutes les").changed() {
                                v.auto_s = if on { Some(60.0) } else { None };
                                changed = true;
                            }
                            if let Some(a) = &mut v.auto_s {
                                changed |= ui.add(egui::DragValue::new(a).speed(1.0).range(1.0..=1.0e6).suffix(" s")).changed();
                            }
                        });
                        ui.label(egui::RichText::new("Les chapitres manuels, s'il y en a, remplacent les automatiques.").weak());
                        let mut del = None;
                        for (k, c) in v.chapters.iter_mut().enumerate() {
                            ui.horizontal(|ui| {
                                changed |= ui.add(egui::DragValue::new(&mut c.t).speed(1.0).range(0.0..=1.0e6).suffix(" s")).changed();
                                ui.label(mmss(c.t));
                                changed |= ui.add(egui::TextEdit::singleline(&mut c.title).hint_text("nom (facultatif)")).changed();
                                if ui.small_button("Retirer").clicked() { del = Some(k); }
                            });
                        }
                        if let Some(k) = del { v.chapters.remove(k); changed = true; }
                        if ui.button("Ajouter un chapitre").clicked() {
                            let t = v.chapters.last().map_or(0.0, |c| c.t + 60.0);
                            v.chapters.push(ChapterEntry { t, title: String::new() });
                            changed = true;
                        }
                        ui.separator();
                        changed |= subs_ui::panel(ui, v, &mut self.sub_probes, &sub_base);
                        ui.separator();
                    } else {
                        ui.label("Ajoutez des vidéos puis sélectionnez-en une pour régler son titre, sa vignette et ses chapitres.");
                        ui.separator();
                    }

                    ui.heading("Disque");
                    let m = &mut self.model;
                    ui.horizontal(|ui| {
                        ui.label("Titre du disque :");
                        changed |= ui.text_edit_singleline(&mut m.title).changed();
                    });
                    ui.horizontal(|ui| {
                        ui.label("Langue (3 lettres) :");
                        changed |= ui.add(egui::TextEdit::singleline(&mut m.lang).desired_width(50.0)).changed();
                    });
                    ui.horizontal(|ui| {
                        ui.label("Qualité x264 (CRF, plus petit = meilleur) :");
                        changed |= ui.add(egui::Slider::new(&mut m.crf, 0..=51)).changed();
                    });
                    egui::ComboBox::from_label("Préréglage d'encodage (plus lent = plus compact)")
                        .selected_text(m.preset.clone())
                        .show_ui(ui, |ui| {
                            for p in PRESETS {
                                changed |= ui.selectable_value(&mut m.preset, p.to_string(), p).changed();
                            }
                        });
                    ui.separator();
                    ui.heading("Menu");
                    ui.horizontal(|ui| {
                        ui.label("Image de fond :");
                        changed |= ui.add(egui::TextEdit::singleline(&mut m.bg).hint_text("(fond généré automatiquement)")).changed();
                        if ui.button("Choisir…").clicked() {
                            if let Some(f) = rfd::FileDialog::new().add_filter("Images", &["png", "jpg", "jpeg", "bmp", "svg", "svgz"]).pick_file() {
                                m.bg = clean(&f);
                                changed = true;
                            }
                        }
                        if ui.button("Aucune").clicked() { m.bg.clear(); changed = true; }
                    });
                    ui.horizontal(|ui| {
                        ui.label("Durée du fond :");
                        changed |= ui.add(egui::DragValue::new(&mut m.bg_seconds).speed(0.5).range(1.0..=600.0).suffix(" s")).changed();
                    });
                    changed |= file_row(ui, "Vidéo de fond (animée) :", &mut m.bg_video, "(aucune)", "Vidéos", VIDEO_EXTS);
                    if !m.bg_video.is_empty() {
                        changed |= ui.checkbox(&mut m.bg_video_audio, "Garder le son de la vidéo de fond (s'il n'y a pas de musique)").changed();
                    }
                    changed |= file_row(ui, "Musique des menus :", &mut m.music, "(aucune)", "Audio", &["mp3", "wav", "flac", "m4a", "aac", "ogg", "opus", "ac3", "wma"]);
                    if !m.bg_video.is_empty() || !m.music.is_empty() {
                        ui.label(egui::RichText::new("Le fond tourne en boucle : il dure autant que la vidéo de fond (2 min au plus) ou, sans vidéo, que la musique (5 min au plus). Les textes et images des pages personnalisées sont posés sur la vidéo.").weak());
                    }
                    changed |= file_row(ui, "Séquence d'ouverture :", &mut m.intro, "(aucune : le menu s'ouvre directement)", "Vidéos", VIDEO_EXTS);
                    changed |= ui.checkbox(&mut m.menu_1080, "Menu en 1920x1080 (sinon : taille de la vidéo)").changed();
                    changed |= ui.checkbox(&mut m.loop_menu, "Rejouer le fond en boucle (sinon : arrêt sur la dernière image)").changed();
                    changed |= ui.checkbox(&mut m.back_to_chapters, "Après une vidéo à chapitres, revenir à son menu de chapitres").changed();
                    ui.horizontal(|ui| {
                        let mut on = m.par_page.is_some();
                        if ui.checkbox(&mut on, "Éléments par page de menu :").changed() {
                            m.par_page = if on { Some(8) } else { None };
                            changed = true;
                        }
                        if let Some(p) = &mut m.par_page {
                            changed |= ui.add(egui::Slider::new(p, 1..=12)).changed();
                        }
                    });
                    changed |= ui.checkbox(&mut m.cache, "Réutiliser les vidéos déjà encodées (cache)").changed();
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
        if ui.button("Choisir…").clicked() {
            if let Some(f) = rfd::FileDialog::new().add_filter(filter, exts).pick_file() {
                *path = clean(&f);
                changed = true;
            }
        }
        if !path.is_empty() && ui.button("Aucun").clicked() {
            path.clear();
            changed = true;
        }
    });
    changed
}

fn main() -> eframe::Result<()> {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default().with_inner_size([1150.0, 760.0]).with_title("bdauthor — disques Blu-ray"),
        ..Default::default()
    };
    eframe::run_native("bdauthor", options, Box::new(|_cc| Ok(Box::new(App::default()))))
}
