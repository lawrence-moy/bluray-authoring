//! Editeur visuel de mise en page (onglet « Mise en page ») : apercu fidele, glisser/redimensionner, inspecteur.
use crate::App;
use bdauthor_core::design::{Align, Item, Kind, MenuType, NavAction, PageDesign, Target};
use bdauthor_core::editor::{copy_decor, dragged_rect, hit_test, list_pages, move_in_list, set_rect, to_back, to_front, video_title, Handle, PageInfo, PageRef};
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
#[derive(Clone, Copy)]
enum SlotShow { Video(usize), Element(usize), Missing }

enum Op { Delete, Duplicate, Up, Down, Front, Back, Assign(Option<usize>) }

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
        *link = if on { targets.first().map(|t| t.0) } else { None };
        *changed = true;
    }
    if link.is_some() {
        destination_combo(ui, link, targets, changed);
        ui.label(egui::RichText::new("Sélectionné à la télécommande, l'élément est entouré d'un cadre bleu.").weak());
    }
}

/// Liste des destinations possibles : pages du menu puis videos a lire.
fn destination_combo(ui: &mut egui::Ui, target: &mut Option<Target>, targets: &[(Target, String)], changed: &mut bool) {
    let shown = target.and_then(|t| targets.iter().find(|(x, _)| *x == t).map(|(_, l)| l.clone())).unwrap_or_else(|| "(destination introuvable : à choisir)".to_string());
    egui::ComboBox::from_label("Destination").selected_text(shown).show_ui(ui, |ui| {
        for (t, label) in targets {
            if ui.selectable_label(*target == Some(*t), label).clicked() { *target = Some(*t); *changed = true; }
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
        for (file, at) in self.slot_sources(info) {
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
            if !matches!(it.kind, Kind::Slot { .. }) { continue; }
            out.push(match it.fixed_video() {
                Some(v) if v < n => SlotShow::Video(v),
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
            SlotShow::Video(v) => Some(self.video_thumb(v)),
            SlotShow::Element(e) => elems.get(e).cloned(),
            SlotShow::Missing => None,
        }).collect()
    }

    /// Contenu de chaque vignette pour l'apercu, dans l'ordre de la liste.
    fn preview_slots(&self, info: &PageInfo) -> Vec<PreviewSlot> {
        let (sources, labels) = (self.element_sources(info), self.element_labels(info));
        self.slot_plan(info).into_iter().map(|s| {
            let (label, src) = match s {
                SlotShow::Video(v) => (video_title(&self.model, v), Some(self.video_thumb(v))),
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
        let Ok(renderer) = &self.renderer else { return };
        let base = if self.model.bg.is_empty() {
            None
        } else {
            let path = abs_path(&base_dir, &self.model.bg);
            if self.base_cache.as_ref().map_or(true, |(p, _)| *p != path) {
                self.base_cache = renderer.cached_image(&path).ok().map(|im| (path.clone(), cover_resize(&im, PREVIEW.0, PREVIEW.1)));
            }
            self.base_cache.as_ref().map(|(_, i)| i.clone())
        };
        let sel = if info.custom(&self.model.designs) { self.sel_item } else { None };
        let img = renderer.render_page_preview(&design, info.menu, info.page, info.pages, &slots, base.as_ref(), PREVIEW, &base_dir, &self.model.designs.style, sel);
        self.preview_sel = self.sel_item;
        let ci = egui::ColorImage::from_rgba_unmultiplied([img.width as usize, img.height as usize], &img.data);
        match &mut self.preview_tex {
            Some(t) => t.set(ci, egui::TextureOptions::LINEAR),
            None => self.preview_tex = Some(ctx.load_texture("apercu", ci, egui::TextureOptions::LINEAR)),
        }
    }

    // ------------------------------------------------------------------ liste des pages (panneau de gauche)
    pub(crate) fn page_list_ui(&mut self, ui: &mut egui::Ui) {
        ui.heading("Pages de menu");
        if !self.model.videos.is_empty() && ui.button("+ Page libre").on_hover_text("Ajoute une page vierge à la suite du menu principal ; ses vignettes peuvent montrer n'importe quelle vidéo.").clicked() {
            let f = bdauthor_core::editor::add_free_page(&mut self.model);
            self.cur = Some(bdauthor_core::editor::PageRef::Free(f));
            self.sel_item = None;
            self.bump();
        }
        let pages = self.pages();
        if pages.is_empty() {
            ui.label("Ajoutez des vidéos dans l'onglet « Projet ».");
            return;
        }
        let cur = self.current_info().map(|p| p.r);
        let mut pick = None;
        egui::ScrollArea::vertical().show(ui, |ui| {
            for p in &pages {
                let mark = if p.is_free() { "◆ " } else if p.custom(&self.model.designs) { "● " } else { "○ " };
                if ui.selectable_label(cur == Some(p.r), format!("{mark}{}", p.label(&self.model))).clicked() { pick = Some(p.r); }
            }
        });
        ui.separator();
        ui.label(egui::RichText::new("● mise en page personnalisée\n○ grille automatique\n◆ page libre").weak());
        if let Some(r) = pick {
            if self.cur != Some(r) {
                self.cur = Some(r);
                self.sel_item = None;
                self.drag = None;
                self.edit_gen += 1;
            }
        }
    }

    // ------------------------------------------------------------------ zone centrale : barre d'outils + apercu
    pub(crate) fn layout_central(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        let Some(info) = self.current_info() else {
            ui.label("Ajoutez des vidéos dans l'onglet « Projet » pour composer les pages de menu.");
            return;
        };
        if let Err(e) = &self.renderer {
            ui.colored_label(Color32::from_rgb(230, 100, 90), format!("Aperçu indisponible : {e}"));
            return;
        }
        self.request_media(&info);
        let custom = info.custom(&self.model.designs);

        ui.horizontal_wrapped(|ui| {
            if ui.add_enabled(self.history.can_undo(), egui::Button::new("↶ Annuler")).on_hover_text("Ctrl+Z").clicked() { self.undo_action(); }
            if ui.add_enabled(self.history.can_redo(), egui::Button::new("↷ Rétablir")).on_hover_text("Ctrl+Y ou Ctrl+Maj+Z").clicked() { self.redo_action(); }
            ui.separator();
            if ui.button("Style des boutons…").on_hover_text("Couleurs, cadre, halo, agrandissement des boutons de tout le projet").clicked() { self.show_style = true; }
            ui.separator();
            ui.strong(info.label(&self.model));
            if !custom {
                if ui.button("Personnaliser cette page").clicked() {
                    if let Ok(d) = info.starting_point(&self.model) {
                        info.set_design(&mut self.model.designs, d);
                        self.sel_item = None;
                        self.bump();
                    }
                }
            } else {
                let free = info.is_free();
                if ui.button(if free { "Supprimer cette page" } else { "Revenir à la grille automatique" }).clicked() {
                    info.remove_design(&mut self.model.designs);
                    self.sel_item = None;
                    if free { self.cur = None; }
                    self.bump();
                }
            }
        });
        if custom {
            let mut add: Option<Item> = None;
            let mut pick_image = false;
            ui.horizontal_wrapped(|ui| {
                if ui.button("+ Texte").clicked() { add = Some(Item::text("Texte", 660, 470, 600, 120, 60)); }
                if ui.button("+ Image…").clicked() { pick_image = true; }
                ui.menu_button("+ Bouton", |ui| {
                    for a in NavAction::ALL {
                        if ui.button(a.label()).clicked() { add = Some(Item::nav(a, 760, 940, 360, 80)); ui.close_menu(); }
                    }
                });
                if ui.button("+ Vignette").clicked() { add = Some(Item { x: 100, y: 300, w: 400, h: 250, kind: Kind::Slot { thumb_h: 200, assign: None, video: if info.is_free() { Some(None) } else { None }, link: None }, look: Default::default() }); }
                ui.separator();
                ui.checkbox(&mut self.snap, "Aimantation 10 px");
            });
            if pick_image {
                if let Some(f) = rfd::FileDialog::new().add_filter("Images", &["png", "jpg", "jpeg", "bmp", "svg", "svgz"]).pick_file() {
                    let rel = self.relative(&f);
                    add = Some(Item { x: 100, y: 100, w: 400, h: 300, kind: Kind::Image { path: rel, link: None }, look: Default::default() });
                }
            }
            if let Some(it) = add {
                if let Some(d) = info.design_mut(&mut self.model.designs) {
                    d.items.push(it);
                    self.sel_item = Some(d.items.len() - 1);
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
        if (self.preview_gen != self.edit_gen && due) || self.preview_tex.is_none() || (self.preview_sel != self.sel_item && self.drag.is_none()) {
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

        let avail = ui.available_size();
        let w = avail.x.min((avail.y - 90.0).max(200.0) * 16.0 / 9.0).max(320.0);
        let (resp, painter) = ui.allocate_painter(vec2(w, w * 9.0 / 16.0), Sense::click_and_drag());
        let rect = resp.rect;
        let k = rect.width() / 1920.0;
        match &self.preview_tex {
            Some(t) => painter.image(t.id(), rect, Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), Color32::WHITE),
            None => painter.rect_filled(rect, 0.0, Color32::from_gray(30)),
        };
        let to_screen = |x: f32, y: f32| pos2(rect.min.x + x * k, rect.min.y + y * k);
        let to_plane = |p: egui::Pos2| ((p.x - rect.min.x) / k, (p.y - rect.min.y) / k);

        if custom {
            // interactions
            let snapshot: Vec<Item> = info.design(&self.model.designs).map(|d| d.items.clone()).unwrap_or_default();
            let grid = if self.snap { 10 } else { 0 };
            if resp.drag_started() {
                if let Some(pos) = resp.interact_pointer_pos() {
                    let (px, py) = to_plane(pos);
                    match hit_test(&snapshot, px, py, self.sel_item, 12.0 / k) {
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
                    self.sel_item = hit_test(&snapshot, px, py, None, 0.0).map(|h| h.0);
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
                    } else if (dx != 0 || dy != 0) && s < d.items.len() {
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
                    let r = Rect::from_min_max(to_screen(it.x as f32, it.y as f32), to_screen((it.x + it.w) as f32, (it.y + it.h) as f32));
                    let selected = self.sel_item == Some(i);
                    let col = kind_color(&it.kind);
                    if selected {
                        painter.rect_filled(r, 0.0, Color32::from_rgba_unmultiplied(255, 255, 255, 28));
                        painter.rect_stroke(r, 0.0, Stroke::new(2.5_f32, Color32::WHITE));
                    } else {
                        painter.rect_stroke(r, 0.0, Stroke::new(1.0_f32, col));
                    }
                    if matches!(it.kind, Kind::Slot { .. }) {
                        let shown = match it.fixed_video() {
                            Some(v) => format!("V{}", v + 1),
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
                let items: Vec<Item> = info.design(&self.model.designs).map(|d| d.items.clone()).unwrap_or_default();
                match hit_test(&items, px, py, self.sel_item, 12.0 / k) {
                    Some((_, Handle::TopLeft)) | Some((_, Handle::BottomRight)) => ctx.set_cursor_icon(egui::CursorIcon::ResizeNwSe),
                    Some((_, Handle::TopRight)) | Some((_, Handle::BottomLeft)) => ctx.set_cursor_icon(egui::CursorIcon::ResizeNeSw),
                    Some((_, Handle::Body)) => ctx.set_cursor_icon(egui::CursorIcon::Grab),
                    None => {}
                }
            }
        }

        // vérifications
        if custom {
            if let Some(d) = info.design(&self.model.designs) {
                let targets = self.page_targets();
                let mut probs = if info.is_free() { d.validate_free(info.page, info.pages, self.model.videos.len()) } else { d.validate(info.count(), info.menu, info.page, info.pages) };
                probs.extend(d.check_targets(&|t| targets.iter().any(|(x, _)| *x == t)));
                for p in probs {
                    let (c, t) = if p.error { (Color32::from_rgb(230, 100, 90), "Erreur") } else { (Color32::from_rgb(230, 190, 80), "Attention") };
                    ui.colored_label(c, format!("{t} : {}", p.msg));
                }
            }
            let others = self.pages().into_iter().filter(|p| p.menu == info.menu && p.r != info.r && !p.is_free()).count();
            if others > 0 && ui.button("Reprendre le décor et les boutons sur les autres pages de ce menu").clicked() {
                self.copy_decor_to_others(&info);
            }
        } else {
            ui.label(egui::RichText::new("Cette page utilise la grille automatique. « Personnaliser » permet de placer librement vignettes, boutons, textes et images.").weak());
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
    pub(crate) fn inspector_ui(&mut self, ui: &mut egui::Ui) {
        ui.heading("Élément");
        let Some(info) = self.current_info() else { return };
        if !info.custom(&self.model.designs) {
            ui.label("Personnalisez la page pour modifier ses éléments.");
            return;
        }
        let Some(i) = self.sel_item else {
            ui.label("Cliquez sur un élément de l'aperçu pour le modifier.\n\nGlissez pour déplacer, tirez un coin pour redimensionner, flèches du clavier pour ajuster (Maj : 10 px), Suppr pour retirer.");
            ui.separator();
            self.layers_ui(ui, &info);
            return;
        };
        let mut op: Option<Op> = None;
        let mut pick_image = false;
        let mut changed = false;
        let mut pick_font = false;
        let base_dir = self.base_dir();
        let font_check = self.renderer.as_ref();
        let count = info.count();
        let labels: Vec<String> = self.element_labels(&info);
        let is_free = info.is_free();
        let page_targets = self.page_targets();
        let video_labels: Vec<String> = (0..self.model.videos.len()).map(|i| video_title(&self.model, i)).collect();
        let proj_style = self.model.designs.style.clone();
        let mut pick_skin: Option<usize> = None;
        {
            let Some(d) = info.design_mut(&mut self.model.designs) else { return };
            let eff = d.slot_assignment(count);
            let slot_pos = d.items.iter().take(i).filter(|x| x.is_element_slot()).count();
            let Some(it) = d.items.get_mut(i) else { self.sel_item = None; return };
            ui.colored_label(kind_color(&it.kind), kind_name(&it.kind));
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
                Kind::Slot { thumb_h, assign, video, link } => {
                    let mut lab = (h - *thumb_h).max(0);
                    ui.horizontal(|ui| {
                        ui.label("Hauteur du libellé :");
                        if ui.add(egui::DragValue::new(&mut lab).range(0..=(h - 8).max(0))).changed() {
                            *thumb_h = (h - lab).max(8);
                            changed = true;
                        }
                    });
                    // contenu : un element de la page (pages automatiques) ou n'importe quelle video du projet
                    let vname = |v: usize| format!("Vidéo {}. {}", v + 1, video_labels.get(v).cloned().unwrap_or_default());
                    let ename = |e: usize| if e == usize::MAX { "(aucun élément : à choisir)".to_string() } else { format!("{}. {}", e + 1, labels.get(e).cloned().unwrap_or_default()) };
                    let cur = eff.get(slot_pos).copied().unwrap_or(usize::MAX);
                    let shown = match *video {
                        Some(Some(v)) => vname(v),
                        Some(None) => "(aucune vidéo : à choisir)".to_string(),
                        None if is_free => "(choisir une vidéo)".to_string(),
                        None => ename(cur),
                    };
                    let old_video = (*video).flatten();
                    egui::ComboBox::from_label("Contenu").selected_text(shown).show_ui(ui, |ui| {
                        if !is_free {
                            ui.label(egui::RichText::new("Éléments de cette page").weak());
                            if ui.selectable_label(video.is_none() && assign.is_none(), "Automatique (dans l'ordre)").clicked() { *video = None; op = Some(Op::Assign(None)); }
                            for e in 0..count {
                                if ui.selectable_label(video.is_none() && assign.is_some() && cur == e, ename(e)).clicked() { *video = None; op = Some(Op::Assign(Some(e))); }
                            }
                            ui.separator();
                        }
                        ui.label(egui::RichText::new("N'importe quelle vidéo du projet").weak());
                        for v in 0..video_labels.len() {
                            if ui.selectable_label(*video == Some(Some(v)), vname(v)).clicked() { *video = Some(Some(v)); changed = true; }
                        }
                    });
                    // « lire la video associee » suit le changement de video
                    if let (Some(o), Some(nv)) = (old_video, (*video).flatten()) {
                        if o != nv && *link == Some(Target::Video(o)) { *link = Some(Target::Video(nv)); }
                    }
                    if !is_free && video.is_none() {
                        ui.label(egui::RichText::new("Plusieurs vignettes peuvent montrer le même élément ou la même vidéo.").weak());
                    }
                    let habit = if info.menu == MenuType::Chapters && video.is_none() { "Habituel : lire depuis ce chapitre" } else { "Habituel : menu de chapitres s'il existe, sinon lecture" };
                    let mut opts: Vec<(Target, String)> = Vec::new();
                    if let Some(Some(v)) = *video { opts.push((Target::Video(v), "Lire la vidéo associée".to_string())); }
                    opts.extend(page_targets.iter().cloned());
                    let shown = link.and_then(|t| opts.iter().find(|(x, _)| *x == t).map(|(_, l)| l.clone())).unwrap_or_else(|| if link.is_some() { "(destination introuvable)".into() } else { habit.to_string() });
                    egui::ComboBox::from_label("Au clic").selected_text(shown).show_ui(ui, |ui| {
                        if ui.selectable_label(link.is_none(), habit).clicked() { *link = None; changed = true; }
                        for (t, l) in &opts {
                            if ui.selectable_label(*link == Some(*t), l).clicked() { *link = Some(*t); changed = true; }
                        }
                    });
                }
                Kind::Nav { action, text, target } => {
                    egui::ComboBox::from_label("Action").selected_text(action.label()).show_ui(ui, |ui| {
                        for a in NavAction::ALL {
                            if ui.selectable_value(action, a, a.label()).changed() {
                                changed = true;
                                if a == NavAction::Goto && target.is_none() { *target = Some(Target::Main(0)); }
                            }
                        }
                    });
                    if *action == NavAction::Goto {
                        destination_combo(ui, target, &page_targets, &mut changed);
                    }
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
                    changed |= crate::style_ui::skin_ui(ui, &mut it.look.skin, &mut pick_skin);
                }
            }
            ui.separator();
            ui.horizontal_wrapped(|ui| {
                if ui.button("Dupliquer").clicked() { op = Some(Op::Duplicate); }
                if ui.button("Supprimer").clicked() { op = Some(Op::Delete); }
            });
            ui.label("Plan (superposition) :");
            ui.horizontal_wrapped(|ui| {
                if ui.button("Tout derrière").clicked() { op = Some(Op::Back); }
                if ui.button("Derrière").clicked() { op = Some(Op::Up); }
                if ui.button("Devant").clicked() { op = Some(Op::Down); }
                if ui.button("Tout devant").clicked() { op = Some(Op::Front); }
            });
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
        if let Some(state) = pick_skin {
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
                    Op::Delete => { d.items.remove(i); self.sel_item = None; }
                    Op::Duplicate => {
                        let mut c = d.items[i].clone();
                        c.x = (c.x + 30).min(1920 - c.w);
                        c.y = (c.y + 30).min(1080 - c.h);
                        d.items.push(c);
                        self.sel_item = Some(d.items.len() - 1);
                    }
                    Op::Up => self.sel_item = Some(move_in_list(d, i, true)),
                    Op::Down => self.sel_item = Some(move_in_list(d, i, false)),
                    Op::Front => self.sel_item = Some(to_front(d, i)),
                    Op::Back => self.sel_item = Some(to_back(d, i)),
                    Op::Assign(e) => d.assign_slot(i, e, count),
                }
            }
            changed = true;
        }
        if changed {
            self.bump();
        }
        ui.separator();
        self.layers_ui(ui, &info);
    }

    /// Liste des elements de la page, du premier plan (en haut) a l'arriere-plan ; un clic selectionne l'element.
    fn layers_ui(&mut self, ui: &mut egui::Ui, info: &PageInfo) {
        let Some(d) = info.design(&self.model.designs) else { return };
        ui.label("Éléments (du premier plan à l'arrière-plan) :");
        let mut pick = None;
        for (idx, it) in d.items.iter().enumerate().rev() {
            let what = match &it.kind {
                Kind::Text { text, link, .. } => format!("Texte « {} »{}", text.lines().next().unwrap_or("").chars().take(24).collect::<String>(), if link.is_some() { " (cliquable)" } else { "" }),
                Kind::Image { path, link } => format!("Image {}{}", std::path::Path::new(path).file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default(), if link.is_some() { " (cliquable)" } else { "" }),
                Kind::Nav { action, text, .. } => if *action == NavAction::Goto { format!("Bouton « {text} »") } else { format!("Bouton {}", action.label()) },
                Kind::Slot { video, link, .. } => format!("Vignette{}{}", match video { Some(Some(v)) => format!(" (vidéo {})", v + 1), Some(None) => " (sans vidéo)".to_string(), None => String::new() }, if link.is_some() { " →" } else { "" }),
            };
            let text = egui::RichText::new(what).color(kind_color(&it.kind));
            if ui.selectable_label(self.sel_item == Some(idx), text).clicked() { pick = Some(idx); }
        }
        if pick.is_some() { self.sel_item = pick; }
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
