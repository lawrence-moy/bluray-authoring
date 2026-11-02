//! Onglet « Génération ISO » de la fenêtre Projet : ce que la construction produit (dossier, image, les deux) et réglages de l'image.
use crate::i18n::{tr, trf};
use bdauthor_core::iso::{fit_info, IsoDate, IsoKeep, IsoSettings, IsoStamp, UdfRev, CAPACITIES, DEFAULT_MARGIN, MAX_LABEL};
use eframe::egui;

/// Rend vrai si un réglage a changé ; `browse` : l'utilisateur veut choisir le fichier de l'image.
pub fn iso_ui(ui: &mut egui::Ui, s: &mut IsoSettings, title: &str, browse: &mut bool) -> bool {
    let mut changed = false;
    ui.heading(tr("Output"));
    ui.label(tr("The build produces:"));
    for (k, label, tip) in [
        (IsoKeep::Folder, tr("the BDMV folder only"), tr("As before: a BDMV folder + CERTIFICATE, to burn with other software or to play as is.")),
        (IsoKeep::Iso, tr("the ISO image only"), tr("The disc is built in a temporary folder, deleted once the image is written: the space is only used once.")),
        (IsoKeep::Both, tr("the folder and the ISO image"), tr("The image is written next to the chosen folder (same name, .iso extension), unless a file is given below.")),
    ] {
        if ui.radio(s.keep == k, label).on_hover_text(tip).clicked() && s.keep != k { s.keep = k; changed = true; }
    }
    ui.add_enabled_ui(s.enabled(), |ui| {
        ui.horizontal_wrapped(|ui| {
            ui.label(tr("Image file:"));
            changed |= ui.add(egui::TextEdit::singleline(&mut s.file).hint_text("automatique").desired_width(260.0)).changed();
            if ui.button(tr("Browse…")).clicked() { *browse = true; }
            if !s.file.is_empty() && ui.small_button("✕").on_hover_text(tr("Back to automatic choice")).clicked() { s.file.clear(); changed = true; }
        });
        if s.file.trim().is_empty() {
            let auto = if s.keep == IsoKeep::Iso { tr("asked at each build") } else { tr("next to the chosen folder, same name + .iso") };
            ui.label(egui::RichText::new(trf!("Automatic: {}.", auto)).weak());
        }
        ui.separator();

        ui.heading(tr("Volume"));
        ui.horizontal_wrapped(|ui| {
            ui.label(tr("Volume name:")).on_hover_text(tr("Name shown when the disc is inserted (drive, Explorer)."));
            changed |= ui.add(egui::TextEdit::singleline(&mut s.opts.label).hint_text(title).desired_width(260.0)).changed();
        });
        let kept = s.label(title);
        let typed = if s.opts.label.trim().is_empty() { title } else { s.opts.label.as_str() };
        let latin = typed.chars().all(|c| (c as u32) < 256);
        let max = if latin { MAX_LABEL } else { MAX_LABEL / 2 };
        if kept != typed {
            ui.colored_label(egui::Color32::from_rgb(230, 160, 60), trf!("Too long: only “{}” will be kept ({} characters at most{}).", kept, max, if latin { "" } else { tr(" with these characters") }));
        } else {
            ui.label(egui::RichText::new(trf!("{} / {} characters{}", typed.chars().count(), max, if s.opts.label.trim().is_empty() { tr(" · empty: disc title") } else { "" })).weak());
        }
        ui.label(tr("Volume information (optional, rarely displayed):"));
        egui::Grid::new("infos_volume").num_columns(2).show(ui, |ui| {
            for (label, v) in [(tr("Owner"), &mut s.opts.owner), (tr("Organization"), &mut s.opts.organization), (tr("Contact"), &mut s.opts.contact)] {
                ui.label(label);
                ui.vertical(|ui| {
                    changed |= ui.add(egui::TextEdit::singleline(v).desired_width(240.0)).changed();
                    let k = fit_info(v);
                    if k != *v { ui.colored_label(egui::Color32::from_rgb(230, 160, 60), trf!("Truncated: “{}”", k)); }
                });
                ui.end_row();
            }
        });
        ui.separator();

        ui.heading(tr("Target disc"));
        ui.horizontal(|ui| {
            ui.label(tr("Capacity:"));
            egui::ComboBox::from_id_salt("capacite_iso").selected_text(CAPACITIES[s.capacity.min(2)].0).show_ui(ui, |ui| {
                for (k, (name, bytes)) in CAPACITIES.iter().enumerate() {
                    changed |= ui.selectable_value(&mut s.capacity, k, trf!("{} ({} GB)", name, crate::i18n::num(format!("{:.1}", *bytes as f64 / 1e9)))).changed();
                }
            });
        });
        crate::help(ui, tr("Used to warn when the image does not fit on the disc; the image itself does not change."));
        ui.horizontal_wrapped(|ui| {
            let mut on = s.margin > 0;
            if ui.checkbox(&mut on, tr("Safety margin: warn beyond")).on_hover_text(tr("Burning is less reliable right at the edge of a BD-R: a warning appears when the image exceeds this percentage of the target disc.")).changed() {
                s.margin = if on { DEFAULT_MARGIN } else { 0 };
                changed = true;
            }
            ui.add_enabled_ui(on, |ui| {
                let mut m = if on { s.margin } else { DEFAULT_MARGIN };
                if ui.add(egui::DragValue::new(&mut m).range(50..=99).suffix(" %")).changed() && on { s.margin = m; changed = true; }
            });
            ui.label(tr("of the disc"));
        });
        ui.horizontal(|ui| {
            ui.label(tr("UDF version:"));
            if ui.radio(s.opts.udf == UdfRev::V250, tr("2.50 (recommended)")).on_hover_text(tr("Blu-ray standard: read by all players.")).clicked() && s.opts.udf != UdfRev::V250 { s.opts.udf = UdfRev::V250; changed = true; }
            if ui.radio(s.opts.udf == UdfRev::V260, "2.60").on_hover_text(tr("Variant for BD-Rs burned in several sessions: pointless for an image burned in one go, and less well supported by some players.")).clicked() && s.opts.udf != UdfRev::V260 { s.opts.udf = UdfRev::V260; changed = true; }
        });
        ui.separator();

        ui.heading(tr("File layout"));
        changed |= ui.checkbox(&mut s.opts.align, tr("Align videos on 64 KB")).on_hover_text(tr("Each video file (.m2ts) starts at the beginning of a disc error-correction block. Costs a few KB; some players read an aligned disc better.")).changed();
        changed |= ui.checkbox(&mut s.opts.play_order, tr("Order files by playback")).on_hover_text(tr("Navigation and menus first, then the videos in order, then the backup copies: shorter read-head movements.")).changed();
        changed |= ui.checkbox(&mut s.opts.compat, tr("Maximum compatibility")).on_hover_text(tr("Adds a third file-system “anchor” and places the backup copy of the volume descriptors at the end of the disc, where some old players look for them. Costs 576 KB.")).changed();
        changed |= ui.checkbox(&mut s.opts.mirror, tr("Mirror copy of the metadata")).on_hover_text(tr("The file tree is written twice, at two places on the disc: it stays readable if one copy is scratched.")).changed();
        ui.separator();

        ui.heading(tr("Check"));
        ui.label(tr("Date written in the image:"));
        let cur = s.opts.date;
        if ui.radio(cur == IsoDate::Now, tr("date and time of the build")).clicked() && cur != IsoDate::Now { s.opts.date = IsoDate::Now; changed = true; }
        if ui.radio(cur == IsoDate::Fixed, tr("fixed date (January 1, 2000)")).on_hover_text(tr("All internal dates are identical: the same disc gives exactly the same image, byte for byte.")).clicked() && cur != IsoDate::Fixed { s.opts.date = IsoDate::Fixed; changed = true; }
        ui.horizontal_wrapped(|ui| {
            let custom = matches!(cur, IsoDate::Custom(_));
            if ui.radio(custom, tr("chosen date:")).on_hover_text(tr("Date shown for the disc and its files (universal time). The image is reproducible as well.")).clicked() && !custom {
                s.opts.date = IsoDate::Custom(IsoStamp::now());
                changed = true;
            }
            if let IsoDate::Custom(mut d) = s.opts.date {
                let before = d;
                ui.add(egui::DragValue::new(&mut d.day).range(1..=31));
                ui.label("/");
                ui.add(egui::DragValue::new(&mut d.month).range(1..=12));
                ui.label("/");
                ui.add(egui::DragValue::new(&mut d.year).range(1900..=2100));
                ui.label(tr("at"));
                ui.add(egui::DragValue::new(&mut d.hour).range(0..=23).suffix(" h"));
                ui.add(egui::DragValue::new(&mut d.minute).range(0..=59).custom_formatter(|v, _| format!("{:02}", v as u32)));
                let d = d.clamped();
                if d != before { s.opts.date = IsoDate::Custom(d); changed = true; }
            }
        });
        changed |= ui.checkbox(&mut s.verify, tr("Verify the image after creation")).on_hover_text(tr("Reads the whole image back and compares it with the folder, file by file. Recommended before burning a BD-R.")).changed();
    });
    changed
}
