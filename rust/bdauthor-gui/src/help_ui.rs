//! Fenetre d'aide des raccourcis clavier (menu « A propos »).
use crate::i18n::tr;
use eframe::egui;

/// Groupes de raccourcis : (titre, [(touches, action)]). Les textes sont les cles de traduction.
fn groups() -> Vec<(&'static str, Vec<(&'static str, &'static str)>)> {
    vec![
        ("Everywhere", vec![
            ("Ctrl+S", "Save the project"),
            ("Ctrl+Shift+F12", "Capture the application window as a PNG image"),
        ]),
        ("Layout", vec![
            ("Ctrl+Z", "Undo"),
            ("Ctrl+Y, Ctrl+Shift+Z", "Redo"),
            ("Ctrl+C, Ctrl+X, Ctrl+V", "Copy, cut, paste the selected items"),
            ("Arrows", "Move the selected items by 1 pixel (Shift: 10 pixels)"),
            ("Delete", "Delete the selected items"),
        ]),
        ("Cover and disc label", vec![
            ("Ctrl+Z", "Undo"),
            ("Ctrl+Y, Ctrl+Shift+Z", "Redo"),
            ("Ctrl+C, Ctrl+X, Ctrl+V", "Copy, cut, paste the selected items"),
            ("Arrows", "Move the selected items by 0.5 mm (Shift: 5 mm, Ctrl: 0.1 mm)"),
            ("Delete", "Delete the selected items"),
        ]),
        ("Typing a text on the sheet", vec![
            ("Arrows, Home, End", "Move the cursor (with Shift: extend the selection)"),
            ("Ctrl+A", "Select all the text"),
            ("Enter", "New line"),
            ("Ctrl+Z, Ctrl+Y", "Undo, redo"),
            ("Esc", "Stop typing"),
        ]),
        ("Chapters and frame choice", vec![
            ("Left, Right", "One frame back, forward (Shift: 1 second, Ctrl: 10 seconds)"),
            ("Home, End", "First frame, last frame (chapter editor)"),
            ("Space", "Play, pause"),
            ("C", "Add a chapter at the current frame (chapter editor)"),
        ]),
        ("Simulation of the menus", vec![
            ("Arrows", "Move between the buttons"),
            ("Enter, Space", "Activate the selected button"),
            ("Esc", "Back to the previous page, or leave the simulation"),
            ("Home, M", "Top menu"),
        ]),
        ("Video preview (subtitles)", vec![
            ("Space", "Play, pause"),
        ]),
    ]
}

/// Affiche la fenetre ; `open` passe a faux a la fermeture.
pub fn shortcuts_window(ctx: &egui::Context, open: &mut bool) {
    let mut win_open = true;
    egui::Window::new(tr("Keyboard shortcuts")).open(&mut win_open).collapsible(false).resizable(true)
        .default_width(560.0).default_height(520.0).anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO).show(ctx, |ui| {
            egui::ScrollArea::vertical().auto_shrink([false, true]).max_height(ui.available_height() - 36.0).show(ui, |ui| {
                for (gi, (title, rows)) in groups().into_iter().enumerate() {
                    ui.add_space(if gi == 0 { 0.0 } else { 8.0 });
                    ui.heading(tr(title));
                    egui::Grid::new(("shortcuts", gi)).num_columns(2).spacing([16.0, 4.0]).show(ui, |ui| {
                        for (keys, what) in rows {
                            ui.label(egui::RichText::new(tr(keys)).monospace().strong());
                            ui.label(tr(what));
                            ui.end_row();
                        }
                    });
                }
            });
            ui.add_space(6.0);
            if ui.button(tr("Close")).clicked() { *open = false; }
        });
    if !win_open { *open = false; }
}
