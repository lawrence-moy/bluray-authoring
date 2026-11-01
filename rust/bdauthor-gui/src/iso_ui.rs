//! Onglet « Génération ISO » de la fenêtre Projet : ce que la construction produit (dossier, image, les deux) et réglages de l'image.
use bdauthor_core::iso::{fit_info, IsoDate, IsoKeep, IsoSettings, IsoStamp, UdfRev, CAPACITIES, DEFAULT_MARGIN, MAX_LABEL};
use eframe::egui;

/// Rend vrai si un réglage a changé ; `browse` : l'utilisateur veut choisir le fichier de l'image.
pub fn iso_ui(ui: &mut egui::Ui, s: &mut IsoSettings, title: &str, browse: &mut bool) -> bool {
    let mut changed = false;
    ui.heading("Sortie");
    ui.label("La construction produit :");
    for (k, label, tip) in [
        (IsoKeep::Folder, "le dossier BDMV seul", "Comme avant : un dossier BDMV + CERTIFICATE, à graver avec un autre logiciel ou à lire tel quel."),
        (IsoKeep::Iso, "l'image ISO seule", "Le disque est construit dans un dossier temporaire, supprimé une fois l'image écrite : la place n'est occupée qu'une fois."),
        (IsoKeep::Both, "le dossier et l'image ISO", "L'image est écrite à côté du dossier choisi (même nom, extension .iso), sauf si un fichier est indiqué ci-dessous."),
    ] {
        if ui.radio(s.keep == k, label).on_hover_text(tip).clicked() && s.keep != k { s.keep = k; changed = true; }
    }
    ui.add_enabled_ui(s.enabled(), |ui| {
        ui.horizontal_wrapped(|ui| {
            ui.label("Fichier de l'image :");
            changed |= ui.add(egui::TextEdit::singleline(&mut s.file).hint_text("automatique").desired_width(260.0)).changed();
            if ui.button("Parcourir…").clicked() { *browse = true; }
            if !s.file.is_empty() && ui.small_button("✕").on_hover_text("Revenir au choix automatique").clicked() { s.file.clear(); changed = true; }
        });
        if s.file.trim().is_empty() {
            let auto = if s.keep == IsoKeep::Iso { "demandé à chaque construction" } else { "à côté du dossier choisi, même nom + .iso" };
            ui.label(egui::RichText::new(format!("Automatique : {auto}.")).weak());
        }
        ui.separator();

        ui.heading("Volume");
        ui.horizontal_wrapped(|ui| {
            ui.label("Nom du volume :").on_hover_text("Nom affiché quand on insère le disque (lecteur, Explorateur).");
            changed |= ui.add(egui::TextEdit::singleline(&mut s.opts.label).hint_text(title).desired_width(260.0)).changed();
        });
        let kept = s.label(title);
        let typed = if s.opts.label.trim().is_empty() { title } else { s.opts.label.as_str() };
        let latin = typed.chars().all(|c| (c as u32) < 256);
        let max = if latin { MAX_LABEL } else { MAX_LABEL / 2 };
        if kept != typed {
            ui.colored_label(egui::Color32::from_rgb(230, 160, 60), format!("Trop long : seul « {kept} » sera gardé ({max} caractères au plus{}).", if latin { "" } else { " avec ces caractères" }));
        } else {
            ui.label(egui::RichText::new(format!("{} / {max} caractères{}", typed.chars().count(), if s.opts.label.trim().is_empty() { " · vide : titre du disque" } else { "" })).weak());
        }
        ui.label("Informations du volume (facultatives, rarement affichées) :");
        egui::Grid::new("infos_volume").num_columns(2).show(ui, |ui| {
            for (label, v) in [("Propriétaire", &mut s.opts.owner), ("Organisation", &mut s.opts.organization), ("Contact", &mut s.opts.contact)] {
                ui.label(label);
                ui.vertical(|ui| {
                    changed |= ui.add(egui::TextEdit::singleline(v).desired_width(240.0)).changed();
                    let k = fit_info(v);
                    if k != *v { ui.colored_label(egui::Color32::from_rgb(230, 160, 60), format!("Tronqué : « {k} »")); }
                });
                ui.end_row();
            }
        });
        ui.separator();

        ui.heading("Disque visé");
        ui.horizontal(|ui| {
            ui.label("Capacité :");
            egui::ComboBox::from_id_salt("capacite_iso").selected_text(CAPACITIES[s.capacity.min(2)].0).show_ui(ui, |ui| {
                for (k, (name, bytes)) in CAPACITIES.iter().enumerate() {
                    changed |= ui.selectable_value(&mut s.capacity, k, format!("{name} ({:.1} Go)", *bytes as f64 / 1e9).replace('.', ",")).changed();
                }
            });
        });
        ui.label(egui::RichText::new("Sert à prévenir si l'image ne rentre pas sur le disque ; l'image elle-même ne change pas.").weak());
        ui.horizontal_wrapped(|ui| {
            let mut on = s.margin > 0;
            if ui.checkbox(&mut on, "Marge de sécurité : prévenir au-delà de").on_hover_text("La gravure est moins fiable tout au bord d'un BD-R : un avertissement apparaît quand l'image dépasse ce pourcentage du disque visé.").changed() {
                s.margin = if on { DEFAULT_MARGIN } else { 0 };
                changed = true;
            }
            ui.add_enabled_ui(on, |ui| {
                let mut m = if on { s.margin } else { DEFAULT_MARGIN };
                if ui.add(egui::DragValue::new(&mut m).range(50..=99).suffix(" %")).changed() && on { s.margin = m; changed = true; }
            });
            ui.label("du disque");
        });
        ui.horizontal(|ui| {
            ui.label("Version UDF :");
            if ui.radio(s.opts.udf == UdfRev::V250, "2.50 (recommandé)").on_hover_text("Norme des Blu-ray : lue par toutes les platines.").clicked() && s.opts.udf != UdfRev::V250 { s.opts.udf = UdfRev::V250; changed = true; }
            if ui.radio(s.opts.udf == UdfRev::V260, "2.60").on_hover_text("Variante pour les BD-R gravés en plusieurs fois : aucun intérêt pour une image gravée d'un coup, et moins bien gérée par certaines platines.").clicked() && s.opts.udf != UdfRev::V260 { s.opts.udf = UdfRev::V260; changed = true; }
        });
        ui.separator();

        ui.heading("Disposition");
        changed |= ui.checkbox(&mut s.opts.align, "Aligner les vidéos sur 64 Ko").on_hover_text("Chaque fichier vidéo (.m2ts) commence au début d'un bloc de correction d'erreurs du disque. Coûte quelques Ko ; certaines platines lisent mieux un disque aligné.").changed();
        changed |= ui.checkbox(&mut s.opts.play_order, "Ordonner les fichiers selon la lecture").on_hover_text("Navigation et menus en tête, puis les vidéos dans l'ordre, puis les copies de secours : déplacements de la tête de lecture plus courts.").changed();
        changed |= ui.checkbox(&mut s.opts.compat, "Compatibilité maximale").on_hover_text("Ajoute une troisième « ancre » du système de fichiers et place la copie de réserve des descripteurs du volume en fin de disque, où certains vieux lecteurs les cherchent. Coûte 576 Ko.").changed();
        changed |= ui.checkbox(&mut s.opts.mirror, "Copie miroir des métadonnées").on_hover_text("L'arborescence des fichiers est écrite deux fois, à deux endroits du disque : il reste lisible si une copie est rayée.").changed();
        ui.separator();

        ui.heading("Contrôle");
        ui.label("Date inscrite dans l'image :");
        let cur = s.opts.date;
        if ui.radio(cur == IsoDate::Now, "date et heure de la construction").clicked() && cur != IsoDate::Now { s.opts.date = IsoDate::Now; changed = true; }
        if ui.radio(cur == IsoDate::Fixed, "date fixe (1er janvier 2000)").on_hover_text("Toutes les dates internes sont identiques : le même disque donne exactement la même image, octet pour octet.").clicked() && cur != IsoDate::Fixed { s.opts.date = IsoDate::Fixed; changed = true; }
        ui.horizontal_wrapped(|ui| {
            let custom = matches!(cur, IsoDate::Custom(_));
            if ui.radio(custom, "date choisie :").on_hover_text("Date affichée pour le disque et ses fichiers (temps universel). L'image est reproductible elle aussi.").clicked() && !custom {
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
                ui.label("à");
                ui.add(egui::DragValue::new(&mut d.hour).range(0..=23).suffix(" h"));
                ui.add(egui::DragValue::new(&mut d.minute).range(0..=59).custom_formatter(|v, _| format!("{:02}", v as u32)));
                let d = d.clamped();
                if d != before { s.opts.date = IsoDate::Custom(d); changed = true; }
            }
        });
        changed |= ui.checkbox(&mut s.verify, "Vérifier l'image après création").on_hover_text("Relit toute l'image et la compare au dossier, fichier par fichier. Recommandé avant de graver un BD-R.").changed();
    });
    changed
}
