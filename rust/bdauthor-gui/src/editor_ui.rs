//! Editeur visuel de mise en page (onglet « Mise en page ») : apercu fidele, glisser/redimensionner, inspecteur.
use crate::App;
use bdauthor_core::design::{Align, Item, Kind, MenuType, NavAction, PageDesign, SlotReturn, Target};
use bdauthor_core::editor::{add_layer, copy_decor, dragged_rect, hit_test_where, insert_item, list_pages, move_item, move_layer, remove_layer, set_rect, video_title, Handle, PageInfo, PageRef};
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
}

pub(crate) enum Job {
    Probe { key: String, path: PathBuf },
    /// duree d'un fichier audio (musique de menu)
    ProbeAudio { key: String, path: PathBuf },
    Thumb { key: String, path: PathBuf, at: f64, duration: f64 },
}

pub(crate) enum Event {
    /// (fichier, duree, hauteur, cadence)
    Duration(String, f64, u32, f64),
    Thumb(String, i64, RgbaImage),
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
                    (Err(e), _) => Event::Error(format!("ffmpeg introuvable : {e}")),
                    (Ok(t), Job::Probe { key, path }) => match probe(t, &path) {
                        Ok(i) => Event::Duration(key, i.duration_s, i.height, i.fps()),
                        Err(e) => Event::Error(format!("Lecture de {key} impossible : {e}")),
                    },
                    (Ok(t), Job::ProbeAudio { key, path }) => match bdauthor_core::ffmpeg::probe_duration(t, &path) {
                        Ok(d) => Event::Duration(key, d, 0, 0.0),
                        Err(e) => Event::Error(format!("Lecture de {key} impossible : {e}")),
                    },
                    (Ok(t), Job::Thumb { key, path, at, duration }) => {
                        match extract_thumbnail_rgba(t, &path, (480, 270), Some(at), duration) {
                            Ok(im) => Event::Thumb(key, (at * 1000.0).round() as i64, im),
                            Err(e) => Event::Error(format!("Miniature de {key} impossible : {e}")),
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
    Element(usize),
    Missing,
}

enum Op { ToLayer(u32) }

/// Supprime (`dup` = false) ou duplique l'element `i` ; retourne l'element a selectionner ensuite. La copie se place juste devant
/// l'original, dans le meme calque, decalee de 30 px.
fn item_edit(d: &mut PageDesign, i: usize, dup: bool) -> Option<usize> {
    if i >= d.items.len() { return None; }
    if !dup {
        d.items.remove(i);
        return None;
    }
    let mut c = d.items[i].clone();
    c.x = (c.x + 30).min(1920 - c.w);
    c.y = (c.y + 30).min(1080 - c.h);
    d.items.insert(i + 1, c);
    Some(i + 1)
}

/// Menu contextuel d'un element (clic droit) : `Some(true)` dupliquer, `Some(false)` supprimer.
fn item_menu(ui: &mut egui::Ui) -> Option<bool> {
    let mut out = None;
    if ui.button("Dupliquer").clicked() { out = Some(true); ui.close_menu(); }
    if ui.button("Supprimer").clicked() { out = Some(false); ui.close_menu(); }
    out
}

/// Element a ajouter depuis la palette (glisser-deposer sur l'apercu).
#[derive(Clone, Copy, PartialEq)]
enum NewWidget { Text, Image, VideoSlot, ChapterSlot, Nav(NavAction) }

/// Icones de la palette : (element, icone, nom).
fn palette() -> Vec<(NewWidget, &'static str, String)> {
    let mut v = vec![
        (NewWidget::Text, "🔤", "Texte".to_string()),
        (NewWidget::Image, "🖼", "Image (PNG, JPEG, SVG…)".to_string()),
        (NewWidget::VideoSlot, "🎬", "Vignette vidéo (lecture depuis le début)".to_string()),
        (NewWidget::ChapterSlot, "🎞", "Vignette de chapitre (lecture depuis un instant)".to_string()),
    ];
    // un seul bouton de navigation : son action (Aller a, Precedent, Suivant, Tout lire) se choisit dans l'inspecteur
    v.push((NewWidget::Nav(NavAction::Goto), "🔘", "Bouton de navigation (destination choisie dans l'inspecteur : page, vidéo, Précédent, Suivant, Tout lire, sous-titres…)".to_string()));
    v
}

/// Nouvel element a sa place par defaut (une image n'a pas encore de fichier).
fn new_widget(w: NewWidget, first_video: Option<usize>) -> Item {
    match w {
        NewWidget::Text => Item::text("Texte", 660, 470, 600, 120, 60),
        NewWidget::Image => Item { x: 100, y: 100, w: 400, h: 300, kind: Kind::Image { path: String::new(), link: None }, look: Default::default(), layer: 0, sub: Default::default() },
        NewWidget::VideoSlot | NewWidget::ChapterSlot => {
            let mut it = Item::slot(100, 300, 400, 250, 200);
            if let Kind::Slot { video, start, .. } = &mut it.kind {
                *video = Some(first_video);
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
        Kind::Slot { .. } => "Vignette",
        Kind::Nav { .. } => "Bouton de navigation",
        Kind::Text { link: None, .. } => "Texte",
        Kind::Text { .. } => "Texte cliquable",
        Kind::Image { link: None, .. } => "Image",
        Kind::Image { .. } => "Image cliquable",
    }
}

/// Case « cliquable » et choix de la destination d'un texte ou d'une image.
fn link_ui(ui: &mut egui::Ui, link: &mut Option<Target>, targets: &[(Target, String)], changed: &mut bool) {
    let mut on = link.is_some();
    if ui.checkbox(&mut on, "Cliquable (devient un bouton du menu)").changed() {
        // par defaut : la premiere page (pas un reglage de sous-titres ou d'audio)
        *link = if on { targets.iter().find(|t| !t.0.is_setting()).or(targets.first()).map(|t| t.0) } else { None };
        *changed = true;
    }
    if link.is_some() {
        destination_combo(ui, link, targets, changed);
        ui.label(egui::RichText::new("Sélectionné à la télécommande, l'élément est entouré d'un cadre bleu.").weak());
    }
}

/// Liste des destinations possibles : pages du menu puis videos a lire.
fn destination_combo(ui: &mut egui::Ui, target: &mut Option<Target>, targets: &[(Target, String)], changed: &mut bool) {
    let shown = target.and_then(|t| targets.iter().find(|(x, _)| *x == t.without_exit()).map(|(_, l)| l.clone())).unwrap_or_else(|| "(destination introuvable : à choisir)".to_string());
    egui::ComboBox::from_label("Destination").selected_text(shown).show_ui(ui, |ui| {
        for (t, label) in targets {
            // meme reglage deja choisi : sa page de sortie est gardee
            if ui.selectable_label(target.map(Target::without_exit) == Some(*t), label).clicked() && target.map(Target::without_exit) != Some(*t) { *target = Some(*t); *changed = true; }
        }
    });
}

impl App {
    pub(crate) fn pages(&self) -> Vec<PageInfo> {
        list_pages(&self.model, &self.durations)
    }

    /// Destinations d'un bouton « Aller a » ou d'un element cliquable : (cible, libelle) ; les pages, puis les videos a lire.
    fn page_targets(&self) -> Vec<(Target, String)> {
        let mut out: Vec<(Target, String)> = self.pages().into_iter().map(|p| {
            let t = match p.r { PageRef::Chapters(v, q) => Target::Chapters(v, q), _ => Target::Main(p.page) };
            (t, format!("Page : {}", p.label(&self.model)))
        }).collect();
        for v in 0..self.model.videos.len() {
            out.push((Target::Video(v), format!("Lire la vidéo {}. {}", v + 1, video_title(&self.model, v))));
        }
        // reglages sans navigation : les sous-titres, puis une entree par langue audio (jamais les deux a la fois)
        let mut stay = vec![(Target::Stay(None), "Régler les sous-titres".to_string())];
        for l in &self.model.languages {
            stay.push((Target::Audio(l.id, None), format!("Régler la langue audio : {}", l.name)));
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

    fn base_dir(&self) -> PathBuf {
        self.path.as_ref().and_then(|p| p.parent()).map(|p| p.to_path_buf()).unwrap_or_else(|| PathBuf::from("."))
    }

    fn bump(&mut self) {
        self.edit_gen += 1;
        self.dirty = true;
    }

    /// Demande au thread de travail la duree des videos et les miniatures utiles a la page affichee.
    pub(crate) fn request_media(&mut self, info: &PageInfo) {
        let base = self.base_dir();
        for v in &self.model.videos {
            if !self.durations.contains_key(&v.file) && self.requested.insert(format!("P|{}", v.file)) {
                if let Some(w) = &self.worker {
                    let _ = w.tx.send(Job::Probe { key: v.file.clone(), path: abs_path(&base, &v.file) });
                }
            }
        }
        // video de fond anime : sa duree puis une image
        let bgv = self.page_visual(info).1.unwrap_or_default();
        if !bgv.is_empty() && !self.durations.contains_key(&bgv) && self.requested.insert(format!("P|{bgv}")) {
            if let Some(w) = &self.worker {
                let _ = w.tx.send(Job::Probe { key: bgv.clone(), path: abs_path(&base, &bgv) });
            }
        }
        let mut sources = self.slot_sources(info);
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
        let d = match info.design(&self.model.designs) { Some(d) => d.clone(), None => match info.starting_point(&self.model) { Ok(d) => d, Err(_) => return vec![] } };
        let n = self.model.videos.len();
        let eff = d.slot_assignment(info.count());
        let mut k = 0;
        let mut out = Vec::new();
        for it in &d.items {
            let Kind::Slot { start, label, thumb_at, .. } = &it.kind else { continue };
            out.push(match it.fixed_video() {
                Some(v) if v < n => {
                    let label = if !label.is_empty() { label.clone() } else if let Some(t) = start { mmss(*t) } else { String::new() };
                    SlotShow::Video(v, thumb_at.or(*start), label)
                }
                Some(_) => SlotShow::Missing,
                None if info.is_free() || it.is_video_slot() => SlotShow::Missing,
                None => {
                    let e = eff.get(k).copied().unwrap_or(usize::MAX);
                    k += 1;
                    if e == usize::MAX { SlotShow::Missing } else { SlotShow::Element(e) }
                }
            });
        }
        out
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
        let fmt = |d: f64| { let s = d.round() as i64; if s >= 60 { format!("{} min {:02} s", s / 60, s % 60) } else { format!("{s} s") } };
        if !video.is_empty() {
            let d = self.media_duration(video, false)?;
            let per_min = if self.menu_plane().1 >= 1080 { 60.0 } else { 30.0 };
            return Some(format!("Durée {} (jouée en entier, en boucle) · environ {:.0} Mo sur le disque pour chaque page qui utilise ce fond (estimation)", fmt(d), d / 60.0 * per_min));
        }
        if !music.is_empty() {
            let d = self.media_duration(music, true)?;
            return Some(format!("Le fond durera {} (la musique, en entier) · environ {:.0} Mo par page qui l'utilise (estimation)", fmt(d), (d / 60.0 * 2.5).max(1.0)));
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

    /// (fichier, instant de la miniature) de chaque element de la page (videos de la page, ou chapitres).
    fn element_sources(&self, info: &PageInfo) -> Vec<(String, f64)> {
        if info.is_free() { return vec![]; }
        let mut out = Vec::new();
        match info.video {
            None => for &i in &info.items {
                out.push(self.video_thumb(i));
            },
            Some(vi) => {
                let v = &self.model.videos[vi];
                let ch = v.chapter_list(self.durations.get(&v.file).copied());
                for &j in &info.items {
                    if let Some(c) = ch.get(j) { out.push((v.file.clone(), c.0)); }
                }
            }
        }
        out
    }

    /// Libelle de chaque element de la page.
    fn element_labels(&self, info: &PageInfo) -> Vec<String> {
        if info.is_free() { return vec![]; }
        match info.video {
            None => info.items.iter().map(|&i| video_title(&self.model, i)).collect(),
            Some(vi) => {
                let v = &self.model.videos[vi];
                let ch = v.chapter_list(self.durations.get(&v.file).copied());
                info.items.iter().filter_map(|&j| ch.get(j)).map(|(t, n)| { let s = t.round() as i64; format!("{n} ({}:{:02})", s / 60, s % 60) }).collect()
            }
        }
    }

    /// Miniatures utiles a la page : (fichier, instant).
    fn slot_sources(&self, info: &PageInfo) -> Vec<(String, f64)> {
        let elems = self.element_sources(info);
        self.slot_plan(info).into_iter().filter_map(|s| match s {
            SlotShow::Video(v, at, _) => { let (f, a) = self.video_thumb(v); Some((f, at.unwrap_or(a))) }
            SlotShow::Element(e) => elems.get(e).cloned(),
            SlotShow::Missing => None,
        }).collect()
    }

    /// Contenu de chaque vignette pour l'apercu, dans l'ordre de la liste.
    fn preview_slots(&self, info: &PageInfo) -> Vec<PreviewSlot> {
        let (sources, labels) = (self.element_sources(info), self.element_labels(info));
        self.slot_plan(info).into_iter().map(|s| {
            let (label, src) = match s {
                SlotShow::Video(v, at, label) => {
                    let (f, a) = self.video_thumb(v);
                    (if label.is_empty() { video_title(&self.model, v) } else { label }, Some((f, at.unwrap_or(a))))
                }
                SlotShow::Element(e) => (labels.get(e).cloned().unwrap_or_default(), sources.get(e).cloned()),
                SlotShow::Missing => ("(choisir une vidéo)".to_string(), None),
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
        let design = match info.design(&self.model.designs).cloned() {
            Some(d) => d,
            None => match info.starting_point(&self.model) { Ok(d) => d, Err(_) => return },
        };
        let mut design = design;
        for it in &mut design.items {
            // les PreviewSlot sont deja dans l'ordre des vignettes
            if let Kind::Slot { assign, video, .. } = &mut it.kind { *assign = None; *video = None; }
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
        let sel = if info.custom(&self.model.designs) || self.sim.is_some() { sel_shift } else { None };
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
        let img = renderer.render_page_preview(&design, info.menu, info.page, info.pages, &slots, base.as_ref(), PREVIEW, &base_dir, &self.model.designs.style, sel, act);
        self.preview_sel = self.sel_item;
        self.preview_act = act;
        let ci = egui::ColorImage::from_rgba_unmultiplied([img.width as usize, img.height as usize], &img.data);
        match &mut self.preview_tex {
            Some(t) => t.set(ci, egui::TextureOptions::LINEAR),
            None => self.preview_tex = Some(ctx.load_texture("apercu", ci, egui::TextureOptions::LINEAR)),
        }
    }

    // ------------------------------------------------------------------ pages generiques
    /// Convertit un ancien projet (pages automatiques, pages libres, menus de chapitres) en pages generiques, une fois connues les
    /// durees utiles (chapitres automatiques). `false` tant que la conversion n'est pas faite (ou sans video).
    pub(crate) fn ensure_generic(&mut self) -> bool {
        if self.model.designs.generic { return true; }
        if self.model.videos.is_empty() { return false; }
        let waiting: Vec<String> = self.model.videos.iter().filter(|v| v.chapters.is_empty() && v.auto_s.is_some()).map(|v| v.file.clone()).collect();
        for f in &waiting { let _ = self.media_duration(f, false); }
        match bdauthor_core::editor::convert_to_generic(&mut self.model, &self.durations) {
            Ok(()) => {
                self.history.reset(&self.model.designs);
                self.cur = None;
                self.sel_item = None;
                self.dirty = true;
                self.edit_gen += 1;
                self.notice = format!("Menus mis en pages génériques : {} page(s), toutes modifiables.", self.model.designs.free.len());
                true
            }
            Err(_) => false,
        }
    }

    // ------------------------------------------------------------------ zone centrale : barre d'outils + apercu
    pub(crate) fn layout_central(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        if !self.ensure_generic() {
            ui.label("Ajoutez des vidéos dans l'onglet « Projet » pour composer les pages de menu.");
            return;
        }
        let Some(info) = self.current_info() else {
            ui.label("Aucune page : « + Page » (à gauche) en ajoute une.");
            return;
        };
        if let Err(e) = &self.renderer {
            ui.colored_label(Color32::from_rgb(230, 100, 90), format!("Aperçu indisponible : {e}"));
            return;
        }
        self.request_media(&info);
        let custom = info.custom(&self.model.designs);

        ui.horizontal_wrapped(|ui| {
            if self.sim.is_none() {
                if ui.button("▶ Simuler la télécommande").on_hover_text("Parcourir le menu comme sur un lecteur : flèches, Entrée, Échap").clicked() {
                    self.sim = Some(crate::sim_ui::Sim::default());
                    self.sel_item = None;
                    self.drag = None;
                    self.edit_gen += 1;
                }
            } else if ui.button("■ Arrêter la simulation").clicked() {
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
            // palette : glisser une icone sur l'apercu pour placer l'element, ou cliquer pour l'ajouter a sa place par defaut
            let first = (!self.model.videos.is_empty()).then_some(0);
            let palette = palette();
            ui.horizontal_wrapped(|ui| {
                for (w, icon, name) in &palette {
                    let b = ui.add(egui::Button::new(egui::RichText::new(*icon).size(27.0)).min_size(vec2(48.0, 48.0)).sense(Sense::click_and_drag()))
                        .on_hover_text(format!("{name}\nGlisser sur la page pour le placer, ou cliquer pour l'ajouter"));
                    if b.drag_started() { egui::DragAndDrop::set_payload(ui.ctx(), *w); }
                    if b.dragged() { ui.ctx().set_cursor_icon(egui::CursorIcon::Grabbing); }
                    if b.clicked() {
                        if *w == NewWidget::Image { pick_image = true; } else { add = Some(new_widget(*w, first)); }
                    }
                }
                ui.separator();
                ui.checkbox(&mut self.snap, "Aimantation 10 px");
            });
            if pick_image {
                if let Some(f) = rfd::FileDialog::new().add_filter("Images", &["png", "jpg", "jpeg", "bmp", "svg", "svgz"]).pick_file() {
                    let rel = self.relative(&f);
                    add = Some(Item { x: 100, y: 100, w: 400, h: 300, kind: Kind::Image { path: rel, link: None }, look: Default::default(), layer: 0, sub: Default::default() });
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

        // fenetre du style des boutons du projet
        let mut open = self.show_style;
        let mut style_changed = false;
        egui::Window::new("Style des boutons").open(&mut open).resizable(false).show(ctx, |ui| {
            egui::ScrollArea::vertical().max_height(560.0).show(ui, |ui| {
                style_changed = crate::style_ui::style_ui(ui, &mut self.model.designs.style, "projet");
            });
            ui.label(egui::RichText::new("S'applique à tous les boutons du projet, sauf ceux qui ont leur propre style (inspecteur).").weak());
        });
        self.show_style = open;
        if style_changed { self.bump(); }

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
        if let Some(d) = info.design(&self.model.designs).cloned().or_else(|| info.starting_point(&self.model).ok()) {
            let plane = if self.model.menu_1080 { (1920, 1080) } else { self.menu_plane() };
            let wt = d.weight(info.count(), &self.model.designs.style, info.menu, info.page, info.pages, plane);
            crate::style_ui::weight_ui(ui, &wt, plane);
        }

        // vérifications de la page : calculees d'abord pour ne reserver sous l'apercu que la place necessaire
        let probs: Vec<bdauthor_core::design::Problem> = match info.design(&self.model.designs).filter(|_| custom) {
            Some(d) => {
                let targets = self.page_targets();
                let mut probs = if info.is_free() { d.validate_free(info.page, info.pages, self.model.videos.len()) } else { d.validate(info.count(), info.menu, info.page, info.pages) };
                probs.extend(d.check_targets(&|t| targets.iter().any(|(x, _)| *x == t.without_exit()) && t.exit().map_or(true, |q| targets.iter().any(|(x, _)| *x == Target::Main(q)))));
                probs
            }
            None => vec![],
        };
        let reserve = if probs.is_empty() { 6.0 } else { 10.0 + 20.0 * probs.len().min(4) as f32 };
        let avail = ui.available_size();
        let w = avail.x.min((avail.y - reserve).max(200.0) * 16.0 / 9.0).max(320.0);
        let (resp, painter) = ui.allocate_painter(vec2(w, w * 9.0 / 16.0), Sense::click_and_drag());
        let rect = resp.rect;
        let k = rect.width() / 1920.0;
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
                            rfd::FileDialog::new().add_filter("Images", &["png", "jpg", "jpeg", "bmp", "svg", "svgz"]).pick_file().map(|f| {
                                let mut it = it;
                                it.kind = Kind::Image { path: self.relative(&f), link: None };
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
            if resp.drag_started() {
                if let Some(pos) = resp.interact_pointer_pos() {
                    let (px, py) = to_plane(pos);
                    match hit_test_where(&snapshot, px, py, self.sel_item, 12.0 / k, &can) {
                        Some((i, h)) => {
                            self.sel_item = Some(i);
                            self.drag = Some(DragState { idx: i, handle: h, start: (px, py), orig: snapshot[i].clone() });
                        }
                        None => self.sel_item = None,
                    }
                }
            }
            if resp.dragged() {
                if let (Some(d), Some(pos)) = (&self.drag, resp.interact_pointer_pos()) {
                    let (px, py) = to_plane(pos);
                    let r = dragged_rect((d.orig.x, d.orig.y, d.orig.w, d.orig.h), d.handle, px - d.start.0, py - d.start.1, grid, 16);
                    let orig = d.orig.clone();
                    let idx = d.idx;
                    if let Some(it) = info.design_mut(&mut self.model.designs).and_then(|dd| dd.items.get_mut(idx)) {
                        set_rect(it, &orig, r);
                        self.dirty = true;
                        self.edit_gen += 1;
                    }
                }
            }
            if resp.drag_stopped() {
                self.drag = None;
                self.bump();
            } else if resp.clicked() {
                if let Some(pos) = resp.interact_pointer_pos() {
                    let (px, py) = to_plane(pos);
                    self.sel_item = hit_test_where(&snapshot, px, py, None, 0.0, &can).map(|h| h.0);
                }
            }
            // clic droit : selection de l'element sous la souris et menu contextuel
            if resp.secondary_clicked() {
                if let Some(pos) = resp.interact_pointer_pos() {
                    let (px, py) = to_plane(pos);
                    self.sel_item = hit_test_where(&snapshot, px, py, None, 0.0, &can).map(|h| h.0);
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
                if let (Some(s), Some(d)) = (self.sel_item, info.design_mut(&mut self.model.designs)) {
                    if del && s < d.items.len() {
                        d.items.remove(s);
                        self.sel_item = None;
                        self.bump();
                    } else if (dx != 0 || dy != 0) && s < d.items.len() && can(s) {
                        let it = &mut d.items[s];
                        it.x = (it.x + dx).clamp(0, 1920 - it.w);
                        it.y = (it.y + dy).clamp(0, 1080 - it.h);
                        self.bump();
                    }
                }
            }
            // surcouche : cadres des éléments
            if let Some(d) = info.design(&self.model.designs) {
                let mut slot_no = 0;
                let eff = d.slot_assignment(info.count());
                for (i, it) in d.items.iter().enumerate() {
                    if d.item_hidden(i) {
                        if matches!(it.kind, Kind::Slot { .. }) && it.fixed_video().is_none() && !info.is_free() { slot_no += 1; }
                        continue;
                    }
                    let r = Rect::from_min_max(to_screen(it.x as f32, it.y as f32), to_screen((it.x + it.w) as f32, (it.y + it.h) as f32));
                    let selected = self.sel_item == Some(i);
                    let col = kind_color(&it.kind);
                    if selected && self.preview_pulse > 0 {
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
                            None if info.is_free() || it.is_video_slot() => "?".to_string(),
                            None => {
                                let e = eff.get(slot_no).copied().unwrap_or(usize::MAX);
                                slot_no += 1;
                                if e == usize::MAX { "?".to_string() } else { (e + 1).to_string() }
                            }
                        };
                        let shown = if it.link().is_some() { format!("{shown} →") } else { shown };
                        painter.text(r.min + vec2(3.0, 2.0), egui::Align2::LEFT_TOP, shown, egui::FontId::proportional(14.0), col);
                    }
                    if selected {
                        for c in [r.left_top(), r.right_top(), r.left_bottom(), r.right_bottom()] {
                            painter.rect_filled(Rect::from_center_size(c, vec2(9.0, 9.0)), 1.0, Color32::WHITE);
                        }
                    }
                }
            }
            if let Some(pos) = resp.hover_pos() {
                let (px, py) = to_plane(pos);
                match hit_test_where(&snapshot, px, py, self.sel_item, 12.0 / k, &can) {
                    Some((_, Handle::TopLeft)) | Some((_, Handle::BottomRight)) => ctx.set_cursor_icon(egui::CursorIcon::ResizeNwSe),
                    Some((_, Handle::TopRight)) | Some((_, Handle::BottomLeft)) => ctx.set_cursor_icon(egui::CursorIcon::ResizeNeSw),
                    Some((_, Handle::Body)) => ctx.set_cursor_icon(egui::CursorIcon::Grab),
                    None => {}
                }
            }
        }

        // vérifications
        if custom {
            if !probs.is_empty() {
                egui::ScrollArea::vertical().id_salt("defil_verifs").max_height(reserve).show(ui, |ui| {
                    for p in &probs {
                        let (c, t) = if p.error { (Color32::from_rgb(230, 100, 90), "Erreur") } else { (Color32::from_rgb(230, 190, 80), "Attention") };
                        ui.colored_label(c, format!("{t} : {}", p.msg));
                    }
                });
            }
            let others = self.pages().into_iter().filter(|p| p.menu == info.menu && p.r != info.r && !p.is_free()).count();
            if others > 0 && ui.button("Reprendre le décor et les boutons sur les autres pages de ce menu").clicked() {
                self.copy_decor_to_others(&info);
            }
        }
    }

    fn copy_decor_to_others(&mut self, info: &PageInfo) {
        let Some(src) = info.design(&self.model.designs).cloned() else { return };
        for t in self.pages().into_iter().filter(|p| p.menu == info.menu && p.r != info.r && !p.is_free()) {
            let default = match t.menu {
                MenuType::Main => PageDesign::default_main(t.count(), t.page, t.pages),
                MenuType::Chapters => PageDesign::default_chapter(t.count(), t.page, t.pages),
            };
            let Ok(default) = default else { continue };
            let base = t.design(&self.model.designs).cloned().unwrap_or_else(|| default.clone());
            t.set_design(&mut self.model.designs, copy_decor(&src, base, &default));
        }
        self.notice = "Décor repris sur les autres pages.".into();
        self.bump();
    }

    fn relative(&self, f: &std::path::Path) -> String {
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
        egui::CollapsingHeader::new("Fond et animations de cette page").id_salt("reglages_page").default_open(true).show(ui, |ui| {
            changed |= page_bg_ui(ui, &mut d.bg, &mut pick);
            if let Some(n) = &note { ui.label(egui::RichText::new(n).weak()); }
            ui.separator();
            ui.strong("Animations des boutons");
            let def = bdauthor_core::design::DEFAULT_ANIM_FPS;
            let cur = if d.anim_fps == 0 { def } else { d.anim_fps };
            egui::ComboBox::from_label("images par seconde").selected_text(cur.to_string()).show_ui(ui, |ui| {
                for f in [4, 6, 8, 12, 25] {
                    if ui.selectable_label(cur == f, f.to_string()).clicked() && cur != f { d.anim_fps = if f == def { 0 } else { f }; changed = true; }
                }
            });
            ui.label(egui::RichText::new("Une seule cadence par page (norme Blu-ray), pour le halo animé et les animations d'images. Arrondie à une fraction de la cadence de la vidéo du menu.").weak());
        });
        if let Some(k) = pick {
            let (name, exts): (&str, &[&str]) = match k {
                0 => ("Images", &["png", "jpg", "jpeg", "bmp", "svg", "svgz"]),
                1 => ("Vidéos", &["mp4", "mkv", "mov", "avi", "m2ts", "mts", "ts", "webm", "wmv", "m4v", "mpg", "mpeg"]),
                _ => ("Audio", &["mp3", "wav", "flac", "m4a", "aac", "ogg", "opus", "ac3", "wma"]),
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
            ui.heading("Simulation");
            ui.label("Le menu se comporte comme sur un lecteur Blu-ray :");
            ui.label("• Flèches : passer au bouton voisin (même règle que sur le disque)");
            ui.label("• Entrée (ou clic sur un bouton) : valider ; l'état « activé » s'affiche un instant");
            ui.label("• M (ou Origine) : touche TOP MENU, retour à la page d'accueil");
            ui.label("• Échap : arrêter la simulation");
            ui.label(egui::RichText::new("Les lectures de vidéo sont simulées par un écran noir ; à la fin, le menu revient là où le disque reviendrait.").weak());
            return;
        }
        let Some(info) = self.current_info() else { return };
        // la page affichee : fond et page d'accueil
        ui.heading("Page");
        ui.horizontal_wrapped(|ui| {
            let this = match info.r { PageRef::Chapters(v, q) => Target::Chapters(v, q), _ => Target::Main(info.page) };
            let is_home = self.model.designs.home.unwrap_or(Target::Main(0)) == this;
            if is_home {
                ui.label(egui::RichText::new("🏠 Page d'accueil").strong());
            } else if ui.button("🏠 Définir comme page d'accueil").on_hover_text("Page affichée à l'ouverture du disque et par la touche TOP MENU").clicked() {
                self.model.designs.home = if this == Target::Main(0) { None } else { Some(this) };
                self.bump();
            }
        });
        if info.custom(&self.model.designs) { self.page_settings_ui(ui, &info); }
        ui.separator();
        ui.heading("Élément");
        if !info.custom(&self.model.designs) {
            ui.label("Personnalisez la page pour modifier ses éléments.");
            return;
        }
        let Some(i) = self.sel_item else {
            ui.label("Cliquez sur un élément de l'aperçu ou de la liste ci-dessus pour le modifier.\n\nGlissez pour déplacer, tirez un coin pour redimensionner, flèches du clavier pour ajuster (Maj : 10 px). Clic droit : dupliquer ou supprimer (Suppr retire aussi l'élément sélectionné).");
            return;
        };
        let mut op: Option<Op> = None;
        let mut pick_image = false;
        let mut changed = false;
        let mut pick_font = false;
        let base_dir = self.base_dir();
        let font_check = self.renderer.as_ref();
        let page_targets = self.page_targets();
        let generic = self.model.designs.generic;
        let page_names: Vec<String> = self.pages().iter().map(|p| p.label(&self.model)).collect();
        let video_durations: Vec<Option<f64>> = self.model.videos.iter().map(|v| self.durations.get(&v.file).copied()).collect();
        // langues de sous-titres du projet (le bouton choisit une langue, pas un numero de piste)
        let sub_langs: Vec<(u32, String)> = self.model.languages.iter().map(|l| (l.id, l.name.clone())).collect();
        let video_labels: Vec<String> = (0..self.model.videos.len()).map(|i| video_title(&self.model, i)).collect();
        let proj_style = self.model.designs.style.clone();
        let mut pick_skin: Option<usize> = None;
        let mut anim_preview = self.anim_preview;
        let page_fps = info.design(&self.model.designs).map_or(0, |d| d.anim_fps);
        let page_fps = if page_fps == 0 { bdauthor_core::design::DEFAULT_ANIM_FPS } else { page_fps };
        {
            let Some(d) = info.design_mut(&mut self.model.designs) else { return };
            let layers: Vec<(u32, String)> = d.layers.iter().rev().map(|l| (l.id, l.name.clone())).collect();
            let cur_layer = d.items.get(i).map_or(0, |it| it.layer);
            let Some(it) = d.items.get_mut(i) else { self.sel_item = None; return };
            ui.colored_label(kind_color(&it.kind), kind_name(&it.kind));
            if !layers.is_empty() {
                let shown = layers.iter().find(|(id, _)| *id == cur_layer).map(|(_, n)| n.clone()).unwrap_or_default();
                egui::ComboBox::from_label("Calque").selected_text(shown).show_ui(ui, |ui| {
                    for (id, name) in &layers {
                        if ui.selectable_label(*id == cur_layer, name).clicked() && *id != cur_layer { op = Some(Op::ToLayer(*id)); }
                    }
                });
            }
            egui::Grid::new("geometrie").num_columns(4).show(ui, |ui| {
                ui.label("X"); changed |= ui.add(egui::DragValue::new(&mut it.x).range(0..=1920)).changed();
                ui.label("Y"); changed |= ui.add(egui::DragValue::new(&mut it.y).range(0..=1080)).changed();
                ui.end_row();
                ui.label("Largeur"); changed |= ui.add(egui::DragValue::new(&mut it.w).range(16..=1920)).changed();
                ui.label("Hauteur"); changed |= ui.add(egui::DragValue::new(&mut it.h).range(16..=1080)).changed();
                ui.end_row();
            });
            let h = it.h;
            match &mut it.kind {
                Kind::Slot { thumb_h, video, link, start, label, thumb_at, ret, .. } => {
                    let mut lab = (h - *thumb_h).max(0);
                    ui.horizontal(|ui| {
                        ui.label("Hauteur du libellé :");
                        if ui.add(egui::DragValue::new(&mut lab).range(0..=(h - 8).max(0))).changed() {
                            *thumb_h = (h - lab).max(8);
                            changed = true;
                        }
                    });
                    // video montree
                    let vname = |v: usize| format!("Vidéo {}. {}", v + 1, video_labels.get(v).cloned().unwrap_or_default());
                    let old_video = (*video).flatten();
                    let shown = match *video { Some(Some(v)) => vname(v), _ => "(aucune vidéo : à choisir)".to_string() };
                    egui::ComboBox::from_label("Vidéo").selected_text(shown).show_ui(ui, |ui| {
                        for v in 0..video_labels.len() {
                            if ui.selectable_label(*video == Some(Some(v)), vname(v)).clicked() { *video = Some(Some(v)); changed = true; }
                        }
                    });
                    if let (Some(o), Some(nv)) = (old_video, (*video).flatten()) {
                        if o != nv && *link == Some(Target::Video(o)) { *link = Some(Target::Video(nv)); }
                    }
                    let dur = (*video).flatten().and_then(|v| video_durations.get(v).copied().flatten());
                    // depart : debut de la video ou instant (vignette de chapitre)
                    let mut chapter = start.is_some();
                    ui.horizontal(|ui| {
                        if ui.radio_value(&mut chapter, false, "Depuis le début").changed() { *start = None; changed = true; }
                        if ui.radio_value(&mut chapter, true, "Chapitre, à partir de :").changed() { *start = Some(0.0); changed = true; }
                    });
                    if let Some(t) = start {
                        ui.horizontal(|ui| {
                            let max = dur.unwrap_or(f64::MAX);
                            changed |= ui.add(egui::DragValue::new(t).range(0.0..=max).speed(0.5)
                                .custom_formatter(|v, _| mmss(v)).custom_parser(parse_mmss)).on_hover_text("Glissez, ou tapez « m:ss » (ex. 12:30) ou des secondes").changed();
                            if let Some(d) = dur { ui.label(egui::RichText::new(format!("sur {}", mmss(d))).weak()); }
                        });
                        if dur.map_or(false, |d| *t >= d) { ui.colored_label(Color32::from_rgb(230, 100, 90), "Après la fin de la vidéo."); }
                        ui.label(egui::RichText::new("Le lecteur démarre à l'image clé la plus proche avant cet instant (au plus 1 s plus tôt).").weak());
                    }
                    // libelle et image
                    ui.horizontal(|ui| {
                        ui.label("Libellé :");
                        let hint = match start { Some(t) => mmss(*t), None => (*video).flatten().and_then(|v| video_labels.get(v).cloned()).unwrap_or_default() };
                        changed |= ui.add(egui::TextEdit::singleline(label).hint_text(hint)).changed();
                    });
                    let mut own_img = thumb_at.is_some();
                    if ui.checkbox(&mut own_img, "Image de la vignette prise à un autre instant").changed() {
                        *thumb_at = if own_img { Some(start.unwrap_or(0.0)) } else { None };
                        changed = true;
                    }
                    if let Some(a) = thumb_at {
                        changed |= ui.add(egui::DragValue::new(a).range(0.0..=dur.unwrap_or(f64::MAX)).speed(0.5).custom_formatter(|v, _| mmss(v)).custom_parser(parse_mmss)).changed();
                    }
                    // au clic : lecture (habituel) ou autre destination
                    let habit = if start.is_some() { "Lire la vidéo depuis ce chapitre" } else { "Lire la vidéo" };
                    let shown = link.and_then(|t| page_targets.iter().find(|(x, _)| *x == t.without_exit()).map(|(_, l)| l.clone())).unwrap_or_else(|| if link.is_some() { "(destination introuvable)".into() } else { habit.to_string() });
                    egui::ComboBox::from_label("Au clic").selected_text(shown).show_ui(ui, |ui| {
                        if ui.selectable_label(link.is_none(), habit).clicked() { *link = None; changed = true; }
                        for (t, l) in &page_targets {
                            if ui.selectable_label(link.map(Target::without_exit) == Some(*t), l).clicked() && link.map(Target::without_exit) != Some(*t) { *link = Some(*t); changed = true; }
                        }
                    });
                    // apres la lecture
                    if matches!(link, None | Some(Target::Video(_))) {
                        let rname = |r: SlotReturn| match r {
                            SlotReturn::Origin => "Revenir à cette page".to_string(),
                            SlotReturn::Home => "Aller à la page d'accueil".to_string(),
                            SlotReturn::Page(p) => format!("Aller à la page {}", page_names.get(p).cloned().unwrap_or_else(|| format!("{} (introuvable)", p + 1))),
                        };
                        egui::ComboBox::from_label("Après la lecture").selected_text(rname(*ret)).show_ui(ui, |ui| {
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
                        target.and_then(|t| page_targets.iter().find(|(x, _)| *x == t.without_exit()).map(|(_, l)| l.clone())).unwrap_or_else(|| "(destination introuvable : à choisir)".to_string())
                    };
                    egui::ComboBox::from_label("Destination").selected_text(shown).show_ui(ui, |ui| {
                        for a in NavAction::ALL {
                            if a == NavAction::Goto { continue; }
                            // « Retour » n'existe que dans les anciens menus de chapitres
                            if generic && a == NavAction::Back && old != NavAction::Back { continue; }
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
                    ui.horizontal(|ui| { ui.label("Texte :"); changed |= ui.text_edit_singleline(text).changed(); });
                }
                Kind::Text { text, size, color, align, font, link } => {
                    link_ui(ui, link, &page_targets, &mut changed);
                    changed |= ui.add(egui::TextEdit::multiline(text).desired_rows(3).desired_width(f32::INFINITY)).changed();
                    ui.horizontal(|ui| {
                        ui.label("Taille :");
                        changed |= ui.add(egui::DragValue::new(size).range(8..=400).suffix(" px")).changed();
                        ui.label("Couleur :");
                        changed |= ui.color_edit_button_srgb(color).changed();
                    });
                    let shown = if font.is_empty() { "Police par défaut".to_string() } else {
                        bdauthor_render::common_fonts().into_iter().find(|(_, f)| f == font).map(|(l, _)| l.to_string()).unwrap_or_else(|| font.clone())
                    };
                    egui::ComboBox::from_label("Police").selected_text(shown).show_ui(ui, |ui| {
                        if ui.selectable_label(font.is_empty(), "Police par défaut").clicked() { font.clear(); changed = true; }
                        for (label, file) in bdauthor_render::common_fonts() {
                            if ui.selectable_label(font == file, label).clicked() { *font = file.to_string(); changed = true; }
                        }
                        if ui.button("Autre fichier…").clicked() { pick_font = true; ui.close_menu(); }
                    });
                    if !font.is_empty() {
                        if let Ok(r) = font_check {
                            if r.custom_font(font, &base_dir).is_none() {
                                ui.colored_label(Color32::from_rgb(230, 100, 90), "Police introuvable : la police par défaut sera utilisée.");
                            }
                        }
                    }
                    egui::ComboBox::from_label("Alignement").selected_text(align.name()).show_ui(ui, |ui| {
                        for a in [Align::Left, Align::Center, Align::Right] {
                            if ui.selectable_value(align, a, a.name()).changed() { changed = true; }
                        }
                    });
                }
                Kind::Image { path, link } => {
                    link_ui(ui, link, &page_targets, &mut changed);
                    ui.horizontal(|ui| { ui.label("Fichier :"); changed |= ui.text_edit_singleline(path).changed(); });
                    if ui.button("Choisir…").clicked() { pick_image = true; }
                    ui.label(egui::RichText::new("L'image est ajustée dans le cadre sans déformation.").weak());
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
                        SubChoice::Keep | SubChoice::Off => "Masquer les sous-titres".to_string(),
                        SubChoice::Lang(id) => sub_langs.iter().find(|(i, _)| *i == id).map_or_else(|| "(langue supprimée : à choisir)".to_string(), |(_, n)| n.clone()),
                        SubChoice::Track(k) => format!("Piste {} (ancien réglage : à remplacer par une langue)", k + 1),
                    };
                    let cur_sub = it.sub;
                    egui::ComboBox::from_label("Sous-titres").selected_text(name(cur_sub)).show_ui(ui, |ui| {
                        for c in std::iter::once(SubChoice::Off).chain(sub_langs.iter().map(|(id, _)| SubChoice::Lang(*id))) {
                            if ui.selectable_label(cur_sub == c, name(c)).clicked() { it.sub = c; changed = true; }
                        }
                    });
                }
                if let Some(t) = cur.filter(|t| t.is_setting()) {
                    // page de sortie : les pages du menu seulement
                    let pages: Vec<(usize, &String)> = page_targets.iter().filter_map(|(x, l)| if let Target::Main(q) = x { Some((*q, l)) } else { None }).collect();
                    let here = "Rester sur la page courante";
                    let exit = t.exit();
                    let shown = exit.map_or_else(|| here.to_string(), |q| pages.iter().find(|(p, _)| *p == q).map_or_else(|| "(page introuvable : à choisir)".to_string(), |(_, l)| (*l).clone()));
                    let mut pick: Option<Option<usize>> = None;
                    egui::ComboBox::from_label("Ensuite").selected_text(shown).show_ui(ui, |ui| {
                        if ui.selectable_label(exit.is_none(), here).clicked() { pick = Some(None); }
                        for (q, l) in &pages {
                            if ui.selectable_label(exit == Some(*q), l.as_str()).clicked() { pick = Some(Some(*q)); }
                        }
                    });
                    if let Some(e) = pick {
                        if let Some(slot) = it.targets_mut() { *slot = Some(t.with_exit(e)); changed = true; }
                    }
                    let after = if exit.is_some() { "puis le menu passe à la page choisie" } else { "et le menu reste affiché" };
                    let txt = match t {
                        Target::Audio(..) => format!("Le clic règle la langue audio {after}. Elle vaut pour toutes les vidéos lues ensuite ; une vidéo qui n'a pas cette langue garde son audio actuel. Le spectateur peut toujours changer avec la touche AUDIO. Les sous-titres ne sont pas modifiés."),
                        _ if sub_langs.is_empty() => "Aucune langue de sous-titres dans le projet (à ajouter dans le menu « Édition > Langues du projet… »).".to_string(),
                        _ => format!("Le clic règle les sous-titres {after}. La langue vaut pour toutes les vidéos lues ensuite ; une vidéo qui n'a pas cette langue s'affiche sans sous-titres. Le spectateur peut toujours changer avec la touche SUBTITLE."),
                    };
                    ui.label(egui::RichText::new(txt).weak());
                }
            }
            // apparence propre a ce bouton
            if it.is_button() {
                ui.separator();
                let mut own = it.look.style.is_some();
                if ui.checkbox(&mut own, "Apparence propre à cet élément").on_hover_text("Sinon : le style des boutons du projet").changed() {
                    it.look.style = if own { Some(proj_style.clone()) } else { None };
                    changed = true;
                }
                if let Some(st) = &mut it.look.style {
                    egui::CollapsingHeader::new("Réglages de cet élément").default_open(true).show(ui, |ui| {
                        changed |= crate::style_ui::style_ui(ui, st, "element");
                    });
                }
                if !matches!(it.kind, Kind::Slot { .. }) {
                    changed |= crate::style_ui::skin_ui(ui, &mut it.look.skin, &mut pick_skin, &mut anim_preview, page_fps);
                }
            }
        }
        if pick_font {
            if let Some(f) = rfd::FileDialog::new().add_filter("Polices", &["ttf", "otf"]).pick_file() {
                let rel = self.relative(&f);
                if let Some(Kind::Text { font, .. }) = info.design_mut(&mut self.model.designs).and_then(|d| d.items.get_mut(i)).map(|it| &mut it.kind) {
                    *font = rel;
                    changed = true;
                }
            }
        }
        self.anim_preview = anim_preview;
        if let Some(state @ (0 | 1)) = pick_skin {
            // image(s) « normal » ou « selectionne » : une = etat fixe, plusieurs = animation (rangees par ordre de nom)
            if let Some(mut fs) = rfd::FileDialog::new().add_filter("Images", &["png", "jpg", "jpeg", "bmp", "svg", "svgz"]).pick_files() {
                fs.sort();
                let rel: Vec<String> = fs.iter().map(|f| self.relative(f)).collect();
                if let Some(it) = info.design_mut(&mut self.model.designs).and_then(|d| d.items.get_mut(i)) {
                    if state == 0 { it.look.skin.set_normal(rel); } else { it.look.skin.set_selected(rel); }
                    if it.look.skin.sel_frames.is_empty() && it.look.skin.normal_frames.is_empty() { self.anim_preview = false; }
                    changed = true;
                }
            }
        } else if let Some(state) = pick_skin {
            if let Some(f) = rfd::FileDialog::new().add_filter("Images", &["png", "jpg", "jpeg", "bmp", "svg", "svgz"]).pick_file() {
                let rel = self.relative(&f);
                if let Some(it) = info.design_mut(&mut self.model.designs).and_then(|d| d.items.get_mut(i)) {
                    let sk = &mut it.look.skin;
                    *[&mut sk.normal, &mut sk.selected, &mut sk.activated][state.min(2)] = rel;
                    changed = true;
                }
            }
        }
        if pick_image {
            if let Some(f) = rfd::FileDialog::new().add_filter("Images", &["png", "jpg", "jpeg", "bmp", "svg", "svgz"]).pick_file() {
                let rel = self.relative(&f);
                if let Some(Kind::Image { path, .. }) = info.design_mut(&mut self.model.designs).and_then(|d| d.items.get_mut(i)).map(|it| &mut it.kind) {
                    *path = rel;
                    changed = true;
                }
            }
        }
        if let Some(op) = op {
            if let Some(d) = info.design_mut(&mut self.model.designs) {
                match op {
                    Op::ToLayer(id) => {
                        let end = d.layers.iter().position(|l| l.id == id).map_or(d.items.len(), |p| d.layer_range(p).end);
                        self.sel_item = Some(move_item(d, i, end, id));
                    }
                }
            }
            changed = true;
        }
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
        ui.heading("Pages et calques");
        if self.model.videos.is_empty() {
            ui.label("Ajoutez des vidéos dans l'onglet « Projet ».");
            return;
        }
        if !self.ensure_generic() {
            ui.label("Préparation des pages (lecture de la durée des vidéos à chapitres automatiques)…\nSi cela dure, vérifiez dans l'onglet « Projet » que ces vidéos sont bien trouvées.");
            return;
        }
        let mut op: Option<LayerOp> = None;
        let info = self.current_info();
        let cur_k = match info.as_ref().map(|i| i.r) { Some(PageRef::Free(k)) => Some(k), _ => None };
        ui.horizontal_wrapped(|ui| {
            if ui.button("+ Page").on_hover_text("Ajoute une page vierge à la fin").clicked() {
                let f = bdauthor_core::editor::add_free_page(&mut self.model);
                self.cur = Some(PageRef::Free(f));
                self.sel_item = None;
                self.bump();
            }
            if cur_k.is_some() && ui.button("+ Calque").on_hover_text("Nouveau calque au premier plan de la page affichée ; les nouveaux éléments vont dans le calque actif").clicked() {
                op = Some(LayerOp::Add);
            }
            if ui.small_button("+ tout").on_hover_text("Déplier toutes les pages et tous les calques").clicked() {
                for k in 0..self.model.designs.free.len() { self.tree_pages.insert(k, true); }
                self.tree_layers_closed.clear();
            }
            if ui.small_button("− tout").on_hover_text("Replier toutes les pages").clicked() {
                for k in 0..self.model.designs.free.len() { self.tree_pages.insert(k, false); }
            }
        });
        let np = self.model.designs.free.len();
        if np == 0 {
            ui.label("Aucune page : ajoutez-en une.");
            return;
        }
        let home_k = match self.model.designs.home { Some(Target::Main(p)) if p < np => p, _ => 0 };
        let names: Vec<String> = self.model.designs.free.iter().enumerate().map(|(k, d)| if d.name.is_empty() { format!("Page {}", k + 1) } else { d.name.clone() }).collect();
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
                            .response.on_hover_text("Glisser pour changer l'ordre des pages");
                        if ui.small_button(pm(opened[k])).on_hover_text(if opened[k] { "Replier la page" } else { "Déplier la page" }).clicked() { toggle_page = Some(k); }
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
                            let resp = ui.selectable_label(shown, label).on_hover_text("Afficher la page (+ pour déplier son contenu). Double-clic : renommer. 🏠 : page d'accueil");
                            if resp.clicked() { pick_page = Some(k); }
                            if resp.double_clicked() { self.page_edit = Some((k, all[k].name.clone())); }
                            if ui.small_button("✏").on_hover_text("Renommer la page").clicked() { self.page_edit = Some((k, all[k].name.clone())); }
                            let _ = ui.menu_button("🗑", |ui| {
                                ui.label("Les liens qui menaient à cette page seront à rechoisir.");
                                if ui.button("Supprimer la page").clicked() { op = Some(LayerOp::PageDelete(k)); ui.close_menu(); }
                            }).response.on_hover_text("Supprimer la page");
                        }
                    }).response;
                    rows.push((DragRow::Page(k), r.rect));
                    if !opened[k] { continue; }
                    // ---- calques et elements de la page
                    let d = &all[k];
                    let layered = !d.layers.is_empty();
                    let positions: Vec<usize> = if layered { (0..d.layers.len()).rev().collect() } else { vec![0] };
                    if d.items.is_empty() && !layered {
                        ui.horizontal(|ui| { ui.add_space(40.0); ui.label(egui::RichText::new("(page vide)").weak().italics()); });
                    }
                    for pos in positions {
                        let mut layer_open = true;
                        if layered {
                            let l = d.layers[pos].clone();
                            layer_open = !self.tree_layers_closed.contains(&(k, l.id));
                            let r = ui.horizontal(|ui| {
                                ui.add_space(18.0);
                                let _ = ui.dnd_drag_source(egui::Id::new(("calque", k, l.id)), DragRow::Layer(k, pos), |ui| ui.label(egui::RichText::new("☰").strong()))
                                    .response.on_hover_text("Glisser pour déplacer le calque (avec ses éléments), ou le déposer sur une autre page");
                                if ui.small_button(pm(layer_open)).on_hover_text(if layer_open { "Replier le calque" } else { "Déplier le calque" }).clicked() { toggle_layer = Some((k, l.id)); }
                                if ui.small_button(if l.hidden { "◌" } else { "👁" }).on_hover_text(if l.hidden { "Calque masqué dans l'aperçu (il reste sur le disque) : afficher" } else { "Masquer dans l'aperçu (le calque reste sur le disque)" }).clicked() { op = Some(LayerOp::Hidden(k, l.id)); }
                                if ui.small_button(if l.locked { "🔒" } else { "🔓" }).on_hover_text(if l.locked { "Verrouillé : ses éléments ne se sélectionnent plus dans l'aperçu ; déverrouiller" } else { "Verrouiller (plus de sélection ni de déplacement dans l'aperçu)" }).clicked() { op = Some(LayerOp::Locked(k, l.id)); }
                                let editing = matches!(&self.layer_edit, Some((ep, eid, _)) if *ep == k && *eid == l.id);
                                if editing {
                                    if let Some((_, _, text)) = &mut self.layer_edit {
                                        let te = ui.add(egui::TextEdit::singleline(text).desired_width(130.0));
                                        if !te.has_focus() && !te.lost_focus() { te.request_focus(); }
                                        if te.lost_focus() { edit_done = true; }
                                    }
                                } else {
                                    let n = d.layer_range(pos).len();
                                    let resp = ui.selectable_label(shown && active == l.id, egui::RichText::new(format!("{}  ({n})", l.name))).on_hover_text("Calque actif : reçoit les nouveaux éléments. Double-clic : renommer");
                                    if resp.clicked() { activate = Some((k, l.id)); }
                                    if resp.double_clicked() { self.layer_edit = Some((k, l.id, l.name.clone())); }
                                    if ui.small_button("✏").on_hover_text("Renommer le calque").clicked() { self.layer_edit = Some((k, l.id, l.name.clone())); }
                                    let _ = ui.menu_button("🗑", |ui| {
                                        if ui.button("Supprimer le calque (ses éléments vont au calque voisin)").clicked() { op = Some(LayerOp::Remove(k, l.id, true)); ui.close_menu(); }
                                        if ui.button("Supprimer le calque et ses éléments").clicked() { op = Some(LayerOp::Remove(k, l.id, false)); ui.close_menu(); }
                                    }).response.on_hover_text("Supprimer le calque");
                                }
                            }).response;
                            rows.push((DragRow::Layer(k, pos), r.rect));
                        }
                        if !layer_open { continue; }
                        let indent = if layered { 40.0 } else { 18.0 };
                        for i in d.layer_range(pos).rev() {
                            let it = &d.items[i];
                            let what = match &it.kind {
                                Kind::Text { text, link, .. } => format!("Texte « {} »{}", text.lines().next().unwrap_or("").chars().take(24).collect::<String>(), if link.is_some() { " (cliquable)" } else { "" }),
                                Kind::Image { path, link } => format!("Image {}{}", std::path::Path::new(path).file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default(), if link.is_some() { " (cliquable)" } else { "" }),
                                Kind::Nav { action, text, .. } => if *action == NavAction::Goto { format!("Bouton « {text} »") } else { format!("Bouton {}", action.label()) },
                                Kind::Slot { video, link, start, .. } => format!("Vignette{}{}{}", match video { Some(Some(v)) => format!(" (vidéo {})", v + 1), Some(None) => " (sans vidéo)".to_string(), None => String::new() },
                                    start.map(|t| format!(" à {}", mmss(t))).unwrap_or_default(), if link.is_some() { " →" } else { "" }),
                            };
                            let mut col = kind_color(&it.kind);
                            if d.item_hidden(i) { col = col.gamma_multiply(0.45); }
                            let r = ui.horizontal(|ui| {
                                ui.add_space(indent);
                                let _ = ui.dnd_drag_source(egui::Id::new(("element", k, i)), DragRow::Item(k, i), |ui| ui.label(egui::RichText::new("☰").weak()))
                                    .response.on_hover_text("Glisser pour changer l'ordre ou le calque, ou déposer sur une autre page");
                                let lr = ui.selectable_label(shown && self.sel_item == Some(i), egui::RichText::new(what).color(col));
                                if lr.clicked() || lr.secondary_clicked() { pick = Some((k, i)); }
                                lr.context_menu(|ui| if let Some(dup) = item_menu(ui) { item_act = Some((k, i, dup)); });
                            }).response;
                            rows.push((DragRow::Item(k, i), r.rect));
                        }
                        if layered && d.layer_range(pos).is_empty() {
                            ui.horizontal(|ui| { ui.add_space(40.0); ui.label(egui::RichText::new("(vide : glissez-y des éléments)").weak().italics()); });
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
            if me.cur != Some(PageRef::Free(k)) {
                me.cur = Some(PageRef::Free(k));
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
        let cur_now = match self.cur { Some(PageRef::Free(k)) => Some(k), _ => None };
        match op {
            LayerOp::Add => {
                if let Some(dm) = cur_k.and_then(|k| self.model.designs.free.get_mut(k)) { self.active_layer = add_layer(dm); }
            }
            LayerOp::PageRename(k, name) => {
                if let Some(p) = self.model.designs.free.get_mut(k) { p.name = name; }
                self.edit_gen += 1;
            }
            LayerOp::PageDelete(k) => {
                if let Some(pi) = self.pages().into_iter().find(|p| p.r == PageRef::Free(k)) { pi.remove_design(&mut self.model.designs); }
                if let Some(ck) = cur_now {
                    if ck == k { self.sel_item = None; }
                    self.cur = Some(PageRef::Free(if ck > k || (ck == k && k > 0) { ck - 1 } else { ck }));
                }
                // etats replie / deplie : indices decales
                self.tree_pages = self.tree_pages.iter().filter(|e| *e.0 != k).map(|(&p, &o)| (if p > k { p - 1 } else { p }, o)).collect();
                self.tree_layers_closed = self.tree_layers_closed.iter().filter(|e| e.0 != k).map(|&(p, l)| (if p > k { p - 1 } else { p }, l)).collect();
                self.edit_gen += 1;
            }
            LayerOp::PageMove { from, to } => {
                bdauthor_core::editor::move_page(&mut self.model, from, to);
                let map = |c: usize| if c == from { to } else if from < to && c > from && c <= to { c - 1 } else if to < from && c >= to && c < from { c + 1 } else { c };
                if let Some(ck) = cur_now { self.cur = Some(PageRef::Free(map(ck))); }
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
                self.notice = format!("{} vers la page « {} ».", if copy { "Copié" } else { "Déplacé" }, names.get(to).cloned().unwrap_or_default());
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
        self.sel_item = None;
        self.drag = None;
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
                Event::Error(m) => self.notice = m,
            }
        }
    }
}

/// Reglages du fond d'une page. `pick` : fichier a choisir (0 image, 1 video, 2 musique). Retourne vrai si quelque chose a change.
fn page_bg_ui(ui: &mut egui::Ui, bg: &mut bdauthor_core::design::PageBg, pick: &mut Option<u8>) -> bool {
    use bdauthor_core::design::{PageMusic, PageVisual};
    let before = bg.clone();
    ui.strong("Image ou vidéo");
    let kind = match bg.visual { PageVisual::Project => 0, PageVisual::Image(_) => 1, PageVisual::Video(..) => 2 };
    ui.horizontal(|ui| {
        if ui.selectable_label(kind == 0, "Comme le projet").clicked() { bg.visual = PageVisual::Project; }
        if ui.selectable_label(kind == 1, "Image").clicked() && kind != 1 { *pick = Some(0); }
        if ui.selectable_label(kind == 2, "Vidéo").clicked() && kind != 2 { *pick = Some(1); }
    });
    match &mut bg.visual {
        PageVisual::Image(f) => {
            ui.horizontal(|ui| { ui.label("Image :"); ui.text_edit_singleline(f); if ui.button("Choisir…").clicked() { *pick = Some(0); } });
        }
        PageVisual::Video(f, son) => {
            ui.horizontal(|ui| { ui.label("Vidéo :"); ui.text_edit_singleline(f); if ui.button("Choisir…").clicked() { *pick = Some(1); } });
            ui.checkbox(son, "Garder le son de la vidéo (s'il n'y a pas de musique)");
            ui.label(egui::RichText::new("Jouée en entier, en boucle ; les textes et images de la page sont posés dessus.").weak());
        }
        PageVisual::Project => {}
    }
    ui.separator();
    ui.strong("Musique");
    let mk = match bg.music { PageMusic::Project => 0, PageMusic::Silent => 1, PageMusic::File(_) => 2 };
    ui.horizontal(|ui| {
        if ui.selectable_label(mk == 0, "Comme le projet").clicked() { bg.music = PageMusic::Project; }
        if ui.selectable_label(mk == 1, "Aucune").clicked() { bg.music = PageMusic::Silent; }
        if ui.selectable_label(mk == 2, "Autre musique").clicked() && mk != 2 { *pick = Some(2); }
    });
    if let PageMusic::File(f) = &mut bg.music {
        ui.horizontal(|ui| { ui.label("Fichier :"); ui.text_edit_singleline(f); if ui.button("Choisir…").clicked() { *pick = Some(2); } });
    }
    ui.label(egui::RichText::new("Chaque page au fond différent a son propre clip sur le disque (encodé une fois, puis gardé en cache).").weak());
    *bg != before
}
