//! Onglet « Jaquette » : feuille a deplier du boitier (dos, tranche, face) et etiquette de disque ; edition a la souris,
//! annuler / refaire, champs automatiques et export PDF en arriere-plan.
use crate::App;
use bdauthor_core::cover::{Cover, CoverItem, CoverKind, DiscLabel, SheetDoc, FIELDS, FORMATS};
use bdauthor_core::design::Align;
use eframe::egui::{self, pos2, vec2, Color32, CursorIcon, Key, Rect, Sense, Stroke};
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::mpsc::{channel, TryRecvError};

/// Resolution (dpi) de l'apercu a l'ecran.
const PREVIEW_DPI: u32 = 96;
/// Taille minimale d'un element (mm) et rayon (pixels) de saisie d'une poignee.
const MIN_MM: f64 = 2.0;
const HANDLE_PX: f32 = 9.0;

/// Deplacement ou redimensionnement a la souris en cours : `handle` 0 = deplacer, 1..4 = coins haut gauche, haut droit, bas gauche, bas droit.
#[derive(Clone)]
pub(crate) struct CoverDrag {
    item: usize,
    handle: u8,
    orig: (f64, f64, f64, f64),
    start: (f64, f64),
}

fn item_label(it: &CoverItem, i: usize) -> String {
    if !it.name.is_empty() { return format!("{}. {}", i + 1, it.name); }
    match &it.kind {
        CoverKind::Text { text, .. } => format!("{}. Texte « {} »", i + 1, text.lines().next().unwrap_or("").chars().take(24).collect::<String>()),
        CoverKind::Image { path } => format!("{}. Image {}", i + 1, std::path::Path::new(path).file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()),
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

fn spawn_export<D: SheetDoc + Send + 'static>(doc: D, what: &'static str, base: PathBuf, vars: BTreeMap<String, String>, path: PathBuf, tx: std::sync::mpsc::Sender<String>) {
    std::thread::spawn(move || {
        let run = || -> Result<String, String> {
            let r = bdauthor_render::FontRenderer::new().map_err(|e| e.to_string())?;
            let img = r.render_sheet(&doc, doc.dpi(), &base, &vars).map_err(|e| e.to_string())?;
            let title = path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_else(|| what.into());
            std::fs::write(&path, doc.to_pdf(&img, &title)).map_err(|e| e.to_string())?;
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
            self.cover_sel = None;
            self.cover_drag = None;
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
            self.cover_sel = None;
            self.cover_drag = None;
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
        } else if ui.button("Exporter en PDF…").on_hover_text("Image à la résolution choisie, dans un PDF aux dimensions exactes").clicked() {
            self.cover_export();
        }
        if changed { self.cover_changed(); }
    }

    /// Liste, boutons et inspecteur des elements d'une feuille.
    fn items_ui(&mut self, ui: &mut egui::Ui, doc: &mut dyn SheetDoc) -> bool {
        let mut changed = false;
        let n = doc.items().len();
        egui::ScrollArea::vertical().id_salt("jaquette_liste").max_height(160.0).show(ui, |ui| {
            for i in 0..n {
                let label = item_label(&doc.items()[i], i);
                if ui.selectable_label(self.cover_sel == Some(i), label).clicked() { self.cover_sel = Some(i); }
            }
            if n == 0 { ui.label(egui::RichText::new("(aucun élément)").weak()); }
        });
        let (sw, sh) = doc.sheet_mm();
        let bleed = doc.bleed_mm();
        let (tw, th) = (sw - 2.0 * bleed, sh - 2.0 * bleed);
        ui.horizontal_wrapped(|ui| {
            if ui.button("+ Texte").clicked() {
                let w = (tw * 0.4).min(100.0);
                doc.items_mut().push(CoverItem::text("Titre", (tw - w) / 2.0, 10.0, w, 30.0, 28.0));
                self.cover_sel = Some(doc.items().len() - 1);
                changed = true;
            }
            if ui.button("+ Image…").clicked() {
                if let Some(f) = rfd::FileDialog::new().add_filter("Images", &IMG_EXT).pick_file() {
                    let rel = self.relative(&f);
                    let w = (tw * 0.4).min(100.0);
                    doc.items_mut().push(CoverItem::image(&rel, (tw - w) / 2.0, 50.0, w, (th - 60.0).max(20.0).min(w)));
                    self.cover_sel = Some(doc.items().len() - 1);
                    changed = true;
                }
            }
            if let Some(i) = self.cover_sel.filter(|&i| i < n) {
                if ui.button("Dupliquer").clicked() {
                    let mut c = doc.items()[i].clone();
                    c.x += 5.0;
                    c.y += 5.0;
                    c.name = if c.name.is_empty() { String::new() } else { format!("{} (copie)", c.name) };
                    doc.items_mut().insert(i + 1, c);
                    self.cover_sel = Some(i + 1);
                    changed = true;
                }
                if ui.button("Supprimer").clicked() {
                    doc.items_mut().remove(i);
                    self.cover_sel = None;
                    changed = true;
                }
                if i > 0 && ui.button("▲").on_hover_text("Reculer (dessiné avant)").clicked() { doc.items_mut().swap(i, i - 1); self.cover_sel = Some(i - 1); changed = true; }
                if i + 1 < n && ui.button("▼").on_hover_text("Avancer (dessiné après)").clicked() { doc.items_mut().swap(i, i + 1); self.cover_sel = Some(i + 1); changed = true; }
            }
        });
        let Some(i) = self.cover_sel.filter(|&i| i < doc.items().len()) else { return changed };
        ui.separator();
        let zones = doc.zones();
        let it = &mut doc.items_mut()[i];
        ui.horizontal(|ui| {
            ui.label("Nom");
            changed |= ui.add(egui::TextEdit::singleline(&mut it.name).hint_text("(facultatif)").desired_width(220.0)).changed();
        });
        let mut pick_image = false;
        match &mut it.kind {
            CoverKind::Text { text, size_pt, color, align, font } => {
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
                    changed |= ui.add(egui::TextEdit::singleline(font).hint_text("(par défaut)").desired_width(220.0)).changed();
                });
            }
            CoverKind::Image { path } => {
                ui.horizontal(|ui| {
                    changed |= ui.add(egui::TextEdit::singleline(path).desired_width(220.0)).changed();
                    if ui.button("Choisir…").clicked() { pick_image = true; }
                });
            }
        }
        if pick_image {
            if let Some(f) = rfd::FileDialog::new().add_filter("Images", &IMG_EXT).pick_file() {
                let rel = self.relative(&f);
                if let CoverKind::Image { path } = &mut doc.items_mut()[i].kind { *path = rel; changed = true; }
            }
        }
        let it = &mut doc.items_mut()[i];
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
    fn cover_export(&mut self) {
        let (what, file) = if self.cover_page == 0 { ("Jaquette", "jaquette.pdf") } else { ("Étiquette", "etiquette.pdf") };
        let Some(path) = rfd::FileDialog::new().add_filter("PDF", &["pdf"]).set_file_name(file).save_file() else { return };
        if let Err(e) = &self.renderer { self.notice = format!("Export impossible : {e}"); return; }
        let (base, vars) = (self.base_dir(), self.cover_vars());
        let (tx, rx) = channel();
        if self.cover_page == 0 { spawn_export(self.model.cover.clone(), what, base, vars, path, tx); }
        else { spawn_export(self.model.label.clone(), what, base, vars, path, tx); }
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
        // apercu regenere quand le document ou les champs automatiques ont change
        let vars = self.cover_vars();
        if vars != self.cover_vars_seen {
            self.cover_vars_seen = vars.clone();
            self.cover_gen += 1;
        }
        if self.cover_gen != self.cover_seen || self.cover_tex.is_none() {
            let base = self.base_dir();
            if let Ok(r) = &self.renderer {
                if let Ok(img) = r.render_sheet(&*doc, PREVIEW_DPI, &base, &vars) {
                    let ci = egui::ColorImage::from_rgba_unmultiplied([img.width as usize, img.height as usize], &img.data);
                    match &mut self.cover_tex {
                        Some(t) => t.set(ci, egui::TextureOptions::LINEAR),
                        None => self.cover_tex = Some(ctx.load_texture("jaquette", ci, egui::TextureOptions::LINEAR)),
                    }
                }
            }
            self.cover_seen = self.cover_gen;
        }
        let (sw, sh) = doc.sheet_mm();
        let avail = ui.available_size();
        let k = ((avail.x / sw as f32).min((avail.y - 24.0) / sh as f32)).max(0.5);
        let (resp, painter) = ui.allocate_painter(vec2(sw as f32 * k, sh as f32 * k), Sense::click_and_drag());
        match &self.cover_tex {
            Some(t) => painter.image(t.id(), resp.rect, Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), Color32::WHITE),
            None => painter.rect_filled(resp.rect, 0.0, Color32::from_gray(30)),
        };
        let bleed = doc.bleed_mm() as f32;
        let origin = resp.rect.min;
        let to_screen = |x: f64, y: f64| pos2(origin.x + (x as f32 + bleed) * k, origin.y + (y as f32 + bleed) * k);
        let to_mm = |p: egui::Pos2| (((p.x - origin.x) / k - bleed) as f64, ((p.y - origin.y) / k - bleed) as f64);
        // reperes : feuille rognee, pliages, cercles
        let guide = Stroke::new(1.0_f32, Color32::from_white_alpha(120));
        let (rects, lines, circles) = doc.guides();
        for (x, y, w, h) in rects { painter.rect_stroke(Rect::from_min_max(to_screen(x, y), to_screen(x + w, y + h)), 0.0, guide); }
        for (x1, y1, x2, y2) in lines { painter.line_segment([to_screen(x1, y1), to_screen(x2, y2)], guide); }
        for (cx, cy, r) in circles { painter.circle_stroke(to_screen(cx, cy), r as f32 * k, guide); }
        let sel = self.cover_sel.filter(|&i| i < doc.items().len());
        // cadres des elements
        for (i, it) in doc.items().iter().enumerate() {
            let r = Rect::from_min_max(to_screen(it.x, it.y), to_screen(it.x + it.w, it.y + it.h));
            if sel == Some(i) {
                painter.rect_stroke(r, 0.0, Stroke::new(2.5_f32, Color32::WHITE));
                for c in [r.left_top(), r.right_top(), r.left_bottom(), r.right_bottom()] {
                    painter.rect_filled(Rect::from_center_size(c, vec2(9.0, 9.0)), 1.0, Color32::WHITE);
                    painter.rect_stroke(Rect::from_center_size(c, vec2(9.0, 9.0)), 1.0, Stroke::new(1.0_f32, Color32::BLACK));
                }
            } else {
                painter.rect_stroke(r, 0.0, Stroke::new(1.0_f32, Color32::from_rgba_unmultiplied(120, 190, 255, 110)));
            }
        }
        // poignee de coin sous le pointeur de l'element selectionne (1..4), 0 sinon
        let handle_at = |p: egui::Pos2, it: &CoverItem| -> u8 {
            let r = Rect::from_min_max(to_screen(it.x, it.y), to_screen(it.x + it.w, it.y + it.h));
            for (h, c) in [(1u8, r.left_top()), (2, r.right_top()), (3, r.left_bottom()), (4, r.right_bottom())] {
                if c.distance(p) <= HANDLE_PX { return h; }
            }
            0
        };
        let hit = |p: egui::Pos2, items: &[CoverItem]| -> Option<usize> {
            let (x, y) = to_mm(p);
            items.iter().rposition(|it| x >= it.x && x <= it.x + it.w && y >= it.y && y <= it.y + it.h)
        };
        // curseur
        let icon_of = |h: u8| if h == 1 || h == 4 { CursorIcon::ResizeNwSe } else { CursorIcon::ResizeNeSw };
        if let Some(d) = &self.cover_drag {
            ctx.set_cursor_icon(if d.handle == 0 { CursorIcon::Grabbing } else { icon_of(d.handle) });
        } else if let Some(p) = resp.hover_pos() {
            let h = sel.map(|i| handle_at(p, &doc.items()[i])).unwrap_or(0);
            if h != 0 { ctx.set_cursor_icon(icon_of(h)); }
            else if hit(p, doc.items()).is_some() { ctx.set_cursor_icon(CursorIcon::Grab); }
        }
        // debut : poignee de l'element selectionne, sinon l'element sous la souris (le plus au premier plan)
        if resp.drag_started() {
            if let Some(p) = ui.input(|i| i.pointer.press_origin()).or(resp.interact_pointer_pos()) {
                let mut target = None;
                if let Some(i) = sel {
                    let h = handle_at(p, &doc.items()[i]);
                    if h != 0 { target = Some((i, h)); }
                }
                if target.is_none() {
                    if let Some(i) = hit(p, doc.items()) { self.cover_sel = Some(i); target = Some((i, 0)); }
                }
                if let Some((i, h)) = target {
                    let it = &doc.items()[i];
                    self.cover_drag = Some(CoverDrag { item: i, handle: h, orig: (it.x, it.y, it.w, it.h), start: to_mm(p) });
                }
            }
        } else if resp.clicked() {
            if let Some(p) = resp.interact_pointer_pos() { self.cover_sel = hit(p, doc.items()); }
        }
        // en cours : la position est toujours calculee depuis le point de depart, sans derive
        if resp.dragged() {
            if let (Some(d), Some(p)) = (self.cover_drag.clone(), resp.interact_pointer_pos()) {
                if d.item < doc.items().len() {
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
                    let r = |v: f64| (v * 10.0).round() / 10.0;
                    let it = &mut doc.items_mut()[d.item];
                    if (it.x, it.y, it.w, it.h) != (r(nx), r(ny), r(nw), r(nh)) {
                        (it.x, it.y, it.w, it.h) = (r(nx), r(ny), r(nw), r(nh));
                        self.cover_changed();
                    }
                }
            }
        }
        if resp.drag_stopped() { self.cover_drag = None; }
        if !ctx.wants_keyboard_input() && ctx.input(|i| i.key_pressed(Key::Delete)) {
            if let Some(i) = sel {
                doc.items_mut().remove(i);
                self.cover_sel = None;
                self.cover_changed();
            }
        }
        ui.label(egui::RichText::new("Glissez un élément pour le déplacer, ses coins pour le redimensionner · Suppr : le retirer · Ctrl+Z / Ctrl+Y : annuler / refaire · les traits blancs montrent les limites de coupe et de pliage").weak());
    }
}
