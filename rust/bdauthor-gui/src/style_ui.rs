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

/// Images d'etats d'un bouton. `pick` recoit l'etat (0 normal, 1 selectionne, 2 active) dont l'utilisateur veut choisir le fichier,
/// ou 3 pour choisir les images de l'animation de l'etat selectionne. `preview` : jouer l'animation dans l'apercu de la page.
pub fn skin_ui(ui: &mut egui::Ui, skin: &mut Skin, pick: &mut Option<usize>, preview: &mut bool, fps: u32) -> bool {
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
    changed |= anim_ui(ui, skin, pick, preview, fps);
    changed
}

/// Animation de l'etat selectionne : liste ordonnee d'images, lecture en boucle ou une fois, apercu.
fn anim_ui(ui: &mut egui::Ui, skin: &mut Skin, pick: &mut Option<usize>, preview: &mut bool, fps: u32) -> bool {
    let mut changed = false;
    ui.add_space(4.0);
    let mut on = !skin.sel_frames.is_empty();
    if ui.checkbox(&mut on, "Animer l'état sélectionné").changed() {
        if on { *pick = Some(3); } else { skin.sel_frames.clear(); skin.sel_once = false; *preview = false; changed = true; }
    }
    if skin.sel_frames.is_empty() { return changed; }
    ui.horizontal(|ui| {
        ui.label(format!("Images : {}", skin.sel_frames.len()));
        if ui.small_button("Choisir les images…").on_hover_text("Plusieurs PNG d'un coup ; rangés par ordre de nom").clicked() { *pick = Some(3); }
        if ui.small_button("Vider").clicked() { skin.sel_frames.clear(); skin.sel_once = false; *preview = false; changed = true; }
    });
    // liste ordonnee : deplacer ou retirer une image
    let mut op: Option<(usize, i32)> = None;
    egui::ScrollArea::vertical().id_salt("anim_frames").max_height(140.0).show(ui, |ui| {
        let n = skin.sel_frames.len();
        for (k, f) in skin.sel_frames.iter().enumerate() {
            ui.horizontal(|ui| {
                ui.label(format!("{:02}", k + 1));
                let name = std::path::Path::new(f).file_name().map(|x| x.to_string_lossy().into_owned()).unwrap_or_else(|| f.clone());
                ui.label(name).on_hover_text(f.as_str());
                if k > 0 && ui.small_button("◀").on_hover_text("Plus tôt").clicked() { op = Some((k, -1)); }
                if k + 1 < n && ui.small_button("▶").on_hover_text("Plus tard").clicked() { op = Some((k, 1)); }
                if ui.small_button("✕").on_hover_text("Retirer").clicked() { op = Some((k, 0)); }
            });
        }
    });
    if let Some((k, d)) = op {
        if d == 0 { skin.sel_frames.remove(k); } else { skin.sel_frames.swap(k, (k as i32 + d) as usize); }
        if skin.sel_frames.is_empty() { *preview = false; }
        changed = true;
    }
    ui.horizontal(|ui| {
        ui.label("Lecture :");
        if ui.radio(!skin.sel_once, "en boucle").clicked() && skin.sel_once { skin.sel_once = false; changed = true; }
        if ui.radio(skin.sel_once, "une fois, puis dernière image").clicked() && !skin.sel_once { skin.sel_once = true; changed = true; }
    });
    ui.horizontal(|ui| {
        ui.toggle_value(preview, "▶ Aperçu").on_hover_text("Joue l'animation sur l'aperçu de la page tant que l'élément est sélectionné. En lecture « une fois », désactiver puis réactiver pour la rejouer.");
        let n = skin.sel_frames.len();
        ui.label(egui::RichText::new(format!("Durée : {:.1} s ({fps} images/s, réglable dans « Fond de cette page »)", n as f64 / fps.max(1) as f64)).weak());
    });
    if skin.normal.is_empty() {
        ui.colored_label(Color32::from_rgb(230, 100, 90), "Choisissez aussi une image « normale » : sans elle, l'animation est ignorée.");
    }
    ui.label(egui::RichText::new("Chaque image remplace l'image « sélectionné » (halo et agrandissement du style appliqués) et compte dans le poids de la page.").weak());
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
