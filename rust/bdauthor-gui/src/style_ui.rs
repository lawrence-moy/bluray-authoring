//! Reglages de l'apparence des boutons (style du projet ou propre a un element) et images d'etats.
use bdauthor_core::style::{ButtonStyle, Skin, Weight, DECODED_LIMIT};
use eframe::egui::{self, Color32};

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

/// Images d'etats d'un bouton. `pick` recoit l'etat (0 normal, 1 selectionne, 2 active) dont l'utilisateur veut choisir le fichier.
pub fn skin_ui(ui: &mut egui::Ui, skin: &mut Skin, pick: &mut Option<usize>) -> bool {
    let mut changed = false;
    ui.strong("Images dessinées (facultatif)");
    for (k, (label, path)) in [("Normal", &mut skin.normal), ("Sélectionné", &mut skin.selected), ("Activé", &mut skin.activated)].into_iter().enumerate() {
        ui.horizontal(|ui| {
            ui.label(label);
            changed |= ui.add(egui::TextEdit::singleline(path).desired_width(160.0).hint_text("aucune")).changed();
            if ui.small_button("Choisir…").clicked() { *pick = Some(k); }
            if !path.is_empty() && ui.small_button("✕").clicked() { path.clear(); changed = true; }
        });
    }
    ui.label(egui::RichText::new("PNG, JPEG ou SVG, ajustés au cadre. L'image normale remplace le dessin du bouton ; sans image « sélectionné », elle est mise en valeur avec le style.").weak());
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
