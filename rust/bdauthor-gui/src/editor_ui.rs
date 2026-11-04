//! Editeur visuel de mise en page (onglet « Mise en page ») : apercu fidele, glisser/redimensionner, inspecteur.
use crate::i18n::{tr, trf};
use crate::App;
use bdauthor_core::design::{Align, Item, Kind, NavAction, NavLink, PageDesign, SlotReturn, Target};
use bdauthor_core::editor::{sim_buttons, align_items, add_layer, ItemAlign, dragged_rect, dragged_rect_ratio, hit_test_where, insert_item, list_pages, move_item, move_layer, remove_layer, set_rect, snap_item_rect, snap_lines, video_title, Handle, PageInfo, PageRef};
use bdauthor_core::ffmpeg::{extract_thumbnail_rgba, probe, Tools};
use bdauthor_core::palette::RgbaImage;
use bdauthor_render::{cover_resize, PreviewSlot};
use eframe::egui::{self, pos2, vec2, Color32, Key, Rect, Sense, Stroke};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{channel, Receiver, Sender};

const PREVIEW: (u32, u32) = (960, 540);

pub(crate) fn abs_path(base: &Path, f: &str) -> PathBuf {
    let p = Path::new(f);
    if p.is_absolute() { p.to_path_buf() } else { base.join(p) }
}

pub(crate) struct DragState {
    pub idx: usize,
    pub handle: Handle,
    pub start: (f32, f32),
    pub orig: Item,
    /// autres elements deplaces avec celui-ci (selection multiple) : (rang, etat d'origine)
    pub group: Vec<(usize, Item)>,
}

pub(crate) enum Job {
    Probe { key: String, path: PathBuf },
    /// duree d'un fichier audio (musique de menu)
    ProbeAudio { key: String, path: PathBuf },
    Thumb { key: String, path: PathBuf, at: f64, duration: f64 },
    /// image entiere (640 de large, proportions de la video) pour le widget « Image de la video »
    Frame { key: String, path: PathBuf, at: f64 },
}

pub(crate) enum Event {
    /// (fichier, duree, hauteur, cadence)
    Duration(String, f64, u32, f64),
    /// (fichier, message) : la video n'a pas pu etre lue (duree inconnue)
    ProbeFailed(String, String),
    Thumb(String, i64, RgbaImage),
    Frame(String, i64, RgbaImage),
    Error(String),
}

/// Thread de travail : sonde les durees et extrait les miniatures sans bloquer l'interface.
pub(crate) struct Worker {
    pub tx: Sender<Job>,
    pub rx: Receiver<Event>,
}

impl Worker {
    pub fn start(ctx: egui::Context) -> Worker {
        let (tx, jobs) = channel::<Job>();
        let (etx, rx) = channel::<Event>();
        std::thread::spawn(move || {
            let tools: Result<Tools, String> = Tools::locate().map_err(|e| e.to_string());
            for job in jobs {
                let ev = match (&tools, job) {
                    (Err(e), _) => Event::Error(trf!("ffmpeg not found: {}", e)),
                    (Ok(t), Job::Probe { key, path }) => match probe(t, &path) {
                        Ok(i) => Event::Duration(key, i.duration_s, i.height, i.fps()),
                        Err(e) => { let m = trf!("Cannot read {}: {}", key, e); Event::ProbeFailed(key, m) }
                    },
                    (Ok(t), Job::ProbeAudio { key, path }) => match bdauthor_core::ffmpeg::probe_duration(t, &path) {
                        Ok(d) => Event::Duration(key, d, 0, 0.0),
                        Err(e) => Event::Error(trf!("Cannot read {}: {}", key, e)),
                    },
                    (Ok(t), Job::Frame { key, path, at }) => {
                        let r = probe(t, &path).and_then(|i| {
                            let h = ((640.0 * i.height.max(1) as f64 / i.width.max(1) as f64 / 2.0).round() as u32 * 2).max(2);
                            bdauthor_core::ffmpeg::extract_frame_rgba(t, &path, at.min((i.duration_s - 0.05).max(0.0)), (640, h), true)
                        });
                        match r {
                            Ok(im) => Event::Frame(key, (at * 1000.0).round() as i64, im),
                            Err(e) => Event::Error(trf!("Cannot create the thumbnail of {}: {}", key, e)),
                        }
                    }
                    (Ok(t), Job::Thumb { key, path, at, duration }) => {
                        match extract_thumbnail_rgba(t, &path, (480, 270), Some(at), duration) {
                            Ok(im) => Event::Thumb(key, (at * 1000.0).round() as i64, im),
                            Err(e) => Event::Error(trf!("Cannot create the thumbnail of {}: {}", key, e)),
                        }
                    }
                };
                if etx.send(ev).is_err() { break; }
                ctx.request_repaint();
            }
        });
        Worker { tx, rx }
    }
}

/// Contenu d'une vignette.
#[derive(Clone)]
enum SlotShow {
    /// video, instant de la miniature (sinon celui de la video), libelle (vide : titre de la video)
    Video(usize, Option<f64>, String),
    Missing,
}


/// Supprime (`dup` = false) ou duplique l'element `i` ; retourne l'element a selectionner ensuite. La copie se place juste devant
/// l'original, dans le meme calque, decalee de 30 px.
fn item_edit(d: &mut PageDesign, i: usize, dup: bool) -> Option<usize> {
    if i >= d.items.len() { return None; }
    if !dup {
        d.items.remove(i);
        d.prune_nav();
        return None;
    }
    let mut c = d.items[i].clone();
    c.id = 0; // la copie n'a ni identifiant ni navigation manuelle
    c.nav = Default::default();
    c.name = copy_name(d, &c);
    c.x = (c.x + 30).min(1920 - c.w);
    c.y = (c.y + 30).min(1080 - c.h);
    d.items.insert(i + 1, c);
    Some(i + 1)
}

/// Nom d'une copie : « Nom (copie) », « Nom (copie 2) »… (premier libre dans la page) ; sans nom, le type du widget sert de base.
fn copy_name(d: &PageDesign, it: &Item) -> String {
    let base = if it.name.is_empty() { kind_name(&it.kind).to_string() } else { it.name.clone() };
    let base = match base.rfind(" (copie").or_else(|| base.rfind(" (copy")) { Some(p) if base.ends_with(')') => base[..p].to_string(), _ => base };
    let used = |n: &str| d.items.iter().any(|x| x.name == n);
    let first = trf!("{} (copy)", base);
    if !used(&first) { return first; }
    (2..).map(|k| trf!("{} (copy {})", base, k)).find(|n| !used(n)).unwrap_or(first)
}

/// Menu contextuel d'un element (clic droit) : `Some(true)` dupliquer, `Some(false)` supprimer.
fn item_menu(ui: &mut egui::Ui) -> Option<bool> {
    let mut out = None;
    if ui.button(tr("Duplicate")).clicked() { out = Some(true); ui.close_menu(); }
    if ui.button(tr("Delete")).clicked() { out = Some(false); ui.close_menu(); }
    out
}

/// Element a ajouter depuis la palette (glisser-deposer sur l'apercu).
#[derive(Clone, Copy, PartialEq)]
enum NewWidget { Text, Image, VideoFrame, VideoSlot, ChapterSlot, Nav(NavAction) }

/// Icones de la palette : (element, icone, nom).
fn palette() -> Vec<(NewWidget, &'static str, String)> {
    let mut v = vec![
        (NewWidget::Text, "🔤", tr("Text").to_string()),
        (NewWidget::Image, "🖼", tr("Image (PNG, JPEG, SVG…)").to_string()),
        (NewWidget::VideoFrame, "📷", tr("Video image (a frame of a project video, chosen by its time)").to_string()),
        (NewWidget::VideoSlot, "🎬", tr("Video thumbnail (plays from the start)").to_string()),
        (NewWidget::ChapterSlot, "🎞", tr("Chapter thumbnail (plays from a time)").to_string()),
    ];
    // un seul bouton de navigation : son action (Aller a, Precedent, Suivant, Tout lire) se choisit dans l'inspecteur
    v.push((NewWidget::Nav(NavAction::Goto), "🔘", tr("Navigation button (destination chosen in the inspector: page, video, Previous, Next, Play all, subtitles…)").to_string()));
    v
}

/// Nouvel element a sa place par defaut (une image n'a pas encore de fichier).
fn new_widget(w: NewWidget, first_video: Option<usize>) -> Item {
    match w {
        NewWidget::Text => Item::text(tr("Text"), 660, 470, 600, 120, 60),
        NewWidget::Image => Item { x: 100, y: 100, w: 400, h: 300, kind: Kind::Image { path: String::new(), link: None, frame: None }, look: Default::default(), layer: 0, sub: Default::default(), id: 0, nav: Default::default(), name: String::new() },
        NewWidget::VideoFrame => Item::video_frame(660, 300, 600, 338, first_video, 1.0),
        NewWidget::VideoSlot | NewWidget::ChapterSlot => {
            let mut it = Item::video_slot(100, 300, 400, 250, 200, 0, None, "");
            if let Kind::Slot { video, start, .. } = &mut it.kind {
                *video = first_video;
                if w == NewWidget::ChapterSlot { *start = Some(0.0); }
            }
            it
        }
        NewWidget::Nav(a) => Item::nav(a, 760, 940, 360, 80),
    }
}

/// Ligne glissee dans l'arborescence des pages.
#[derive(Clone, Copy, PartialEq)]
enum DragRow {
    /// page generique (indice)
    Page(usize),
    /// element : (page, indice dans la liste de la page)
    Item(usize, usize),
    /// calque : (page, position de l'arriere-plan au premier plan)
    Layer(usize, usize),
}

impl DragRow {
    fn page(self) -> usize { match self { DragRow::Page(p) | DragRow::Item(p, _) | DragRow::Layer(p, _) => p } }
}

/// Changement demande depuis l'arborescence (`p` : page concernee).
enum LayerOp {
    Add,
    Remove(usize, u32, bool),
    Hidden(usize, u32),
    Locked(usize, u32),
    Rename(usize, u32, String),
    MoveItem { p: usize, from: usize, before: usize, layer: u32 },
    MoveLayer { p: usize, from: usize, to: usize },
    PageRename(usize, String),
    PageDelete(usize),
    PageMove { from: usize, to: usize },
    /// element ou calque (`DragRow`) envoye vers une autre page ; `copy` : copie
    Transfer { what: DragRow, to: usize, copy: bool },
}

/// Bouton d'alignement dessine (les glyphes fleches/barres ne sont pas dans la police par defaut) : un trait de reference et deux barres.
pub(crate) fn align_button(ui: &mut egui::Ui, a: ItemAlign, tip: &str) -> bool {
    let (rect, resp) = ui.allocate_exact_size(vec2(28.0, 28.0), Sense::click());
    let vis = *ui.style().interact(&resp);
    let p = ui.painter();
    p.rect(rect, 3.0, vis.bg_fill, vis.bg_stroke);
    let col = vis.fg_stroke.color;
    let r = rect.shrink(6.0);
    let (long, short, th) = (r.width(), r.width() * 0.55, 4.0_f32);
    let line = Stroke::new(1.5_f32, col);
    match a {
        ItemAlign::Left | ItemAlign::CenterH | ItemAlign::Right => {
            let ex = match a { ItemAlign::Left => r.left(), ItemAlign::Right => r.right(), _ => r.center().x };
            p.line_segment([pos2(ex, r.top() - 1.0), pos2(ex, r.bottom() + 1.0)], line);
            for (y, len) in [(r.top() + 1.0, long * 0.9), (r.bottom() - 1.0 - th, short)] {
                let x0 = match a { ItemAlign::Left => r.left(), ItemAlign::Right => r.right() - len, _ => ex - len / 2.0 };
                p.rect_filled(Rect::from_min_size(pos2(x0, y), vec2(len, th)), 0.0, col);
            }
        }
        _ => {
            let ey = match a { ItemAlign::Top => r.top(), ItemAlign::Bottom => r.bottom(), _ => r.center().y };
            p.line_segment([pos2(r.left() - 1.0, ey), pos2(r.right() + 1.0, ey)], line);
            for (x, len) in [(r.left() + 1.0, long * 0.9), (r.right() - 1.0 - th, short)] {
                let y0 = match a { ItemAlign::Top => r.top(), ItemAlign::Bottom => r.bottom() - len, _ => ey - len / 2.0 };
                p.rect_filled(Rect::from_min_size(pos2(x, y0), vec2(th, len)), 0.0, col);
            }
        }
    }
    resp.on_hover_text(tip).clicked()
}

/// Couleur de chaque fleche de la telecommande (haut, bas, gauche, droite) : nom dans le panneau et lien sur l'apercu.
const NAV_COLORS: [Color32; 4] = [Color32::from_rgb(255, 170, 60), Color32::from_rgb(90, 160, 255), Color32::from_rgb(110, 220, 120), Color32::from_rgb(235, 100, 210)];

/// Texte depose dans le presse-papiers systeme a la copie de widgets (le contenu reel reste dans l'application).
const CLIP_MARK: &str = "bdauthor:widgets";

fn mmss(t: f64) -> String {
    let s = t.max(0.0);
    let whole = s.floor() as i64;
    let tenth = ((s - whole as f64) * 10.0).round() as i64;
    if tenth == 0 || tenth == 10 { let w = s.round() as i64; format!("{}:{:02}", w / 60, w % 60) } else { format!("{}:{:02}.{tenth}", whole / 60, whole % 60) }
}

/// « m:ss », « h:mm:ss » ou des secondes (virgule ou point) -> secondes.
fn parse_mmss(s: &str) -> Option<f64> {
    let s = s.trim().replace(',', ".");
    let mut t = 0.0;
    for part in s.split(':') {
        t = t * 60.0 + part.trim().parse::<f64>().ok()?;
    }
    (t >= 0.0).then_some(t)
}

fn kind_color(k: &Kind) -> Color32 {
    match k {
        Kind::Slot { .. } => Color32::from_rgb(80, 160, 255),
        Kind::Nav { .. } => Color32::from_rgb(90, 220, 120),
        Kind::Text { .. } => Color32::from_rgb(255, 210, 60),
        Kind::Image { .. } => Color32::from_rgb(210, 120, 255),
    }
}

fn kind_name(k: &Kind) -> &'static str {
    match k {
        Kind::Slot { .. } => tr("Thumbnail"),
        Kind::Nav { .. } => tr("Navigation button"),
        Kind::Text { link: None, .. } => tr("Text"),
        Kind::Text { .. } => tr("Clickable text"),
        Kind::Image { frame: Some(_), link: None, .. } => tr("Video image"),
        Kind::Image { frame: Some(_), .. } => tr("Clickable video image"),
        Kind::Image { link: None, .. } => tr("Image"),
        Kind::Image { .. } => tr("Clickable image"),
    }
}

/// Case « cliquable » et choix de la destination d'un texte ou d'une image.
fn link_ui(ui: &mut egui::Ui, link: &mut Option<Target>, targets: &[(Target, String)], changed: &mut bool) {
    let mut on = link.is_some();
    if ui.checkbox(&mut on, tr("Clickable (becomes a menu button)")).changed() {
        // par defaut : la premiere page (pas un reglage de sous-titres ou d'audio)
        *link = if on { targets.iter().find(|t| !t.0.is_setting()).or(targets.first()).map(|t| t.0) } else { None };
        *changed = true;
    }
    if link.is_some() {
        destination_combo(ui, link, targets, changed);
        crate::help(ui, tr("Selected with the remote control, the element is surrounded by a blue frame."));
    }
}

/// Liste des destinations possibles : pages du menu puis videos a lire.
fn destination_combo(ui: &mut egui::Ui, target: &mut Option<Target>, targets: &[(Target, String)], changed: &mut bool) {
    let shown = target.and_then(|t| targets.iter().find(|(x, _)| *x == t.without_exit()).map(|(_, l)| l.clone())).unwrap_or_else(|| tr("(destination not found: to choose)").to_string());
    egui::ComboBox::from_label(tr("Destination")).selected_text(shown).show_ui(ui, |ui| {
        for (t, label) in targets {
            // meme reglage deja choisi : sa page de sortie est gardee
            if ui.selectable_label(target.map(Target::without_exit) == Some(*t), label).clicked() && target.map(Target::without_exit) != Some(*t) { *target = Some(*t); *changed = true; }
        }
    });
}

impl App {
    pub(crate) fn pages(&self) -> Vec<PageInfo> {
        list_pages(&self.model)
    }

    /// Destinations d'un bouton « Aller a » ou d'un element cliquable : (cible, libelle) ; les pages, puis les videos a lire.
    fn page_targets(&self) -> Vec<(Target, String)> {
        let mut out: Vec<(Target, String)> = self.pages().into_iter().map(|p| (Target::Main(p.page), trf!("Page: {}", p.label(&self.model)))).collect();
        for v in 0..self.model.videos.len() {
            out.push((Target::Video(v), trf!("Play video {}. {}", v + 1, video_title(&self.model, v))));
        }
        // reglages sans navigation : les sous-titres, puis une entree par langue audio (jamais les deux a la fois)
        let mut stay = vec![(Target::Stay(None), tr("Set subtitles").to_string())];
        for l in &self.model.languages {
            stay.push((Target::Audio(l.id, None), trf!("Set audio language: {}", l.name)));
        }
        out.splice(0..0, stay);
        out
    }

    fn current_info(&self) -> Option<PageInfo> {
        let pages = self.pages();
        self.cur.and_then(|c| pages.iter().find(|p| p.r == c).cloned()).or_else(|| pages.first().cloned())
    }

    /// Taille du menu sans l'option 1080 : celle de la premiere video une fois encodee (1080 si sa hauteur depasse 800, sinon 720).
    fn menu_plane(&self) -> (u32, u32) {
        let first = self.model.videos.first().map(|v| v.file.clone()).unwrap_or_default();
        // meme regle que ffmpeg::pick_target : 1080 seulement pour une source haute definition a 25 i/s au plus
        match self.video_sizes.get(&first) {
            Some(&(h, fps)) if h > 800 && fps < 26.0 => (1920, 1080),
            Some(_) => (1280, 720),
            None => (1920, 1080),
        }
    }

    pub(crate) fn base_dir(&self) -> PathBuf {
        self.path.as_ref().and_then(|p| p.parent()).map(|p| p.to_path_buf()).unwrap_or_else(|| PathBuf::from("."))
    }

    pub(crate) fn bump(&mut self) {
        self.edit_gen += 1;
        self.dirty = true;
    }

    /// Demande au thread de travail la duree des videos du projet (une seule fois par fichier).
    pub(crate) fn request_durations(&mut self) {
        let base = self.base_dir();
        for v in &self.model.videos {
            if !self.durations.contains_key(&v.file) && self.requested.insert(format!("P|{}", v.file)) {
                if let Some(w) = &self.worker {
                    let _ = w.tx.send(Job::Probe { key: v.file.clone(), path: abs_path(&base, &v.file) });
                }
            }
        }
    }

    /// Demande au thread de travail la duree des videos et les miniatures utiles a la page affichee.
    pub(crate) fn request_media(&mut self, info: &PageInfo) {
        let base = self.base_dir();
        self.request_durations();
        // video de fond anime : sa duree puis une image
        let bgv = self.page_visual(info).1.unwrap_or_default();
        if !bgv.is_empty() && !self.durations.contains_key(&bgv) && self.requested.insert(format!("P|{bgv}")) {
            if let Some(w) = &self.worker {
                let _ = w.tx.send(Job::Probe { key: bgv.clone(), path: abs_path(&base, &bgv) });
            }
        }
        let mut sources = self.slot_sources(info);
        for (file, at) in self.frame_sources(info) {
            let ms = (at * 1000.0).round() as i64;
            if !self.frames.contains_key(&(file.clone(), ms)) && self.requested.insert(format!("F|{file}|{ms}")) {
                if let Some(w) = &self.worker {
                    let _ = w.tx.send(Job::Frame { key: file.clone(), path: abs_path(&base, &file), at });
                }
            }
        }
        if let Some(&d) = self.durations.get(&bgv) {
            sources.push((bgv.clone(), (d / 2.0).min(1.0)));
        }
        for (file, at) in sources {
            let Some(&dur) = self.durations.get(&file) else { continue };
            let ms = (at * 1000.0).round() as i64;
            if !self.thumbs.contains_key(&(file.clone(), ms)) && self.requested.insert(format!("T|{file}|{ms}")) {
                if let Some(w) = &self.worker {
                    let _ = w.tx.send(Job::Thumb { key: file.clone(), path: abs_path(&base, &file), at, duration: dur });
                }
            }
        }
    }

    /// Ce que montre chaque vignette de la page, dans l'ordre de la liste.
    fn slot_plan(&self, info: &PageInfo) -> Vec<SlotShow> {
        let Some(d) = info.design(&self.model.designs) else { return vec![] };
        let n = self.model.videos.len();
        d.items.iter().filter_map(|it| {
            let Kind::Slot { start, label, thumb_at, .. } = &it.kind else { return None };
            Some(match it.fixed_video() {
                Some(v) if v < n => {
                    let label = if !label.is_empty() { label.clone() } else if let Some(t) = start { mmss(*t) } else { String::new() };
                    SlotShow::Video(v, thumb_at.or(*start), label)
                }
                _ => SlotShow::Missing,
            })
        }).collect()
    }

    fn video_thumb(&self, i: usize) -> (String, f64) {
        let v = &self.model.videos[i];
        let at = v.thumb_s.unwrap_or_else(|| self.durations.get(&v.file).map_or(0.0, |d| (d * 0.25).min(1.0)));
        (v.file.clone(), at)
    }

    /// Duree d'un fichier (video ou, si `audio`, son seul) une fois sondee ; demande le sondage sinon.
    pub(crate) fn media_duration(&mut self, file: &str, audio: bool) -> Option<f64> {
        if file.is_empty() { return None; }
        if let Some(&d) = self.durations.get(file) { return Some(d); }
        if self.requested.insert(format!("P|{file}")) {
            let path = abs_path(&self.base_dir(), file);
            if let Some(w) = &self.worker {
                let key = file.to_string();
                let _ = w.tx.send(if audio { Job::ProbeAudio { key, path } } else { Job::Probe { key, path } });
            }
        }
        None
    }

    /// Duree et place estimee d'un fond anime (`video`) ou sonore (`music` sur image fixe).
    pub(crate) fn bg_note(&mut self, video: &str, music: &str) -> Option<String> {
        let fmt = |d: f64| { let s = d.round() as i64; if s >= 60 { trf!("{} min {} s", s / 60, format!("{:02}", s % 60)) } else { trf!("{} s", s) } };
        if !video.is_empty() {
            let d = self.media_duration(video, false)?;
            let per_min = if self.menu_plane().1 >= 1080 { 60.0 } else { 30.0 };
            return Some(trf!("Duration {} (played in full, looped) · about {} MB on the disc for each page that uses this background (estimate)", fmt(d), format!("{:.0}", d / 60.0 * per_min)));
        }
        if !music.is_empty() {
            let d = self.media_duration(music, true)?;
            return Some(trf!("The background will last {} (the whole music) · about {} MB per page that uses it (estimate)", fmt(d), format!("{:.0}", (d / 60.0 * 2.5).max(1.0))));
        }
        None
    }

    /// Fond visible sur une page : (image, video), d'apres le fond propre a la page ou celui du projet.
    pub(crate) fn page_visual(&self, info: &PageInfo) -> (Option<String>, Option<String>) {
        use bdauthor_core::design::PageVisual;
        let nz = |s: &String| (!s.is_empty()).then(|| s.clone());
        match info.design(&self.model.designs).map(|d| &d.bg.visual) {
            Some(PageVisual::Image(f)) => (Some(f.clone()), None),
            Some(PageVisual::Video(f, _)) => (None, Some(f.clone())),
            _ => (nz(&self.model.bg), nz(&self.model.bg_video)),
        }
    }

    /// Image (1 s) d'une video de fond pour l'apercu, si elle est deja extraite.
    fn video_frame(&self, f: &String) -> Option<std::sync::Arc<RgbaImage>> {
        let d = *self.durations.get(f)?;
        let ms = ((d / 2.0).min(1.0) * 1000.0).round() as i64;
        self.thumbs.get(&(f.clone(), ms)).cloned()
    }

    /// Images de video (widget « Image de la video ») de la page : (fichier, instant).
    fn frame_sources(&self, info: &PageInfo) -> Vec<(String, f64)> {
        let Some(d) = info.design(&self.model.designs) else { return vec![] };
        d.items.iter().filter_map(|it| match it.kind {
            Kind::Image { frame: Some(bdauthor_core::design::VideoFrame { video: Some(v), at, .. }), .. } => self.model.videos.get(v).map(|x| (x.file.clone(), at)),
            _ => None,
        }).collect()
    }

    /// Miniatures utiles a la page : (fichier, instant).
    fn slot_sources(&self, info: &PageInfo) -> Vec<(String, f64)> {
        self.slot_plan(info).into_iter().filter_map(|s| match s {
            SlotShow::Video(v, at, _) => { let (f, a) = self.video_thumb(v); Some((f, at.unwrap_or(a))) }
            SlotShow::Missing => None,
        }).collect()
    }

    /// Contenu de chaque vignette pour l'apercu, dans l'ordre de la liste.
    fn preview_slots(&self, info: &PageInfo) -> Vec<PreviewSlot> {
        self.slot_plan(info).into_iter().map(|s| {
            let (label, src) = match s {
                SlotShow::Video(v, at, label) => {
                    let (f, a) = self.video_thumb(v);
                    (if label.is_empty() { video_title(&self.model, v) } else { label }, Some((f, at.unwrap_or(a))))
                }
                SlotShow::Missing => (tr("(choose a video)").to_string(), None),
            };
            let (file, at) = src.unwrap_or((String::new(), 0.0));
            let ms = (at * 1000.0).round() as i64;
            let thumb = self.thumbs.get(&(file.clone(), ms)).cloned();
            let key = if thumb.is_some() {
                use std::hash::{Hash, Hasher};
                let mut h = std::collections::hash_map::DefaultHasher::new();
                (&file, ms).hash(&mut h);
                h.finish()
            } else { 0 };
            PreviewSlot { label, thumb, key }
        }).collect()
    }

    fn refresh_preview(&mut self, ctx: &egui::Context, info: &PageInfo) {
        let base_dir = self.base_dir();
        let slots = self.preview_slots(info);
        // les PreviewSlot sont dans l'ordre des vignettes de la page
        let Some(mut design) = info.design(&self.model.designs).cloned() else { return };
        // images de video : la miniature extraite, ecrite une fois en PNG temporaire, remplace l'element pour l'apercu
        for it in &mut design.items {
            let Kind::Image { path, frame, .. } = &mut it.kind else { continue };
            let Some(f) = *frame else { continue };
            *frame = None;
            path.clear();
            let Some(file) = f.video.and_then(|v| self.model.videos.get(v)).map(|v| v.file.clone()) else { continue };
            let ms = (f.at * 1000.0).round() as i64;
            let Some(img) = self.frames.get(&(file.clone(), ms)) else { continue };
            use std::hash::{Hash, Hasher};
            let mut h = std::collections::hash_map::DefaultHasher::new();
            let crop = f.crop.map(|c| c.map(|v| (v * 1e4).round() as i64));
            (&file, ms, crop, std::process::id()).hash(&mut h);
            let png = std::env::temp_dir().join(format!("bdauthor_image_video_{:016x}.png", h.finish()));
            if !png.is_file() {
                let (cx, cy, cw, ch) = f.crop_px(img.width, img.height);
                let part = bdauthor_core::cover::crop(img, cx as i32, cy as i32, cw as i32, ch as i32);
                let _ = std::fs::write(&png, bdauthor_core::png::write_png(&part));
            }
            *path = png.to_string_lossy().into_owned();
        }
        // calques masques (hors simulation) : leurs elements et vignettes sont retires de l'apercu
        let mut slots = slots;
        let mut sel_shift: Option<usize> = self.sel_item;
        if self.sim.is_none() && design.layers.iter().any(|l| l.hidden) {
            let hidden: Vec<bool> = (0..design.items.len()).map(|i| design.item_hidden(i)).collect();
            let mut slot_k = 0;
            let mut kept_slots = Vec::new();
            for (i, it) in design.items.iter().enumerate() {
                if matches!(it.kind, Kind::Slot { .. }) {
                    if !hidden[i] { if let Some(sl) = slots.get(slot_k) { kept_slots.push(sl.clone()); } }
                    slot_k += 1;
                }
            }
            slots = kept_slots;
            sel_shift = sel_shift.filter(|&s| !hidden.get(s).copied().unwrap_or(true)).map(|s| (0..s).filter(|&j| !hidden[j]).count());
            let mut k = 0;
            design.items.retain(|_| { let keep = !hidden[k]; k += 1; keep });
        }
        let Ok(renderer) = &self.renderer else { return };
        let (vis_image, vis_video) = self.page_visual(info);
        let bg_frame = vis_video.as_ref().and_then(|v| self.video_frame(v));
        let base = if let Some(f) = bg_frame {
            Some(cover_resize(&f, PREVIEW.0, PREVIEW.1))
        } else if vis_video.is_some() || vis_image.is_none() {
            None
        } else {
            let path = abs_path(&base_dir, vis_image.as_deref().unwrap_or(""));
            if self.base_cache.as_ref().map_or(true, |(p, _)| *p != path) {
                self.base_cache = renderer.cached_image(&path).ok().map(|im| (path.clone(), cover_resize(&im, PREVIEW.0, PREVIEW.1)));
            }
            self.base_cache.as_ref().map(|(_, i)| i.clone())
        };
        let sel = if info.design(&self.model.designs).is_some() || self.sim.is_some() { sel_shift } else { None };
        // image courante de l'animation du bouton selectionne
        if self.preview_pulse > 0 {
            let i = self.preview_pulse - 1;
            if let Some(it) = sel.and_then(|s| design.items.get_mut(s)) {
                if it.look.skin.anim_frames() > 0 && !matches!(it.kind, Kind::Slot { .. }) {
                    let f = it.look.skin.sel_frames[i % it.look.skin.sel_frames.len()].clone();
                    it.look.skin.selected = f;
                } else {
                    let st = it.look.style.clone().unwrap_or_else(|| self.model.designs.style.clone());
                    it.look.style = Some(st.pulse_style(i));
                }
            }
        }
        // etat normal anime des autres elements : image courante
        if self.preview_nstep > 0 {
            let k = self.preview_nstep - 1;
            for (idx, it) in design.items.iter_mut().enumerate() {
                if Some(idx) == sel || matches!(it.kind, Kind::Slot { .. }) { continue; }
                let sk = &mut it.look.skin;
                let n = sk.normal_frames.len();
                if n >= 2 {
                    let i = if sk.normal_once { k.min(n - 1) } else { k % n };
                    sk.normal = sk.normal_frames[i].clone();
                    sk.normal_frames.clear();
                }
            }
        }
        let act = self.sim_pressed();
        let img = renderer.render_page_preview(&design, info.page, info.pages, &slots, base.as_ref(), PREVIEW, &base_dir, &self.model.designs.style, sel, act);
        self.preview_sel = self.sel_item;
        self.preview_act = act;
        let ci = egui::ColorImage::from_rgba_unmultiplied([img.width as usize, img.height as usize], &img.data);
        match &mut self.preview_tex {
            Some(t) => t.set(ci, egui::TextureOptions::LINEAR),
            None => self.preview_tex = Some(ctx.load_texture("apercu", ci, egui::TextureOptions::LINEAR)),
        }
    }

    // ------------------------------------------------------------------ pages de depart
    /// Cree les pages de depart (grille des videos, pages de chapitres) d'un projet qui n'a encore aucune page, une fois connues les
    /// durees utiles (chapitres automatiques). `false` tant que ce n'est pas possible.
    pub(crate) fn create_starter_pages(&mut self) -> bool {
        let waiting: Vec<String> = self.model.videos.iter().filter(|v| v.chapters.is_empty() && v.auto_s.is_some()).map(|v| v.file.clone()).collect();
        for f in &waiting { let _ = self.media_duration(f, false); }
        match bdauthor_core::editor::ensure_pages(&mut self.model, &self.durations) {
            Ok(true) => {
                self.history.reset(&self.model.designs);
                self.cur = None;
                self.sel_item = None;
                self.dirty = true;
                self.edit_gen += 1;
                self.notice = trf!("Starter pages created: {} page(s), all editable.", self.model.designs.free.len());
                true
            }
            _ => false,
        }
    }

    // ------------------------------------------------------------------ zone centrale : barre d'outils + apercu
    pub(crate) fn layout_central(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        let Some(info) = self.current_info() else {
            if self.model.videos.is_empty() {
                ui.label(tr("Add videos in the “Videos” tab to compose the menu pages."));
                return;
            }
            ui.label(tr("No menu page."));
            if ui.button(tr("Create starter pages")).on_hover_text(tr("A grid of video thumbnails, then a chapter page per video that has chapters; everything stays editable")).clicked() && !self.create_starter_pages() {
                self.notice = tr("Starter pages: the duration of videos with automatic chapters is being read, try again in a moment.").into();
            }
            crate::help(ui, tr("Or “+ Page” (on the left) to start from a blank page. With no page, building the disc creates these starter pages."));
            return;
        };
        if let Err(e) = &self.renderer {
            ui.colored_label(Color32::from_rgb(230, 100, 90), trf!("Preview unavailable: {}", e));
            return;
        }
        self.request_media(&info);
        let custom = info.design(&self.model.designs).is_some();

        ui.horizontal_wrapped(|ui| {
            if self.sim.is_none() {
                if ui.button(tr("▶ Simulate the remote control")).on_hover_text(tr("Browse the menu as on a player: arrows, Enter, Esc")).clicked() {
                    self.sim = Some(crate::sim_ui::Sim::default());
                    self.sel_item = None;
                    self.drag = None;
                    self.edit_gen += 1;
                }
            } else if ui.button(tr("■ Stop the simulation")).clicked() {
                self.sim = None;
                self.sel_item = None;
                self.edit_gen += 1;
            }
            ui.separator();
            ui.strong(info.label(&self.model));
        });
        if custom {
            let mut add: Option<Item> = None;
            let mut pick_image = false;
        let mut pick_frame = false;
            // palette : glisser une icone sur l'apercu pour placer l'element, ou cliquer pour l'ajouter a sa place par defaut
            let first = (!self.model.videos.is_empty()).then_some(0);
            let palette = palette();
            ui.horizontal_wrapped(|ui| {
                for (w, icon, name) in &palette {
                    let b = ui.add(egui::Button::new(egui::RichText::new(*icon).size(27.0)).min_size(vec2(48.0, 48.0)).sense(Sense::click_and_drag()))
                        .on_hover_text(trf!("{}\nDrag onto the page to place it, or click to add it", name));
                    if b.drag_started() { egui::DragAndDrop::set_payload(ui.ctx(), *w); }
                    if b.dragged() { ui.ctx().set_cursor_icon(egui::CursorIcon::Grabbing); }
                    if b.clicked() {
                        if *w == NewWidget::Image { pick_image = true; } else { add = Some(new_widget(*w, first)); }
                    }
                }
                ui.separator();
                ui.checkbox(&mut self.snap, tr("Snap to 10 px"));
                ui.checkbox(&mut self.guide_snap, tr("Align with the other items")).on_hover_text(tr("While moving or resizing, the edges and centers snap to the edges and center of the screen and of the other visible items (magenta lines). Alt: temporarily off."));
            });
            if pick_image {
                if let Some(f) = rfd::FileDialog::new().add_filter(tr("Images"), &["png", "jpg", "jpeg", "bmp", "svg", "svgz"]).pick_file() {
                    let rel = self.relative(&f);
                    add = Some(Item { x: 100, y: 100, w: 400, h: 300, kind: Kind::Image { path: rel, link: None, frame: None }, look: Default::default(), layer: 0, sub: Default::default(), id: 0, nav: Default::default(), name: String::new() });
                }
            }
            if let Some(it) = add {
                let layer = self.target_layer(&info);
                if let Some(d) = info.design_mut(&mut self.model.designs) {
                    self.sel_item = Some(insert_item(d, it, layer));
                    self.bump();
                }
            }
        }

        // aperçu régénéré hors glissement
        let min_gap = if self.cfg.adaptive_preview { (self.render_ms * 1.25).clamp(16.0, 200.0) } else { 0.0 };
        let due = self.drag.is_none() || self.last_preview.elapsed().as_secs_f32() * 1000.0 >= min_gap;
        // animation du bouton selectionne : animation d'images (simulation ou « Apercu ») sinon halo anime (simulation),
        // a la cadence de la page ; `pulse` = image + 1 (0 : image fixe)
        let (n_frames, fps, once) = {
            let d = info.design(&self.model.designs);
            let fps = d.map_or(0, |d| d.anim_fps);
            let fps = if fps == 0 { bdauthor_core::design::DEFAULT_ANIM_FPS } else { fps };
            let (n, once) = self.sel_item.and_then(|i| d.and_then(|d| d.items.get(i))).map_or((0, false), |it| {
                let custom = if matches!(it.kind, Kind::Slot { .. }) { 0 } else { it.look.skin.anim_frames() };
                if custom > 0 && (self.sim.is_some() || self.anim_preview) { (custom, it.look.skin.sel_once) }
                else if custom == 0 && self.sim.is_some() { let p = it.look.style.as_ref().unwrap_or(&self.model.designs.style).pulse_frames(); (if p > 1 { p } else { 0 }, false) }
                else { (0, false) }
            });
            (n, fps, once)
        };
        // comme le lecteur : l'animation repart du debut a chaque nouvelle selection ; « une fois » s'arrete sur la derniere image
        let key = (self.sel_item, self.anim_preview, self.sim.is_some(), once);
        if key != self.anim_key { self.anim_key = key; self.anim_t0 = std::time::Instant::now(); }
        let step = (self.anim_t0.elapsed().as_secs_f64() * fps as f64) as usize;
        let pulse = if n_frames == 0 { 0 } else if once { step.min(n_frames - 1) + 1 } else { step % n_frames + 1 };
        if n_frames > 0 && !(once && step >= n_frames - 1) { ctx.request_repaint_after(std::time::Duration::from_millis(30)); }
        // animations de l'etat normal des elements de la page (simulation ou « Apercu ») : meme horloge
        let (nmax, nloop) = if self.sim.is_some() || self.anim_preview {
            info.design(&self.model.designs).map_or((0, false), |d| d.items.iter()
                .filter(|it| !matches!(it.kind, Kind::Slot { .. }) && it.look.skin.normal_anim_frames() >= 2)
                .fold((0, false), |(m, l), it| (m.max(it.look.skin.normal_anim_frames()), l || !it.look.skin.normal_once)))
        } else { (0, false) };
        let nstep = if nmax == 0 { 0 } else if nloop { step + 1 } else { step.min(nmax - 1) + 1 };
        if nmax > 0 && (nloop || step < nmax - 1) { ctx.request_repaint_after(std::time::Duration::from_millis(30)); }
        if (self.preview_gen != self.edit_gen && due) || self.preview_tex.is_none() || (self.preview_sel != self.sel_item && self.drag.is_none()) || self.preview_act != self.sim_pressed() || self.preview_pulse != pulse || self.preview_nstep != nstep {
            self.preview_pulse = pulse;
            self.preview_nstep = nstep;
            let t = std::time::Instant::now();
            self.refresh_preview(ctx, &info);
            self.render_ms = t.elapsed().as_secs_f32() * 1000.0;
            self.preview_gen = self.edit_gen;
            self.last_preview = std::time::Instant::now();
        } else if self.preview_gen != self.edit_gen {
            ctx.request_repaint();
        }

        // poids de la page (images de boutons a decoder)
        if let Some(d) = info.design(&self.model.designs) {
            let plane = if self.model.menu_1080 { (1920, 1080) } else { self.menu_plane() };
            let wt = d.weight(&self.model.designs.style, info.page, info.pages, plane);
            crate::style_ui::weight_ui(ui, &wt, plane);
        }

        // vérifications de la page : calculees d'abord pour ne reserver sous l'apercu que la place necessaire
        let probs: Vec<bdauthor_core::design::Problem> = match info.design(&self.model.designs).filter(|_| custom) {
            Some(d) => {
                let targets = self.page_targets();
                let mut probs = d.validate(info.page, info.pages, self.model.videos.len());
                probs.extend(d.check_targets(&|t| targets.iter().any(|(x, _)| *x == t.without_exit()) && t.exit().map_or(true, |q| targets.iter().any(|(x, _)| *x == Target::Main(q)))));
                probs
            }
            None => vec![],
        };
        // problemes de la page (chevauchements, destinations...) : au-dessus de l'apercu, toujours visibles
        if !probs.is_empty() {
            egui::ScrollArea::vertical().id_salt("defil_verifs").max_height(10.0 + 20.0 * probs.len().min(4) as f32).show(ui, |ui| {
                for p in &probs {
                    let (c, t) = if p.error { (Color32::from_rgb(230, 100, 90), tr("Error")) } else { (Color32::from_rgb(230, 190, 80), tr("Warning")) };
                    ui.colored_label(c, format!("{t} : {}", p.msg));
                }
            });
        }
        let reserve = 6.0;
        // vue : zoom et deplacement de l'apercu (reinitialises a chaque changement de page)
        if self.view_page != Some(info.r) { self.view_page = Some(info.r); self.view_zoom = 1.0; self.view_pan = egui::Vec2::ZERO; }
        ui.horizontal(|ui| {
            let z = self.view_zoom;
            if ui.small_button("−").on_hover_text(tr("Zoom out")).clicked() { let nz = (z / 1.25).max(0.25); self.view_pan *= nz / z; self.view_zoom = nz; }
            if ui.small_button("+").on_hover_text(tr("Zoom in")).clicked() { let nz = (z * 1.25).min(8.0); self.view_pan *= nz / z; self.view_zoom = nz; }
            if ui.small_button(tr("Fit")).on_hover_text(tr("Whole page in the window")).clicked() { self.view_zoom = 1.0; self.view_pan = egui::Vec2::ZERO; }
            ui.label(format!("{:.0} %", self.view_zoom * 100.0));
            crate::help(ui, tr("Wheel: zoom · wheel click (or Space + drag): pan the view · Shift while resizing: proportional"));
        });
        let avail = ui.available_size();
        let w = avail.x.min((avail.y - reserve).max(200.0) * 16.0 / 9.0).max(320.0);
        let (resp, painter) = ui.allocate_painter(vec2(w, w * 9.0 / 16.0), Sense::click_and_drag());
        let viewport = resp.rect;
        // deplacement (clic molette ou Espace + bouton gauche) et zoom a la molette, centre sur le curseur
        let panning = (resp.hovered() || self.view_panning) && !ctx.wants_keyboard_input()
            && ctx.input(|i| i.pointer.middle_down() || (i.key_down(Key::Space) && i.pointer.primary_down()));
        let skip_input = panning || self.view_panning; // pas de clic ni de glissement d'element pendant (et juste apres) le deplacement
        self.view_panning = panning;
        if panning {
            self.view_pan += ctx.input(|i| i.pointer.delta());
            ctx.set_cursor_icon(egui::CursorIcon::Grabbing);
        }
        if resp.hovered() {
            let (scroll, zd, ptr) = ctx.input(|i| (i.smooth_scroll_delta.y, i.zoom_delta(), i.pointer.hover_pos()));
            let f = (scroll * 0.0015).exp() * zd;
            if (f - 1.0).abs() > 1e-4 {
                let z = self.view_zoom;
                let nz = (z * f).clamp(0.25, 8.0);
                if let Some(p) = ptr {
                    let c0 = viewport.center() + self.view_pan;
                    self.view_pan = (p - (p - c0) * (nz / z)) - viewport.center();
                }
                self.view_zoom = nz;
                // la molette sert au zoom : le panneau ne defile pas en meme temps
                ctx.input_mut(|i| i.smooth_scroll_delta = egui::Vec2::ZERO);
            }
        }
        let half = viewport.size() * (0.5 + self.view_zoom * 0.5) - vec2(40.0, 40.0);
        self.view_pan = vec2(self.view_pan.x.clamp(-half.x.max(0.0), half.x.max(0.0)), self.view_pan.y.clamp(-half.y.max(0.0), half.y.max(0.0)));
        let rect = Rect::from_center_size(viewport.center() + self.view_pan, viewport.size() * self.view_zoom);
        let k = rect.width() / 1920.0;
        if self.view_zoom != 1.0 || self.view_pan != egui::Vec2::ZERO { painter.rect_filled(viewport, 0.0, Color32::from_gray(18)); }
        match &self.preview_tex {
            Some(t) => painter.image(t.id(), rect, Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), Color32::WHITE),
            None => painter.rect_filled(rect, 0.0, Color32::from_gray(30)),
        };
        let to_screen = |x: f32, y: f32| pos2(rect.min.x + x * k, rect.min.y + y * k);
        let to_plane = |p: egui::Pos2| ((p.x - rect.min.x) / k, (p.y - rect.min.y) / k);

        // depot d'un element de la palette : apercu de sa place sous la souris, ajout au relachement
        if self.sim.is_none() && custom {
            if let Some(w) = egui::DragAndDrop::payload::<NewWidget>(ctx) {
                if let Some(pos) = ctx.pointer_hover_pos().filter(|p| rect.contains(*p)) {
                    let mut it = new_widget(*w, (!self.model.videos.is_empty()).then_some(0));
                    let (px, py) = to_plane(pos);
                    let g = |v: i32| if self.snap { (v + 5).div_euclid(10) * 10 } else { v };
                    it.x = g(px.round() as i32 - it.w / 2).clamp(0, 1920 - it.w);
                    it.y = g(py.round() as i32 - it.h / 2).clamp(0, 1080 - it.h);
                    let r = Rect::from_min_max(to_screen(it.x as f32, it.y as f32), to_screen((it.x + it.w) as f32, (it.y + it.h) as f32));
                    painter.rect_filled(r, 0.0, Color32::from_rgba_unmultiplied(90, 170, 255, 40));
                    painter.rect_stroke(r, 0.0, Stroke::new(2.0_f32, Color32::from_rgb(90, 170, 255)));
                    if ctx.input(|i| i.pointer.any_released()) {
                        egui::DragAndDrop::clear_payload(ctx);
                        // une image demande son fichier au moment du depot
                        let dropped = if *w == NewWidget::Image {
                            rfd::FileDialog::new().add_filter(tr("Images"), &["png", "jpg", "jpeg", "bmp", "svg", "svgz"]).pick_file().map(|f| {
                                let mut it = it;
                                it.kind = Kind::Image { path: self.relative(&f), link: None, frame: None };
                                it
                            })
                        } else {
                            Some(it)
                        };
                        if let Some(it) = dropped {
                            let layer = self.target_layer(&info);
                            if let Some(d) = info.design_mut(&mut self.model.designs) {
                                self.sel_item = Some(insert_item(d, it, layer));
                                self.bump();
                            }
                        }
                    }
                }
            }
        }

        if self.sim.is_some() {
            self.sim_ui(ctx, &info, &resp, &painter, rect, k);
        } else if custom {
            // interactions
            let snapshot: Vec<Item> = info.design(&self.model.designs).map(|d| d.items.clone()).unwrap_or_default();
            // elements des calques masques ou verrouilles : ni selection ni deplacement a la souris
            let usable: Vec<bool> = info.design(&self.model.designs).map(|d| (0..d.items.len()).map(|i| !d.item_hidden(i) && !d.item_locked(i)).collect()).unwrap_or_default();
            let can = |i: usize| usable.get(i).copied().unwrap_or(false);
            let grid = if self.snap { 10 } else { 0 };
            // lignes d'aimantation trouvees pendant ce glissement (dessinees sur l'apercu)
            let mut guides: (Option<i32>, Option<i32>) = (None, None);
            let (r_started, r_dragged, r_stopped, r_clicked, r_secondary) = if skip_input { (false, false, false, false, false) } else { (resp.drag_started(), resp.dragged(), resp.drag_stopped(), resp.clicked(), resp.secondary_clicked()) };
            if r_started {
                if let Some(pos) = resp.interact_pointer_pos() {
                    let (px, py) = to_plane(pos);
                    let prio = if self.extra_sel.is_empty() { self.sel_item } else { None };
                    match hit_test_where(&snapshot, px, py, prio, 12.0 / k, &can) {
                        Some((i, h)) => {
                            // element deja dans une selection multiple : on deplace tout le groupe ; sinon la selection se reduit a lui
                            let in_group = self.sel_item == Some(i) || self.extra_sel.contains(&i);
                            if in_group {
                                if let Some(old) = self.sel_item.filter(|&o| o != i) { if !self.extra_sel.contains(&old) { self.extra_sel.push(old); } }
                                self.extra_sel.retain(|&e| e != i);
                            } else {
                                self.extra_sel.clear();
                            }
                            self.sel_item = Some(i);
                            let group: Vec<(usize, Item)> = if h == Handle::Body { self.extra_sel.iter().filter(|&&e| e < snapshot.len() && can(e)).map(|&e| (e, snapshot[e].clone())).collect() } else { Vec::new() };
                            self.drag = Some(DragState { idx: i, handle: h, start: (px, py), orig: snapshot[i].clone(), group });
                        }
                        None => {
                            // depart dans le vide : rectangle de selection (Maj : s'ajoute a la selection actuelle)
                            let keep = ctx.input(|i| i.modifiers.shift || i.modifiers.command);
                            let base: Vec<usize> = if keep { self.sel_item.into_iter().chain(self.extra_sel.iter().copied()).collect() } else { Vec::new() };
                            if !keep { self.sel_item = None; self.extra_sel.clear(); }
                            self.marquee = Some(((px, py), base));
                        }
                    }
                }
            }
            if r_dragged {
                if let (Some(d), Some(pos)) = (&self.drag, resp.interact_pointer_pos()) {
                    let (px, py) = to_plane(pos);
                    // Maj : redimensionnement proportionnel (le coin oppose reste fixe)
                    let ratio = ctx.input(|i| i.modifiers.shift);
                    let o = (d.orig.x, d.orig.y, d.orig.w, d.orig.h);
                    let mut r = if ratio { dragged_rect_ratio(o, d.handle, px - d.start.0, py - d.start.1, grid, 16) } else { dragged_rect(o, d.handle, px - d.start.0, py - d.start.1, grid, 16) };
                    let orig = d.orig.clone();
                    let idx = d.idx;
                    let group = d.group.clone();
                    // aimantation aux bords et centres de l'ecran et des autres elements visibles (Alt : desactivee)
                    if self.guide_snap && !(ratio && !d.handle.is_side() && d.handle != Handle::Body) && !ctx.input(|i| i.modifiers.alt) {
                        let others = snapshot.iter().enumerate().filter(|(j, _)| *j != idx && !group.iter().any(|g| g.0 == *j) && can(*j)).map(|(_, it)| it);
                        let (xs, ys) = snap_lines(others);
                        let (r2, gx, gy) = snap_item_rect(d.handle, r, &xs, &ys, (6.0 / k).round().max(1.0) as i32, 16);
                        if d.handle == Handle::Body {
                            r = (r2.0.clamp(0, (1920 - r2.2).max(0)), r2.1.clamp(0, (1080 - r2.3).max(0)), r2.2, r2.3);
                        } else {
                            r = r2;
                        }
                        guides = (gx, gy);
                    }
                    let (gdx, gdy) = (r.0 - orig.x, r.1 - orig.y);
                    if let Some(dd) = info.design_mut(&mut self.model.designs) {
                        if let Some(it) = dd.items.get_mut(idx) {
                            set_rect(it, &orig, r);
                            self.dirty = true;
                            self.edit_gen += 1;
                        }
                        for (gi, go) in &group {
                            if let Some(it) = dd.items.get_mut(*gi) {
                                it.x = (go.x + gdx).clamp(0, (1920 - go.w).max(0));
                                it.y = (go.y + gdy).clamp(0, (1080 - go.h).max(0));
                            }
                        }
                    }
                }
            }
            if let (Some((s0, base)), Some(pos)) = (self.marquee.clone(), resp.interact_pointer_pos()) {
                if r_dragged {
                    let (px, py) = to_plane(pos);
                    let (x0, x1, y0, y1) = (s0.0.min(px), s0.0.max(px), s0.1.min(py), s0.1.max(py));
                    let mut sel = base;
                    for (i, it) in snapshot.iter().enumerate() {
                        if can(i) && (it.x as f32) < x1 && ((it.x + it.w) as f32) > x0 && (it.y as f32) < y1 && ((it.y + it.h) as f32) > y0 && !sel.contains(&i) { sel.push(i); }
                    }
                    self.sel_item = sel.pop();
                    self.extra_sel = sel;
                    let r = Rect::from_two_pos(to_screen(x0, y0), to_screen(x1, y1));
                    painter.rect_filled(r, 0.0, Color32::from_rgba_unmultiplied(120, 190, 255, 30));
                    painter.rect_stroke(r, 0.0, Stroke::new(1.0_f32, Color32::from_rgb(120, 190, 255)));
                }
            }
            if r_stopped {
                self.marquee = None;
                self.drag = None;
                self.bump();
            } else if r_clicked {
                if let Some(pos) = resp.interact_pointer_pos() {
                    let (px, py) = to_plane(pos);
                    let hit = hit_test_where(&snapshot, px, py, None, 0.0, &can).map(|h| h.0);
                    if ctx.input(|i| i.modifiers.shift || i.modifiers.command) {
                        // Maj+clic : ajoute ou retire l'element de la selection
                        if let Some(h) = hit {
                            if self.extra_sel.contains(&h) {
                                self.extra_sel.retain(|&e| e != h);
                            } else if self.sel_item == Some(h) {
                                self.sel_item = self.extra_sel.pop();
                            } else if self.sel_item.is_none() {
                                self.sel_item = Some(h);
                            } else {
                                self.extra_sel.push(h);
                            }
                        }
                    } else {
                        self.sel_item = hit;
                        self.extra_sel.clear();
                    }
                }
            }
            // clic droit : selection de l'element sous la souris et menu contextuel
            if r_secondary {
                if let Some(pos) = resp.interact_pointer_pos() {
                    let (px, py) = to_plane(pos);
                    let hit = hit_test_where(&snapshot, px, py, None, 0.0, &can).map(|h| h.0);
                    if hit.map_or(true, |h| self.sel_item != Some(h) && !self.extra_sel.contains(&h)) {
                        self.sel_item = hit;
                        self.extra_sel.clear();
                    }
                    self.drag = None;
                }
            }
            if let Some(s) = self.sel_item.filter(|&s| can(s)) {
                let mut act = None;
                resp.context_menu(|ui| act = item_menu(ui));
                if let (Some(dup), Some(d)) = (act, info.design_mut(&mut self.model.designs)) {
                    self.sel_item = item_edit(d, s, dup);
                    self.bump();
                }
            }
            // clavier
            if self.sel_item.is_some() && !ctx.wants_keyboard_input() {
                let (dx, dy, del) = ctx.input(|i| {
                    let step = if i.modifiers.shift { 10 } else { 1 };
                    let mut dx = 0;
                    let mut dy = 0;
                    if i.key_pressed(Key::ArrowLeft) { dx -= step; }
                    if i.key_pressed(Key::ArrowRight) { dx += step; }
                    if i.key_pressed(Key::ArrowUp) { dy -= step; }
                    if i.key_pressed(Key::ArrowDown) { dy += step; }
                    (dx, dy, i.key_pressed(Key::Delete))
                });
                let mut all: Vec<usize> = self.extra_sel.clone();
                all.extend(self.sel_item);
                if let (Some(s), Some(d)) = (self.sel_item, info.design_mut(&mut self.model.designs)) {
                    if del && s < d.items.len() {
                        all.sort_unstable();
                        all.dedup();
                        for &k in all.iter().rev() { if k < d.items.len() && can(k) { d.items.remove(k); } }
                        d.prune_nav();
                        self.sel_item = None;
                        self.extra_sel.clear();
                        self.bump();
                    } else if dx != 0 || dy != 0 {
                        for &k in &all {
                            if k < d.items.len() && can(k) {
                                let it = &mut d.items[k];
                                it.x = (it.x + dx).clamp(0, (1920 - it.w).max(0));
                                it.y = (it.y + dy).clamp(0, (1080 - it.h).max(0));
                            }
                        }
                        self.bump();
                    }
                }
            }
            // surcouche : cadres des éléments
            if let Some(d) = info.design(&self.model.designs) {
                for (i, it) in d.items.iter().enumerate() {
                    if d.item_hidden(i) { continue; }
                    let r = Rect::from_min_max(to_screen(it.x as f32, it.y as f32), to_screen((it.x + it.w) as f32, (it.y + it.h) as f32));
                    let selected = self.sel_item == Some(i);
                    let col = kind_color(&it.kind);
                    let multi = !self.extra_sel.is_empty() && (selected || self.extra_sel.contains(&i));
                    if multi {
                        // selection multiple : contour en tirets sur chaque element (fond sombre + tirets clairs : visible sur tout decor)
                        let pts = [r.left_top(), r.right_top(), r.right_bottom(), r.left_bottom(), r.left_top()];
                        painter.add(egui::Shape::line(pts.to_vec(), Stroke::new(3.0_f32, Color32::from_black_alpha(170))));
                        painter.extend(egui::Shape::dashed_line(&pts, Stroke::new(2.0_f32, Color32::WHITE), 7.0, 5.0));
                    } else if selected && self.preview_pulse > 0 {
                        // animation en cours dans l'apercu : ni voile ni cadre epais (ils la cacheraient), un contour fin seulement
                        painter.rect_stroke(r, 0.0, Stroke::new(1.0_f32, Color32::from_white_alpha(140)));
                    } else if selected {
                        painter.rect_filled(r, 0.0, Color32::from_rgba_unmultiplied(255, 255, 255, 28));
                        painter.rect_stroke(r, 0.0, Stroke::new(2.5_f32, Color32::WHITE));
                    } else {
                        painter.rect_stroke(r, 0.0, Stroke::new(1.0_f32, col));
                    }
                    if let Kind::Slot { start, .. } = it.kind {
                        let shown = match it.fixed_video() {
                            Some(v) => match start { Some(t) => format!("V{} @{}", v + 1, mmss(t)), None => format!("V{}", v + 1) },
                            None => "?".to_string(),
                        };
                        let shown = if it.link().is_some() { format!("{shown} →") } else { shown };
                        painter.text(r.min + vec2(3.0, 2.0), egui::Align2::LEFT_TOP, shown, egui::FontId::proportional(14.0), col);
                    }
                    if let Kind::Image { frame: Some(f), .. } = it.kind {
                        let shown = match f.video { Some(v) => format!("V{} @{}", v + 1, mmss(f.at)), None => "?".to_string() };
                        painter.text(r.min + vec2(3.0, 2.0), egui::Align2::LEFT_TOP, shown, egui::FontId::proportional(14.0), col);
                    }
                    if selected && !multi {
                        for c in [r.left_top(), r.right_top(), r.left_bottom(), r.right_bottom()] {
                            painter.rect_filled(Rect::from_center_size(c, vec2(9.0, 9.0)), 1.0, Color32::WHITE);
                        }
                        // poignees des cotes (redimensionnement dans un seul sens)
                        for c in [r.center_top(), r.center_bottom(), r.left_center(), r.right_center()] {
                            painter.rect_filled(Rect::from_center_size(c, vec2(7.0, 7.0)), 1.0, Color32::from_gray(225));
                        }
                    }
                }
            }
            // liens de navigation choisis a la main depuis l'element selectionne (fleches colorees comme les noms Haut / Bas / Gauche / Droite)
            if let (Some(s), true) = (self.sel_item, self.extra_sel.is_empty()) {
                if let Some(d) = info.design(&self.model.designs) {
                    if let Some(from) = d.items.get(s) {
                        let c = |it: &Item| to_screen(it.x as f32 + it.w as f32 / 2.0, it.y as f32 + it.h as f32 / 2.0);
                        for dir in 0..4 {
                            if let NavLink::Item(id) = from.nav.get(dir) {
                                if let Some(to) = d.item_by_id(id).and_then(|t| d.items.get(t)) {
                                    let (a, b) = (c(from), c(to));
                                    painter.arrow(a, b - a, Stroke::new(2.5_f32, NAV_COLORS[dir]));
                                }
                            }
                        }
                    }
                }
            }
            if self.drag.is_some() {
                let magenta = Stroke::new(1.0_f32, Color32::from_rgb(255, 80, 200));
                if let Some(gx) = guides.0 { painter.line_segment([to_screen(gx as f32, 0.0), to_screen(gx as f32, 1080.0)], magenta); }
                if let Some(gy) = guides.1 { painter.line_segment([to_screen(0.0, gy as f32), to_screen(1920.0, gy as f32)], magenta); }
            }
            if let Some(pos) = resp.hover_pos() {
                let (px, py) = to_plane(pos);
                // en selection multiple : pas de poignees de redimensionnement
                let prio = if self.extra_sel.is_empty() { self.sel_item } else { None };
                match hit_test_where(&snapshot, px, py, prio, 12.0 / k, &can) {
                    Some((_, Handle::TopLeft)) | Some((_, Handle::BottomRight)) => ctx.set_cursor_icon(egui::CursorIcon::ResizeNwSe),
                    Some((_, Handle::TopRight)) | Some((_, Handle::BottomLeft)) => ctx.set_cursor_icon(egui::CursorIcon::ResizeNeSw),
                    Some((_, Handle::Left)) | Some((_, Handle::Right)) => ctx.set_cursor_icon(egui::CursorIcon::ResizeHorizontal),
                    Some((_, Handle::Top)) | Some((_, Handle::Bottom)) => ctx.set_cursor_icon(egui::CursorIcon::ResizeVertical),
                    Some((_, Handle::Body)) => ctx.set_cursor_icon(egui::CursorIcon::Grab),
                    None => {}
                }
            }
        }

    }

    pub(crate) fn relative(&self, f: &std::path::Path) -> String {
        let base = self.base_dir();
        let s = match (std::fs::canonicalize(f), std::fs::canonicalize(&base)) {
            (Ok(a), Ok(b)) => a.strip_prefix(&b).map(|r| r.to_path_buf()).unwrap_or_else(|_| f.to_path_buf()),
            _ => f.to_path_buf(),
        };
        s.to_string_lossy().replace('\\', "/")
    }

    // ------------------------------------------------------------------ inspecteur (panneau de droite)
    /// Reglages de la page affichee, directement dans l'inspecteur : fond (image ou video), musique, cadence des animations.
    fn page_settings_ui(&mut self, ui: &mut egui::Ui, info: &PageInfo) {
        let mut pick: Option<u8> = None;
        let mut changed = false;
        let note = {
            use bdauthor_core::design::{PageMusic, PageVisual};
            let bg = info.design(&self.model.designs).map(|d| d.bg.clone()).unwrap_or_default();
            let video = match &bg.visual { PageVisual::Video(f, _) => f.clone(), PageVisual::Project => self.model.bg_video.clone(), _ => String::new() };
            let music = match &bg.music { PageMusic::File(f) => f.clone(), PageMusic::Project => self.model.music.clone(), PageMusic::Silent => String::new() };
            if bg.is_default() { None } else { self.bg_note(&video, &music) }
        };
        let Some(d) = info.design_mut(&mut self.model.designs) else { return };
        egui::CollapsingHeader::new(tr("Background and animations of this page")).id_salt("reglages_page").default_open(true).show(ui, |ui| {
            changed |= page_bg_ui(ui, &mut d.bg, &mut pick);
            if let Some(n) = &note { ui.label(egui::RichText::new(n).weak()); }
            ui.separator();
            ui.strong(tr("Button animations"));
            let def = bdauthor_core::design::DEFAULT_ANIM_FPS;
            let cur = if d.anim_fps == 0 { def } else { d.anim_fps };
            egui::ComboBox::from_label(tr("frames per second")).selected_text(cur.to_string()).show_ui(ui, |ui| {
                for f in [4, 6, 8, 12, 25] {
                    if ui.selectable_label(cur == f, f.to_string()).clicked() && cur != f { d.anim_fps = if f == def { 0 } else { f }; changed = true; }
                }
            });
            crate::help(ui, tr("A single frame rate per page (Blu-ray standard), for the animated glow and image animations. Rounded to a fraction of the menu video's frame rate."));
        });
        if let Some(k) = pick {
            let (name, exts): (&str, &[&str]) = match k {
                0 => (tr("Images"), &["png", "jpg", "jpeg", "bmp", "svg", "svgz"]),
                1 => (tr("Videos"), &["mp4", "mkv", "mov", "avi", "m2ts", "mts", "ts", "webm", "wmv", "m4v", "mpg", "mpeg"]),
                _ => (tr("Audio"), &["mp3", "wav", "flac", "m4a", "aac", "ogg", "opus", "ac3", "wma"]),
            };
            if let Some(f) = rfd::FileDialog::new().add_filter(name, exts).pick_file() {
                let rel = self.relative(&f);
                if let Some(d) = info.design_mut(&mut self.model.designs) {
                    use bdauthor_core::design::{PageMusic, PageVisual};
                    match k {
                        0 => d.bg.visual = PageVisual::Image(rel),
                        1 => d.bg.visual = PageVisual::Video(rel, matches!(d.bg.visual, PageVisual::Video(_, true))),
                        _ => d.bg.music = PageMusic::File(rel),
                    }
                    changed = true;
                }
            }
        }
        if changed { self.bump(); }
    }

    pub(crate) fn inspector_ui(&mut self, ui: &mut egui::Ui) {
        if self.sim.is_some() {
            ui.heading(tr("Simulation"));
            ui.label(tr("The menu behaves as on a Blu-ray player:"));
            ui.label(tr("• Arrows: move to the neighboring button (same rule as on the disc)"));
            ui.label(tr("• Enter (or click on a button): confirm; the “activated” state shows for a moment"));
            ui.label(tr("• M (or Home): TOP MENU key, back to the home page"));
            ui.label(tr("• Esc: stop the simulation"));
            crate::help(ui, tr("Video playback is simulated by a black screen; at the end, the menu returns where the disc would return."));
            return;
        }
        let Some(info) = self.current_info() else { return };
        // la page affichee : fond et page d'accueil
        // un element selectionne : seuls ses reglages sont affiches (ceux de la page reviennent quand rien n'est selectionne)
        let widget = info.design(&self.model.designs).is_some() && self.sel_item.is_some();
        if !widget {
            ui.heading(tr("Page"));
            ui.horizontal_wrapped(|ui| {
                let this = Target::Main(info.page);
                let is_home = self.model.designs.home.unwrap_or(Target::Main(0)) == this;
                if is_home {
                    ui.label(egui::RichText::new(tr("🏠 Home page")).strong());
                } else if ui.button(tr("🏠 Set as home page")).on_hover_text(tr("Page displayed when the disc opens and by the TOP MENU key")).clicked() {
                    self.model.designs.home = if this == Target::Main(0) { None } else { Some(this) };
                    self.bump();
                }
            });
            self.page_settings_ui(ui, &info);
            // page affichee sans element selectionne : pas de rubrique « Element »
            return;
        }
        ui.heading(tr("Item"));
        crate::help(ui, tr("Page settings (background, music, animations, home page): click in an empty area of the preview."));
        let Some(i) = self.sel_item else { return };
        // selection multiple (Maj+clic) et alignement : seul l'ensemble ou, pour un element seul, la page sert de reference
        self.extra_sel.retain(|&e| e != i);
        let mut group: Vec<usize> = self.extra_sel.clone();
        group.push(i);
        let mut align: Option<ItemAlign> = None;
        ui.collapsing(if group.len() > 1 { trf!("Alignment ({} items)", group.len()) } else { tr("Alignment and centering").to_string() }, |ui| {
            crate::help(ui, tr("Shift+click on the preview adds or removes an item from the selection; dragging one of them moves the whole group. With several items, alignment is done on their common frame; with one, on the page."));
            ui.horizontal_wrapped(|ui| {
                for (tip, a) in [(tr("Align left"), ItemAlign::Left), (tr("Center horizontally"), ItemAlign::CenterH), (tr("Align right"), ItemAlign::Right),
                                 (tr("Align top"), ItemAlign::Top), (tr("Center vertically"), ItemAlign::CenterV), (tr("Align bottom"), ItemAlign::Bottom)] {
                    if align_button(ui, a, tip) { align = Some(a); }
                }
            });
            ui.horizontal_wrapped(|ui| {
                if ui.button(tr("Center on the page (↔)")).clicked() { align = Some(ItemAlign::PageH); }
                if ui.button(tr("Center on the page (↕)")).clicked() { align = Some(ItemAlign::PageV); }
            });
            if group.len() >= 3 {
                ui.horizontal_wrapped(|ui| {
                    if ui.button(tr("Distribute horizontally")).clicked() { align = Some(ItemAlign::DistributeH); }
                    if ui.button(tr("Distribute vertically")).clicked() { align = Some(ItemAlign::DistributeV); }
                });
            }
            if group.len() > 1 && ui.button(tr("Keep only this item")).clicked() { self.extra_sel.clear(); }
        });
        if let Some(a) = align {
            if let Some(d) = info.design_mut(&mut self.model.designs) {
                if align_items(&mut d.items, &group, a) { self.bump(); }
            }
        }
        // navigation aux fleches de la telecommande : voisins choisis a la main (sinon calcules automatiquement)
        if group.len() == 1 {
            let label = |d: &PageDesign, k: usize| -> String {
                match d.items.get(k) {
                    // widget nomme : son nom seul (suivi de son rang si un autre widget porte le meme nom)
                    Some(it) if !it.name.is_empty() => if d.items.iter().filter(|x| x.name == it.name).count() > 1 { format!("{} ({})", it.name, k + 1) } else { it.name.clone() },
                    Some(it) => match it.fixed_video() {
                        Some(v) if matches!(it.kind, Kind::Slot { .. }) => trf!("{}. {} (video {})", k + 1, kind_name(&it.kind), v + 1),
                        _ => format!("{}. {}", k + 1, kind_name(&it.kind)),
                    },
                    None => tr("(deleted)").to_string(),
                }
            };
            // boutons du disque (hors l'element lui-meme) et reglage actuel
            let state = info.design(&self.model.designs).map(|d| {
                let cands: Vec<(usize, String)> = sim_buttons(&info, d, &self.model.designs.style, self.model.videos.len()).iter().filter(|b| b.item != i).map(|b| (b.item, label(d, b.item))).collect();
                let cur = d.items.get(i).map(|it| it.nav).unwrap_or_default();
                let shown: Vec<String> = (0..4).map(|k| match cur.get(k) {
                    NavLink::Auto => tr("Automatic").to_string(),
                    NavLink::Stay => tr("None (stays in place)").to_string(),
                    NavLink::Item(id) => label(d, d.item_by_id(id).unwrap_or(usize::MAX)),
                }).collect();
                (cands, shown, sim_buttons(&info, d, &self.model.designs.style, self.model.videos.len()).iter().any(|b| b.item == i))
            });
            if let Some((cands, shown, is_button)) = state {
                // (direction, choix) : Err(None) automatique, Err(Some(None)) aucun, Ok(rang de l'element)
                let mut pick: Option<(usize, Result<usize, Option<()>>)> = None;
                ui.collapsing(tr("Navigation (remote control arrows)"), |ui| {
                    if !is_button {
                        crate::help(ui, tr("This item is not a disc button: navigation does not apply."));
                        return;
                    }
                    crate::help(ui, tr("By default, each arrow goes to the nearest button in its direction. Choose another button here, or “None” so the arrow moves nothing."));
                    for (d, name) in [tr("↑ Up"), tr("↓ Down"), tr("← Left"), tr("→ Right")].into_iter().enumerate() {
                        ui.horizontal(|ui| {
                            ui.label(egui::RichText::new(name).strong().color(NAV_COLORS[d]));
                            egui::ComboBox::from_id_salt(("nav_fleche", d)).selected_text(shown[d].clone()).width(230.0).show_ui(ui, |ui| {
                                if ui.selectable_label(shown[d] == tr("Automatic"), tr("Automatic")).clicked() { pick = Some((d, Err(None))); }
                                if ui.selectable_label(shown[d] == tr("None (stays in place)"), tr("None (stays in place)")).clicked() { pick = Some((d, Err(Some(())))); }
                                for (k, l) in &cands {
                                    if ui.selectable_label(shown[d] == *l, l).clicked() { pick = Some((d, Ok(*k))); }
                                }
                            });
                        });
                    }
                });
                if let Some((dir, what)) = pick {
                    if let Some(d) = info.design_mut(&mut self.model.designs) {
                        let link = match what {
                            Err(None) => NavLink::Auto,
                            Err(Some(())) => NavLink::Stay,
                            Ok(k) if k < d.items.len() => NavLink::Item(d.ensure_id(k)),
                            Ok(_) => NavLink::Auto,
                        };
                        if let Some(it) = d.items.get_mut(i) { it.nav.set(dir, link); }
                        self.bump();
                    }
                }
            }
        }
        let mut pick_image = false;
        let mut changed = false;
        let mut pick_font = false;
        let base_dir = self.base_dir();
        let font_check = self.renderer.as_ref();
        let page_targets = self.page_targets();
        let page_names: Vec<String> = self.pages().iter().map(|p| p.label(&self.model)).collect();
        let video_durations: Vec<Option<f64>> = self.model.videos.iter().map(|v| self.durations.get(&v.file).copied()).collect();
        // langues de sous-titres du projet (le bouton choisit une langue, pas un numero de piste)
        let sub_langs: Vec<(u32, String)> = self.model.languages.iter().map(|l| (l.id, l.name.clone())).collect();
        let video_labels: Vec<String> = (0..self.model.videos.len()).map(|i| video_title(&self.model, i)).collect();
        let video_durs: Vec<Option<f64>> = self.model.videos.iter().map(|v| self.durations.get(&v.file).copied()).collect();
        let proj_style = self.model.designs.style.clone();
        // chapitres de chaque video (identifiant, instant, titre), par instant croissant ; videos aux chapitres automatiques
        let video_chapters: Vec<Vec<(u32, f64, String)>> = self.model.videos.iter().map(|v| {
            let mut c: Vec<(u32, f64, String)> = v.chapters.iter().map(|c| (c.id, c.t, c.title.clone())).collect();
            c.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal));
            c
        }).collect();
        let video_auto: Vec<bool> = self.model.videos.iter().map(|v| v.chapters.is_empty() && v.auto_s.is_some()).collect();
        // « Creer le chapitre » : (video, instant, titre), traite apres l'edition de la vignette
        let mut new_chapter: Option<(usize, f64, String)> = None;
        let mut pick_skin: Option<usize> = None;
        let mut pick_sound_el: Option<u8> = None;
        let mut anim_preview = self.anim_preview;
        let page_fps = info.design(&self.model.designs).map_or(0, |d| d.anim_fps);
        let page_fps = if page_fps == 0 { bdauthor_core::design::DEFAULT_ANIM_FPS } else { page_fps };
        {
            let Some(d) = info.design_mut(&mut self.model.designs) else { return };
            let Some(it) = d.items.get_mut(i) else { self.sel_item = None; return };
            ui.colored_label(kind_color(&it.kind), kind_name(&it.kind));
            ui.horizontal(|ui| {
                ui.label(tr("Name"));
                changed |= ui.add(egui::TextEdit::singleline(&mut it.name).hint_text(tr("(optional: to find your way around)")).desired_width(220.0)).changed();
            });
            egui::Grid::new("geometrie").num_columns(4).show(ui, |ui| {
                ui.label(tr("X")); changed |= ui.add(egui::DragValue::new(&mut it.x).range(0..=1920)).changed();
                ui.label(tr("Y")); changed |= ui.add(egui::DragValue::new(&mut it.y).range(0..=1080)).changed();
                ui.end_row();
                ui.label(tr("Width")); changed |= ui.add(egui::DragValue::new(&mut it.w).range(16..=1920)).changed();
                ui.label(tr("Height")); changed |= ui.add(egui::DragValue::new(&mut it.h).range(16..=1080)).changed();
                ui.end_row();
            });
            let h = it.h;
            match &mut it.kind {
                Kind::Slot { thumb_h, video, link, start, chapter, label, thumb_at, ret } => {
                    let mut lab = (h - *thumb_h).max(0);
                    ui.horizontal(|ui| {
                        ui.label(tr("Label height:"));
                        if ui.add(egui::DragValue::new(&mut lab).range(0..=(h - 8).max(0))).changed() {
                            *thumb_h = (h - lab).max(8);
                            changed = true;
                        }
                    });
                    // video montree
                    let vname = |v: usize| trf!("Video {}. {}", v + 1, video_labels.get(v).cloned().unwrap_or_default());
                    let old_video = *video;
                    let shown = match *video { Some(v) => vname(v), None => tr("(no video: to choose)").to_string() };
                    egui::ComboBox::from_label(tr("Video")).selected_text(shown).show_ui(ui, |ui| {
                        for v in 0..video_labels.len() {
                            if ui.selectable_label(*video == Some(v), vname(v)).clicked() {
                                // les identifiants de chapitre sont propres a chaque video : le lien ne suit pas
                                if *video != Some(v) { *chapter = None; }
                                *video = Some(v);
                                changed = true;
                            }
                        }
                    });
                    if let (Some(o), Some(nv)) = (old_video, *video) {
                        if o != nv && *link == Some(Target::Video(o)) { *link = Some(Target::Video(nv)); }
                    }
                    let dur = (*video).and_then(|v| video_durations.get(v).copied().flatten());
                    // depart : debut de la video, ou un chapitre de la video (liste de la video) ou un instant libre
                    let vch: &[(u32, f64, String)] = (*video).and_then(|v| video_chapters.get(v)).map_or(&[] as &[(u32, f64, String)], |c| c.as_slice());
                    let chap_text = |t: f64, title: &str| if title.is_empty() { mmss(t) } else { format!("{title} ({})", mmss(t)) };
                    let mut is_chapter = start.is_some();
                    ui.horizontal(|ui| {
                        if ui.radio_value(&mut is_chapter, false, tr("From the start")).changed() { *start = None; *chapter = None; changed = true; }
                        if ui.radio_value(&mut is_chapter, true, tr("Chapter")).changed() { *start = Some(0.0); *chapter = None; changed = true; }
                    });
                    if let Some(t) = start {
                        let cur = chapter.and_then(|id| vch.iter().find(|c| c.0 == id));
                        let shown = cur.map_or_else(|| tr("Free time (typed by hand)").to_string(), |c| chap_text(c.1, &c.2));
                        let prev_title = cur.map(|c| c.2.clone());
                        egui::ComboBox::from_label(tr("Video chapter")).selected_text(shown).show_ui(ui, |ui| {
                            if ui.selectable_label(chapter.is_none(), tr("Free time (typed by hand)")).clicked() && chapter.is_some() { *chapter = None; changed = true; }
                            for (id, ct, title) in vch {
                                if ui.selectable_label(*chapter == Some(*id), chap_text(*ct, title)).clicked() && *chapter != Some(*id) {
                                    *chapter = Some(*id);
                                    *t = *ct;
                                    // le libelle suit le nom du chapitre tant qu'il n'a pas ete saisi a la main
                                    if label.is_empty() || Some(label.as_str()) == prev_title.as_deref() { *label = title.clone(); }
                                    changed = true;
                                }
                            }
                        });
                        if vch.is_empty() && video.is_some() { ui.label(egui::RichText::new(tr("This video has no chapter yet: “Create the chapter” adds one at the time below.")).weak()); }
                        if chapter.is_some() {
                            ui.horizontal(|ui| {
                                ui.label(trf!("at {}", mmss(*t)));
                                if let Some(d) = dur { ui.label(egui::RichText::new(trf!("on {}", mmss(d))).weak()); }
                                if ui.small_button(tr("Detach")).on_hover_text(tr("The thumbnail keeps this time but no longer follows the chapter.")).clicked() { *chapter = None; changed = true; }
                            });
                            crate::help(ui, tr("The time is set in “Selected video”: all thumbnails linked to this chapter follow it."));
                        } else {
                            ui.horizontal(|ui| {
                                let max = dur.unwrap_or(f64::MAX);
                                changed |= ui.add(egui::DragValue::new(t).range(0.0..=max).speed(0.5)
                                    .custom_formatter(|v, _| mmss(v)).custom_parser(parse_mmss)).on_hover_text(tr("Drag, or type “m:ss” (e.g. 12:30) or seconds")).changed();
                                if let Some(d) = dur { ui.label(egui::RichText::new(trf!("on {}", mmss(d))).weak()); }
                                if ui.add_enabled(video.is_some(), egui::Button::new(tr("Create the chapter"))).on_hover_text(tr("Adds a chapter at this time to the video's chapter list (or reuses the one already there) and links the thumbnail to it.")).clicked() {
                                    if let Some(v) = *video { new_chapter = Some((v, *t, label.trim().to_string())); }
                                }
                            });
                            if let Some(v) = *video { if video_auto.get(v).copied().unwrap_or(false) {
                                crate::help(ui, tr("This video has automatic chapters: creating one by hand replaces them."));
                            } }
                        }
                        if dur.map_or(false, |d| *t >= d) { ui.colored_label(Color32::from_rgb(230, 100, 90), tr("After the end of the video.")); }
                        crate::help(ui, tr("The player starts at the nearest key frame before this time (at most 1 s earlier)."));
                    }
                    // libelle et image
                    ui.horizontal(|ui| {
                        ui.label(tr("Label:"));
                        let hint = match start { Some(t) => mmss(*t), None => (*video).and_then(|v| video_labels.get(v).cloned()).unwrap_or_default() };
                        changed |= ui.add(egui::TextEdit::singleline(label).hint_text(hint)).changed();
                    });
                    let mut own_img = thumb_at.is_some();
                    if ui.checkbox(&mut own_img, tr("Thumbnail image taken at another time")).changed() {
                        *thumb_at = if own_img { Some(start.unwrap_or(0.0)) } else { None };
                        changed = true;
                    }
                    if let Some(a) = thumb_at {
                        changed |= ui.add(egui::DragValue::new(a).range(0.0..=dur.unwrap_or(f64::MAX)).speed(0.5).custom_formatter(|v, _| mmss(v)).custom_parser(parse_mmss)).changed();
                    }
                    // au clic : lecture (habituel) ou autre destination
                    let habit = if start.is_some() { tr("Play the video from this chapter") } else { tr("Play the video") };
                    let shown = link.and_then(|t| page_targets.iter().find(|(x, _)| *x == t.without_exit()).map(|(_, l)| l.clone())).unwrap_or_else(|| if link.is_some() { tr("(destination not found)").into() } else { habit.to_string() });
                    egui::ComboBox::from_label(tr("On click")).selected_text(shown).show_ui(ui, |ui| {
                        if ui.selectable_label(link.is_none(), habit).clicked() { *link = None; changed = true; }
                        for (t, l) in &page_targets {
                            if ui.selectable_label(link.map(Target::without_exit) == Some(*t), l).clicked() && link.map(Target::without_exit) != Some(*t) { *link = Some(*t); changed = true; }
                        }
                    });
                    // apres la lecture
                    if matches!(link, None | Some(Target::Video(_))) {
                        let rname = |r: SlotReturn| match r {
                            SlotReturn::Origin => tr("Return to this page").to_string(),
                            SlotReturn::Home => tr("Go to the home page").to_string(),
                            SlotReturn::Page(p) => trf!("Go to page {}", page_names.get(p).cloned().unwrap_or_else(|| trf!("{} (not found)", p + 1))),
                        };
                        egui::ComboBox::from_label(tr("After playback")).selected_text(rname(*ret)).show_ui(ui, |ui| {
                            for r in [SlotReturn::Origin, SlotReturn::Home].into_iter().chain((0..page_names.len()).map(SlotReturn::Page)) {
                                if ui.selectable_label(*ret == r, rname(r)).clicked() { *ret = r; changed = true; }
                            }
                        });
                    }
                }
                Kind::Nav { action, text, target } => {
                    // une seule liste « Destination » : les actions speciales (page precedente/suivante, tout lire),
                    // puis les pages, les videos et le reglage des sous-titres
                    let old = *action;
                    let shown = if *action != NavAction::Goto { action.label().to_string() } else {
                        target.and_then(|t| page_targets.iter().find(|(x, _)| *x == t.without_exit()).map(|(_, l)| l.clone())).unwrap_or_else(|| tr("(destination not found: to choose)").to_string())
                    };
                    egui::ComboBox::from_label(tr("Destination")).selected_text(shown).show_ui(ui, |ui| {
                        for a in NavAction::ALL {
                            if a == NavAction::Goto { continue; }
                            if ui.selectable_label(*action == a, a.label()).clicked() {
                                changed = true;
                                // texte encore par defaut : il suit l'action
                                if *text == old.default_text() { *text = a.default_text().to_string(); }
                                *action = a;
                            }
                        }
                        ui.separator();
                        for (t, l) in &page_targets {
                            let same = *action == NavAction::Goto && target.map(Target::without_exit) == Some(*t);
                            if ui.selectable_label(same, l).clicked() && !same {
                                changed = true;
                                if *text == old.default_text() { *text = NavAction::Goto.default_text().to_string(); }
                                *action = NavAction::Goto;
                                *target = Some(*t);
                            }
                        }
                    });
                    ui.horizontal(|ui| { ui.label(tr("Text:")); changed |= ui.text_edit_singleline(text).changed(); });
                }
                Kind::Text { text, size, color, align, font, link, deco } => {
                    link_ui(ui, link, &page_targets, &mut changed);
                    changed |= ui.add(egui::TextEdit::multiline(text).desired_rows(3).desired_width(f32::INFINITY)).changed();
                    ui.horizontal(|ui| {
                        ui.label(tr("Size:"));
                        changed |= ui.add(egui::DragValue::new(size).range(8..=400).suffix(" px")).changed();
                        ui.label(tr("Color:"));
                        changed |= ui.color_edit_button_srgb(color).changed();
                    });
                    let shown = if font.is_empty() { tr("Default font").to_string() } else {
                        bdauthor_render::common_fonts().into_iter().find(|(_, f)| f == font).map(|(l, _)| l.to_string()).unwrap_or_else(|| font.clone())
                    };
                    egui::ComboBox::from_label(tr("Font")).selected_text(shown).show_ui(ui, |ui| {
                        if ui.selectable_label(font.is_empty(), tr("Default font")).clicked() { font.clear(); changed = true; }
                        for (label, file) in bdauthor_render::common_fonts() {
                            if ui.selectable_label(font == file, label).clicked() { *font = file.to_string(); changed = true; }
                        }
                        if ui.button(tr("Other file…")).clicked() { pick_font = true; ui.close_menu(); }
                    });
                    if !font.is_empty() {
                        if let Ok(r) = font_check {
                            if r.custom_font(font, &base_dir).is_none() {
                                ui.colored_label(Color32::from_rgb(230, 100, 90), tr("Font not found: the default font will be used."));
                            }
                        }
                    }
                    egui::ComboBox::from_label(tr("Alignment")).selected_text(align.name()).show_ui(ui, |ui| {
                        for a in [Align::Left, Align::Center, Align::Right] {
                            if ui.selectable_value(align, a, a.name()).changed() { changed = true; }
                        }
                    });
                    ui.horizontal(|ui| {
                        ui.label(tr("Background:"));
                        // choisir une couleur rend le fond visible s'il etait transparent
                        if ui.color_edit_button_srgb(&mut deco.bg_color).changed() { if deco.bg_opacity == 0 { deco.bg_opacity = 60; } changed = true; }
                        ui.label(tr("Opacity:"));
                        changed |= ui.add(egui::DragValue::new(&mut deco.bg_opacity).range(0..=100).suffix(" %")).changed();
                        ui.label(tr("Corners:"));
                        changed |= ui.add(egui::DragValue::new(&mut deco.bg_radius).range(0..=200).suffix(" px")).changed();
                    });
                    ui.horizontal(|ui| {
                        ui.label(tr("Text outline:"));
                        changed |= ui.add(egui::DragValue::new(&mut deco.outline).range(0..=30).suffix(" px")).changed();
                        ui.label(tr("Color:"));
                        if ui.color_edit_button_srgb(&mut deco.outline_color).changed() { if deco.outline == 0 { deco.outline = 3; } changed = true; }
                        crate::help(ui, tr("Background: covers the whole frame of the item; opacity 0 % = no background. Outline: thickness of the border drawn around the letters, 0 = none. Sizes given for a 1920 × 1080 menu."));
                    });
                }
                Kind::Image { frame: Some(f), link, .. } => {
                    link_ui(ui, link, &page_targets, &mut changed);
                    let vname = |v: usize| trf!("Video {}. {}", v + 1, video_labels.get(v).cloned().unwrap_or_default());
                    let shown = match f.video { Some(v) => vname(v), None => tr("(no video: to choose)").to_string() };
                    egui::ComboBox::from_label(tr("Video")).selected_text(shown).show_ui(ui, |ui| {
                        for v in 0..video_labels.len() {
                            if ui.selectable_label(f.video == Some(v), vname(v)).clicked() { f.video = Some(v); changed = true; }
                        }
                    });
                    let max_t = f.video.and_then(|v| video_durs.get(v).copied().flatten()).unwrap_or(1.0e6);
                    ui.horizontal(|ui| {
                        ui.label(tr("Time:"));
                        changed |= ui.add(egui::DragValue::new(&mut f.at).speed(0.2).range(0.0..=max_t).suffix(" s")).changed();
                        ui.label(egui::RichText::new(mmss(f.at)).weak());
                        if max_t < 1.0e6 { ui.label(egui::RichText::new(format!("/ {}", mmss(max_t))).weak()); }
                    });
                    ui.horizontal(|ui| {
                        if ui.add_enabled(f.video.is_some(), egui::Button::new(tr("Choose the image…"))).on_hover_text(tr("Opens a window with the video image by image and a timeline, to choose the exact image and crop it")).clicked() { pick_frame = true; }
                        ui.label(egui::RichText::new(if f.crop.is_some() { tr("cropped by hand") } else { tr("center of the image in 16:9") }).weak());
                    });
                    crate::help(ui, tr("A frame of the video at this time, fitted into the frame without distortion. “Choose the image…” lets you pick it image by image and crop it."));
                }
                Kind::Image { path, link, .. } => {
                    link_ui(ui, link, &page_targets, &mut changed);
                    ui.horizontal(|ui| { ui.label(tr("File:")); changed |= ui.text_edit_singleline(path).changed(); });
                    if ui.button(tr("Choose…")).clicked() { pick_image = true; }
                    crate::help(ui, tr("The image is fitted into the frame without distortion."));
                }
            }
            // actions « Regler les sous-titres » / « Regler la langue audio » : destination de reglage + langue, puis page de
            // sortie (ou rester sur la page) ; aucune autre destination ne touche aux sous-titres ni a l'audio
            {
                use bdauthor_core::design::SubChoice;
                let cur = it.targets_mut().and_then(|t| *t);
                let stay = matches!(cur, Some(Target::Stay(_)));
                if !stay {
                    if it.sub != SubChoice::Keep { it.sub = SubChoice::Keep; changed = true; }
                } else {
                    if it.sub == SubChoice::Keep {
                        it.sub = sub_langs.first().map_or(SubChoice::Off, |(id, _)| SubChoice::Lang(*id));
                        changed = true;
                    }
                    let name = |c: SubChoice| match c {
                        SubChoice::Keep | SubChoice::Off => tr("Hide subtitles").to_string(),
                        SubChoice::Lang(id) => sub_langs.iter().find(|(i, _)| *i == id).map_or_else(|| tr("(language removed: to choose)").to_string(), |(_, n)| n.clone()),
                    };
                    let cur_sub = it.sub;
                    egui::ComboBox::from_label(tr("Subtitles")).selected_text(name(cur_sub)).show_ui(ui, |ui| {
                        for c in std::iter::once(SubChoice::Off).chain(sub_langs.iter().map(|(id, _)| SubChoice::Lang(*id))) {
                            if ui.selectable_label(cur_sub == c, name(c)).clicked() { it.sub = c; changed = true; }
                        }
                    });
                }
                if let Some(t) = cur.filter(|t| t.is_setting()) {
                    // page de sortie : les pages du menu seulement
                    let pages: Vec<(usize, &String)> = page_targets.iter().filter_map(|(x, l)| if let Target::Main(q) = x { Some((*q, l)) } else { None }).collect();
                    let here = tr("Stay on the current page");
                    let exit = t.exit();
                    let shown = exit.map_or_else(|| here.to_string(), |q| pages.iter().find(|(p, _)| *p == q).map_or_else(|| tr("(page not found: to choose)").to_string(), |(_, l)| (*l).clone()));
                    let mut pick: Option<Option<usize>> = None;
                    egui::ComboBox::from_label(tr("Then")).selected_text(shown).show_ui(ui, |ui| {
                        if ui.selectable_label(exit.is_none(), here).clicked() { pick = Some(None); }
                        for (q, l) in &pages {
                            if ui.selectable_label(exit == Some(*q), l.as_str()).clicked() { pick = Some(Some(*q)); }
                        }
                    });
                    if let Some(e) = pick {
                        if let Some(slot) = it.targets_mut() { *slot = Some(t.with_exit(e)); changed = true; }
                    }
                    let after = if exit.is_some() { tr("then the menu moves to the chosen page") } else { tr("and the menu stays displayed") };
                    let txt = match t {
                        Target::Audio(..) => trf!("The click sets the audio language {}. It applies to all videos played afterwards; a video that lacks this language keeps its current audio. The viewer can still change it with the AUDIO key. Subtitles are not changed.", after),
                        _ if sub_langs.is_empty() => tr("No subtitle language in the project (add some in the menu “Edit > Project…”, tab “Subtitle languages”).").to_string(),
                        _ => trf!("The click sets the subtitles {}. The language applies to all videos played afterwards; a video that lacks this language is displayed without subtitles. The viewer can still change it with the SUBTITLE key.", after),
                    };
                    ui.label(egui::RichText::new(txt).weak());
                }
            }
            // apparence propre a ce bouton
            if it.is_button() {
                ui.separator();
                let mut own = it.look.style.is_some();
                if ui.checkbox(&mut own, tr("Appearance specific to this item")).on_hover_text(tr("Otherwise: the project's button style")).changed() {
                    it.look.style = if own { Some(proj_style.clone()) } else { None };
                    changed = true;
                }
                if let Some(st) = &mut it.look.style {
                    egui::CollapsingHeader::new(tr("Settings of this item")).default_open(true).show(ui, |ui| {
                        changed |= crate::style_ui::style_ui(ui, st, "element", &mut pick_sound_el);
                    });
                }
                if !matches!(it.kind, Kind::Slot { .. }) {
                    changed |= crate::style_ui::skin_ui(ui, &mut it.look.skin, &mut pick_skin, &mut anim_preview, page_fps);
                }
                // sons : independants de l'apparence
                ui.separator();
                let mut own_snd = it.look.sounds.is_some();
                if ui.checkbox(&mut own_snd, tr("Sounds specific to this item")).on_hover_text(tr("Otherwise: the sounds of the project's button style")).changed() {
                    it.look.sounds = if own_snd {
                        Some(bdauthor_core::style::ItemSounds { select: proj_style.sound_select.clone(), activate: proj_style.sound_activate.clone() })
                    } else { None };
                    changed = true;
                }
                if let Some(snd) = &mut it.look.sounds {
                    changed |= crate::style_ui::sounds_ui(ui, &mut snd.select, &mut snd.activate, &mut pick_sound_el);
                }
            }
        }
        if pick_font {
            if let Some(f) = rfd::FileDialog::new().add_filter(tr("Fonts"), &["ttf", "otf"]).pick_file() {
                let rel = self.relative(&f);
                if let Some(Kind::Text { font, .. }) = info.design_mut(&mut self.model.designs).and_then(|d| d.items.get_mut(i)).map(|it| &mut it.kind) {
                    *font = rel;
                    changed = true;
                }
            }
        }
        self.anim_preview = anim_preview;
        if let Some(k) = pick_sound_el {
            if let Some(f) = rfd::FileDialog::new().add_filter(tr("Sounds"), &crate::style_ui::SOUND_EXTS).pick_file() {
                let rel = self.relative(&f);
                if let Some(snd) = info.design_mut(&mut self.model.designs).and_then(|d| d.items.get_mut(i)).and_then(|it| it.look.sounds.as_mut()) {
                    if k == 1 { snd.select = rel; } else { snd.activate = rel; }
                    changed = true;
                }
            }
        }
        if let Some(state @ (0 | 1)) = pick_skin {
            // image(s) « normal » ou « selectionne » : une = etat fixe, plusieurs = animation (rangees par ordre de nom)
            if let Some(mut fs) = rfd::FileDialog::new().add_filter(tr("Images"), &["png", "jpg", "jpeg", "bmp", "svg", "svgz"]).pick_files() {
                fs.sort();
                let rel: Vec<String> = fs.iter().map(|f| self.relative(f)).collect();
                if let Some(it) = info.design_mut(&mut self.model.designs).and_then(|d| d.items.get_mut(i)) {
                    if state == 0 { it.look.skin.set_normal(rel); } else { it.look.skin.set_selected(rel); }
                    if it.look.skin.sel_frames.is_empty() && it.look.skin.normal_frames.is_empty() { self.anim_preview = false; }
                    changed = true;
                }
            }
        } else if let Some(state) = pick_skin {
            if let Some(f) = rfd::FileDialog::new().add_filter(tr("Images"), &["png", "jpg", "jpeg", "bmp", "svg", "svgz"]).pick_file() {
                let rel = self.relative(&f);
                if let Some(it) = info.design_mut(&mut self.model.designs).and_then(|d| d.items.get_mut(i)) {
                    let sk = &mut it.look.skin;
                    *[&mut sk.normal, &mut sk.selected, &mut sk.activated][state.min(2)] = rel;
                    changed = true;
                }
            }
        }
        if pick_image {
            if let Some(f) = rfd::FileDialog::new().add_filter(tr("Images"), &["png", "jpg", "jpeg", "bmp", "svg", "svgz"]).pick_file() {
                let rel = self.relative(&f);
                if let Some(Kind::Image { path, .. }) = info.design_mut(&mut self.model.designs).and_then(|d| d.items.get_mut(i)).map(|it| &mut it.kind) {
                    *path = rel;
                    changed = true;
                }
            }
        }
        if let Some((v, t, title)) = new_chapter {
            if let Some(ve) = self.model.videos.get_mut(v) {
                let id = ve.add_chapter(t, &title);
                if let Some(Kind::Slot { chapter, .. }) = info.design_mut(&mut self.model.designs).and_then(|d| d.items.get_mut(i)).map(|it| &mut it.kind) { *chapter = Some(id); }
                changed = true;
            }
        }
        if pick_frame { self.frame_request = Some((info.r.0, i)); }
        if changed {
            self.bump();
        }
    }

    /// Calque des nouveaux elements : le calque actif s'il existe sur la page, sinon celui de l'element selectionne, sinon le
    /// premier plan.
    fn target_layer(&self, info: &PageInfo) -> u32 {
        let Some(d) = info.design(&self.model.designs) else { return 0 };
        if d.layers.iter().any(|l| l.id == self.active_layer) { return self.active_layer; }
        if let Some(it) = self.sel_item.and_then(|s| d.items.get(s)) { if d.layers.iter().any(|l| l.id == it.layer) { return it.layer; } }
        d.layers.last().map_or(0, |l| l.id)
    }

    // ------------------------------------------------------------------ arborescence pages / calques / elements (panneau de gauche)
    /// Page depliee dans l'arborescence ? (repliee tant qu'on ne l'a pas depliee avec +, meme affichee)
    fn page_open(&self, k: usize) -> bool {
        self.tree_pages.get(&k).copied().unwrap_or(false)
    }

    /// Pages du menu, et sous chaque page depliee ses calques et ses elements (du premier plan, en haut, a l'arriere-plan).
    /// La poignee ☰ de chaque ligne se glisse : pages reordonnees ; elements et calques reordonnes, changes de calque, ou
    /// envoyes sur une autre page en les deposant sur elle (Ctrl : copie). + / − deplie ou replie une page ou un calque.
    pub(crate) fn page_list_ui(&mut self, ui: &mut egui::Ui) {
        ui.heading(tr("Pages and layers"));
        if self.model.videos.is_empty() {
            ui.label(tr("Add videos in the “Videos” tab."));
            return;
        }
        let mut op: Option<LayerOp> = None;
        let info = self.current_info();
        let cur_k = match info.as_ref().map(|i| i.r) { Some(PageRef(k)) => Some(k), None => None };
        ui.horizontal_wrapped(|ui| {
            if ui.button(tr("+ Page")).on_hover_text(tr("Adds a blank page at the end")).clicked() {
                let f = bdauthor_core::editor::add_free_page(&mut self.model);
                self.cur = Some(PageRef(f));
                self.sel_item = None;
                self.bump();
            }
            if cur_k.is_some() && ui.button(tr("+ Layer")).on_hover_text(tr("New layer in the foreground of the displayed page; new items go into the active layer")).clicked() {
                op = Some(LayerOp::Add);
            }
            if ui.small_button(tr("+ all")).on_hover_text(tr("Expand all pages and layers")).clicked() {
                for k in 0..self.model.designs.free.len() { self.tree_pages.insert(k, true); }
                self.tree_layers_closed.clear();
            }
            if ui.small_button(tr("− all")).on_hover_text(tr("Collapse all pages")).clicked() {
                for k in 0..self.model.designs.free.len() { self.tree_pages.insert(k, false); }
            }
        });
        let np = self.model.designs.free.len();
        if np == 0 {
            ui.label(tr("No page: add one."));
            return;
        }
        let home_k = match self.model.designs.home { Some(Target::Main(p)) if p < np => p, _ => 0 };
        let names: Vec<String> = self.model.designs.free.iter().enumerate().map(|(k, d)| if d.name.is_empty() { trf!("Page {}", k + 1) } else { d.name.clone() }).collect();
        let all: Vec<PageDesign> = self.model.designs.free.clone();
        let active = info.as_ref().map_or(0, |i| self.target_layer(i));
        let opened: Vec<bool> = (0..np).map(|k| self.page_open(k)).collect();
        let mut toggle_page: Option<usize> = None;
        let mut toggle_layer: Option<(usize, u32)> = None;
        let mut pick_page: Option<usize> = None;
        let mut pick: Option<(usize, usize)> = None;
        let mut activate: Option<(usize, u32)> = None;
        let mut edit_done = false;
        let mut page_edit_done = false;
        let mut item_act: Option<(usize, usize, bool)> = None;
        let mut rows: Vec<(DragRow, Rect)> = Vec::new();
        let pm = |open: bool| if open { "−" } else { "+" };

        egui::ScrollArea::vertical().id_salt("defil_pages").auto_shrink([false, true]).max_height(ui.available_height().max(80.0)).show(ui, |ui| {
            let list = egui::Frame::group(ui.style()).inner_margin(4.0).show(ui, |ui| {
                ui.set_min_width(ui.available_width());
                for k in 0..np {
                    // ---- ligne de page
                    let shown = cur_k == Some(k);
                    let r = ui.horizontal(|ui| {
                        let _ = ui.dnd_drag_source(egui::Id::new(("page", k)), DragRow::Page(k), |ui| ui.label(egui::RichText::new("☰").strong()))
                            .response.on_hover_text(tr("Drag to change the order of the pages"));
                        if ui.small_button(pm(opened[k])).on_hover_text(if opened[k] { tr("Collapse the page") } else { tr("Expand the page") }).clicked() { toggle_page = Some(k); }
                        let editing = matches!(&self.page_edit, Some((e, _)) if *e == k);
                        if editing {
                            if let Some((_, text)) = &mut self.page_edit {
                                let te = ui.add(egui::TextEdit::singleline(text).desired_width(150.0));
                                if !te.has_focus() && !te.lost_focus() { te.request_focus(); }
                                if te.lost_focus() { page_edit_done = true; }
                            }
                        } else {
                            let home = if k == home_k { "  🏠" } else { "" };
                            let label = egui::RichText::new(format!("{}. {}{home}", k + 1, names[k])).strong();
                            let resp = ui.selectable_label(shown, label).on_hover_text(tr("Show the page (+ to expand its content). Double-click: rename. 🏠: home page"));
                            if resp.clicked() { pick_page = Some(k); }
                            if resp.double_clicked() { self.page_edit = Some((k, all[k].name.clone())); }
                            if ui.small_button("✏").on_hover_text(tr("Rename the page")).clicked() { self.page_edit = Some((k, all[k].name.clone())); }
                            let _ = ui.menu_button("🗑", |ui| {
                                ui.label(tr("Links that led to this page will have to be chosen again."));
                                if ui.button(tr("Delete the page")).clicked() { op = Some(LayerOp::PageDelete(k)); ui.close_menu(); }
                            }).response.on_hover_text(tr("Delete the page"));
                        }
                    }).response;
                    rows.push((DragRow::Page(k), r.rect));
                    if !opened[k] { continue; }
                    // ---- calques et elements de la page
                    let d = &all[k];
                    let layered = !d.layers.is_empty();
                    let positions: Vec<usize> = if layered { (0..d.layers.len()).rev().collect() } else { vec![0] };
                    if d.items.is_empty() && !layered {
                        ui.horizontal(|ui| { ui.add_space(40.0); ui.label(egui::RichText::new(tr("(empty page)")).weak().italics()); });
                    }
                    for pos in positions {
                        let mut layer_open = true;
                        if layered {
                            let l = d.layers[pos].clone();
                            layer_open = !self.tree_layers_closed.contains(&(k, l.id));
                            let r = ui.horizontal(|ui| {
                                ui.add_space(18.0);
                                let _ = ui.dnd_drag_source(egui::Id::new(("calque", k, l.id)), DragRow::Layer(k, pos), |ui| ui.label(egui::RichText::new("☰").strong()))
                                    .response.on_hover_text(tr("Drag to move the layer (with its items), or drop it on another page"));
                                if ui.small_button(pm(layer_open)).on_hover_text(if layer_open { tr("Collapse the layer") } else { tr("Expand the layer") }).clicked() { toggle_layer = Some((k, l.id)); }
                                if ui.small_button(if l.hidden { "◌" } else { "👁" }).on_hover_text(if l.hidden { tr("Layer hidden in the preview (it stays on the disc): show") } else { tr("Hide in the preview (the layer stays on the disc)") }).clicked() { op = Some(LayerOp::Hidden(k, l.id)); }
                                if ui.small_button(if l.locked { "🔒" } else { "🔓" }).on_hover_text(if l.locked { tr("Locked: its items can no longer be selected in the preview; unlock") } else { tr("Lock (no more selection or moving in the preview)") }).clicked() { op = Some(LayerOp::Locked(k, l.id)); }
                                let editing = matches!(&self.layer_edit, Some((ep, eid, _)) if *ep == k && *eid == l.id);
                                if editing {
                                    if let Some((_, _, text)) = &mut self.layer_edit {
                                        let te = ui.add(egui::TextEdit::singleline(text).desired_width(130.0));
                                        if !te.has_focus() && !te.lost_focus() { te.request_focus(); }
                                        if te.lost_focus() { edit_done = true; }
                                    }
                                } else {
                                    let n = d.layer_range(pos).len();
                                    let resp = ui.selectable_label(shown && active == l.id, egui::RichText::new(format!("{}  ({n})", l.name))).on_hover_text(tr("Active layer: receives new items. Double-click: rename"));
                                    if resp.clicked() { activate = Some((k, l.id)); }
                                    if resp.double_clicked() { self.layer_edit = Some((k, l.id, l.name.clone())); }
                                    if ui.small_button("✏").on_hover_text(tr("Rename the layer")).clicked() { self.layer_edit = Some((k, l.id, l.name.clone())); }
                                    let _ = ui.menu_button("🗑", |ui| {
                                        if ui.button(tr("Delete the layer (its items go to the neighboring layer)")).clicked() { op = Some(LayerOp::Remove(k, l.id, true)); ui.close_menu(); }
                                        if ui.button(tr("Delete the layer and its items")).clicked() { op = Some(LayerOp::Remove(k, l.id, false)); ui.close_menu(); }
                                    }).response.on_hover_text(tr("Delete the layer"));
                                }
                            }).response;
                            rows.push((DragRow::Layer(k, pos), r.rect));
                        }
                        if !layer_open { continue; }
                        let indent = if layered { 40.0 } else { 18.0 };
                        for i in d.layer_range(pos).rev() {
                            let it = &d.items[i];
                            let what = match &it.kind {
                                Kind::Text { text, link, .. } => trf!("Text “{}”{}", text.lines().next().unwrap_or("").chars().take(24).collect::<String>(), if link.is_some() { tr(" (clickable)") } else { "" }),
                                Kind::Image { frame: Some(f), link, .. } => trf!("Video image {}{}", f.video.map_or("?".to_string(), |v| format!("V{} @{}", v + 1, mmss(f.at))), if link.is_some() { tr(" (clickable)") } else { "" }),
                                Kind::Image { path, link, .. } => trf!("Image {}{}", std::path::Path::new(path).file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default(), if link.is_some() { tr(" (clickable)") } else { "" }),
                                Kind::Nav { action, text, .. } => if *action == NavAction::Goto { trf!("Button “{}”", text) } else { trf!("Button {}", action.label()) },
                                Kind::Slot { video, link, start, .. } => trf!("Thumbnail{}{}{}", match video { Some(v) => trf!(" (video {})", v + 1), None => tr(" (no video)").to_string() }, start.map(|t| trf!(" at {}", mmss(t))).unwrap_or_default(), if link.is_some() { " →" } else { "" }),
                            };
                            let what = if it.name.is_empty() { what } else { format!("{} ({what})", it.name) };
                            let mut col = kind_color(&it.kind);
                            if d.item_hidden(i) { col = col.gamma_multiply(0.45); }
                            let r = ui.horizontal(|ui| {
                                ui.add_space(indent);
                                let _ = ui.dnd_drag_source(egui::Id::new(("element", k, i)), DragRow::Item(k, i), |ui| ui.label(egui::RichText::new("☰").weak()))
                                    .response.on_hover_text(tr("Drag to change the order or the layer, or drop on another page"));
                                let lr = ui.selectable_label(shown && self.sel_item == Some(i), egui::RichText::new(what).color(col));
                                if lr.clicked() || lr.secondary_clicked() { pick = Some((k, i)); }
                                lr.context_menu(|ui| if let Some(dup) = item_menu(ui) { item_act = Some((k, i, dup)); });
                            }).response;
                            rows.push((DragRow::Item(k, i), r.rect));
                        }
                        if layered && d.layer_range(pos).is_empty() {
                            ui.horizontal(|ui| { ui.add_space(40.0); ui.label(egui::RichText::new(tr("(empty: drag items here)")).weak().italics()); });
                        }
                    }
                }
            });

            // ---- glisser-deposer
            let area = list.response.rect;
            let (Some(drag), Some(pos)) = (egui::DragAndDrop::payload::<DragRow>(ui.ctx()), ui.ctx().pointer_hover_pos()) else { return };
            if !area.expand(8.0).contains(pos) || rows.is_empty() { return; }
            let released = ui.input(|i| i.pointer.any_released());
            let copy = ui.input(|i| i.modifiers.command);
            let blue = Stroke::new(2.5_f32, Color32::from_rgb(90, 170, 255));
            let bottom_of = |rs: &[(DragRow, Rect)]| rs.last().map_or(area.bottom(), |x| x.1.bottom());
            match *drag {
                DragRow::Page(from) => {
                    // une page s'insere entre deux pages
                    let page_rows: Vec<usize> = rows.iter().enumerate().filter(|(_, (row, _))| matches!(row, DragRow::Page(_))).map(|(k, _)| k).collect();
                    let r = rows.iter().filter(|(_, rect)| rect.center().y < pos.y).count();
                    let above = page_rows.iter().filter(|&&k| k < r).count();
                    let line_at = page_rows.get(above).copied().unwrap_or(rows.len());
                    let y = if line_at < rows.len() { rows[line_at].1.top() } else { bottom_of(&rows) };
                    ui.painter().hline(area.x_range(), y, blue);
                    if released {
                        egui::DragAndDrop::clear_payload(ui.ctx());
                        let to = if above > from { above - 1 } else { above };
                        if to != from { op = Some(LayerOp::PageMove { from, to }); }
                    }
                }
                what => {
                    let p = what.page();
                    // page sous la souris : celle de la derniere ligne au-dessus du pointeur
                    let Some(q) = rows.iter().filter(|(_, rect)| rect.top() <= pos.y).last().map(|(row, _)| row.page()) else { return };
                    let block: Vec<(DragRow, Rect)> = rows.iter().copied().filter(|(row, _)| row.page() == q).collect();
                    let head = block.first().map_or(area.top(), |x| x.1.top());
                    if q != p {
                        // envoi sur une autre page : tout son bloc s'encadre
                        let r = Rect::from_x_y_ranges(area.x_range(), head..=bottom_of(&block));
                        ui.painter().rect_stroke(r.shrink(1.0), 3.0, blue);
                        if released {
                            egui::DragAndDrop::clear_payload(ui.ctx());
                            op = Some(LayerOp::Transfer { what, to: q, copy });
                        }
                        return;
                    }
                    // dans sa page : lignes de ses calques et elements
                    let Some(d) = all.get(p) else { return };
                    let sub: Vec<(DragRow, Rect)> = block.iter().copied().filter(|(row, _)| !matches!(row, DragRow::Page(_))).collect();
                    if sub.is_empty() { return; }
                    let layered = !d.layers.is_empty();
                    let r = sub.iter().filter(|(_, rect)| rect.center().y < pos.y).count();
                    let header_rows: Vec<usize> = sub.iter().enumerate().filter(|(_, (row, _))| matches!(row, DragRow::Layer(..))).map(|(k, _)| k).collect();
                    let headers_above = header_rows.iter().filter(|&&k| k < r).count();
                    // un calque s'insere entre deux calques
                    let line_at = if matches!(what, DragRow::Layer(..)) { header_rows.get(headers_above).copied().unwrap_or(sub.len()) } else { r };
                    let y = if line_at < sub.len() { sub[line_at].1.top() } else { bottom_of(&sub) };
                    ui.painter().hline(area.x_range(), y, blue);
                    if released {
                        egui::DragAndDrop::clear_payload(ui.ctx());
                        match what {
                            DragRow::Item(_, from) if from < d.items.len() => {
                                let lid = |l: usize| d.layers.get(l).map_or(0, |x| x.id);
                                let (layer, before) = if r == 0 {
                                    let top = d.layers.len().saturating_sub(1);
                                    (lid(top), if layered { d.layer_range(top).end } else { d.items.len() })
                                } else {
                                    match sub[r - 1].0 {
                                        // calque replie ou deplie : au premier plan de ce calque
                                        DragRow::Layer(_, l) => (lid(l), d.layer_range(l).end),
                                        DragRow::Item(_, j) => (lid(d.layer_pos(j)), j),
                                        DragRow::Page(_) => (lid(d.layers.len().saturating_sub(1)), d.items.len()),
                                    }
                                };
                                if !(before == from || before == from + 1) || layer != d.items[from].layer {
                                    op = Some(LayerOp::MoveItem { p, from, before, layer });
                                }
                            }
                            DragRow::Layer(_, from) if from < d.layers.len() => {
                                let n = d.layers.len();
                                let disp_from = n - 1 - from;
                                let disp_to = if headers_above > disp_from { headers_above - 1 } else { headers_above };
                                let to = n - 1 - disp_to.min(n - 1);
                                if to != from { op = Some(LayerOp::MoveLayer { p, from, to }); }
                            }
                            _ => {}
                        }
                    }
                }
            }
        });

        // ---- application des changements
        if let Some(k) = toggle_page { let o = opened[k]; self.tree_pages.insert(k, !o); }
        if let Some(key) = toggle_layer { if !self.tree_layers_closed.remove(&key) { self.tree_layers_closed.insert(key); } }
        if page_edit_done {
            if let Some((k, name)) = self.page_edit.take() { op = Some(LayerOp::PageRename(k, name.trim().to_string())); }
        }
        if edit_done {
            if let Some((k, id, name)) = self.layer_edit.take() {
                let name = name.trim().to_string();
                if !name.is_empty() { op = Some(LayerOp::Rename(k, id, name)); }
            }
        }
        let show = |me: &mut Self, k: usize| {
            if me.cur != Some(PageRef(k)) {
                me.cur = Some(PageRef(k));
                me.sel_item = None;
                me.drag = None;
                me.edit_gen += 1;
            }
        };
        if let Some(k) = pick_page { show(self, k); }
        if let Some((k, id)) = activate { show(self, k); self.active_layer = id; }
        if let Some((k, i)) = pick {
            show(self, k);
            self.sel_item = Some(i);
            if let Some(it) = all.get(k).and_then(|d| d.items.get(i)) { if it.layer != 0 { self.active_layer = it.layer; } }
        }
        if let Some((k, i, dup)) = item_act {
            show(self, k);
            if let Some(dm) = self.model.designs.free.get_mut(k) {
                self.sel_item = item_edit(dm, i, dup);
                self.bump();
            }
            return;
        }
        let Some(op) = op else { return };
        // la page affichee apres l'operation (indice susceptible de changer)
        let cur_now = match self.cur { Some(PageRef(k)) => Some(k), None => None };
        match op {
            LayerOp::Add => {
                if let Some(dm) = cur_k.and_then(|k| self.model.designs.free.get_mut(k)) { self.active_layer = add_layer(dm); }
                // la page se deplie pour montrer le nouveau calque
                if let Some(k) = cur_k { self.tree_pages.insert(k, true); }
            }
            LayerOp::PageRename(k, name) => {
                if let Some(p) = self.model.designs.free.get_mut(k) { p.name = name; }
                self.edit_gen += 1;
            }
            LayerOp::PageDelete(k) => {
                if let Some(pi) = self.pages().into_iter().find(|p| p.r == PageRef(k)) { pi.remove_design(&mut self.model.designs); }
                if let Some(ck) = cur_now {
                    if ck == k { self.sel_item = None; }
                    self.cur = Some(PageRef(if ck > k || (ck == k && k > 0) { ck - 1 } else { ck }));
                }
                // etats replie / deplie : indices decales
                self.tree_pages = self.tree_pages.iter().filter(|e| *e.0 != k).map(|(&p, &o)| (if p > k { p - 1 } else { p }, o)).collect();
                self.tree_layers_closed = self.tree_layers_closed.iter().filter(|e| e.0 != k).map(|&(p, l)| (if p > k { p - 1 } else { p }, l)).collect();
                self.edit_gen += 1;
            }
            LayerOp::PageMove { from, to } => {
                bdauthor_core::editor::move_page(&mut self.model, from, to);
                let map = |c: usize| if c == from { to } else if from < to && c > from && c <= to { c - 1 } else if to < from && c >= to && c < from { c + 1 } else { c };
                if let Some(ck) = cur_now { self.cur = Some(PageRef(map(ck))); }
                self.tree_pages = self.tree_pages.iter().map(|(&p, &o)| (map(p), o)).collect();
                self.tree_layers_closed = self.tree_layers_closed.iter().map(|&(p, l)| (map(p), l)).collect();
                self.edit_gen += 1;
            }
            LayerOp::Transfer { what, to, copy } => {
                let p = what.page();
                let done = match what {
                    DragRow::Item(_, i) => bdauthor_core::editor::transfer_item(&mut self.model.designs, p, i, to, copy).is_some(),
                    DragRow::Layer(_, pos) => {
                        let lid = self.model.designs.free.get(p).and_then(|d| d.layers.get(pos)).map(|l| l.id);
                        lid.and_then(|l| bdauthor_core::editor::transfer_layer(&mut self.model.designs, p, l, to, copy)).is_some()
                    }
                    DragRow::Page(_) => false,
                };
                if !done { return; }
                if !copy && cur_now == Some(p) { self.sel_item = None; }
                self.tree_pages.insert(to, true);
                self.notice = trf!("{} to page “{}”.", if copy { tr("Copied") } else { tr("Moved") }, names.get(to).cloned().unwrap_or_default());
            }
            LayerOp::Remove(k, id, keep) => {
                if let Some(dm) = self.model.designs.free.get_mut(k) {
                    remove_layer(dm, id, keep);
                    if cur_now == Some(k) { self.sel_item = None; }
                }
            }
            LayerOp::Hidden(k, id) => if let Some(l) = self.model.designs.free.get_mut(k).and_then(|d| d.layers.iter_mut().find(|l| l.id == id)) { l.hidden = !l.hidden; },
            LayerOp::Locked(k, id) => if let Some(l) = self.model.designs.free.get_mut(k).and_then(|d| d.layers.iter_mut().find(|l| l.id == id)) { l.locked = !l.locked; },
            LayerOp::Rename(k, id, name) => if let Some(l) = self.model.designs.free.get_mut(k).and_then(|d| d.layers.iter_mut().find(|l| l.id == id)) { l.name = name; },
            LayerOp::MoveItem { p, from, before, layer } => {
                if let Some(dm) = self.model.designs.free.get_mut(p) {
                    let k = move_item(dm, from, before, layer);
                    if cur_now == Some(p) {
                        self.sel_item = Some(k);
                        if layer != 0 { self.active_layer = layer; }
                    }
                }
            }
            LayerOp::MoveLayer { p, from, to } => {
                let sel = self.sel_item;
                if let Some(dm) = self.model.designs.free.get_mut(p) {
                    // l'element selectionne suit son calque : on le retrouve par son contenu
                    let selected = if cur_now == Some(p) { sel.and_then(|s| dm.items.get(s).cloned()) } else { None };
                    move_layer(dm, from, to);
                    if cur_now == Some(p) { self.sel_item = selected.and_then(|it| dm.items.iter().position(|x| *x == it)); }
                }
            }
        }
        self.bump();
    }

    // ------------------------------------------------------------------ annuler / refaire
    /// Enregistre les changements de l'image precedente et traite Ctrl+Z / Ctrl+Y.
    pub(crate) fn history_step(&mut self, ctx: &egui::Context) {
        if self.drag.is_none() {
            let ms = self.t0.elapsed().as_millis() as u64;
            self.history.observe(&self.model.designs, ms);
        }
        if self.mode != crate::Mode::MiseEnPage || self.running() || self.drag.is_some() || ctx.wants_keyboard_input() {
            return;
        }
        use egui::Modifiers;
        let redo = ctx.input_mut(|i| i.consume_key(Modifiers::COMMAND | Modifiers::SHIFT, Key::Z) || i.consume_key(Modifiers::COMMAND, Key::Y));
        let undo = !redo && ctx.input_mut(|i| i.consume_key(Modifiers::COMMAND, Key::Z));
        if redo { self.redo_action(); }
        if undo { self.undo_action(); }
        // copier / couper / coller : egui les transmet en evenements Copy / Cut / Paste (et non en touches)
        let (copy, cut, paste) = ctx.input_mut(|i| {
            let has = |i: &egui::InputState, f: fn(&egui::Event) -> bool| i.events.iter().any(f);
            let copy = has(i, |e| matches!(e, egui::Event::Copy)) || i.consume_key(Modifiers::COMMAND, Key::C);
            let cut = has(i, |e| matches!(e, egui::Event::Cut)) || i.consume_key(Modifiers::COMMAND, Key::X);
            let paste = has(i, |e| matches!(e, egui::Event::Paste(t) if t == CLIP_MARK)) || i.consume_key(Modifiers::COMMAND, Key::V);
            (copy, cut, paste)
        });
        if copy || cut {
            self.copy_selection(cut);
            // egui-winit ne transmet « coller » que si le presse-papiers systeme contient du texte : on y depose un marqueur
            if !self.clipboard.is_empty() { ctx.copy_text(CLIP_MARK.to_string()); }
        }
        if paste { self.paste_clipboard(); }
    }

    /// Copie (ou coupe) les widgets selectionnes dans le presse-papiers de l'application.
    fn copy_selection(&mut self, cut: bool) {
        let Some(info) = self.current_info() else { return };
        let mut idx: Vec<usize> = self.extra_sel.clone();
        idx.extend(self.sel_item);
        idx.sort_unstable();
        idx.dedup();
        let Some(d) = info.design_mut(&mut self.model.designs) else { return };
        idx.retain(|&i| i < d.items.len() && !d.item_hidden(i) && !d.item_locked(i));
        if idx.is_empty() { return; }
        self.clipboard = idx.iter().map(|&i| d.items[i].clone()).collect();
        self.clip_from = Some(info.r);
        self.clip_cut = cut;
        self.paste_n = 0;
        let n = idx.len();
        if cut {
            for &i in idx.iter().rev() { d.items.remove(i); }
            d.prune_nav();
            self.sel_item = None;
            self.extra_sel.clear();
            self.bump();
        }
        let (verb, pl) = (if cut { tr("cut") } else { tr("copied") }, if n > 1 { "s" } else { "" });
        self.notice = trf!("{0} widget{1} {2}.", n, pl, verb, pl);
    }

    /// Colle les widgets du presse-papiers dans la page affichee (decales si c'est la page d'origine).
    fn paste_clipboard(&mut self) {
        if self.clipboard.is_empty() { return; }
        let Some(info) = self.current_info() else { return };
        let layer = self.target_layer(&info);
        self.paste_n += 1;
        let off = if self.clip_from == Some(info.r) { 30 * self.paste_n } else { 0 };
        let items = self.clipboard.clone();
        let Some(d) = info.design_mut(&mut self.model.designs) else { return };
        let mut added = Vec::new();
        for mut it in items {
            it.id = 0;
            it.nav = Default::default();
            if !self.clip_cut { it.name = copy_name(d, &it); }
            it.x = (it.x + off).clamp(0, (1920 - it.w).max(0));
            it.y = (it.y + off).clamp(0, (1080 - it.h).max(0));
            added.push(insert_item(d, it, layer));
        }
        // insert_item range chaque widget a la fin de son calque : les rangs precedents peuvent avoir bouge si le calque est le meme
        // (insertions successives en fin de calque : rangs croissants, aucun decalage des precedents)
        self.sel_item = added.pop();
        self.extra_sel = added;
        self.bump();
        self.notice = tr("Widgets pasted.").to_string();
    }

    pub(crate) fn undo_action(&mut self) {
        if self.history.undo(&mut self.model.designs) {
            self.after_history();
        }
    }

    pub(crate) fn redo_action(&mut self) {
        if self.history.redo(&mut self.model.designs) {
            self.after_history();
        }
    }

    fn after_history(&mut self) {
        // la selection (multiple comprise) est conservee tant que ses elements existent encore dans la page affichee
        let n = self.current_info().and_then(|info| info.design(&self.model.designs).map(|d| d.items.len())).unwrap_or(0);
        self.sel_item = self.sel_item.filter(|&s| s < n);
        self.extra_sel.retain(|&e| e < n);
        if self.sel_item.is_none() { self.sel_item = self.extra_sel.pop(); }
        self.drag = None;
        self.marquee = None;
        self.bump();
    }

    // ------------------------------------------------------------------ thread de travail
    pub(crate) fn poll_worker(&mut self) {
        let mut evs = Vec::new();
        if let Some(w) = &self.worker {
            while let Ok(e) = w.rx.try_recv() { evs.push(e); }
        }
        for e in evs {
            match e {
                Event::Duration(k, d, h, f) => { self.video_sizes.insert(k.clone(), (h, f)); self.durations.insert(k, d); self.edit_gen += 1; }
                Event::Thumb(k, ms, im) => { self.thumbs.insert((k, ms), std::sync::Arc::new(im)); self.edit_gen += 1; }
                Event::Frame(k, ms, im) => { self.frames.insert((k, ms), std::sync::Arc::new(im)); self.edit_gen += 1; }
                Event::ProbeFailed(k, m) => { self.probe_failed.insert(k); self.notice = m; self.edit_gen += 1; }
                Event::Error(m) => self.notice = m,
            }
        }
    }
}

/// Reglages du fond d'une page. `pick` : fichier a choisir (0 image, 1 video, 2 musique). Retourne vrai si quelque chose a change.
fn page_bg_ui(ui: &mut egui::Ui, bg: &mut bdauthor_core::design::PageBg, pick: &mut Option<u8>) -> bool {
    use bdauthor_core::design::{PageMusic, PageVisual};
    let before = bg.clone();
    ui.strong(tr("Image or video"));
    let kind = match bg.visual { PageVisual::Project => 0, PageVisual::Image(_) => 1, PageVisual::Video(..) => 2 };
    ui.horizontal(|ui| {
        if ui.selectable_label(kind == 0, tr("Same as the project")).clicked() { bg.visual = PageVisual::Project; }
        if ui.selectable_label(kind == 1, tr("Image")).clicked() && kind != 1 { *pick = Some(0); }
        if ui.selectable_label(kind == 2, tr("Video")).clicked() && kind != 2 { *pick = Some(1); }
    });
    match &mut bg.visual {
        PageVisual::Image(f) => {
            ui.horizontal(|ui| { ui.label(tr("Image:")); ui.text_edit_singleline(f); if ui.button(tr("Choose…")).clicked() { *pick = Some(0); } });
        }
        PageVisual::Video(f, son) => {
            ui.horizontal(|ui| { ui.label(tr("Video:")); ui.text_edit_singleline(f); if ui.button(tr("Choose…")).clicked() { *pick = Some(1); } });
            ui.checkbox(son, tr("Keep the video's sound (if there is no music)"));
            crate::help(ui, tr("Played in full, looped; the page's texts and images are placed over it."));
        }
        PageVisual::Project => {}
    }
    ui.separator();
    ui.strong(tr("Music"));
    let mk = match bg.music { PageMusic::Project => 0, PageMusic::Silent => 1, PageMusic::File(_) => 2 };
    ui.horizontal(|ui| {
        if ui.selectable_label(mk == 0, tr("Same as the project")).clicked() { bg.music = PageMusic::Project; }
        if ui.selectable_label(mk == 1, tr("No music")).clicked() { bg.music = PageMusic::Silent; }
        if ui.selectable_label(mk == 2, tr("Other music")).clicked() && mk != 2 { *pick = Some(2); }
    });
    if let PageMusic::File(f) = &mut bg.music {
        ui.horizontal(|ui| { ui.label(tr("File:")); ui.text_edit_singleline(f); if ui.button(tr("Choose…")).clicked() { *pick = Some(2); } });
    }
    crate::help(ui, tr("Each page with a different background has its own clip on the disc (encoded once, then cached)."));
    *bg != before
}
