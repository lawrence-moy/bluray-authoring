//! Panneau « Sous-titres » d'une video : pistes PGS issues de fichiers .srt/.ass ou de pistes texte integrees a la video.
use bdauthor_core::ffmpeg::{list_subtitle_streams, SubStreamInfo, Tools};
use bdauthor_core::film::MAX_SUB_TRACKS;
use bdauthor_core::model::{ProjectModel, VideoEntry};
use bdauthor_core::subtitle::{default_language_name, next_language_id, Language, SubSource, SubStyle, SubTrackCfg, VPos};
use eframe::egui;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// Pistes integrees deja recherchees, par fichier video (ou le message d'erreur de la recherche).
pub type Probes = HashMap<String, Result<Vec<SubStreamInfo>, String>>;

fn clean(p: &Path) -> String {
    let s = p.to_string_lossy().into_owned();
    s.strip_prefix(r"\\?\").map(String::from).unwrap_or(s)
}

fn abs(base: &Path, f: &str) -> PathBuf {
    let q = Path::new(f);
    if q.is_absolute() { q.to_path_buf() } else { base.join(q) }
}

/// Langue du projet portant ce code (creee avec son nom usuel si elle n'existe pas) ; retourne son identifiant.
fn language_for_code(langs: &mut Vec<Language>, code: &str) -> u32 {
    if let Some(l) = langs.iter().find(|l| l.code == code) { return l.id; }
    let id = next_language_id(langs);
    langs.push(Language { id, name: default_language_name(code), code: code.to_string() });
    id
}

/// Retourne vrai si quelque chose a change. `langs` : langues de sous-titres du projet (le rang d'une langue est son numero de piste).
pub fn panel(ui: &mut egui::Ui, v: &mut VideoEntry, langs: &mut Vec<Language>, probes: &mut Probes, base: &Path) -> bool {
    let mut changed = false;
    ui.label("Sous-titres (affichés par le lecteur, au choix avec la télécommande) :");
    let mut del = None;
    for (k, t) in v.subs.iter_mut().enumerate() {
        ui.push_id(k, |ui| {
            let what = match &t.source {
                SubSource::File(f) => Path::new(f).file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| f.clone()),
                SubSource::Embedded(n) => format!("piste intégrée n° {}", n + 1),
            };
            let lang_label = langs.iter().find(|l| l.id == t.lang_id).map_or_else(|| if langs.is_empty() { t.lang.clone() } else { "langue à choisir".to_string() }, |l| l.name.clone());
            egui::CollapsingHeader::new(format!("{} — {} ({})", k + 1, what, lang_label)).default_open(false).show(ui, |ui| {
                if let SubSource::File(f) = &mut t.source {
                    ui.horizontal(|ui| {
                        ui.label("Fichier :");
                        changed |= ui.text_edit_singleline(f).changed();
                        if ui.button("Choisir…").clicked() {
                            if let Some(p) = rfd::FileDialog::new().add_filter("Sous-titres", &["srt", "ass", "ssa"]).pick_file() {
                                *f = clean(&p);
                                changed = true;
                            }
                        }
                    });
                    if !f.trim().is_empty() && !abs(base, f).is_file() {
                        ui.colored_label(egui::Color32::from_rgb(230, 100, 90), "Fichier introuvable.");
                    }
                }
                ui.horizontal(|ui| {
                    let cur = langs.iter().find(|l| l.id == t.lang_id).map(|l| l.name.clone());
                    let shown = cur.clone().unwrap_or_else(|| "(à choisir)".to_string());
                    egui::ComboBox::from_label("Langue").selected_text(shown).show_ui(ui, |ui| {
                        for l in langs.iter() {
                            if ui.selectable_label(t.lang_id == l.id, &l.name).clicked() { t.lang_id = l.id; t.lang = l.code.clone(); changed = true; }
                        }
                    });
                    if cur.is_none() { ui.colored_label(egui::Color32::from_rgb(230, 100, 90), "à choisir"); }
                });
                ui.horizontal(|ui| {
                    ui.label("Décalage :");
                    changed |= ui.add(egui::DragValue::new(&mut t.offset_ms).speed(10.0).range(-600_000..=600_000).suffix(" ms")).changed();
                });
                style_ui(ui, &mut t.style, &mut changed);
                if ui.button("Retirer cette piste").clicked() { del = Some(k); }
            });
        });
    }
    if let Some(k) = del { v.subs.remove(k); changed = true; }

    let full = v.subs.len() >= MAX_SUB_TRACKS;
    ui.add_enabled_ui(!full, |ui| {
        ui.horizontal(|ui| {
            if ui.button("Ajouter un fichier .srt / .ass…").clicked() {
                if let Some(p) = rfd::FileDialog::new().add_filter("Sous-titres", &["srt", "ass", "ssa"]).pick_file() {
                    let mut t = SubTrackCfg::from_file(&clean(&p), "fra");
                    // premiere langue du projet : le francais par defaut ; ensuite, a choisir
                    if langs.is_empty() { t.lang_id = language_for_code(langs, "fra"); }
                    v.subs.push(t);
                    changed = true;
                }
            }
            if ui.button("Pistes intégrées à la vidéo…").clicked() {
                let r = match Tools::locate() {
                    Ok(t) => list_subtitle_streams(&t, &abs(base, &v.file)).map_err(|e| e.to_string()),
                    Err(e) => Err(e.to_string()),
                };
                probes.insert(v.file.clone(), r);
            }
        });
    });
    if full {
        ui.label(egui::RichText::new(format!("{MAX_SUB_TRACKS} pistes au maximum par vidéo.")).weak());
    }
    match probes.get(&v.file) {
        Some(Err(e)) => { ui.colored_label(egui::Color32::from_rgb(230, 100, 90), e.as_str()); }
        Some(Ok(list)) if list.is_empty() => { ui.label(egui::RichText::new("Aucune piste de sous-titres dans cette vidéo.").weak()); }
        Some(Ok(list)) => {
            for s in list {
                ui.horizontal(|ui| {
                    let name = format!("Piste {} : {} {} {}", s.index + 1, s.codec, if s.lang.is_empty() { "?" } else { &s.lang }, s.title);
                    ui.label(name);
                    if s.is_text() {
                        let used = v.subs.iter().any(|t| t.source == SubSource::Embedded(s.index));
                        if ui.add_enabled(!used && !full, egui::Button::new("Ajouter")).clicked() {
                            let lang = if s.lang.len() == 3 { s.lang.as_str() } else { "und" };
                            let mut t = SubTrackCfg::embedded(s.index, lang);
                            // la langue annoncee par la piste est ajoutee a la liste du projet si elle n'y est pas
                            t.lang_id = language_for_code(langs, lang);
                            v.subs.push(t);
                            changed = true;
                        }
                    } else {
                        ui.label(egui::RichText::new("(sous-titres en images : non gérés)").weak());
                    }
                });
            }
        }
        None => {}
    }
    ui.label(egui::RichText::new("La liste des langues du projet se règle dans le menu « Édition > Langues des sous-titres… » : chaque langue y garde le même numéro de piste dans toutes les vidéos.").weak());
    ui.label(egui::RichText::new("Le décalage corrige un retard ou une avance. Les sous-titres sont dessinés en images (format Blu-ray PGS) ; la télécommande les active et choisit la piste.").weak());
    changed
}

fn style_ui(ui: &mut egui::Ui, s: &mut SubStyle, changed: &mut bool) {
    ui.horizontal(|ui| {
        ui.label("Taille :");
        *changed |= ui.add(egui::DragValue::new(&mut s.size).range(8..=400).suffix(" px")).changed();
        ui.label("Couleur :");
        *changed |= ui.color_edit_button_srgb(&mut s.color).changed();
    });
    ui.horizontal(|ui| {
        ui.label("Contour :");
        *changed |= ui.add(egui::DragValue::new(&mut s.outline).range(0..=30).suffix(" px")).changed();
        ui.label("Couleur du contour :");
        *changed |= ui.color_edit_button_srgb(&mut s.outline_color).changed();
    });
    ui.horizontal(|ui| {
        ui.label("Position :");
        for p in [VPos::Top, VPos::Middle, VPos::Bottom] {
            if ui.selectable_label(s.pos == p, p.name()).clicked() && s.pos != p { s.pos = p; *changed = true; }
        }
        ui.label("Marge :");
        *changed |= ui.add(egui::DragValue::new(&mut s.margin).range(0..=540).suffix(" px")).changed();
    });
    let shown = if s.font.is_empty() { "Police par défaut".to_string() } else {
        bdauthor_render::common_fonts().into_iter().find(|(_, f)| *f == s.font).map(|(l, _)| l.to_string()).unwrap_or_else(|| s.font.clone())
    };
    let mut pick = false;
    egui::ComboBox::from_label("Police").selected_text(shown).show_ui(ui, |ui| {
        if ui.selectable_label(s.font.is_empty(), "Police par défaut").clicked() { s.font.clear(); *changed = true; }
        for (label, file) in bdauthor_render::common_fonts() {
            if ui.selectable_label(s.font == file, label).clicked() { s.font = file.to_string(); *changed = true; }
        }
        if ui.button("Autre fichier…").clicked() { pick = true; ui.close_menu(); }
    });
    if pick {
        if let Some(p) = rfd::FileDialog::new().add_filter("Polices", &["ttf", "otf"]).pick_file() {
            s.font = clean(&p);
            *changed = true;
        }
    }
    ui.label(egui::RichText::new("Tailles données pour une vidéo de 1080 lignes ; elles s'adaptent automatiquement à 720.").weak());
}

/// Fenetre « Langues des sous-titres » : liste des langues du projet (le rang d'une langue est son numero de piste dans toutes
/// les videos). Retourne vrai si quelque chose a change.
pub fn languages_window(ctx: &egui::Context, open: &mut bool, m: &mut ProjectModel) -> bool {
    let mut changed = false;
    egui::Window::new("Langues des sous-titres").open(open).resizable(false).show(ctx, |ui| {
        ui.label("Le rang dans la liste est le numéro de piste, identique dans toutes les vidéos.");
        let mut op: Option<(usize, i32)> = None; // (rang, -1 haut / 1 bas / 0 retirer)
        let n = m.languages.len();
        for (k, l) in m.languages.iter_mut().enumerate() {
            ui.push_id(("langue", l.id), |ui| {
                ui.horizontal(|ui| {
                    ui.label(format!("{}.", k + 1));
                    changed |= ui.add(egui::TextEdit::singleline(&mut l.name).desired_width(150.0)).changed();
                    ui.label("code :");
                    changed |= ui.add(egui::TextEdit::singleline(&mut l.code).desired_width(40.0)).changed();
                    if ui.add_enabled(k > 0, egui::Button::new("▲").small()).clicked() { op = Some((k, -1)); }
                    if ui.add_enabled(k + 1 < n, egui::Button::new("▼").small()).clicked() { op = Some((k, 1)); }
                    if ui.small_button("Retirer").clicked() { op = Some((k, 0)); }
                });
            });
        }
        if let Some((k, a)) = op {
            match a {
                -1 => m.move_language(k, k - 1),
                1 => m.move_language(k, k + 1),
                _ => { let id = m.languages[k].id; m.remove_language(id); }
            }
            changed = true;
        }
        ui.add_enabled_ui(m.languages.len() < MAX_SUB_TRACKS, |ui| {
            ui.menu_button("Ajouter une langue ▾", |ui| {
                for code in ["fra", "eng", "deu", "spa", "ita", "por", "nld", "jpn", "zho", "rus", "ara", "kor"] {
                    let name = default_language_name(code);
                    if ui.button(&name).clicked() {
                        m.add_language(&name, code);
                        changed = true;
                        ui.close_menu();
                    }
                }
                if ui.button("Autre…").clicked() { m.add_language("Nouvelle langue", "und"); changed = true; ui.close_menu(); }
            });
        });
        if m.languages.len() >= MAX_SUB_TRACKS {
            ui.label(egui::RichText::new(format!("{MAX_SUB_TRACKS} langues au maximum.")).weak());
        }
        ui.separator();
        if m.languages.is_empty() {
            ui.label(egui::RichText::new("Aucune langue : ajoutez-en pour que les boutons « Régler les sous-titres » choisissent une langue.").weak());
        } else {
            ui.label(egui::RichText::new("Chaque piste de sous-titres d'une vidéo choisit sa langue dans cette liste. Une vidéo qui n'a pas une langue garde une piste vide à cet endroit, pour que le bouton « Anglais », par exemple, fonctionne dans toutes les vidéos et avec « Tout lire ».").weak());
            ui.label(egui::RichText::new("Changer l'ordre change les numéros de piste : les boutons suivent leur langue.").weak());
        }
    });
    changed
}
