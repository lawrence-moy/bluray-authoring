//! Onglet « Jaquette » : feuille a deplier du boitier (dos, tranche, face) et etiquette de disque ; edition a la souris,
//! annuler / refaire, champs automatiques et export PDF en arriere-plan.
use crate::App;
use bdauthor_core::cover::{LineArrow, LineStyle, ShapeKind, VAlign, layer_state, layers_or_default, normalize_layers, Cover, CoverItem, CoverKind, CoverLayer, DiscLabel, SheetDoc, FIELDS, FORMATS};
use bdauthor_core::design::Align;
use bdauthor_core::editor::ItemAlign;
use eframe::egui::{self, pos2, vec2, Color32, CursorIcon, Key, Rect, Sense, Stroke};
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::mpsc::{channel, TryRecvError};

/// Resolution (dpi) de l'apercu a l'ecran.
const PREVIEW_DPI: u32 = 96;
/// Taille minimale d'un element (mm) et rayon (pixels) de saisie d'une poignee.
const MIN_MM: f64 = 2.0;
const HANDLE_PX: f32 = 9.0;
/// Epaisseur des regles (pixels) : largeur de celle de gauche, hauteur de celle du haut.
const RULER_W: f32 = 30.0;
const RULER_H: f32 = 18.0;
/// Graduations (pas des chiffres, pas des petits traits) en mm et en pouces.
const MM_STEPS: [(f64, f64); 8] = [(1.0, 0.2), (2.0, 0.5), (5.0, 1.0), (10.0, 2.0), (20.0, 5.0), (50.0, 10.0), (100.0, 20.0), (200.0, 50.0)];
const INCH_STEPS: [(f64, f64); 7] = [(0.25, 0.0625), (0.5, 0.125), (1.0, 0.25), (2.0, 0.5), (5.0, 1.0), (10.0, 2.0), (20.0, 5.0)];

fn ruler_text(v: f64, inch: bool) -> String {
    if (v - v.round()).abs() < 1e-6 { return format!("{}", v.round() as i64); }
    if inch { format!("{v:.3}").trim_end_matches('0').to_string() } else { format!("{v:.1}") }
}

/// Regle graduee : `horizontal` (en haut) ou verticale (a gauche) ; `origin` : position ecran du zero, `ppmm` : points par mm,
/// `cursor` : position du pointeur, `band` : etendue de l'element selectionne.
fn draw_ruler(p: &egui::Painter, area: Rect, horizontal: bool, origin: f32, ppmm: f32, inch: bool, cursor: Option<f32>, band: Option<(f32, f32)>) {
    let p = p.with_clip_rect(area);
    p.rect_filled(area, 0.0, Color32::from_gray(38));
    let unit_px = if inch { ppmm * 25.4 } else { ppmm };
    let steps: &[(f64, f64)] = if inch { &INCH_STEPS } else { &MM_STEPS };
    let (major, minor) = *steps.iter().find(|s| s.0 as f32 * unit_px >= 56.0).unwrap_or(steps.last().unwrap());
    let (lo, hi) = if horizontal { (area.min.x, area.max.x) } else { (area.min.y, area.max.y) };
    if let Some((a, b)) = band {
        let (a, b) = (a.min(b).max(lo), a.max(b).min(hi));
        if b > a {
            let r = if horizontal { Rect::from_min_max(pos2(a, area.min.y), pos2(b, area.max.y)) } else { Rect::from_min_max(pos2(area.min.x, a), pos2(area.max.x, b)) };
            p.rect_filled(r, 0.0, Color32::from_rgba_unmultiplied(120, 190, 255, 70));
        }
    }
    let thick = if horizontal { area.height() } else { area.width() };
    let n0 = (((lo - origin) / unit_px) as f64 / minor).floor() as i64;
    let n1 = (((hi - origin) / unit_px) as f64 / minor).ceil() as i64;
    if n1 - n0 < 4000 {
        let col = Color32::from_gray(170);
        let font = egui::FontId::proportional(9.0);
        for n in n0..=n1 {
            let v = n as f64 * minor;
            let pos = origin + v as f32 * unit_px;
            if pos < lo || pos > hi { continue; }
            let is_major = ((v / major).round() * major - v).abs() < minor * 0.01;
            let len = thick * if is_major { 0.75 } else { 0.35 };
            let stroke = Stroke::new(1.0_f32, col);
            if horizontal {
                p.line_segment([pos2(pos, area.max.y), pos2(pos, area.max.y - len)], stroke);
                if is_major { p.text(pos2(pos + 2.0, area.min.y), egui::Align2::LEFT_TOP, ruler_text(v, inch), font.clone(), col); }
            } else {
                p.line_segment([pos2(area.max.x, pos), pos2(area.max.x - len, pos)], stroke);
                if is_major { p.text(pos2(area.min.x + 1.0, pos + 1.0), egui::Align2::LEFT_TOP, ruler_text(v, inch), font.clone(), col); }
            }
        }
    }
    if let Some(c) = cursor.filter(|c| *c >= lo && *c <= hi) {
        let stroke = Stroke::new(1.5_f32, Color32::from_rgb(255, 120, 90));
        if horizontal { p.line_segment([pos2(c, area.min.y), pos2(c, area.max.y)], stroke); } else { p.line_segment([pos2(area.min.x, c), pos2(area.max.x, c)], stroke); }
    }
    let edge = Stroke::new(1.0_f32, Color32::from_gray(90));
    if horizontal { p.line_segment([area.left_bottom(), area.right_bottom()], edge); } else { p.line_segment([area.right_top(), area.right_bottom()], edge); }
}

/// Deplacement ou redimensionnement a la souris en cours : `handle` 0 = deplacer, 1..4 = coins haut gauche, haut droit, bas gauche, bas droit.
#[derive(Clone)]
pub(crate) struct CoverDrag {
    item: usize,
    handle: u8,
    orig: (f64, f64, f64, f64),
    start: (f64, f64),
    /// deplacement d'un groupe : tous les elements deplaces avec leur rectangle d'origine
    group: Vec<(usize, (f64, f64, f64, f64))>,
    /// ligne : extremite qui reste fixe pendant qu'on deplace l'autre (poignees 5 et 6)
    fixed: (f64, f64),
}

/// Distance (points ecran) d'un point `p` au segment [a, b].
fn seg_dist(p: egui::Pos2, a: egui::Pos2, b: egui::Pos2) -> f32 {
    let ab = b - a;
    let l2 = ab.length_sq();
    let t = if l2 > 0.0 { ((p - a).dot(ab) / l2).clamp(0.0, 1.0) } else { 0.0 };
    p.distance(a + ab * t)
}

/// Ligne de la liste, glissable par sa poignee ☰ (calque ou element).
#[derive(Clone, Copy)]
enum CoverRow {
    Layer(usize),
    Item(usize),
}

/// Modification demandee par la liste, appliquee apres son dessin.
enum CoverOp {
    Add,
    Remove(usize, bool),
    MoveItem { from: usize, layer: usize, before: Option<usize> },
    MoveLayer { from: usize, to: usize },
}

fn item_label(it: &CoverItem, i: usize) -> String {
    if !it.name.is_empty() { return format!("{}. {}", i + 1, it.name); }
    match &it.kind {
        CoverKind::Text { text, .. } => format!("{}. Texte « {} »", i + 1, text.lines().next().unwrap_or("").chars().take(24).collect::<String>()),
        CoverKind::Image { path } => format!("{}. Image {}", i + 1, std::path::Path::new(path).file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()),
        CoverKind::Shape { shape, .. } => format!("{}. {}", i + 1, shape.label()),
    }
}

/// Annee en cours (UTC).
fn current_year() -> i32 {
    let secs = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0);
    let z = secs.div_euclid(86400) + 719468;
    let era = z.div_euclid(146097);
    let doe = z.rem_euclid(146097);
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    (if m <= 2 { y + 1 } else { y }) as i32
}

const IMG_EXT: [&str; 6] = ["png", "jpg", "jpeg", "bmp", "svg", "svgz"];

fn cover_format(ui: &mut egui::Ui, c: &mut Cover) -> bool {
    let mut changed = false;
    egui::CollapsingHeader::new("Format et impression").default_open(true).show(ui, |ui| {
        egui::ComboBox::from_id_salt("jaquette_format").selected_text("Choisir un format usuel…").show_ui(ui, |ui| {
            for (k, f) in FORMATS.iter().enumerate() {
                if ui.selectable_label(false, f.0).clicked() { c.set_format(k); changed = true; }
            }
        });
        egui::Grid::new("jaquette_dims").num_columns(4).show(ui, |ui| {
            ui.label("Dos / face"); changed |= ui.add(egui::DragValue::new(&mut c.face_mm).range(20.0..=400.0).speed(0.5).suffix(" mm")).changed();
            ui.label("Hauteur"); changed |= ui.add(egui::DragValue::new(&mut c.height_mm).range(20.0..=400.0).speed(0.5).suffix(" mm")).changed();
            ui.end_row();
            ui.label("Tranche"); changed |= ui.add(egui::DragValue::new(&mut c.spine_mm).range(0.0..=100.0).speed(0.25).suffix(" mm")).changed();
            ui.label("Fond perdu"); changed |= ui.add(egui::DragValue::new(&mut c.bleed_mm).range(0.0..=20.0).speed(0.25).suffix(" mm")).changed();
            ui.end_row();
            ui.label("Résolution"); changed |= ui.add(egui::DragValue::new(&mut c.dpi).range(72..=600).speed(5.0).suffix(" dpi")).changed();
            ui.end_row();
        });
        let (sw, sh) = c.sheet_mm();
        ui.label(egui::RichText::new(format!("Feuille avec fond perdu : {sw:.1} × {sh:.1} mm ({} × {} px à {} dpi)", (sw / 25.4 * c.dpi as f64).round(), (sh / 25.4 * c.dpi as f64).round(), c.dpi)).weak());
        changed |= ui.checkbox(&mut c.marks, "Traits de coupe et de pliage (page agrandie de 10 mm de chaque côté)").changed();
    });
    egui::CollapsingHeader::new("Fond").default_open(true).show(ui, |ui| {
        ui.horizontal(|ui| {
            for (i, name) in ["Dos", "Tranche", "Face"].into_iter().enumerate() {
                ui.label(name);
                changed |= ui.color_edit_button_srgb(&mut c.colors[i]).changed();
            }
        });
        changed |= crate::file_row(ui, "Image de fond :", &mut c.bg_image, "(aucune : couleurs ci-dessus)", "Images", &IMG_EXT);
        ui.label(egui::RichText::new("L'image de fond couvre toute la feuille, fond perdu compris.").weak());
    });
    changed
}

fn label_format(ui: &mut egui::Ui, l: &mut DiscLabel) -> bool {
    let mut changed = false;
    egui::CollapsingHeader::new("Format et impression").default_open(true).show(ui, |ui| {
        egui::Grid::new("etiquette_dims").num_columns(4).show(ui, |ui| {
            ui.label("Diamètre"); changed |= ui.add(egui::DragValue::new(&mut l.diameter_mm).range(30.0..=200.0).speed(0.5).suffix(" mm")).changed();
            ui.label("Trou central"); changed |= ui.add(egui::DragValue::new(&mut l.hole_mm).range(0.0..=(l.diameter_mm - 5.0).max(0.0)).speed(0.25).suffix(" mm")).changed();
            ui.end_row();
            ui.label("Fond perdu"); changed |= ui.add(egui::DragValue::new(&mut l.bleed_mm).range(0.0..=20.0).speed(0.25).suffix(" mm")).changed();
            ui.label("Résolution"); changed |= ui.add(egui::DragValue::new(&mut l.dpi).range(72..=600).speed(5.0).suffix(" dpi")).changed();
            ui.end_row();
        });
        let (sw, _) = l.sheet_mm();
        ui.label(egui::RichText::new(format!("Feuille carrée avec fond perdu : {sw:.1} × {sw:.1} mm ({} px de côté à {} dpi). Le PDF découpe le disque et son trou central.", (sw / 25.4 * l.dpi as f64).round(), l.dpi)).weak());
        changed |= ui.checkbox(&mut l.marks, "Cercles de repère (contour et trou)").changed();
    });
    egui::CollapsingHeader::new("Fond").default_open(true).show(ui, |ui| {
        ui.horizontal(|ui| {
            ui.label("Couleur");
            changed |= ui.color_edit_button_srgb(&mut l.color).changed();
        });
        changed |= crate::file_row(ui, "Image de fond :", &mut l.bg_image, "(aucune : couleur ci-dessus)", "Images", &IMG_EXT);
    });
    changed
}

/// Format d'export : PDF a la taille de la feuille, PDF A4 a decouper, ou image PNG (rognee au format fini si demande).
#[derive(Clone, Copy)]
enum ExportKind {
    Pdf,
    A4,
    Png { trim: bool },
}

fn spawn_export<D: SheetDoc + Send + 'static>(doc: D, kind: ExportKind, what: &'static str, base: PathBuf, vars: BTreeMap<String, String>, path: PathBuf, tx: std::sync::mpsc::Sender<String>) {
    std::thread::spawn(move || {
        let run = || -> Result<String, String> {
            let r = bdauthor_render::FontRenderer::new().map_err(|e| e.to_string())?;
            let img = r.render_sheet(&doc, doc.dpi(), &base, &vars).map_err(|e| e.to_string())?;
            let title = path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_else(|| what.into());
            let bytes = match kind {
                ExportKind::Pdf => doc.to_pdf(&img, &title),
                ExportKind::A4 => doc.to_pdf_a4(&img, &title)?,
                ExportKind::Png { trim } => {
                    let out = if trim {
                        // le fond perdu occupe la meme largeur de chaque cote : on le retire
                        let (sw, _) = doc.sheet_mm();
                        let b = (doc.bleed_mm() / sw * img.width as f64).round() as i32;
                        bdauthor_core::cover::crop(&img, b, b, img.width as i32 - 2 * b, img.height as i32 - 2 * b)
                    } else { img };
                    bdauthor_core::png::write_png_print(&out, doc.dpi())
                }
            };
            std::fs::write(&path, bytes).map_err(|e| e.to_string())?;
            Ok(format!("{what} exportée : {}", path.display()))
        };
        let _ = tx.send(run().unwrap_or_else(|e| format!("Export impossible : {e}")));
    });
}

impl App {
    pub(crate) fn cover_screen(&mut self, ctx: &egui::Context, busy: bool) {
        self.cover_history_step(ctx);
        // fin de l'export en arriere-plan
        match self.cover_export_rx.as_ref().map(|rx| rx.try_recv()) {
            Some(Ok(m)) => { self.notice = m; self.cover_export_rx = None; }
            Some(Err(TryRecvError::Disconnected)) => { self.notice = "Export interrompu.".into(); self.cover_export_rx = None; }
            Some(Err(TryRecvError::Empty)) => ctx.request_repaint_after(std::time::Duration::from_millis(100)),
            None => {}
        }
        egui::SidePanel::left("jaquette_panneau").resizable(true).default_width(380.0).show(ctx, |ui| {
            egui::ScrollArea::vertical().id_salt("defil_jaquette").auto_shrink([false, false]).show(ui, |ui| {
                ui.add_enabled_ui(!busy, |ui| self.cover_panel(ui));
            });
        });
        egui::CentralPanel::default().show(ctx, |ui| {
            ui.add_enabled_ui(!busy, |ui| self.cover_central(ui, ctx));
        });
    }

    fn cover_changed(&mut self) {
        self.dirty = true;
        self.cover_gen += 1;
    }

    fn cover_vars(&self) -> BTreeMap<String, String> {
        self.model.cover_vars(&self.durations, current_year())
    }

    /// Enregistre les modifications dans l'historique et traite Ctrl+Z / Ctrl+Y.
    fn cover_history_step(&mut self, ctx: &egui::Context) {
        let ms = self.t0.elapsed().as_millis() as u64;
        let cur = (self.model.cover.clone(), self.model.label.clone());
        self.cover_hist.get_or_insert_with(|| bdauthor_core::cover::EditHistory::new(&cur)).observe(&cur, ms);
        if ctx.wants_keyboard_input() { return; }
        let (undo, redo) = ctx.input(|i| {
            let z = i.key_pressed(Key::Z) && i.modifiers.command;
            let y = i.key_pressed(Key::Y) && i.modifiers.command;
            (z && !i.modifiers.shift, y || (z && i.modifiers.shift))
        });
        if undo { self.cover_undo_redo(false); } else if redo { self.cover_undo_redo(true); }
    }

    fn cover_undo_redo(&mut self, redo: bool) {
        let mut cur = (self.model.cover.clone(), self.model.label.clone());
        let ok = match &mut self.cover_hist {
            Some(h) => if redo { h.redo(&mut cur) } else { h.undo(&mut cur) },
            None => false,
        };
        if ok {
            (self.model.cover, self.model.label) = cur;
            self.cover_select_only(None);
            self.cover_drag = None;
            self.cover_marquee = None;
            self.cover_closed.clear();
            self.cover_layer_edit = None;
            self.cover_changed();
        }
    }

    fn cover_panel(&mut self, ui: &mut egui::Ui) {
        let mut changed = false;
        let (can_undo, can_redo) = self.cover_hist.as_ref().map(|h| (h.can_undo(), h.can_redo())).unwrap_or((false, false));
        let mut page = self.cover_page;
        let (mut undo, mut redo) = (false, false);
        ui.horizontal(|ui| {
            undo = ui.add_enabled(can_undo, egui::Button::new("↶")).on_hover_text("Annuler (Ctrl+Z)").clicked();
            redo = ui.add_enabled(can_redo, egui::Button::new("↷")).on_hover_text("Refaire (Ctrl+Y)").clicked();
            ui.separator();
            ui.selectable_value(&mut page, 0, "Jaquette");
            ui.selectable_value(&mut page, 1, "Étiquette de disque");
        });
        if undo { self.cover_undo_redo(false); }
        if redo { self.cover_undo_redo(true); }
        if page != self.cover_page {
            self.cover_page = page;
            self.cover_closed.clear();
            self.cover_layer_edit = None;
            self.cover_layer = 0;
            self.cover_zoom = 1.0;
            self.cover_pan = egui::Vec2::ZERO;
            self.cover_select_only(None);
            self.cover_drag = None;
            self.cover_marquee = None;
            self.cover_tex = None;
            self.cover_gen += 1;
        }
        if self.cover_page == 0 {
            ui.heading("Jaquette");
            ui.label(egui::RichText::new("Feuille à déplier du boîtier : dos à gauche, tranche au milieu, face à droite. Toutes les mesures sont en millimètres, depuis le coin haut gauche de la jaquette rognée (le fond perdu a donc des valeurs négatives).").weak());
            changed |= cover_format(ui, &mut self.model.cover);
        } else {
            ui.heading("Étiquette de disque");
            ui.label(egui::RichText::new("Disque imprimé : mesures en millimètres depuis le coin haut gauche de la feuille carrée rognée (le fond perdu a donc des valeurs négatives). Le trou central n'est pas imprimé.").weak());
            changed |= label_format(ui, &mut self.model.label);
        }
        ui.separator();
        ui.heading("Éléments");
        if self.cover_page == 0 {
            let mut d = std::mem::take(&mut self.model.cover);
            changed |= self.items_ui(ui, &mut d);
            self.model.cover = d;
        } else {
            let mut d = std::mem::take(&mut self.model.label);
            changed |= self.items_ui(ui, &mut d);
            self.model.label = d;
        }
        ui.separator();
        if self.cover_export_rx.is_some() {
            ui.horizontal(|ui| {
                ui.spinner();
                ui.label("Export en cours… (vous pouvez continuer à travailler)");
            });
        } else {
            ui.horizontal_wrapped(|ui| {
                if ui.button("Exporter en PDF…").on_hover_text("Image à la résolution choisie, dans un PDF aux dimensions exactes").clicked() { self.cover_export(ExportKind::Pdf); }
                if ui.button("Exporter en PNG…").on_hover_text("Image à la résolution choisie (la résolution est enregistrée dans le fichier)").clicked() { self.cover_export(ExportKind::Png { trim: self.cover_png_trim }); }
                if ui.button("Imprimer sur A4…").on_hover_text("PDF A4 avec la jaquette à sa taille réelle, centrée, et des repères de coupe : à imprimer à 100 % (sans « ajuster à la page »), puis à découper").clicked() { self.cover_export(ExportKind::A4); }
            });
            ui.checkbox(&mut self.cover_png_trim, "PNG rogné au format fini (sans fond perdu)");
        }
        if changed { self.cover_changed(); }
    }

    /// Liste, boutons et inspecteur des elements d'une feuille.
    fn items_ui(&mut self, ui: &mut egui::Ui, doc: &mut dyn SheetDoc) -> bool {
        let mut changed = false;
        let n = doc.items().len();
        let orig = layers_or_default(doc.layers());
        let mut layers = orig.clone();
        let nl = layers.len();
        let mut active = self.cover_layer.min(nl - 1);
        let selected = self.cover_selection(n);
        let mut op: Option<CoverOp> = None;
        let mut pick: Option<usize> = None;
        let mut toggle: Option<usize> = None;
        let mut activate: Option<usize> = None;
        let mut rename_done = false;
        let mut rows: Vec<(CoverRow, Rect)> = Vec::new();
        let pm = |open: bool| if open { "−" } else { "+" };
        ui.label(egui::RichText::new("Calques et éléments (du premier plan, en haut, à l'arrière-plan). Glissez la poignée ☰ pour réordonner, changer un élément de calque, ou déplacer un calque. + / − : déplier ou replier.").weak());
        ui.horizontal_wrapped(|ui| {
            if ui.button("+ Calque").on_hover_text("Nouveau calque au premier plan ; les nouveaux éléments vont dans le calque actif").clicked() { op = Some(CoverOp::Add); }
            if ui.small_button("+ tout").on_hover_text("Déplier tous les calques").clicked() { self.cover_closed.clear(); }
            if ui.small_button("− tout").on_hover_text("Replier tous les calques").clicked() { for l in 0..nl { self.cover_closed.insert(l); } }
        });
        egui::ScrollArea::vertical().id_salt("jaquette_liste").auto_shrink([false, true]).max_height(300.0).show(ui, |ui| {
            let list = egui::Frame::group(ui.style()).inner_margin(4.0).show(ui, |ui| {
                ui.set_min_width(ui.available_width());
                for l in (0..nl).rev() {
                    let open = !self.cover_closed.contains(&l);
                    let count = (0..n).filter(|&i| doc.items()[i].layer == l).count();
                    let r = ui.horizontal(|ui| {
                        let _ = ui.dnd_drag_source(egui::Id::new(("cal", l)), CoverRow::Layer(l), |ui| ui.label(egui::RichText::new("☰").strong()))
                            .response.on_hover_text("Glisser pour déplacer le calque (avec ses éléments)");
                        if ui.small_button(pm(open)).on_hover_text(if open { "Replier le calque" } else { "Déplier le calque" }).clicked() { toggle = Some(l); }
                        let vis = layers[l].visible;
                        if ui.small_button(if vis { "👁" } else { "◌" }).on_hover_text(if vis { "Masquer (un calque masqué n'est ni affiché ni exporté)" } else { "Calque masqué : afficher" }).clicked() { layers[l].visible = !vis; }
                        let locked = layers[l].locked;
                        if ui.small_button(if locked { "🔒" } else { "🔓" }).on_hover_text(if locked { "Verrouillé : ses éléments ne se sélectionnent plus à la souris ; déverrouiller" } else { "Verrouiller (plus de sélection ni de déplacement à la souris)" }).clicked() { layers[l].locked = !locked; }
                        let editing = matches!(&self.cover_layer_edit, Some((e, _)) if *e == l);
                        if editing {
                            if let Some((_, text)) = &mut self.cover_layer_edit {
                                let te = ui.add(egui::TextEdit::singleline(text).desired_width(130.0));
                                if !te.has_focus() && !te.lost_focus() { te.request_focus(); }
                                if te.lost_focus() { rename_done = true; }
                            }
                        } else {
                            let resp = ui.selectable_label(active == l, egui::RichText::new(format!("{}  ({count})", layers[l].name))).on_hover_text("Calque actif : reçoit les nouveaux éléments. Double-clic : renommer");
                            if resp.clicked() { activate = Some(l); }
                            if resp.double_clicked() { self.cover_layer_edit = Some((l, layers[l].name.clone())); }
                            if ui.small_button("✏").on_hover_text("Renommer le calque").clicked() { self.cover_layer_edit = Some((l, layers[l].name.clone())); }
                            if nl > 1 {
                                let _ = ui.menu_button("🗑", |ui| {
                                    if ui.button("Supprimer le calque (ses éléments vont au calque voisin)").clicked() { op = Some(CoverOp::Remove(l, true)); ui.close_menu(); }
                                    if ui.button("Supprimer le calque et ses éléments").clicked() { op = Some(CoverOp::Remove(l, false)); ui.close_menu(); }
                                }).response.on_hover_text("Supprimer le calque");
                            }
                        }
                    }).response;
                    rows.push((CoverRow::Layer(l), r.rect));
                    if !open { continue; }
                    for i in (0..n).rev().filter(|&i| doc.items()[i].layer == l) {
                        let mut text = egui::RichText::new(item_label(&doc.items()[i], i));
                        if !layers[l].visible { text = text.weak(); }
                        let r = ui.horizontal(|ui| {
                            ui.add_space(40.0);
                            let _ = ui.dnd_drag_source(egui::Id::new(("cel", i)), CoverRow::Item(i), |ui| ui.label(egui::RichText::new("☰").weak()))
                                .response.on_hover_text("Glisser pour changer l'ordre ou le calque");
                            if ui.selectable_label(selected.contains(&i), text).clicked() { pick = Some(i); }
                        }).response;
                        rows.push((CoverRow::Item(i), r.rect));
                    }
                    if count == 0 {
                        ui.horizontal(|ui| { ui.add_space(40.0); ui.label(egui::RichText::new("(vide : glissez-y des éléments)").weak().italics()); });
                    }
                }
            });

            // ---- glisser-deposer : trait bleu a l'endroit de depot
            let area = list.response.rect;
            let (Some(drag), Some(pos)) = (egui::DragAndDrop::payload::<CoverRow>(ui.ctx()), ui.ctx().pointer_hover_pos()) else { return };
            if !area.expand(8.0).contains(pos) || rows.is_empty() { return; }
            let released = ui.input(|i| i.pointer.any_released());
            let blue = Stroke::new(2.5_f32, Color32::from_rgb(90, 170, 255));
            let r = rows.iter().filter(|(_, rc)| rc.center().y < pos.y).count();
            let bottom = rows.last().map_or(area.bottom(), |x| x.1.bottom());
            match *drag {
                CoverRow::Item(from) => {
                    let y = if r < rows.len() { rows[r].1.top() } else { bottom };
                    ui.painter().hline(area.x_range(), y, blue);
                    if released {
                        egui::DragAndDrop::clear_payload(ui.ctx());
                        // sous un en-tete : au premier plan de ce calque ; sous un element : juste derriere lui
                        let (layer, before) = if r == 0 { (nl - 1, None) } else {
                            match rows[r - 1].0 {
                                CoverRow::Layer(l) => (l, None),
                                CoverRow::Item(j) => (doc.items()[j].layer, Some(j)),
                            }
                        };
                        op = Some(CoverOp::MoveItem { from, layer, before });
                    }
                }
                CoverRow::Layer(from) => {
                    // un calque s'insere entre deux calques
                    let header_rows: Vec<usize> = rows.iter().enumerate().filter(|(_, (row, _))| matches!(row, CoverRow::Layer(_))).map(|(k, _)| k).collect();
                    let above = header_rows.iter().filter(|&&k| k < r).count();
                    let line_at = header_rows.get(above).copied().unwrap_or(rows.len());
                    let y = if line_at < rows.len() { rows[line_at].1.top() } else { bottom };
                    ui.painter().hline(area.x_range(), y, blue);
                    if released {
                        egui::DragAndDrop::clear_payload(ui.ctx());
                        let disp_from = nl - 1 - from;
                        let disp_to = if above > disp_from { above - 1 } else { above };
                        let to = nl - 1 - disp_to.min(nl - 1);
                        if to != from { op = Some(CoverOp::MoveLayer { from, to }); }
                    }
                }
            }
        });
        if let Some(i) = pick {
            if ui.input(|inp| inp.modifiers.command || inp.modifiers.shift) {
                let mut v = self.cover_selection(n);
                if let Some(k) = v.iter().position(|&e| e == i) { v.remove(k); } else { v.push(i); }
                match v.last().copied() { Some(last) => self.cover_set_selection(v, last), None => self.cover_select_only(None) }
            } else { self.cover_select_only(Some(i)); }
        }
        if let Some(l) = activate { active = l; }
        if let Some(l) = toggle { if !self.cover_closed.remove(&l) { self.cover_closed.insert(l); } }
        if rename_done {
            if let Some((l, t)) = self.cover_layer_edit.take() {
                if l < layers.len() && !t.trim().is_empty() { layers[l].name = t.trim().to_string(); }
            }
        }
        match op {
            Some(CoverOp::Add) => {
                layers.push(CoverLayer::new(&format!("Calque {}", nl + 1)));
                active = nl;
            }
            Some(CoverOp::Remove(l, keep)) if nl > 1 && l < nl => {
                let items = doc.items_mut();
                if !keep { items.retain(|it| it.layer != l); }
                for it in items.iter_mut() {
                    if it.layer > l { it.layer -= 1; } else if it.layer == l { it.layer = l.saturating_sub(1); }
                }
                layers.remove(l);
                active = active.min(layers.len() - 1);
                self.cover_select_only(None);
                self.cover_closed.clear();
            }
            Some(CoverOp::MoveItem { from, layer, before }) if from < n && layer < nl => {
                let items = doc.items_mut();
                let mut it = items.remove(from);
                it.layer = layer;
                let at = match before { Some(j) => if j > from { j - 1 } else { j }, None => items.len() };
                items.insert(at, it);
                self.cover_select_only(Some(at));
                changed = true;
            }
            Some(CoverOp::MoveLayer { from, to }) if from < nl && to < nl => {
                let moved = layers.remove(from);
                layers.insert(to, moved);
                for it in doc.items_mut().iter_mut() {
                    let l = it.layer;
                    it.layer = if l == from { to } else if from < to && l > from && l <= to { l - 1 } else if from > to && l >= to && l < from { l + 1 } else { l };
                }
                active = if active == from { to } else if from < to && active > from && active <= to { active - 1 } else if from > to && active >= to && active < from { active + 1 } else { active };
                self.cover_closed.clear();
            }
            _ => {}
        }
        if layers != orig {
            *doc.layers_mut() = normalize_layers(layers);
            changed = true;
        }
        self.cover_layer = active;
        let n = doc.items().len();
        let (sw, sh) = doc.sheet_mm();
        let bleed = doc.bleed_mm();
        let (tw, th) = (sw - 2.0 * bleed, sh - 2.0 * bleed);
        let sel = self.cover_selection(n);
        ui.horizontal_wrapped(|ui| {
            if ui.button("+ Texte").clicked() {
                let w = (tw * 0.4).min(100.0);
                let mut it = CoverItem::text("Titre", (tw - w) / 2.0, 10.0, w, 30.0, 28.0);
                it.layer = active;
                doc.items_mut().push(it);
                self.cover_select_only(Some(doc.items().len() - 1));
                changed = true;
            }
            if ui.button("+ Image…").clicked() {
                if let Some(f) = rfd::FileDialog::new().add_filter("Images", &IMG_EXT).pick_file() {
                    let rel = self.relative(&f);
                    let w = (tw * 0.4).min(100.0);
                    let mut it = CoverItem::image(&rel, (tw - w) / 2.0, 50.0, w, (th - 60.0).max(20.0).min(w));
                    it.layer = active;
                    doc.items_mut().push(it);
                    self.cover_select_only(Some(doc.items().len() - 1));
                    changed = true;
                }
            }
            for kind in [ShapeKind::Rect, ShapeKind::Ellipse, ShapeKind::Line] {
                if ui.button(format!("+ {}", kind.label())).clicked() {
                    let w = (tw * 0.3).min(80.0);
                    let h = if kind == ShapeKind::Line { 0.0 } else { w };
                    let mut it = CoverItem::shape(kind, (tw - w) / 2.0, (th - h) / 2.0, w, h);
                    it.layer = active;
                    doc.items_mut().push(it);
                    self.cover_select_only(Some(doc.items().len() - 1));
                    changed = true;
                }
            }
        });
        ui.horizontal_wrapped(|ui| {
            if !sel.is_empty() {
                if ui.button("Dupliquer").clicked() { changed |= self.cover_duplicate(doc); }
                if ui.button("Supprimer").clicked() { changed |= self.cover_delete(doc); }
                if ui.button("Copier").on_hover_text("Ctrl+C").clicked() { self.cover_copy(&*doc); }
                if ui.button("Couper").on_hover_text("Ctrl+X").clicked() { if self.cover_copy(&*doc) { changed |= self.cover_delete(doc); } }
            }
            if ui.add_enabled(!self.cover_clip.is_empty(), egui::Button::new("Coller")).on_hover_text("Ctrl+V — fonctionne aussi d'une feuille à l'autre (jaquette ↔ étiquette)").clicked() {
                changed |= self.cover_paste(doc);
            }
            if sel.len() == 1 {
                let i = sel[0];
                let l = doc.items()[i].layer;
                if let Some(j) = (0..i).rev().find(|&j| doc.items()[j].layer == l) {
                    if ui.button("▼").on_hover_text("Reculer dans le calque").clicked() { doc.items_mut().swap(i, j); self.cover_select_only(Some(j)); changed = true; }
                }
                if let Some(j) = (i + 1..n).find(|&j| doc.items()[j].layer == l) {
                    if ui.button("▲").on_hover_text("Avancer dans le calque").clicked() { doc.items_mut().swap(i, j); self.cover_select_only(Some(j)); changed = true; }
                }
            }
        });
        // alignement : sur le cadre commun de la selection, ou sur la feuille pour un element seul
        if !sel.is_empty() {
            let mut align = None;
            ui.collapsing(if sel.len() > 1 { format!("Alignement ({} éléments)", sel.len()) } else { "Alignement et centrage".to_string() }, |ui| {
                ui.horizontal_wrapped(|ui| {
                    for (tip, a) in [("Aligner à gauche", ItemAlign::Left), ("Centrer horizontalement", ItemAlign::CenterH), ("Aligner à droite", ItemAlign::Right),
                                     ("Aligner en haut", ItemAlign::Top), ("Centrer verticalement", ItemAlign::CenterV), ("Aligner en bas", ItemAlign::Bottom)] {
                        if crate::editor_ui::align_button(ui, a, tip) { align = Some(a); }
                    }
                });
                ui.horizontal_wrapped(|ui| {
                    if ui.button("Centrer sur la feuille (↔)").clicked() { align = Some(ItemAlign::PageH); }
                    if ui.button("Centrer sur la feuille (↕)").clicked() { align = Some(ItemAlign::PageV); }
                });
                if sel.len() >= 3 {
                    ui.horizontal_wrapped(|ui| {
                        if ui.button("Répartir horizontalement").clicked() { align = Some(ItemAlign::DistributeH); }
                        if ui.button("Répartir verticalement").clicked() { align = Some(ItemAlign::DistributeV); }
                    });
                }
            });
            if let Some(a) = align { changed |= bdauthor_core::cover::align_cover(doc.items_mut(), &sel, a, (tw, th)); }
        }
        if sel.len() > 1 {
            ui.separator();
            ui.label(format!("{} éléments sélectionnés : glissez-les ensemble, alignez-les ou copiez-les. Sélectionnez-en un seul pour modifier ses réglages.", sel.len()));
            return changed;
        }
        let Some(i) = sel.first().copied() else { return changed };
        ui.separator();
        let zones = doc.zones();
        let base_dir = self.base_dir();
        let it = &mut doc.items_mut()[i];
        ui.horizontal(|ui| {
            ui.label("Nom");
            changed |= ui.add(egui::TextEdit::singleline(&mut it.name).hint_text("(facultatif)").desired_width(220.0)).changed();
        });
        let mut pick_image = false;
        let mut pick_font = false;
        match &mut it.kind {
            CoverKind::Text { text, size_pt, color, align, font, fx } => {
                changed |= ui.add(egui::TextEdit::multiline(text).desired_rows(3).desired_width(f32::INFINITY)).changed();
                ui.horizontal(|ui| {
                    egui::ComboBox::from_id_salt("jaquette_champs").selected_text("Insérer un champ automatique…").show_ui(ui, |ui| {
                        for (k, desc) in FIELDS {
                            if ui.selectable_label(false, format!("{{{k}}} — {desc}")).clicked() { text.push_str(&format!("{{{k}}}")); changed = true; }
                        }
                    });
                });
                ui.horizontal(|ui| {
                    ui.label("Taille");
                    changed |= ui.add(egui::DragValue::new(size_pt).range(4.0..=400.0).speed(0.5).suffix(" pt")).changed();
                    ui.label("Couleur");
                    changed |= ui.color_edit_button_srgb(color).changed();
                    egui::ComboBox::from_id_salt("jaquette_align").selected_text(align.name()).show_ui(ui, |ui| {
                        for a in [Align::Left, Align::Center, Align::Right] {
                            if ui.selectable_label(*align == a, a.name()).clicked() { *align = a; changed = true; }
                        }
                    });
                });
                ui.horizontal(|ui| {
                    ui.label("Police");
                    let shown = if font.is_empty() { "Police par défaut".to_string() } else {
                        bdauthor_render::common_fonts().into_iter().find(|(_, f)| f == font).map(|(l, _)| l.to_string()).unwrap_or_else(|| font.clone())
                    };
                    egui::ComboBox::from_id_salt("jaquette_police").selected_text(shown).width(200.0).show_ui(ui, |ui| {
                        if ui.selectable_label(font.is_empty(), "Police par défaut").clicked() { font.clear(); changed = true; }
                        for (label, file) in bdauthor_render::common_fonts() {
                            if ui.selectable_label(font == file, label).clicked() { *font = file.to_string(); changed = true; }
                        }
                        if ui.button("Autre fichier…").clicked() { pick_font = true; ui.close_menu(); }
                    });
                });
                ui.horizontal(|ui| {
                    ui.label("Position verticale");
                    for (v, t) in [(VAlign::Top, "Haut"), (VAlign::Middle, "Milieu"), (VAlign::Bottom, "Bas")] {
                        if ui.selectable_label(fx.valign == v, t).clicked() { fx.valign = v; changed = true; }
                    }
                    ui.label("Interligne");
                    changed |= ui.add(egui::DragValue::new(&mut fx.spacing).range(0.5..=3.0).speed(0.02)).changed();
                });
                changed |= ui.checkbox(&mut fx.autofit, "Ajuster au cadre").on_hover_text("Réduit la taille du texte s'il déborde du cadre").changed();
                ui.horizontal(|ui| {
                    ui.label("Contour");
                    changed |= ui.add(egui::DragValue::new(&mut fx.outline_mm).range(0.0..=5.0).speed(0.05).suffix(" mm")).changed();
                    changed |= ui.color_edit_button_srgb(&mut fx.outline_color).changed();
                    ui.label("Ombre");
                    changed |= ui.add(egui::DragValue::new(&mut fx.shadow_mm).range(0.0..=5.0).speed(0.05).suffix(" mm")).changed();
                    changed |= ui.color_edit_button_srgb(&mut fx.shadow_color).changed();
                });
                if !font.is_empty() {
                    if let Ok(r) = &self.renderer {
                        if r.custom_font(font, &base_dir).is_none() {
                            ui.colored_label(Color32::from_rgb(230, 100, 90), "Police introuvable : la police par défaut sera utilisée.");
                        }
                    }
                }
            }
            CoverKind::Image { path } => {
                ui.horizontal(|ui| {
                    changed |= ui.add(egui::TextEdit::singleline(path).desired_width(220.0)).changed();
                    if ui.button("Choisir…").clicked() { pick_image = true; }
                });
            }
            CoverKind::Shape { shape, fill, stroke, stroke_mm, radius_mm, .. } => {
                if *shape == ShapeKind::Line {
                    ui.horizontal(|ui| {
                        ui.label("Couleur");
                        let mut col = stroke.or(*fill).unwrap_or([255, 255, 255]);
                        if ui.color_edit_button_srgb(&mut col).changed() { *stroke = Some(col); *fill = None; changed = true; }
                        ui.label("Épaisseur");
                        changed |= ui.add(egui::DragValue::new(stroke_mm).range(0.1..=50.0).speed(0.05).suffix(" mm")).changed();
                    });
                } else {
                    ui.horizontal(|ui| {
                        let mut on = fill.is_some();
                        if ui.checkbox(&mut on, "Remplissage").changed() { *fill = if on { Some([255, 255, 255]) } else { None }; changed = true; }
                        if let Some(c) = fill { changed |= ui.color_edit_button_srgb(c).changed(); }
                    });
                    ui.horizontal(|ui| {
                        let mut on = stroke.is_some();
                        if ui.checkbox(&mut on, "Bordure").changed() { *stroke = if on { Some([0, 0, 0]) } else { None }; changed = true; }
                        if let Some(c) = stroke {
                            changed |= ui.color_edit_button_srgb(c).changed();
                            changed |= ui.add(egui::DragValue::new(stroke_mm).range(0.1..=50.0).speed(0.05).suffix(" mm")).changed();
                        }
                    });
                    if *shape == ShapeKind::Rect {
                        ui.horizontal(|ui| {
                            ui.label("Coins arrondis");
                            changed |= ui.add(egui::DragValue::new(radius_mm).range(0.0..=100.0).speed(0.1).suffix(" mm")).changed();
                        });
                    }
                }
            }
        }
        if pick_image {
            if let Some(f) = rfd::FileDialog::new().add_filter("Images", &IMG_EXT).pick_file() {
                let rel = self.relative(&f);
                if let CoverKind::Image { path } = &mut doc.items_mut()[i].kind { *path = rel; changed = true; }
            }
        }
        if pick_font {
            if let Some(f) = rfd::FileDialog::new().add_filter("Polices", &["ttf", "otf"]).pick_file() {
                let rel = self.relative(&f);
                if let CoverKind::Text { font, .. } = &mut doc.items_mut()[i].kind { *font = rel; changed = true; }
            }
        }
        let it = &mut doc.items_mut()[i];
        // ligne : seulement ses deux extremites (glisser les poignees sur la feuille, Maj : angle par pas de 45 degres)
        if let Some((mut p, mut q)) = it.line_ends() {
            let before = (p, q);
            egui::Grid::new("jaquette_line").num_columns(4).show(ui, |ui| {
                ui.label("Début X"); ui.add(egui::DragValue::new(&mut p.0).range(-bleed..=sw).speed(0.5).suffix(" mm"));
                ui.label("Y"); ui.add(egui::DragValue::new(&mut p.1).range(-bleed..=sh).speed(0.5).suffix(" mm"));
                ui.end_row();
                ui.label("Fin X"); ui.add(egui::DragValue::new(&mut q.0).range(-bleed..=sw).speed(0.5).suffix(" mm"));
                ui.label("Y"); ui.add(egui::DragValue::new(&mut q.1).range(-bleed..=sh).speed(0.5).suffix(" mm"));
                ui.end_row();
            });
            if p != before.0 { it.move_line_end(true, q, p); changed = true; }
            else if q != before.1 { it.move_line_end(false, p, q); changed = true; }
            if let CoverKind::Shape { style, arrow, .. } = &mut it.kind {
                ui.horizontal(|ui| {
                    ui.label("Type de trait");
                    egui::ComboBox::from_id_salt("jaquette_trait").selected_text(style.label()).show_ui(ui, |ui| {
                        for st in LineStyle::ALL { if ui.selectable_label(*style == st, st.label()).clicked() { *style = st; changed = true; } }
                    });
                });
                ui.horizontal(|ui| {
                    ui.label("Flèches");
                    egui::ComboBox::from_id_salt("jaquette_fleche").selected_text(arrow.label()).show_ui(ui, |ui| {
                        for a in LineArrow::ALL { if ui.selectable_label(*arrow == a, a.label()).clicked() { *arrow = a; changed = true; } }
                    });
                });
            }
            ui.label(egui::RichText::new("Déplacez les deux extrémités sur la feuille (Maj : angle par pas de 45°). « Début » = extrémité de gauche (ou du haut).").weak());
            return changed;
        }
        egui::Grid::new("jaquette_geom").num_columns(4).show(ui, |ui| {
            ui.label("X"); changed |= ui.add(egui::DragValue::new(&mut it.x).range(-bleed..=sw).speed(0.5).suffix(" mm")).changed();
            ui.label("Y"); changed |= ui.add(egui::DragValue::new(&mut it.y).range(-bleed..=sh).speed(0.5).suffix(" mm")).changed();
            ui.end_row();
            ui.label("Largeur"); changed |= ui.add(egui::DragValue::new(&mut it.w).range(1.0..=sw).speed(0.5).suffix(" mm")).changed();
            ui.label("Hauteur"); changed |= ui.add(egui::DragValue::new(&mut it.h).range(1.0..=sh).speed(0.5).suffix(" mm")).changed();
            ui.end_row();
        });
        ui.horizontal(|ui| {
            ui.label("Rotation");
            for (r, t) in [(0u16, "0°"), (90, "90°"), (270, "270°")] {
                if ui.selectable_label(it.rot == r, t).on_hover_text(if r == 0 { "Horizontal" } else if r == 90 { "Texte qui se lit de haut en bas (tranche)" } else { "Texte qui se lit de bas en haut" }).clicked() { it.rot = r; changed = true; }
            }
        });
        ui.horizontal_wrapped(|ui| {
            ui.label("Placer sur");
            for (name, z) in &zones {
                if ui.button(*name).clicked() {
                    let m = match *name { "la tranche" => 1.0, "le disque" => 0.0, "le haut" | "le bas" => 2.0, _ => 8.0 };
                    (it.x, it.y, it.w, it.h) = (z.0 + m, z.1 + m, (z.2 - 2.0 * m).max(1.0), (z.3 - 2.0 * m).max(1.0));
                    if *name == "la tranche" { it.rot = 90; }
                    changed = true;
                }
            }
        });
        ui.horizontal(|ui| {
            if ui.button("Centrer horizontalement").clicked() { it.x = (tw - it.w) / 2.0; changed = true; }
            if ui.button("Centrer verticalement").clicked() { it.y = (th - it.h) / 2.0; changed = true; }
        });
        ui.label(egui::RichText::new("Champs automatiques : {titre}, {liste_videos}, {nb_videos}, {duree_totale}, {nb_chapitres}, {annee}, remplis depuis le projet.").weak());
        changed
    }

    /// Choisit le fichier puis lance le rendu et l'ecriture du PDF dans un fil separe.
    fn cover_export(&mut self, kind: ExportKind) {
        let name = if self.cover_page == 0 { "jaquette" } else { "etiquette" };
        let what = if self.cover_page == 0 { "Jaquette" } else { "Étiquette" };
        let (ext, label, file) = match kind {
            ExportKind::Pdf => ("pdf", "PDF", format!("{name}.pdf")),
            ExportKind::A4 => ("pdf", "PDF", format!("{name}_A4.pdf")),
            ExportKind::Png { .. } => ("png", "PNG", format!("{name}.png")),
        };
        let Some(path) = rfd::FileDialog::new().add_filter(label, &[ext]).set_file_name(file).save_file() else { return };
        if let Err(e) = &self.renderer { self.notice = format!("Export impossible : {e}"); return; }
        let (base, vars) = (self.base_dir(), self.cover_vars());
        let (tx, rx) = channel();
        if self.cover_page == 0 { spawn_export(self.model.cover.clone(), kind, what, base, vars, path, tx); }
        else { spawn_export(self.model.label.clone(), kind, what, base, vars, path, tx); }
        self.cover_export_rx = Some(rx);
        self.notice = "Export en cours…".into();
    }

    fn cover_central(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        if self.cover_page == 0 {
            let mut d = std::mem::take(&mut self.model.cover);
            self.sheet_central(ui, ctx, &mut d);
            self.model.cover = d;
        } else {
            let mut d = std::mem::take(&mut self.model.label);
            self.sheet_central(ui, ctx, &mut d);
            self.model.label = d;
        }
    }

    fn sheet_central(&mut self, ui: &mut egui::Ui, ctx: &egui::Context, doc: &mut dyn SheetDoc) {
        // apercu regenere quand le document, les champs automatiques ou la finesse necessaire ont change
        let vars = self.cover_vars();
        if vars != self.cover_vars_seen {
            self.cover_vars_seen = vars.clone();
            self.cover_gen += 1;
        }
        let (sw, sh) = doc.sheet_mm();
        let bleed = doc.bleed_mm() as f32;
        let avail = ui.available_size();
        let area = vec2(avail.x, (avail.y - 54.0).max(120.0));
        let vp_size = vec2((area.x - RULER_W).max(50.0), (area.y - RULER_H).max(50.0));
        // echelle « page entiere dans la fenetre » (points par mm) ; le zoom la multiplie
        let k0 = ((vp_size.x - 16.0) / sw as f32).min((vp_size.y - 16.0) / sh as f32).max(0.2);
        let real = 96.0_f32 / 25.4; // points par mm a la taille reelle (ecran a 96 dpi)
        ui.horizontal(|ui| {
            let z = self.cover_zoom;
            if ui.small_button("−").on_hover_text("Dézoomer").clicked() { let nz = (z / 1.25).max(0.25); self.cover_pan *= nz / z; self.cover_zoom = nz; }
            if ui.small_button("+").on_hover_text("Zoomer").clicked() { let nz = (z * 1.25).min(16.0); self.cover_pan *= nz / z; self.cover_zoom = nz; }
            if ui.small_button("Ajuster").on_hover_text("Feuille entière dans la fenêtre").clicked() { self.cover_zoom = 1.0; self.cover_pan = egui::Vec2::ZERO; }
            if ui.small_button("1:1").on_hover_text("Taille réelle (approximative : écran supposé à 96 dpi)").clicked() { self.cover_zoom = (real / k0).clamp(0.25, 16.0); self.cover_pan = egui::Vec2::ZERO; }
            ui.label(format!("{:.0} %", k0 * self.cover_zoom / real * 100.0)).on_hover_text("100 % = taille réelle");
            ui.separator();
            ui.selectable_value(&mut self.cover_inch, false, "mm");
            ui.selectable_value(&mut self.cover_inch, true, "pouces");
            ui.label(egui::RichText::new("Molette : zoom · clic molette (ou Espace + glisser) : déplacer la vue").weak());
        });
        let (resp, full_painter) = ui.allocate_painter(area, Sense::click_and_drag());
        let full = resp.rect;
        let viewport = Rect::from_min_max(full.min + vec2(RULER_W, RULER_H), full.max);
        // deplacement (clic molette ou Espace + bouton gauche) et zoom a la molette, centre sur le curseur
        let panning = (resp.hovered() || self.cover_panning) && !ctx.wants_keyboard_input()
            && ctx.input(|i| i.pointer.middle_down() || (i.key_down(Key::Space) && i.pointer.primary_down()));
        let skip_input = panning || self.cover_panning;
        self.cover_panning = panning;
        if panning {
            self.cover_pan += ctx.input(|i| i.pointer.delta());
            ctx.set_cursor_icon(CursorIcon::Grabbing);
        }
        let hover_vp = resp.hover_pos().filter(|p| viewport.contains(*p));
        if let Some(ptr) = hover_vp {
            let (scroll, zd) = ctx.input(|i| (i.smooth_scroll_delta.y, i.zoom_delta()));
            let f = (scroll * 0.0015).exp() * zd;
            if (f - 1.0).abs() > 1e-4 {
                let z = self.cover_zoom;
                let nz = (z * f).clamp(0.25, 16.0);
                let c0 = viewport.center() + self.cover_pan;
                self.cover_pan = (ptr - (ptr - c0) * (nz / z)) - viewport.center();
                self.cover_zoom = nz;
                ctx.input_mut(|i| i.smooth_scroll_delta = egui::Vec2::ZERO);
            }
        }
        let k = k0 * self.cover_zoom;
        let half = (viewport.size() + vec2(sw as f32 * k, sh as f32 * k)) * 0.5 - vec2(40.0, 40.0);
        self.cover_pan = vec2(self.cover_pan.x.clamp(-half.x.max(0.0), half.x.max(0.0)), self.cover_pan.y.clamp(-half.y.max(0.0), half.y.max(0.0)));
        let rect = Rect::from_center_size(viewport.center() + self.cover_pan, vec2(sw as f32 * k, sh as f32 * k));
        // finesse de l'apercu selon le zoom : 96, 144 ou 192 dpi
        let need = k * ctx.pixels_per_point() * 25.4;
        let want_dpi = if need <= 110.0 { PREVIEW_DPI } else if need <= 165.0 { 144 } else { 192 };
        if self.cover_gen != self.cover_seen || self.cover_tex.is_none() || self.cover_tex_dpi != want_dpi {
            let base = self.base_dir();
            if let Ok(r) = &self.renderer {
                if let Ok(img) = r.render_sheet(&*doc, want_dpi, &base, &vars) {
                    let ci = egui::ColorImage::from_rgba_unmultiplied([img.width as usize, img.height as usize], &img.data);
                    match &mut self.cover_tex {
                        Some(t) => t.set(ci, egui::TextureOptions::LINEAR),
                        None => self.cover_tex = Some(ctx.load_texture("jaquette", ci, egui::TextureOptions::LINEAR)),
                    }
                }
            }
            self.cover_seen = self.cover_gen;
            self.cover_tex_dpi = want_dpi;
        }
        let painter = full_painter.with_clip_rect(viewport);
        painter.rect_filled(viewport, 0.0, Color32::from_gray(18));
        match &self.cover_tex {
            Some(t) => painter.image(t.id(), rect, Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), Color32::WHITE),
            None => painter.rect_filled(rect, 0.0, Color32::from_gray(30)),
        };
        let origin = rect.min;
        let to_screen = |x: f64, y: f64| pos2(origin.x + (x as f32 + bleed) * k, origin.y + (y as f32 + bleed) * k);
        let to_mm = |p: egui::Pos2| (((p.x - origin.x) / k - bleed) as f64, ((p.y - origin.y) / k - bleed) as f64);
        // reperes : feuille rognee, pliages, cercles
        let guide = Stroke::new(1.0_f32, Color32::from_white_alpha(120));
        let (rects, lines, circles) = doc.guides();
        for (x, y, w, h) in rects { painter.rect_stroke(Rect::from_min_max(to_screen(x, y), to_screen(x + w, y + h)), 0.0, guide); }
        for (x1, y1, x2, y2) in lines { painter.line_segment([to_screen(x1, y1), to_screen(x2, y2)], guide); }
        for (cx, cy, r) in circles { painter.circle_stroke(to_screen(cx, cy), r as f32 * k, guide); }
        let selection = self.cover_selection(doc.items().len());
        // un seul element selectionne : poignees de redimensionnement
        let sel = if selection.len() == 1 { Some(selection[0]) } else { None };
        // zone de securite
        if self.cover_safe {
            let orange = Stroke::new(1.0_f32, Color32::from_rgb(255, 170, 60));
            let (srects, scircles) = doc.safe_guides();
            for (x, y, w, h) in srects {
                let pts = [to_screen(x, y), to_screen(x + w, y), to_screen(x + w, y + h), to_screen(x, y + h), to_screen(x, y)];
                painter.extend(egui::Shape::dashed_line(&pts, orange, 6.0, 4.0));
            }
            for (cx, cy, r) in scircles { painter.circle_stroke(to_screen(cx, cy), r as f32 * k, orange); }
        }
        // regles graduees (mm ou pouces), zero au coin haut gauche de la feuille rognee ; reperes du pointeur et de la selection
        {
            let top = Rect::from_min_max(pos2(viewport.min.x, full.min.y), pos2(viewport.max.x, viewport.min.y));
            let left = Rect::from_min_max(pos2(full.min.x, viewport.min.y), pos2(viewport.min.x, viewport.max.y));
            let band = if selection.is_empty() { None } else {
                let (mut x0, mut y0, mut x1, mut y1) = (f64::MAX, f64::MAX, f64::MIN, f64::MIN);
                for &i in &selection { let it = &doc.items()[i]; x0 = x0.min(it.x); y0 = y0.min(it.y); x1 = x1.max(it.x + it.w); y1 = y1.max(it.y + it.h); }
                Some((to_screen(x0, y0), to_screen(x1, y1)))
            };
            draw_ruler(&full_painter, top, true, origin.x + bleed * k, k, self.cover_inch, hover_vp.map(|p| p.x), band.map(|b| (b.0.x, b.1.x)));
            draw_ruler(&full_painter, left, false, origin.y + bleed * k, k, self.cover_inch, hover_vp.map(|p| p.y), band.map(|b| (b.0.y, b.1.y)));
            let corner = Rect::from_min_max(full.min, viewport.min);
            full_painter.rect_filled(corner, 0.0, Color32::from_gray(38));
            full_painter.text(corner.center(), egui::Align2::CENTER_CENTER, if self.cover_inch { "in" } else { "mm" }, egui::FontId::proportional(9.0), Color32::from_gray(190));
        }
        let layers = doc.layers().clone();
        let usable = |it: &CoverItem| { let (v, l) = layer_state(&layers, it.layer); v && !l };
        // cadres des elements : poignees pour un element seul, tirets pour une selection multiple ; une ligne n'a que ses deux extremites
        for (i, it) in doc.items().iter().enumerate() {
            if !layer_state(&layers, it.layer).0 { continue; }
            let r = Rect::from_min_max(to_screen(it.x, it.y), to_screen(it.x + it.w, it.y + it.h));
            let pts: Vec<egui::Pos2> = match it.line_ends() {
                Some((a, b)) => vec![to_screen(a.0, a.1), to_screen(b.0, b.1)],
                None => vec![r.left_top(), r.right_top(), r.right_bottom(), r.left_bottom(), r.left_top()],
            };
            let is_line = pts.len() == 2;
            if sel == Some(i) {
                if is_line {
                    painter.add(egui::Shape::line(pts.clone(), Stroke::new(1.0_f32, Color32::from_black_alpha(170))));
                } else {
                    painter.rect_stroke(r, 0.0, Stroke::new(2.5_f32, Color32::WHITE));
                }
                let corners: Vec<egui::Pos2> = if is_line { pts.clone() } else { vec![r.left_top(), r.right_top(), r.left_bottom(), r.right_bottom()] };
                for c in corners {
                    painter.rect_filled(Rect::from_center_size(c, vec2(9.0, 9.0)), 1.0, Color32::WHITE);
                    painter.rect_stroke(Rect::from_center_size(c, vec2(9.0, 9.0)), 1.0, Stroke::new(1.0_f32, Color32::BLACK));
                }
            } else if selection.contains(&i) {
                // selection multiple : meme rendu que l'editeur de menu (fond sombre + tirets clairs, visible sur tout decor)
                painter.add(egui::Shape::line(pts.clone(), Stroke::new(3.0_f32, Color32::from_black_alpha(170))));
                painter.extend(egui::Shape::dashed_line(&pts, Stroke::new(2.0_f32, Color32::WHITE), 7.0, 5.0));
            } else if is_line {
                painter.add(egui::Shape::line(pts, Stroke::new(1.0_f32, Color32::from_rgba_unmultiplied(120, 190, 255, 110))));
            } else {
                painter.rect_stroke(r, 0.0, Stroke::new(1.0_f32, Color32::from_rgba_unmultiplied(120, 190, 255, 110)));
            }
        }
        // poignee de coin sous le pointeur de l'element selectionne (1..4), 0 sinon
        let handle_at = |p: egui::Pos2, it: &CoverItem| -> u8 {
            if let Some((a, b)) = it.line_ends() {
                let (sa, sb) = (to_screen(a.0, a.1), to_screen(b.0, b.1));
                return if sa.distance(p) <= HANDLE_PX { 5 } else if sb.distance(p) <= HANDLE_PX { 6 } else { 0 };
            }
            let r = Rect::from_min_max(to_screen(it.x, it.y), to_screen(it.x + it.w, it.y + it.h));
            for (h, c) in [(1u8, r.left_top()), (2, r.right_top()), (3, r.left_bottom()), (4, r.right_bottom())] {
                if c.distance(p) <= HANDLE_PX { return h; }
            }
            0
        };
        let hit = |p: egui::Pos2, items: &[CoverItem]| -> Option<usize> {
            if !viewport.contains(p) { return None; }
            let (x, y) = to_mm(p);
            (0..items.len()).filter(|&i| {
                let it = &items[i];
                if !usable(it) { return false; }
                match it.line_ends() {
                    Some((a, b)) => {
                        let sw = match &it.kind { CoverKind::Shape { stroke_mm, .. } => *stroke_mm as f32 * k / 2.0, _ => 0.0 };
                        seg_dist(p, to_screen(a.0, a.1), to_screen(b.0, b.1)) <= sw.max(6.0)
                    }
                    None => x >= it.x && x <= it.x + it.w && y >= it.y && y <= it.y + it.h,
                }
            }).max_by_key(|&i| (items[i].layer, i))
        };
        // curseur
        let icon_of = |h: u8| if h == 1 || h == 4 { CursorIcon::ResizeNwSe } else { CursorIcon::ResizeNeSw };
        if let Some(d) = &self.cover_drag {
            ctx.set_cursor_icon(if d.handle == 0 { CursorIcon::Grabbing } else if d.handle >= 5 { CursorIcon::Crosshair } else { icon_of(d.handle) });
        } else if let Some(p) = hover_vp {
            let h = sel.filter(|&i| usable(&doc.items()[i])).map(|i| handle_at(p, &doc.items()[i])).unwrap_or(0);
            if h >= 5 { ctx.set_cursor_icon(CursorIcon::Crosshair); }
            else if h != 0 { ctx.set_cursor_icon(icon_of(h)); }
            else if hit(p, doc.items()).is_some() { ctx.set_cursor_icon(CursorIcon::Grab); }
        }
        let extend = ctx.input(|i| i.modifiers.shift || i.modifiers.command);
        let len = doc.items().len();
        // debut : poignee de l'element selectionne ; sinon element sous la souris (Maj/Ctrl : s'ajoute a la selection) ; sinon rectangle de selection
        if skip_input {
            self.cover_drag = None;
            self.cover_marquee = None;
        } else if resp.drag_started() {
            if let Some(p) = ui.input(|i| i.pointer.press_origin()).or(resp.interact_pointer_pos()) {
                let mut target = None;
                if let Some(i) = sel.filter(|&i| usable(&doc.items()[i])) {
                    let h = handle_at(p, &doc.items()[i]);
                    if h != 0 { target = Some((i, h)); }
                }
                if target.is_none() {
                    if let Some(i) = hit(p, doc.items()) {
                        if !selection.contains(&i) {
                            if extend { let mut v = selection.clone(); v.push(i); self.cover_set_selection(v, i); } else { self.cover_select_only(Some(i)); }
                        }
                        target = Some((i, 0));
                    }
                }
                match target {
                    Some((i, h)) => {
                        let group: Vec<(usize, (f64, f64, f64, f64))> = if h == 0 {
                            self.cover_selection(len).into_iter().filter(|&j| usable(&doc.items()[j])).map(|j| { let it = &doc.items()[j]; (j, (it.x, it.y, it.w, it.h)) }).collect()
                        } else { Vec::new() };
                        let it = &doc.items()[i];
                        self.cover_drag = Some(CoverDrag { item: i, handle: h, orig: (it.x, it.y, it.w, it.h), start: to_mm(p), group, fixed: it.line_ends().map(|(a, b)| if h == 5 { b } else { a }).unwrap_or((0.0, 0.0)) });
                    }
                    None => if viewport.contains(p) {
                        let base = if extend { selection.clone() } else { Vec::new() };
                        if !extend { self.cover_select_only(None); }
                        self.cover_marquee = Some((to_mm(p), base));
                    }
                }
            }
        } else if resp.clicked() {
            if let Some(p) = resp.interact_pointer_pos() {
                match hit(p, doc.items()) {
                    Some(i) if extend => {
                        let mut v = selection.clone();
                        if let Some(k) = v.iter().position(|&e| e == i) { v.remove(k); } else { v.push(i); }
                        match v.last().copied() { Some(last) => self.cover_set_selection(v, last), None => self.cover_select_only(None) }
                    }
                    Some(i) => self.cover_select_only(Some(i)),
                    None => if !extend && viewport.contains(p) { self.cover_select_only(None); },
                }
            }
        }
        // en cours : la position est toujours calculee depuis le point de depart, sans derive
        let mut snapped: (Option<f64>, Option<f64>) = (None, None);
        if resp.dragged() && !skip_input {
            if let (Some(d), Some(p)) = (self.cover_drag.clone(), resp.interact_pointer_pos()) {
                if d.item < len && d.handle < 5 {
                    let (mx, my) = to_mm(p);
                    let (dx, dy) = (mx - d.start.0, my - d.start.1);
                    let (x, y, w, h) = d.orig;
                    let (nx, ny, nw, nh) = match d.handle {
                        0 => (x + dx, y + dy, w, h),
                        1 => { let nw = (w - dx).max(MIN_MM); let nh = (h - dy).max(MIN_MM); (x + w - nw, y + h - nh, nw, nh) }
                        2 => { let nw = (w + dx).max(MIN_MM); let nh = (h - dy).max(MIN_MM); (x, y + h - nh, nw, nh) }
                        3 => { let nw = (w - dx).max(MIN_MM); let nh = (h + dy).max(MIN_MM); (x + w - nw, y, nw, nh) }
                        _ => (x, y, (w + dx).max(MIN_MM), (h + dy).max(MIN_MM)),
                    };
                    // Maj : coin tire en gardant les proportions d'origine (le coin oppose reste fixe)
                    let proportional = d.handle != 0 && ctx.input(|i| i.modifiers.shift) && w > 0.0 && h > 0.0;
                    let (mut nx, mut ny, mut nw, mut nh) = if proportional {
                        let s = (nw / w).max(nh / h).max(MIN_MM / w).max(MIN_MM / h);
                        let (rw, rh) = (w * s, h * s);
                        (if d.handle == 1 || d.handle == 3 { x + w - rw } else { x }, if d.handle == 1 || d.handle == 2 { y + h - rh } else { y }, rw, rh)
                    } else { (nx, ny, nw, nh) };
                    // aimantation (Alt : desactivee) sur les reperes de la feuille et les bords et centres des autres elements
                    if self.cover_snap && !proportional && !ctx.input(|i| i.modifiers.alt) {
                        let (mut xs, mut ys) = doc.snap_lines();
                        for (j, it) in doc.items().iter().enumerate() {
                            if j == d.item || d.group.iter().any(|g| g.0 == j) || !layer_state(&layers, it.layer).0 { continue; }
                            xs.extend([it.x, it.x + it.w / 2.0, it.x + it.w]);
                            ys.extend([it.y, it.y + it.h / 2.0, it.y + it.h]);
                        }
                        let (r2, gx, gy) = bdauthor_core::cover::snap_rect(d.handle, (nx, ny, nw, nh), &xs, &ys, 6.0 / k as f64);
                        (nx, ny, nw, nh) = r2;
                        snapped = (gx, gy);
                    }
                    let r = |v: f64| (v * 100.0).round() / 100.0;
                    let items = doc.items_mut();
                    let moved = (items[d.item].x, items[d.item].y, items[d.item].w, items[d.item].h) != (r(nx), r(ny), r(nw), r(nh));
                    if moved {
                        let (ddx, ddy) = (r(nx) - x, r(ny) - y);
                        if d.handle == 0 && !d.group.is_empty() {
                            for (j, o) in &d.group { if *j < len { items[*j].x = r(o.0 + ddx); items[*j].y = r(o.1 + ddy); } }
                        }
                        let it = &mut items[d.item];
                        (it.x, it.y, it.w, it.h) = (r(nx), r(ny), r(nw), r(nh));
                        self.cover_changed();
                    }
                }
            }
        }
        // extremite d'une ligne deplacee : libre, aimantee (Alt : non) ou, avec Maj, par pas de 45 degres autour de l'autre extremite
        if resp.dragged() && !skip_input {
            if let (Some(d), Some(p)) = (self.cover_drag.clone(), resp.interact_pointer_pos()) {
                if d.item < len && d.handle >= 5 {
                    let (mut mx, mut my) = to_mm(p);
                    let (fx, fy) = d.fixed;
                    if ctx.input(|i| i.modifiers.shift) {
                        let (ddx, ddy) = (mx - fx, my - fy);
                        let step = std::f64::consts::FRAC_PI_4;
                        let ang = (ddy.atan2(ddx) / step).round() * step;
                        let len_mm = ddx.hypot(ddy);
                        (mx, my) = (fx + len_mm * ang.cos(), fy + len_mm * ang.sin());
                    } else if self.cover_snap && !ctx.input(|i| i.modifiers.alt) {
                        let (mut xs, mut ys) = doc.snap_lines();
                        for (j, it) in doc.items().iter().enumerate() {
                            if j == d.item || !layer_state(&layers, it.layer).0 { continue; }
                            xs.extend([it.x, it.x + it.w / 2.0, it.x + it.w]);
                            ys.extend([it.y, it.y + it.h / 2.0, it.y + it.h]);
                        }
                        // l'autre extremite compte aussi : lignes parfaitement horizontales ou verticales
                        xs.push(fx);
                        ys.push(fy);
                        let thr = 6.0 / k as f64;
                        let near = |v: f64, l: &[f64]| l.iter().copied().filter(|g| (g - v).abs() <= thr).min_by(|a, b| (a - v).abs().total_cmp(&(b - v).abs()));
                        if let Some(g) = near(mx, &xs) { mx = g; snapped.0 = Some(g); }
                        if let Some(g) = near(my, &ys) { my = g; snapped.1 = Some(g); }
                    }
                    let before = doc.items()[d.item].line_ends();
                    doc.items_mut()[d.item].move_line_end(d.handle == 5, (fx, fy), (mx, my));
                    if doc.items()[d.item].line_ends() != before { self.cover_changed(); }
                }
            }
        }
        // rectangle de selection
        if let (Some((s0, base)), true) = (self.cover_marquee.clone(), resp.dragged() && !skip_input) {
            if let Some(p) = resp.interact_pointer_pos() {
                let (mx, my) = to_mm(p);
                let (x0, x1, y0, y1) = (s0.0.min(mx), s0.0.max(mx), s0.1.min(my), s0.1.max(my));
                let mut v = base;
                for (j, it) in doc.items().iter().enumerate() {
                    if usable(it) && it.x < x1 && it.x + it.w > x0 && it.y < y1 && it.y + it.h > y0 && !v.contains(&j) { v.push(j); }
                }
                match v.last().copied() { Some(last) => self.cover_set_selection(v, last), None => self.cover_select_only(None) }
                let rc = Rect::from_two_pos(to_screen(x0, y0), to_screen(x1, y1));
                painter.rect_filled(rc, 0.0, Color32::from_rgba_unmultiplied(120, 190, 255, 30));
                painter.rect_stroke(rc, 0.0, Stroke::new(1.0_f32, Color32::from_rgb(120, 190, 255)));
            }
        }
        // traits d'aimantation
        if let (Some(g), true) = (snapped.0, self.cover_drag.is_some()) {
            painter.line_segment([to_screen(g, -(bleed as f64)), to_screen(g, sh - bleed as f64)], Stroke::new(1.0_f32, Color32::from_rgb(255, 80, 200)));
        }
        if let (Some(g), true) = (snapped.1, self.cover_drag.is_some()) {
            painter.line_segment([to_screen(-(bleed as f64), g), to_screen(sw - bleed as f64, g)], Stroke::new(1.0_f32, Color32::from_rgb(255, 80, 200)));
        }
        if resp.drag_stopped() { self.cover_drag = None; self.cover_marquee = None; }
        if self.cover_keys(ctx, doc) { self.cover_changed(); }
        let pos_txt = hover_vp.map(|p| { let (x, y) = to_mm(p); if self.cover_inch { format!("X {:.2} in   Y {:.2} in   ·   ", x / 25.4, y / 25.4) } else { format!("X {x:.1} mm   Y {y:.1} mm   ·   ") } }).unwrap_or_default();
        ui.label(egui::RichText::new(format!("{pos_txt}Glisser : déplacer · coins : redimensionner (Maj : proportionnel) · rectangle ou Maj+clic : sélection multiple · flèches : déplacer de 0,5 mm (Maj 5 mm, Ctrl 0,1 mm) · Alt : sans aimantation · Ctrl+C/X/V · Suppr · Ctrl+Z / Ctrl+Y")).weak());
    }

    /// Elements selectionnes (principal et supplementaires), tries et sans doublon.
    fn cover_selection(&self, len: usize) -> Vec<usize> {
        let mut v: Vec<usize> = self.cover_sel.into_iter().chain(self.cover_extra.iter().copied()).filter(|&i| i < len).collect();
        v.sort_unstable();
        v.dedup();
        v
    }

    fn cover_select_only(&mut self, i: Option<usize>) {
        self.cover_sel = i;
        self.cover_extra.clear();
    }

    /// Selection de `v`, `primary` etant l'element principal.
    fn cover_set_selection(&mut self, v: Vec<usize>, primary: usize) {
        self.cover_sel = Some(primary);
        self.cover_extra = v.into_iter().filter(|&e| e != primary).collect();
    }

    /// Copie la selection dans le presse-papiers de l'application ; faux si rien n'est selectionne.
    fn cover_copy(&mut self, doc: &dyn SheetDoc) -> bool {
        let sel = self.cover_selection(doc.items().len());
        if sel.is_empty() { return false; }
        self.cover_clip = sel.iter().map(|&i| doc.items()[i].clone()).collect();
        self.cover_paste_n = 0;
        true
    }

    /// Supprime les elements selectionnes.
    fn cover_delete(&mut self, doc: &mut dyn SheetDoc) -> bool {
        let sel = self.cover_selection(doc.items().len());
        for &i in sel.iter().rev() { doc.items_mut().remove(i); }
        self.cover_select_only(None);
        !sel.is_empty()
    }

    /// Colle le presse-papiers (decale de 5 mm a chaque collage, y compris d'un onglet a l'autre : jaquette <-> etiquette).
    fn cover_paste(&mut self, doc: &mut dyn SheetDoc) -> bool {
        if self.cover_clip.is_empty() { return false; }
        self.cover_paste_n += 1;
        let off = 5.0 * self.cover_paste_n as f64;
        let nl = layers_or_default(doc.layers()).len();
        let first = doc.items().len();
        for it in self.cover_clip.clone() {
            let mut c = it;
            c.x += off;
            c.y += off;
            c.layer = c.layer.min(nl - 1);
            if !c.name.is_empty() { c.name = unique_copy_name(&c.name, doc.items()); }
            doc.items_mut().push(c);
        }
        let v: Vec<usize> = (first..doc.items().len()).collect();
        if let Some(&last) = v.last() { self.cover_set_selection(v, last); }
        true
    }

    /// Duplique les elements selectionnes (decales de 5 mm, au premier plan de leur calque).
    fn cover_duplicate(&mut self, doc: &mut dyn SheetDoc) -> bool {
        let sel = self.cover_selection(doc.items().len());
        if sel.is_empty() { return false; }
        let first = doc.items().len();
        for &i in &sel {
            let mut c = doc.items()[i].clone();
            c.x += 5.0;
            c.y += 5.0;
            if !c.name.is_empty() { c.name = unique_copy_name(&c.name, doc.items()); }
            doc.items_mut().push(c);
        }
        let v: Vec<usize> = (first..doc.items().len()).collect();
        if let Some(&last) = v.last() { self.cover_set_selection(v, last); }
        true
    }

    /// Clavier : copier / couper / coller (evenements egui, marqueur dans le presse-papiers systeme), fleches, Suppr.
    fn cover_keys(&mut self, ctx: &egui::Context, doc: &mut dyn SheetDoc) -> bool {
        if ctx.wants_keyboard_input() || self.cover_drag.is_some() { return false; }
        use egui::Modifiers;
        let (copy, cut, paste) = ctx.input_mut(|i| {
            let has = |i: &egui::InputState, f: fn(&egui::Event) -> bool| i.events.iter().any(f);
            let copy = has(i, |e| matches!(e, egui::Event::Copy)) || i.consume_key(Modifiers::COMMAND, Key::C);
            let cut = has(i, |e| matches!(e, egui::Event::Cut)) || i.consume_key(Modifiers::COMMAND, Key::X);
            let paste = has(i, |e| matches!(e, egui::Event::Paste(t) if t.as_str() == COVER_MARK)) || i.consume_key(Modifiers::COMMAND, Key::V);
            (copy, cut, paste)
        });
        let mut changed = false;
        if copy || cut {
            if self.cover_copy(&*doc) {
                // egui-winit ne transmet « coller » que si le presse-papiers systeme contient du texte : on y depose un marqueur
                ctx.copy_text(COVER_MARK.to_string());
                if cut { changed |= self.cover_delete(doc); }
            }
        }
        if paste { changed |= self.cover_paste(doc); }
        if ctx.input(|i| i.key_pressed(Key::Delete)) { changed |= self.cover_delete(doc); }
        // fleches : 0,5 mm (Maj : 5 mm, Ctrl : 0,1 mm)
        let (mut dx, mut dy) = (0.0_f64, 0.0_f64);
        ctx.input_mut(|i| {
            for (m, s) in [(Modifiers::NONE, 0.5), (Modifiers::SHIFT, 5.0), (Modifiers::COMMAND, 0.1)] {
                if i.consume_key(m, Key::ArrowLeft) { dx -= s; }
                if i.consume_key(m, Key::ArrowRight) { dx += s; }
                if i.consume_key(m, Key::ArrowUp) { dy -= s; }
                if i.consume_key(m, Key::ArrowDown) { dy += s; }
            }
        });
        if dx != 0.0 || dy != 0.0 {
            let layers = doc.layers().clone();
            for i in self.cover_selection(doc.items().len()) {
                let it = &mut doc.items_mut()[i];
                if layer_state(&layers, it.layer) != (true, false) && layer_state(&layers, it.layer).1 { continue; }
                it.x = ((it.x + dx) * 100.0).round() / 100.0;
                it.y = ((it.y + dy) * 100.0).round() / 100.0;
                changed = true;
            }
        }
        changed
    }
}

/// Texte depose dans le presse-papiers systeme a la copie d'elements de jaquette (le contenu reel reste dans l'application).
const COVER_MARK: &str = "bdauthor:jaquette";

/// « Nom (copie) », puis « Nom (copie 2) », « Nom (copie 3) »… : premier nom libre parmi les elements.
fn unique_copy_name(name: &str, items: &[CoverItem]) -> String {
    let base = match name.rfind(" (copie") { Some(k) if name.ends_with(')') => &name[..k], _ => name };
    let taken = |n: &str| items.iter().any(|it| it.name == n);
    let first = format!("{base} (copie)");
    if !taken(&first) { return first; }
    (2..).map(|k| format!("{base} (copie {k})")).find(|n| !taken(n)).unwrap_or(first)
}
