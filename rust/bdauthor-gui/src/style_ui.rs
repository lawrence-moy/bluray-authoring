//! Reglages de l'apparence des boutons (style du projet ou propre a un element) et images d'etats.
use bdauthor_core::style::{ButtonStyle, Skin, Weight, DECODED_LIMIT};
use eframe::egui::{self, Color32, Rect, Stroke};

/// Image d'animation en cours de glisser-deposer : (etat : 0 normal, 1 selectionne ; rang dans la liste).
#[derive(Clone, Copy)]
struct FrameDrag(usize, usize);

/// Retourne vrai si le style a change.
pub fn style_ui(ui: &mut egui::Ui, st: &mut ButtonStyle, id: &str) -> bool {
    let before = st.clone();
    let px = |ui: &mut egui::Ui, v: &mut i32, max: i32| { ui.add(egui::DragValue::new(v).range(0..=max).suffix(" px")); };
    let pct = |ui: &mut egui::Ui, v: &mut i32, max: i32| { ui.add(egui::DragValue::new(v).range(0..=max).suffix(" %")); };
    ui.strong("Bouton non sélectionné");
    egui::Grid::new(format!("{id}_normal")).num_columns(2).show(ui, |ui| {
        ui.label("Fond"); ui.color_edit_button_srgb(&mut st.normal_bg); ui.end_row();
        ui.label("Texte"); ui.color_edit_button_srgb(&mut st.normal_text); ui.end_row();
        ui.label("Cadre"); ui.horizontal(|ui| { ui.color_edit_button_srgb(&mut st.normal_border); px(ui, &mut st.normal_border_w, 40); }); ui.end_row();
        ui.label("Assombrir"); pct(ui, &mut st.dim, 80); ui.end_row();
    });
    ui.strong("Bouton sélectionné");
    egui::Grid::new(format!("{id}_sel")).num_columns(2).show(ui, |ui| {
        ui.label("Fond"); ui.color_edit_button_srgb(&mut st.sel_bg); ui.end_row();
        ui.label("Texte"); ui.color_edit_button_srgb(&mut st.sel_text); ui.end_row();
        ui.label("Cadre"); ui.horizontal(|ui| { ui.color_edit_button_srgb(&mut st.sel_border); px(ui, &mut st.sel_border_w, 40); }); ui.end_row();
        ui.label("Halo"); ui.horizontal(|ui| { ui.color_edit_button_srgb(&mut st.glow_color); px(ui, &mut st.glow, 60); }); ui.end_row();
        ui.label("");
        ui.add_enabled(st.glow > 0, egui::Checkbox::new(&mut st.glow_pulse, "Halo animé (pulsation)"))
            .on_hover_text(format!("Le halo grandit et diminue en boucle tant que le bouton est sélectionné ({} images, environ 8 par seconde). Augmente le poids de la page. Visible dans la simulation.", bdauthor_core::style::PULSE_FRAMES));
        ui.end_row();
        ui.label("Agrandir"); pct(ui, &mut st.zoom, 20); ui.end_row();
    });
    ui.strong("Commun");
    egui::Grid::new(format!("{id}_commun")).num_columns(2).show(ui, |ui| {
        ui.label("Arrondi des coins"); px(ui, &mut st.radius, 200); ui.end_row();
    });
    ui.checkbox(&mut st.activated, "État « activé » distinct (au moment de l'appui sur OK)");
    if st.activated {
        egui::Grid::new(format!("{id}_act")).num_columns(2).show(ui, |ui| {
            ui.label("Fond"); ui.color_edit_button_srgb(&mut st.act_bg); ui.end_row();
            ui.label("Texte"); ui.color_edit_button_srgb(&mut st.act_text); ui.end_row();
            ui.label("Cadre"); ui.color_edit_button_srgb(&mut st.act_border); ui.end_row();
        });
    }
    ui.strong("Textes et images cliquables (sélectionnés)");
    egui::Grid::new(format!("{id}_deco")).num_columns(2).show(ui, |ui| {
        ui.label("Fond coloré (opacité)"); pct(ui, &mut st.backdrop, 100); ui.end_row();
    });
    ui.checkbox(&mut st.underline, "Souligner au lieu d'encadrer");
    ui.label(egui::RichText::new("Tailles données pour un menu 1920 × 1080. Le halo et l'agrandissement élargissent l'image des boutons : laissez de l'espace entre eux.").weak());
    if ui.button("Valeurs par défaut").clicked() {
        *st = ButtonStyle::default();
    }
    *st != before
}

/// Images d'etats d'un bouton. `pick` recoit l'etat (0 normal, 1 selectionne, 2 active) dont l'utilisateur veut choisir le ou les
/// fichiers (plusieurs pour « selectionne » : animation). `preview` : jouer l'animation dans l'apercu de la page.
pub fn skin_ui(ui: &mut egui::Ui, skin: &mut Skin, pick: &mut Option<usize>, preview: &mut bool, fps: u32) -> bool {
    let mut changed = false;
    ui.strong("Images dessinées (facultatif)");
    let hint = "Une image : état fixe. Plusieurs images (sélection multiple) : animation, dans l'ordre des noms.";
    // « normal » et « selectionne » : une image (etat fixe) ou plusieurs (animation)
    for state in [0usize, 1] {
        let label = if state == 0 { "Normal" } else { "Sélectionné" };
        let (fixed, frames) = if state == 0 { (&mut skin.normal, &skin.normal_frames) } else { (&mut skin.selected, &skin.sel_frames) };
        let animated = !frames.is_empty();
        let count = frames.len();
        let mut clear = false;
        ui.horizontal(|ui| {
            ui.label(label);
            if animated {
                ui.add_enabled(false, egui::TextEdit::singleline(&mut format!("{count} images (animation)")).desired_width(160.0));
            } else {
                changed |= ui.add(egui::TextEdit::singleline(fixed).desired_width(160.0).hint_text("aucune")).changed();
            }
            if ui.small_button("Choisir…").on_hover_text(hint).clicked() { *pick = Some(state); }
            if (animated || !fixed.is_empty()) && ui.small_button("✕").clicked() { clear = true; }
        });
        if clear {
            if state == 0 { skin.set_normal(vec![]); } else { skin.set_selected(vec![]); }
            changed = true;
        }
        if animated {
            changed |= anim_ui(ui, skin, state, preview, fps);
        }
    }
    ui.horizontal(|ui| {
        ui.label("Activé");
        changed |= ui.add(egui::TextEdit::singleline(&mut skin.activated).desired_width(160.0).hint_text("aucune")).changed();
        if ui.small_button("Choisir…").clicked() { *pick = Some(2); }
        if !skin.activated.is_empty() && ui.small_button("✕").clicked() { skin.activated.clear(); changed = true; }
    });
    if skin.normal_frames.is_empty() && skin.sel_frames.is_empty() { *preview = false; }
    ui.label(egui::RichText::new("PNG, JPEG ou SVG, ajustés au cadre. L'image normale remplace le dessin du bouton ; sans image « sélectionné », elle est mise en valeur avec le style. Plusieurs images « normal » ou « sélectionné » forment une animation.").weak());
    changed
}

/// Animation d'un etat (0 normal, 1 selectionne ; au moins deux images) : liste ordonnee par glisser-deposer, lecture en boucle ou
/// une fois, apercu.
fn anim_ui(ui: &mut egui::Ui, skin: &mut Skin, state: usize, preview: &mut bool, fps: u32) -> bool {
    let mut changed = false;
    let no_normal = !skin.has_normal();
    let (frames, once) = if state == 0 { (&mut skin.normal_frames, &mut skin.normal_once) } else { (&mut skin.sel_frames, &mut skin.sel_once) };
    // (rang, destination) : deplacement par glisser-deposer ; (rang, None) : retrait
    let mut op: Option<(usize, Option<usize>)> = None;
    ui.indent(("anim_etat", state), |ui| {
        let mut rows: Vec<Rect> = Vec::new();
        let list = egui::ScrollArea::vertical().id_salt(("anim_frames", state)).max_height(140.0).show(ui, |ui| {
            for (k, f) in frames.iter().enumerate() {
                let r = ui.horizontal(|ui| {
                    let _ = ui.dnd_drag_source(egui::Id::new(("image_animation", state, k)), FrameDrag(state, k), |ui| ui.label(egui::RichText::new("☰").weak()))
                        .response.on_hover_text("Glisser pour changer l'ordre des images");
                    ui.label(format!("{:02}", k + 1));
                    let name = std::path::Path::new(f).file_name().map(|x| x.to_string_lossy().into_owned()).unwrap_or_else(|| f.clone());
                    ui.label(name).on_hover_text(f.as_str());
                    if ui.small_button("✕").on_hover_text("Retirer").clicked() { op = Some((k, None)); }
                }).response;
                rows.push(r.rect);
            }
        });
        // glisser-deposer (meme mecanique que la liste des pages et calques) : une ligne bleue montre la place d'arrivee
        let area = list.inner_rect;
        if let (Some(drag), Some(pos)) = (egui::DragAndDrop::payload::<FrameDrag>(ui.ctx()), ui.ctx().pointer_hover_pos()) {
            if drag.0 == state && area.expand(8.0).contains(pos) && !rows.is_empty() {
                let r = rows.iter().filter(|rect| rect.center().y < pos.y).count();
                let y = if r < rows.len() { rows[r].top() } else { rows[rows.len() - 1].bottom() };
                ui.painter().hline(area.x_range(), y, Stroke::new(2.5_f32, Color32::from_rgb(90, 170, 255)));
                if ui.input(|i| i.pointer.any_released()) {
                    egui::DragAndDrop::clear_payload(ui.ctx());
                    let from = drag.1;
                    let to = if r > from { r - 1 } else { r };
                    if to != from { op = Some((from, Some(to))); }
                }
            }
        }
        ui.horizontal(|ui| {
            ui.label("Lecture :");
            if ui.radio(!*once, "en boucle").clicked() && *once { *once = false; changed = true; }
            if ui.radio(*once, "une fois, puis dernière image").clicked() && !*once { *once = true; changed = true; }
        });
        ui.horizontal(|ui| {
            let tip = if state == 0 { "Joue les animations de l'état normal des éléments de la page (et celle de l'élément sélectionné)." } else { "Joue l'animation sur l'aperçu de la page tant que l'élément est sélectionné." };
            ui.toggle_value(preview, "▶ Aperçu").on_hover_text(format!("{tip} En lecture « une fois », désactiver puis réactiver pour la rejouer."));
            let n = frames.len();
            ui.label(egui::RichText::new(format!("Durée : {:.1} s ({fps} images/s, réglable dans « Fond de cette page »)", n as f64 / fps.max(1) as f64)).weak());
        });
        if state == 1 && no_normal {
            ui.colored_label(Color32::from_rgb(230, 100, 90), "Choisissez aussi une image « normale » : sans elle, l'animation est ignorée.");
        }
        if state == 0 {
            ui.label(egui::RichText::new("L'animation de l'état normal tourne en permanence et compte dans le poids de la page.").weak());
        }
    });
    if let Some((k, d)) = op {
        // une animation garde au moins deux images ; en dessous, l'image restante devient l'image fixe
        let mut files = frames.clone();
        if k < files.len() {
            let f = files.remove(k);
            if let Some(to) = d { files.insert(to.min(files.len()), f); }
        }
        let keep = *once;
        if state == 0 { skin.set_normal(files); } else { skin.set_selected(files); }
        let (frames, once) = if state == 0 { (&skin.normal_frames, &mut skin.normal_once) } else { (&skin.sel_frames, &mut skin.sel_once) };
        if frames.len() >= 2 { *once = keep; }
        changed = true;
    }
    changed
}

/// Ligne « poids de la page » sous l'apercu.
pub fn weight_ui(ui: &mut egui::Ui, w: &Weight, plane: (u32, u32)) {
    let r = w.ratio();
    let col = if r >= 1.0 { Color32::from_rgb(230, 90, 80) } else if r >= 0.6 { Color32::from_rgb(235, 170, 60) } else { Color32::from_rgb(110, 200, 120) };
    let text = format!(
        "Poids des boutons : {:.1} Mo sur {:.0} Mo ({} bouton(s), menu {}×{}) — affichage en ≈ {:.1} s",
        w.bytes as f64 / 1048576.0, DECODED_LIMIT as f64 / 1048576.0, w.buttons, plane.0, plane.1, w.seconds
    );
    ui.colored_label(col, text).on_hover_text("Mémoire d'images que le lecteur doit décoder avant d'afficher la page. Au-delà de la limite, le menu risque de ne pas s'afficher sur certains lecteurs. Halo, agrandissement et état « activé » augmentent ce poids.");
}
