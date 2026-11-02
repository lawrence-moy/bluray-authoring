//! Panneau « Pistes audio » d'une video : pistes de la video ou fichiers audio separes, chacune avec sa langue du projet.
use crate::i18n::{tr, trf};
use crate::subs_ui::language_for_code;
use bdauthor_core::audio::{AudioSource, AudioTrackCfg, MAX_AUDIO_TRACKS};
use bdauthor_core::ffmpeg::{list_audio_streams, AudioStreamInfo, Tools};
use bdauthor_core::model::VideoEntry;
use bdauthor_core::subtitle::Language;
use eframe::egui;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// Pistes audio deja recherchees, par fichier video (ou le message d'erreur de la recherche).
pub type AudioProbes = HashMap<String, Result<Vec<AudioStreamInfo>, String>>;

fn clean(p: &Path) -> String {
    let s = p.to_string_lossy().into_owned();
    s.strip_prefix(r"\\?\").map(String::from).unwrap_or(s)
}

fn abs(base: &Path, f: &str) -> PathBuf {
    let q = Path::new(f);
    if q.is_absolute() { q.to_path_buf() } else { base.join(q) }
}

/// Retourne vrai si quelque chose a change. `langs` : langues du projet (le rang d'une langue audio est son numero de piste).
pub fn panel(ui: &mut egui::Ui, v: &mut VideoEntry, langs: &mut Vec<Language>, probes: &mut AudioProbes, base: &Path) -> bool {
    let mut changed = false;
    ui.label(tr("Audio tracks (selectable with the remote control):"));
    if v.audio.is_empty() {
        ui.label(egui::RichText::new(tr("No track chosen: the video's first audio track is used, in the default audio language (Edit > Project…).")).weak());
    }
    let mut del = None;
    for (k, t) in v.audio.iter_mut().enumerate() {
        ui.push_id(("audio", k), |ui| {
            let what = match &t.source {
                AudioSource::File(f) => Path::new(f).file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| f.clone()),
                AudioSource::Embedded(n) => trf!("embedded track no. {}", n + 1),
            };
            let lang_label = langs.iter().find(|l| l.id == t.lang_id).map_or_else(|| tr("language to choose").to_string(), |l| l.name.clone());
            egui::CollapsingHeader::new(format!("{} — {} ({})", k + 1, what, lang_label)).default_open(false).show(ui, |ui| {
                if let AudioSource::File(f) = &mut t.source {
                    ui.horizontal(|ui| {
                        ui.label(tr("File:"));
                        changed |= ui.text_edit_singleline(f).changed();
                        if ui.button(tr("Choose…")).clicked() {
                            if let Some(p) = rfd::FileDialog::new().add_filter("Audio", &["wav", "flac", "mp3", "ac3", "eac3", "dts", "aac", "m4a", "ogg", "opus", "mka"]).pick_file() {
                                *f = clean(&p);
                                changed = true;
                            }
                        }
                    });
                    if !f.trim().is_empty() && !abs(base, f).is_file() {
                        ui.colored_label(egui::Color32::from_rgb(230, 100, 90), tr("File not found."));
                    }
                }
                ui.horizontal(|ui| {
                    let cur = langs.iter().find(|l| l.id == t.lang_id).map(|l| l.name.clone());
                    egui::ComboBox::from_label(tr("Language")).selected_text(cur.clone().unwrap_or_else(|| tr("(to choose)").to_string())).show_ui(ui, |ui| {
                        for l in langs.iter() {
                            if ui.selectable_label(t.lang_id == l.id, &l.name).clicked() { t.lang_id = l.id; t.lang = l.code.clone(); changed = true; }
                        }
                    });
                    if cur.is_none() { ui.colored_label(egui::Color32::from_rgb(230, 100, 90), tr("to choose")); }
                });
                ui.horizontal(|ui| {
                    ui.label(tr("Offset:"));
                    changed |= ui.add(egui::DragValue::new(&mut t.offset_ms).speed(10.0).range(-600_000..=600_000).suffix(" ms")).changed();
                });
                if ui.button(tr("Remove this track")).clicked() { del = Some(k); }
            });
        });
    }
    if let Some(k) = del { v.audio.remove(k); changed = true; }

    let full = v.audio.len() >= MAX_AUDIO_TRACKS;
    ui.add_enabled_ui(!full, |ui| {
        ui.horizontal(|ui| {
            if ui.button(tr("Add an audio file…")).clicked() {
                if let Some(p) = rfd::FileDialog::new().add_filter("Audio", &["wav", "flac", "mp3", "ac3", "eac3", "dts", "aac", "m4a", "ogg", "opus", "mka"]).pick_file() {
                    let mut t = AudioTrackCfg::from_file(&clean(&p), "fra");
                    // premiere langue du projet : le francais par defaut ; ensuite, a choisir
                    if langs.is_empty() { t.lang_id = language_for_code(langs, "fra"); }
                    v.audio.push(t);
                    changed = true;
                }
            }
            if ui.button(tr("Audio tracks of the video…")).clicked() {
                let r = match Tools::locate() {
                    Ok(t) => list_audio_streams(&t, &abs(base, &v.file)).map_err(|e| e.to_string()),
                    Err(e) => Err(e.to_string()),
                };
                probes.insert(v.file.clone(), r);
            }
        });
    });
    if full {
        ui.label(egui::RichText::new(trf!("{} tracks maximum per video.", MAX_AUDIO_TRACKS)).weak());
    }
    match probes.get(&v.file) {
        Some(Err(e)) => { ui.colored_label(egui::Color32::from_rgb(230, 100, 90), e.as_str()); }
        Some(Ok(list)) if list.is_empty() => { ui.label(egui::RichText::new(tr("No audio track in this video.")).weak()); }
        Some(Ok(list)) => {
            for s in list {
                ui.horizontal(|ui| {
                    let name = trf!("Track {} : {} {} {} {}", s.index + 1, s.codec, if s.lang.is_empty() { "?" } else { &s.lang }, if s.channels > 0 { trf!("{} channels", s.channels) } else { String::new() }, s.title);
                    ui.label(name);
                    let used = v.audio.iter().any(|t| t.source == AudioSource::Embedded(s.index));
                    if ui.add_enabled(!used && !full, egui::Button::new(tr("Add"))).clicked() {
                        let lang = if s.lang.len() == 3 { s.lang.as_str() } else { "und" };
                        let mut t = AudioTrackCfg::embedded(s.index, lang);
                        // la langue annoncee par la piste est ajoutee a la liste du projet si elle n'y est pas
                        t.lang_id = language_for_code(langs, lang);
                        v.audio.push(t);
                        changed = true;
                    }
                });
            }
        }
        None => {}
    }
    crate::help(ui, tr("Each language keeps the same track number in all videos: their audio languages must therefore follow the order of the language list (menu “Edit > Project…”, tab “Subtitle languages”), with no gaps. Tracks are converted to stereo AC-3. The offset corrects an advance or a delay."));
    changed
}
