//! Reglages de l'apparence des boutons (style du projet ou propre a un element) et images d'etats.
use crate::i18n::{tr, trf};
use bdauthor_core::style::{ButtonStyle, Skin, Weight, DECODED_LIMIT};
use eframe::egui::{self, Color32, Rect, Stroke};

/// Image d'animation en cours de glisser-deposer : (etat : 0 normal, 1 selectionne ; rang dans la liste).
#[derive(Clone, Copy)]
struct FrameDrag(usize, usize);

/// Formats proposes pour les sons des menus.
pub const SOUND_EXTS: [&str; 9] = ["wav", "mp3", "flac", "ogg", "m4a", "aac", "opus", "aif", "aiff"];

/// Retourne vrai si le style a change. `pick_sound` recoit 1 (son de selection) ou 2 (son d'appui sur OK) quand l'utilisateur veut
/// choisir le fichier.
pub fn style_ui(ui: &mut egui::Ui, st: &mut ButtonStyle, id: &str, pick_sound: &mut Option<u8>) -> bool {
    let before = st.clone();
    let px = |ui: &mut egui::Ui, v: &mut i32, max: i32| { ui.add(egui::DragValue::new(v).range(0..=max).suffix(" px")); };
    let pct = |ui: &mut egui::Ui, v: &mut i32, max: i32| { ui.add(egui::DragValue::new(v).range(0..=max).suffix(" %")); };
    ui.strong(tr("Unselected button"));
    egui::Grid::new(format!("{id}_normal")).num_columns(2).show(ui, |ui| {
        ui.label(tr("Background")); ui.color_edit_button_srgb(&mut st.normal_bg); ui.end_row();
        ui.label(tr("Text")); ui.color_edit_button_srgb(&mut st.normal_text); ui.end_row();
        ui.label(tr("Frame")); ui.horizontal(|ui| { ui.color_edit_button_srgb(&mut st.normal_border); px(ui, &mut st.normal_border_w, 40); }); ui.end_row();
        ui.label(tr("Darken")); pct(ui, &mut st.dim, 80); ui.end_row();
    });
    ui.strong(tr("Selected button"));
    egui::Grid::new(format!("{id}_sel")).num_columns(2).show(ui, |ui| {
        ui.label(tr("Background")); ui.color_edit_button_srgb(&mut st.sel_bg); ui.end_row();
        ui.label(tr("Text")); ui.color_edit_button_srgb(&mut st.sel_text); ui.end_row();
        ui.label(tr("Frame")); ui.horizontal(|ui| { ui.color_edit_button_srgb(&mut st.sel_border); px(ui, &mut st.sel_border_w, 40); }); ui.end_row();
        ui.label(tr("Glow")); ui.horizontal(|ui| { ui.color_edit_button_srgb(&mut st.glow_color); px(ui, &mut st.glow, 60); }); ui.end_row();
        ui.label("");
        ui.add_enabled(st.glow > 0, egui::Checkbox::new(&mut st.glow_pulse, tr("Animated glow (pulse)")))
            .on_hover_text(trf!("The glow grows and shrinks in a loop while the button is selected ({} frames, about 8 per second). Increases the page weight. Visible in the simulation.", bdauthor_core::style::PULSE_FRAMES));
        ui.end_row();
        ui.label(tr("Enlarge")); pct(ui, &mut st.zoom, 20); ui.end_row();
    });
    ui.strong(tr("Common"));
    egui::Grid::new(format!("{id}_commun")).num_columns(2).show(ui, |ui| {
        ui.label(tr("Corner rounding")); px(ui, &mut st.radius, 200); ui.end_row();
    });
    ui.checkbox(&mut st.activated, tr("Distinct “activated” state (when OK is pressed)"));
    if st.activated {
        egui::Grid::new(format!("{id}_act")).num_columns(2).show(ui, |ui| {
            ui.label(tr("Background")); ui.color_edit_button_srgb(&mut st.act_bg); ui.end_row();
            ui.label(tr("Text")); ui.color_edit_button_srgb(&mut st.act_text); ui.end_row();
            ui.label(tr("Frame")); ui.color_edit_button_srgb(&mut st.act_border); ui.end_row();
        });
    }
    ui.strong(tr("Clickable texts and images (selected)"));
    egui::Grid::new(format!("{id}_deco")).num_columns(2).show(ui, |ui| {
        ui.label(tr("Colored background (opacity)")); pct(ui, &mut st.backdrop, 100); ui.end_row();
    });
    ui.checkbox(&mut st.underline, tr("Underline instead of framing"));
    crate::help(ui, tr("Sizes given for a 1920 × 1080 menu. The glow and the enlargement widen the button image: leave some space between buttons."));
    // sons : seulement dans le style du projet (un element a sa propre case « Sons propres a cet element », hors apparence)
    if id == "projet" {
        ui.strong(tr("Sounds"));
        sounds_ui(ui, &mut st.sound_select, &mut st.sound_activate, pick_sound);
    }
    if ui.button(tr("Default values")).on_hover_text(tr("Colors, border, glow… (sounds are kept)")).clicked() {
        let sounds = (std::mem::take(&mut st.sound_select), std::mem::take(&mut st.sound_activate));
        *st = ButtonStyle::default();
        (st.sound_select, st.sound_activate) = sounds;
    }
    *st != before
}

/// Lignes « Selection » et « Appui sur OK » (fichiers audio). `pick` recoit 1 ou 2 pour choisir le fichier. Retourne vrai si un
/// chemin a change.
pub fn sounds_ui(ui: &mut egui::Ui, select: &mut String, activate: &mut String, pick: &mut Option<u8>) -> bool {
    let mut changed = false;
    for (k, label, path) in [(1u8, tr("Selection"), select), (2, tr("OK press"), activate)] {
        ui.horizontal_wrapped(|ui| {
            ui.label(label);
            changed |= ui.add(egui::TextEdit::singleline(path).desired_width(140.0).hint_text("aucun")).changed();
            if ui.small_button(tr("Choose…")).clicked() { *pick = Some(k); }
            if !path.is_empty() && ui.small_button("✕").clicked() { path.clear(); changed = true; }
        });
    }
    crate::help(ui, tr("Short sounds (under one second), played by the player when the selection reaches the button or when OK is pressed. Converted to 48 kHz stereo on the disc (10 s at most)."));
    changed
}

/// Images d'etats d'un bouton. `pick` recoit l'etat (0 normal, 1 selectionne, 2 active) dont l'utilisateur veut choisir le ou les
/// fichiers (plusieurs pour « selectionne » : animation). `preview` : jouer l'animation dans l'apercu de la page.
pub fn skin_ui(ui: &mut egui::Ui, skin: &mut Skin, pick: &mut Option<usize>, preview: &mut bool, fps: u32) -> bool {
    let mut changed = false;
    ui.strong(tr("Drawn images (optional)"));
    let hint = tr("One image: fixed state. Several images (multiple selection): animation, in file-name order.");
    // « normal » et « selectionne » : une image (etat fixe) ou plusieurs (animation)
    for state in [0usize, 1] {
        let label = if state == 0 { tr("Normal") } else { tr("Selected") };
        let (fixed, frames) = if state == 0 { (&mut skin.normal, &skin.normal_frames) } else { (&mut skin.selected, &skin.sel_frames) };
        let animated = !frames.is_empty();
        let count = frames.len();
        let mut clear = false;
        ui.horizontal(|ui| {
            ui.label(label);
            if animated {
                ui.add_enabled(false, egui::TextEdit::singleline(&mut trf!("{} images (animation)", count)).desired_width(160.0));
            } else {
                changed |= ui.add(egui::TextEdit::singleline(fixed).desired_width(160.0).hint_text("aucune")).changed();
            }
            if ui.small_button(tr("Choose…")).on_hover_text(hint).clicked() { *pick = Some(state); }
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
        ui.label(tr("Activated"));
        changed |= ui.add(egui::TextEdit::singleline(&mut skin.activated).desired_width(160.0).hint_text("aucune")).changed();
        if ui.small_button(tr("Choose…")).clicked() { *pick = Some(2); }
        if !skin.activated.is_empty() && ui.small_button("✕").clicked() { skin.activated.clear(); changed = true; }
    });
    if skin.normal_frames.is_empty() && skin.sel_frames.is_empty() { *preview = false; }
    crate::help(ui, tr("PNG, JPEG or SVG, fitted to the frame. The normal image replaces the button drawing; without a “selected” image, it is highlighted with the style. Several “normal” or “selected” images form an animation."));
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
                        .response.on_hover_text(tr("Drag to change the order of the images"));
                    ui.label(format!("{:02}", k + 1));
                    let name = std::path::Path::new(f).file_name().map(|x| x.to_string_lossy().into_owned()).unwrap_or_else(|| f.clone());
                    ui.label(name).on_hover_text(f.as_str());
                    if ui.small_button("✕").on_hover_text(tr("Remove")).clicked() { op = Some((k, None)); }
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
        // rangees repliables : elles ne doivent pas imposer une largeur minimale au panneau
        ui.horizontal_wrapped(|ui| {
            ui.label(tr("Playback:"));
            if ui.radio(!*once, tr("loop")).clicked() && *once { *once = false; changed = true; }
            if ui.radio(*once, tr("once")).on_hover_text(tr("Only once, then stops on the last image")).clicked() && !*once { *once = true; changed = true; }
        });
        ui.horizontal_wrapped(|ui| {
            let tip = if state == 0 { tr("Plays the animations of the normal state of the page elements (and that of the selected element).") } else { tr("Plays the animation on the page preview while the element is selected.") };
            ui.toggle_value(preview, tr("▶ Preview")).on_hover_text(trf!("{} In “once” playback, turn off then on again to replay it.", tip));
            let n = frames.len();
            ui.label(egui::RichText::new(trf!("{} s at {} fps", format!("{:.1}", n as f64 / fps.max(1) as f64), fps)).weak())
                .on_hover_text(tr("Frame rate adjustable in “Background and animations of this page” (with no element selected)"));
        });
        if state == 1 && no_normal {
            ui.colored_label(Color32::from_rgb(230, 100, 90), tr("Also choose a “normal” image: without it, the animation is ignored."));
        }
        if state == 0 {
            crate::help(ui, tr("The normal-state animation runs permanently and counts in the page weight."));
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
    let text = trf!("Button weight: {} MB out of {} MB ({} button(s), menu {}×{}) — displayed in ≈ {} s", format!("{:.1}", w.bytes as f64 / 1048576.0), format!("{:.0}", DECODED_LIMIT as f64 / 1048576.0), w.buttons, plane.0, plane.1, format!("{:.1}", w.seconds));
    ui.colored_label(col, text).on_hover_text(tr("Image memory the player must decode before displaying the page. Beyond the limit, the menu may not display on some players. Glow, enlargement and “activated” state increase this weight."));
}
