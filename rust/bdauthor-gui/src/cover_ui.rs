//! Onglet « Jaquette » : feuille a deplier du boitier (dos, tranche, face), edition et export PDF.
use crate::App;
use bdauthor_core::cover::{CoverItem, CoverKind, Zone, FORMATS};
use bdauthor_core::design::Align;
use eframe::egui::{self, pos2, vec2, Color32, Key, Rect, Sense, Stroke};

/// Resolution (dpi) de l'apercu a l'ecran.
const PREVIEW_DPI: u32 = 96;

fn item_label(it: &CoverItem, i: usize) -> String {
    if !it.name.is_empty() { return format!("{}. {}", i + 1, it.name); }
    match &it.kind {
        CoverKind::Text { text, .. } => format!("{}. Texte « {} »", i + 1, text.lines().next().unwrap_or("").chars().take(24).collect::<String>()),
        CoverKind::Image { path } => format!("{}. Image {}", i + 1, std::path::Path::new(path).file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()),
    }
}

impl App {
    pub(crate) fn cover_screen(&mut self, ctx: &egui::Context, busy: bool) {
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

    fn cover_panel(&mut self, ui: &mut egui::Ui) {
        let mut changed = false;
        ui.heading("Jaquette");
        ui.label(egui::RichText::new("Feuille à déplier du boîtier : dos à gauche, tranche au milieu, face à droite. Toutes les mesures sont en millimètres, depuis le coin haut gauche de la jaquette rognée (le fond perdu a donc des valeurs négatives).").weak());
        egui::CollapsingHeader::new("Format et impression").default_open(true).show(ui, |ui| {
            let c = &mut self.model.cover;
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
            let c = &mut self.model.cover;
            ui.horizontal(|ui| {
                for (i, name) in ["Dos", "Tranche", "Face"].into_iter().enumerate() {
                    ui.label(name);
                    changed |= ui.color_edit_button_srgb(&mut c.colors[i]).changed();
                }
            });
            changed |= crate::file_row(ui, "Image de fond :", &mut c.bg_image, "(aucune : couleurs ci-dessus)", "Images", &["png", "jpg", "jpeg", "bmp", "svg", "svgz"]);
            ui.label(egui::RichText::new("L'image de fond couvre toute la feuille, fond perdu compris.").weak());
        });
        ui.separator();
        ui.heading("Éléments");
        let n = self.model.cover.items.len();
        egui::ScrollArea::vertical().id_salt("jaquette_liste").max_height(160.0).show(ui, |ui| {
            for i in 0..n {
                let label = item_label(&self.model.cover.items[i], i);
                if ui.selectable_label(self.cover_sel == Some(i), label).clicked() { self.cover_sel = Some(i); }
            }
            if n == 0 { ui.label(egui::RichText::new("(aucun élément)").weak()); }
        });
        let (face, spine, height) = (self.model.cover.face_mm, self.model.cover.spine_mm, self.model.cover.height_mm);
        ui.horizontal_wrapped(|ui| {
            if ui.button("+ Texte").clicked() {
                self.model.cover.items.push(CoverItem::text("Titre", face + spine + 10.0, 10.0, face - 20.0, 30.0, 28.0));
                self.cover_sel = Some(self.model.cover.items.len() - 1);
                changed = true;
            }
            if ui.button("+ Image…").clicked() {
                if let Some(f) = rfd::FileDialog::new().add_filter("Images", &["png", "jpg", "jpeg", "bmp", "svg", "svgz"]).pick_file() {
                    let rel = self.relative(&f);
                    self.model.cover.items.push(CoverItem::image(&rel, face + spine + 10.0, 50.0, face - 20.0, (height - 60.0).max(20.0)));
                    self.cover_sel = Some(self.model.cover.items.len() - 1);
                    changed = true;
                }
            }
            if let Some(i) = self.cover_sel.filter(|&i| i < n) {
                if ui.button("Dupliquer").clicked() {
                    let mut c = self.model.cover.items[i].clone();
                    c.x += 5.0;
                    c.y += 5.0;
                    c.name = if c.name.is_empty() { String::new() } else { format!("{} (copie)", c.name) };
                    self.model.cover.items.insert(i + 1, c);
                    self.cover_sel = Some(i + 1);
                    changed = true;
                }
                if ui.button("Supprimer").clicked() {
                    self.model.cover.items.remove(i);
                    self.cover_sel = None;
                    changed = true;
                }
                if i > 0 && ui.button("▲").on_hover_text("Reculer (dessiné avant)").clicked() { self.model.cover.items.swap(i, i - 1); self.cover_sel = Some(i - 1); changed = true; }
                if i + 1 < n && ui.button("▼").on_hover_text("Avancer (dessiné après)").clicked() { self.model.cover.items.swap(i, i + 1); self.cover_sel = Some(i + 1); changed = true; }
            }
        });
        if let Some(i) = self.cover_sel.filter(|&i| i < self.model.cover.items.len()) {
            ui.separator();
            let zones = [Zone::Back, Zone::Spine, Zone::Front].map(|z| self.model.cover.zone_mm(z));
            let bleed = self.model.cover.bleed_mm;
            let (sw, sh) = self.model.cover.sheet_mm();
            let it = &mut self.model.cover.items[i];
            ui.horizontal(|ui| {
                ui.label("Nom");
                changed |= ui.add(egui::TextEdit::singleline(&mut it.name).hint_text("(facultatif)").desired_width(220.0)).changed();
            });
            let mut pick_image = false;
            match &mut it.kind {
                CoverKind::Text { text, size_pt, color, align, font } => {
                    changed |= ui.add(egui::TextEdit::multiline(text).desired_rows(3).desired_width(f32::INFINITY)).changed();
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
                        ui.add(egui::TextEdit::singleline(path).desired_width(220.0));
                        if ui.button("Choisir…").clicked() { pick_image = true; }
                    });
                }
            }
            if pick_image {
                if let Some(f) = rfd::FileDialog::new().add_filter("Images", &["png", "jpg", "jpeg", "bmp", "svg", "svgz"]).pick_file() {
                    let rel = self.relative(&f);
                    if let CoverKind::Image { path } = &mut self.model.cover.items[i].kind { *path = rel; changed = true; }
                }
            }
            let it = &mut self.model.cover.items[i];
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
                for (k, name) in ["le dos", "la tranche", "la face"].into_iter().enumerate() {
                    if ui.button(name).clicked() {
                        let z = zones[k];
                        let m = if k == 1 { 1.0 } else { 8.0 };
                        (it.x, it.y, it.w, it.h) = (z.0 + m, z.1 + m, (z.2 - 2.0 * m).max(1.0), (z.3 - 2.0 * m).max(1.0));
                        if k == 1 { it.rot = 90; }
                        changed = true;
                    }
                }
            });
            ui.horizontal(|ui| {
                if ui.button("Centrer horizontalement").clicked() { it.x = (sw - 2.0 * bleed - it.w) / 2.0; changed = true; }
                if ui.button("Centrer verticalement").clicked() { it.y = (sh - 2.0 * bleed - it.h) / 2.0; changed = true; }
            });
        }
        ui.separator();
        if ui.button("Exporter en PDF…").on_hover_text("Image de la jaquette à la résolution choisie, dans un PDF aux dimensions exactes").clicked() { self.cover_export(); }
        if changed { self.cover_changed(); }
    }

    /// Rend la jaquette a la resolution choisie et l'enregistre en PDF.
    fn cover_export(&mut self) {
        let Some(path) = rfd::FileDialog::new().add_filter("PDF", &["pdf"]).set_file_name("jaquette.pdf").save_file() else { return };
        let renderer = match &self.renderer { Ok(r) => r, Err(e) => { self.notice = format!("Export impossible : {e}"); return; } };
        let base = self.base_dir();
        let cover = &self.model.cover;
        let result = renderer.render_cover(cover, cover.dpi, &base).and_then(|img| {
            let title = path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_else(|| "Jaquette".into());
            std::fs::write(&path, cover.to_pdf(&img, &title)).map_err(|e| e.to_string())
        });
        self.notice = match result {
            Ok(()) => format!("Jaquette exportée : {}", path.display()),
            Err(e) => format!("Export impossible : {e}"),
        };
    }

    fn cover_central(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        // apercu regenere quand la jaquette a change
        if self.cover_gen != self.cover_seen || self.cover_tex.is_none() {
            if let Ok(r) = &self.renderer {
                let base = self.base_dir();
                if let Ok(img) = r.render_cover(&self.model.cover, PREVIEW_DPI, &base) {
                    let ci = egui::ColorImage::from_rgba_unmultiplied([img.width as usize, img.height as usize], &img.data);
                    match &mut self.cover_tex {
                        Some(t) => t.set(ci, egui::TextureOptions::LINEAR),
                        None => self.cover_tex = Some(ctx.load_texture("jaquette", ci, egui::TextureOptions::LINEAR)),
                    }
                }
            }
            self.cover_seen = self.cover_gen;
        }
        let (sw, sh) = self.model.cover.sheet_mm();
        let avail = ui.available_size();
        let k = ((avail.x / sw as f32).min((avail.y - 24.0) / sh as f32)).max(0.5);
        let (resp, painter) = ui.allocate_painter(vec2(sw as f32 * k, sh as f32 * k), Sense::click_and_drag());
        match &self.cover_tex {
            Some(t) => painter.image(t.id(), resp.rect, Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), Color32::WHITE),
            None => painter.rect_filled(resp.rect, 0.0, Color32::from_gray(30)),
        };
        let bleed = self.model.cover.bleed_mm as f32;
        let to_screen = |x: f64, y: f64| pos2(resp.rect.min.x + (x as f32 + bleed) * k, resp.rect.min.y + (y as f32 + bleed) * k);
        let to_mm = |p: egui::Pos2| (((p.x - resp.rect.min.x) / k - bleed) as f64, ((p.y - resp.rect.min.y) / k - bleed) as f64);
        // reperes : jaquette rognee et pliages de la tranche
        let (tw, th) = self.model.cover.trim_mm();
        let guide = Stroke::new(1.0_f32, Color32::from_white_alpha(120));
        painter.rect_stroke(Rect::from_min_max(to_screen(0.0, 0.0), to_screen(tw, th)), 0.0, guide);
        for fx in [self.model.cover.face_mm, self.model.cover.face_mm + self.model.cover.spine_mm] {
            painter.line_segment([to_screen(fx, 0.0), to_screen(fx, th)], guide);
        }
        // cadres des elements
        for (i, it) in self.model.cover.items.iter().enumerate() {
            let r = Rect::from_min_max(to_screen(it.x, it.y), to_screen(it.x + it.w, it.y + it.h));
            if self.cover_sel == Some(i) {
                painter.rect_stroke(r, 0.0, Stroke::new(2.5_f32, Color32::WHITE));
                for c in [r.left_top(), r.right_top(), r.left_bottom(), r.right_bottom()] { painter.rect_filled(Rect::from_center_size(c, vec2(7.0, 7.0)), 1.0, Color32::WHITE); }
            } else {
                painter.rect_stroke(r, 0.0, Stroke::new(1.0_f32, Color32::from_rgba_unmultiplied(120, 190, 255, 110)));
            }
        }
        // selection au clic, deplacement au glisser (l'element sous la souris, le plus au premier plan)
        let hit = |p: egui::Pos2, items: &[CoverItem]| -> Option<usize> {
            let (x, y) = to_mm(p);
            items.iter().rposition(|it| x >= it.x && x <= it.x + it.w && y >= it.y && y <= it.y + it.h)
        };
        if resp.drag_started() || resp.clicked() {
            if let Some(p) = resp.interact_pointer_pos() {
                match hit(p, &self.model.cover.items) { Some(i) => self.cover_sel = Some(i), None => if resp.clicked() { self.cover_sel = None } }
            }
        }
        if resp.dragged() {
            if let Some(i) = self.cover_sel.filter(|&i| i < self.model.cover.items.len()) {
                let d = resp.drag_delta() / k;
                if d != egui::Vec2::ZERO {
                    let it = &mut self.model.cover.items[i];
                    it.x += d.x as f64;
                    it.y += d.y as f64;
                    self.cover_changed();
                }
            }
        }
        if !ctx.wants_keyboard_input() && ctx.input(|i| i.key_pressed(Key::Delete)) {
            if let Some(i) = self.cover_sel.filter(|&i| i < self.model.cover.items.len()) {
                self.model.cover.items.remove(i);
                self.cover_sel = None;
                self.cover_changed();
            }
        }
        ui.label(egui::RichText::new("Glissez un élément pour le déplacer · Suppr : le retirer · les traits blancs montrent la jaquette rognée et les pliages de la tranche").weak());
    }
}
