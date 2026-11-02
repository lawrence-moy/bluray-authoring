//! Panneau « Sous-titres » d'une video : pistes PGS issues de fichiers .srt/.ass ou de pistes texte integrees a la video.
use bdauthor_core::ffmpeg::{extract_subtitle_srt, extract_thumbnail_rgba, list_subtitle_streams, SubStreamInfo, Tools};
use bdauthor_core::film::MAX_SUB_TRACKS;
use bdauthor_core::model::{ProjectModel, VideoEntry};
use bdauthor_core::palette::RgbaImage;
use bdauthor_core::project::Renderer;
use bdauthor_core::subtitle::{default_language_name, load_cues, merge, next_language_id, shift, Cue, Language, SubSource, SubStyle, SubTrackCfg, VPos};
use bdauthor_render::FontRenderer;
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
pub(crate) fn language_for_code(langs: &mut Vec<Language>, code: &str) -> u32 {
    if let Some(l) = langs.iter().find(|l| l.code == code) { return l.id; }
    let id = next_language_id(langs);
    langs.push(Language { id, name: default_language_name(code), code: code.to_string() });
    id
}

/// Retourne vrai si quelque chose a change. `langs` : langues de sous-titres du projet (le rang d'une langue est son numero de piste).
pub fn panel(ui: &mut egui::Ui, v: &mut VideoEntry, langs: &mut Vec<Language>, probes: &mut Probes, base: &Path, preview: &mut Option<usize>) -> bool {
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
                            if let Some(p) = rfd::FileDialog::new().add_filter("Sous-titres", &["srt", "ass", "ssa", "vtt", "smi", "sami", "sub", "txt", "sbv", "ttml", "dfxp", "xml", "lrc", "stl"]).pick_file() {
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
                ui.horizontal(|ui| {
                    if ui.button("Aperçu du rendu…").on_hover_text("Voir les répliques de cette piste dessinées avec son style").clicked() { *preview = Some(k); }
                    if ui.button("Retirer cette piste").clicked() { del = Some(k); }
                });
            });
        });
    }
    if let Some(k) = del { v.subs.remove(k); changed = true; }

    let full = v.subs.len() >= MAX_SUB_TRACKS;
    ui.add_enabled_ui(!full, |ui| {
        ui.horizontal(|ui| {
            if ui.button("Ajouter un fichier de sous-titres…").clicked() {
                if let Some(p) = rfd::FileDialog::new().add_filter("Sous-titres", &["srt", "ass", "ssa", "vtt", "smi", "sami", "sub", "txt", "sbv", "ttml", "dfxp", "xml", "lrc", "stl"]).pick_file() {
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
    ui.label(egui::RichText::new("La liste des langues du projet se règle dans le menu « Édition > Projet… », onglet « Langues des sous-titres » : chaque langue y garde le même numéro de piste dans toutes les vidéos.").weak());
    ui.label(egui::RichText::new("Formats lus : SRT, ASS/SSA, WebVTT, MicroDVD (23,976 i/s sauf indication {1}{1}cadence), SubViewer, SBV, SAMI ; TTML, LRC, STL et autres via ffmpeg.").weak());
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

/// Onglet « Langues » de la fenetre Projet (audio et sous-titres) : liste des langues du projet (le rang d'une langue est son numero de
/// piste dans toutes les videos). Retourne vrai si quelque chose a change.
pub fn languages_ui(ui: &mut egui::Ui, m: &mut ProjectModel) -> bool {
    let mut changed = false;
    {
        ui.label("Langues des pistes audio et des sous-titres. Le rang dans la liste fixe le numéro de piste, identique dans toutes les vidéos.");
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
            ui.label(egui::RichText::new("Aucune langue : ajoutez-en pour choisir la langue des pistes audio et des sous-titres, et pour les boutons « Régler… ».").weak());
        } else {
            ui.label(egui::RichText::new("Chaque piste audio ou de sous-titres d'une vidéo choisit sa langue dans cette liste. Sous-titres : une vidéo qui n'a pas une langue garde une piste vide à cet endroit. Audio : seules comptent les langues qui ont de l'audio quelque part, et chaque vidéo doit avoir les premières de l'ordre de la liste, sans trou.").weak());
            ui.label(egui::RichText::new("Changer l'ordre change les numéros de piste : les boutons suivent leur langue.").weak());
        }
    }
    changed
}

// ------------------------------------------------------------------ apercu du rendu
/// Fenetre d'apercu d'une piste de sous-titres : les repliques (decalage applique, repliques simultanees fusionnees) dessinees avec le style de la piste.
pub struct SubPreview {
    pub video: usize,
    pub track: usize,
    raw: Option<Result<Vec<Cue>, String>>,
    rx: Option<std::sync::mpsc::Receiver<Result<Vec<Cue>, String>>>,
    shown: Vec<Cue>,
    shown_offset: Option<i64>,
    cue: usize,
    bg: u8,
    vh: u32,
    safe: bool,
    key: String,
    tex: Option<egui::TextureHandle>,
    /// fond « image de la video » : fichier video, images deja extraites (instant en ms, hauteur) et extraction en cours
    video_file: PathBuf,
    frames: HashMap<(i64, u32), Result<RgbaImage, String>>,
    frame_pending: Option<(i64, u32)>,
    frame_rx: Option<std::sync::mpsc::Receiver<((i64, u32), Result<RgbaImage, String>)>>,
    key_frame: (i64, u32),
    /// zoom de l'image (1 = largeur de la fenetre)
    zoom: f32,
    /// zone de l'image a l'image precedente (pour savoir si la molette vise l'image)
    img_rect: egui::Rect,
}

impl SubPreview {
    /// Charge les repliques : un fichier tout de suite, une piste integree par ffmpeg dans un fil separe.
    pub fn new(video: usize, track: usize, t: &SubTrackCfg, video_file: &Path, base: &Path) -> SubPreview {
        let mut p = SubPreview { video, track, raw: None, rx: None, shown: Vec::new(), shown_offset: None, cue: 0, bg: 0, vh: 1080, safe: false, key: String::new(), tex: None, video_file: video_file.to_path_buf(), frames: HashMap::new(), frame_pending: None, frame_rx: None, key_frame: (0, 0), zoom: 1.0, img_rect: egui::Rect::NOTHING };
        match &t.source {
            SubSource::File(f) => p.raw = Some(load_cues(&abs(base, f))),
            SubSource::Embedded(idx) => {
                let (tx, rx) = std::sync::mpsc::channel();
                let (src, idx) = (video_file.to_path_buf(), *idx);
                std::thread::spawn(move || {
                    let r: Result<Vec<Cue>, String> = (|| {
                        let tools = Tools::locate().map_err(|e| e.to_string())?;
                        let dst = std::env::temp_dir().join(format!("bdauthor_apercu_{}_{idx}.srt", std::process::id()));
                        extract_subtitle_srt(&tools, &src, idx, &dst).map_err(|e| e.to_string())?;
                        let r = load_cues(&dst);
                        let _ = std::fs::remove_file(&dst);
                        r
                    })();
                    let _ = tx.send(r);
                });
                p.rx = Some(rx);
            }
        }
        p
    }
}

fn clock(ms: i64) -> String {
    let ms = ms.max(0);
    format!("{}:{:02}:{:02}.{:03}", ms / 3_600_000, ms / 60_000 % 60, ms / 1000 % 60, ms % 1000)
}

/// Une replique sur un fond uni de `vh` lignes (16/9), placee comme dans la piste PGS finale.
fn render_preview(r: &FontRenderer, text: &str, style: &SubStyle, base: &Path, vh: u32, bg: u8, frame: Option<&RgbaImage>) -> egui::ColorImage {
    let vw = vh * 16 / 9;
    let mut st = bdauthor_core::pgs::scaled_style(style, vh);
    if !st.font.is_empty() {
        let c = abs(base, &st.font);
        if c.is_file() { st.font = c.to_string_lossy().into_owned(); }
    }
    let g = [0u8, 110, 225][bg.min(2) as usize];
    let mut canvas = match frame {
        Some(f) if f.width == vw && f.height == vh => f.clone(),
        _ => {
            let mut c = RgbaImage::new(vw, vh);
            let g = if bg >= 3 { 70 } else { g };
            for p in c.data.chunks_exact_mut(4) { p.copy_from_slice(&[g, g, g, 255]); }
            c
        }
    };
    if let Some(img) = r.subtitle(text, &st, vw) {
        if let Some((img, _, _)) = bdauthor_core::pgs::crop_visible(&img, 12) {
            let (w, h) = (img.width.min(vw), img.height.min(vh));
            let x0 = (vw - w) / 2;
            let y0 = match style.pos {
                VPos::Bottom => vh.saturating_sub(st.margin.max(0) as u32 + h),
                VPos::Top => (st.margin.max(0) as u32).min(vh - h),
                VPos::Middle => (vh - h) / 2,
            };
            for y in 0..h {
                for x in 0..w {
                    let p = img.pixel(x, y);
                    if p[3] == 0 { continue; }
                    let a = p[3] as f32 / 255.0;
                    let mut d = canvas.pixel(x0 + x, y0 + y);
                    for c in 0..3 { d[c] = (p[c] as f32 * a + d[c] as f32 * (1.0 - a)).round() as u8; }
                    canvas.set(x0 + x, y0 + y, d);
                }
            }
        }
    }
    egui::ColorImage::from_rgba_unmultiplied([vw as usize, vh as usize], &canvas.data)
}

/// Dessine la fenetre. Retourne (le style a change, la fenetre reste ouverte).
pub fn preview_window(ctx: &egui::Context, pv: &mut SubPreview, v: &mut VideoEntry, renderer: &FontRenderer, base: &Path) -> (bool, bool) {
    let mut changed = false;
    if let Some(rx) = &pv.rx {
        match rx.try_recv() {
            Ok(r) => { pv.raw = Some(r); pv.rx = None; }
            Err(_) => ctx.request_repaint_after(std::time::Duration::from_millis(150)),
        }
    }
    let Some(t) = v.subs.get_mut(pv.track) else { return (false, false) };
    if let Some(Ok(raw)) = &pv.raw {
        if pv.shown_offset != Some(t.offset_ms) {
            pv.shown = merge(&shift(raw, t.offset_ms));
            pv.shown_offset = Some(t.offset_ms);
            pv.cue = pv.cue.min(pv.shown.len().saturating_sub(1));
            pv.key.clear();
        }
    }
    let name = match &t.source {
        SubSource::File(f) => Path::new(f).file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| f.clone()),
        SubSource::Embedded(n) => format!("piste intégrée n° {}", n + 1),
    };
    let mut open = true;
    crate::os_window(ctx, "apercu_sous_titres", "Aperçu des sous-titres", [840.0, 760.0], &mut open, &mut |ui| {
        ui.label(egui::RichText::new(name.as_str()).strong());
        match &pv.raw {
            None => { ui.horizontal(|ui| { ui.spinner(); ui.label("Extraction de la piste…"); }); return; }
            Some(Err(e)) => { ui.colored_label(egui::Color32::from_rgb(230, 100, 90), e.as_str()); return; }
            Some(Ok(_)) => {}
        }
        let n = pv.shown.len();
        if n == 0 {
            ui.label("Aucune réplique affichable (vérifiez le décalage).");
            return;
        }
        ui.horizontal(|ui| {
            if ui.button("<").on_hover_text("Réplique précédente").clicked() { pv.cue = pv.cue.saturating_sub(1); }
            ui.add(egui::Slider::new(&mut pv.cue, 0..=n - 1).custom_formatter(|v, _| format!("{} / {}", v as usize + 1, n)).custom_parser(|s| s.split('/').next().and_then(|a| a.trim().parse::<f64>().ok()).map(|v| v - 1.0)));
            if ui.button(">").on_hover_text("Réplique suivante").clicked() { pv.cue = (pv.cue + 1).min(n - 1); }
            if ui.button("Plus longue").on_hover_text("Va à la réplique la plus longue, la plus exigeante pour la mise en page").clicked() {
                pv.cue = (0..n).max_by_key(|&i| pv.shown[i].text.lines().map(|l| l.chars().count()).max().unwrap_or(0) + pv.shown[i].text.lines().count() * 8).unwrap_or(0);
            }
        });
        pv.cue = pv.cue.min(n - 1);
        let c = pv.shown[pv.cue].clone();
        ui.label(egui::RichText::new(format!("{} → {}", clock(c.start_ms), clock(c.end_ms))).weak());
        ui.horizontal(|ui| {
            ui.label("Fond :");
            for (i, l) in ["Noir", "Gris", "Clair", "Image de la vidéo"].iter().enumerate() { if ui.selectable_label(pv.bg == i as u8, *l).clicked() { pv.bg = i as u8; } }
            ui.separator();
            ui.label("Vidéo :");
            for h in [1080u32, 720] { if ui.selectable_label(pv.vh == h, format!("{h}p")).clicked() { pv.vh = h; } }
            ui.separator();
            ui.checkbox(&mut pv.safe, "Zone de sécurité");
        });
        // fond « image de la video » : l'image reelle a l'apparition de la replique (extraite par ffmpeg en arriere-plan, gardee en memoire)
        let mut frame_note = None;
        if pv.bg == 3 {
            let fk = (c.start_ms + ((c.end_ms - c.start_ms) / 2).min(300), pv.vh);
            if let Some(rx) = &pv.frame_rx {
                if let Ok((k, r)) = rx.try_recv() {
                    if pv.frames.len() >= 6 { pv.frames.clear(); }
                    pv.frames.insert(k, r);
                    pv.frame_pending = None;
                    pv.frame_rx = None;
                    pv.key.clear();
                }
            }
            match pv.frames.get(&fk) {
                Some(Err(e)) => frame_note = Some(format!("Image indisponible : {e}")),
                Some(Ok(_)) => {}
                None => {
                    frame_note = Some("Extraction de l'image de la vidéo…".to_string());
                    if pv.frame_pending.is_none() {
                        let (tx, rx) = std::sync::mpsc::channel();
                        let (src, vh) = (pv.video_file.clone(), pv.vh);
                        std::thread::spawn(move || {
                            let r = Tools::locate().map_err(|e| e.to_string()).and_then(|tools| {
                                extract_thumbnail_rgba(&tools, &src, (vh * 16 / 9, vh), Some(fk.0 as f64 / 1000.0), 0.0).map_err(|e| e.to_string())
                            });
                            let _ = tx.send((fk, r));
                        });
                        pv.frame_pending = Some(fk);
                        pv.frame_rx = Some(rx);
                    }
                    ctx.request_repaint_after(std::time::Duration::from_millis(150));
                }
            }
            pv.key_frame = fk;
        }
        let key = format!("{}|{}|{}|{:?}|{}|{}", pv.cue, pv.bg, pv.vh, t.style, c.text, pv.frames.contains_key(&pv.key_frame));
        if key != pv.key || pv.tex.is_none() {
            let frame = if pv.bg == 3 { pv.frames.get(&pv.key_frame).and_then(|r| r.as_ref().ok()) } else { None };
            let img = render_preview(renderer, &c.text, &t.style, base, pv.vh, pv.bg, frame);
            pv.tex = Some(ctx.load_texture("apercu_sous_titres", img, egui::TextureOptions::LINEAR));
            pv.key = key;
        }
        if let Some(n) = &frame_note { ui.label(egui::RichText::new(n).weak()); }
        // zoom : boutons, molette (ou Ctrl + molette) sur l'image ; glisser l'image pour la deplacer
        // largeur de la fenetre (le contenu est dans une zone defilante, donc sa largeur disponible ne veut rien dire)
        let fit_w = (ctx.screen_rect().width() - 40.0).clamp(200.0, 2400.0);
        let ppp = ctx.pixels_per_point();
        ui.horizontal(|ui| {
            if ui.small_button("−").on_hover_text("Dézoomer").clicked() { pv.zoom = (pv.zoom / 1.25).max(0.25); }
            if ui.small_button("+").on_hover_text("Zoomer").clicked() { pv.zoom = (pv.zoom * 1.25).min(8.0); }
            if ui.small_button("Ajuster").on_hover_text("Image entière dans la largeur de la fenêtre").clicked() { pv.zoom = 1.0; }
            if ui.small_button("1:1").on_hover_text("Un pixel de la vidéo = un pixel de l'écran").clicked() { pv.zoom = ((pv.vh * 16 / 9) as f32 / ppp / fit_w).clamp(0.25, 8.0); }
            ui.label(format!("{:.0} %", pv.zoom * fit_w * ppp / (pv.vh * 16 / 9) as f32 * 100.0)).on_hover_text("100 % = taille réelle de la vidéo");
            ui.label(egui::RichText::new("Molette : zoom · glisser : déplacer").weak());
        });
        // la molette sur l'image zoome (lue avant les zones de defilement, qui l'utiliseraient sinon pour defiler)
        if ctx.pointer_hover_pos().map_or(false, |p| pv.img_rect.contains(p)) {
            let (scroll, zd) = ctx.input(|i| (i.smooth_scroll_delta.y, i.zoom_delta()));
            let f = (scroll * 0.0015).exp() * zd;
            if (f - 1.0).abs() > 1e-4 {
                pv.zoom = (pv.zoom * f).clamp(0.25, 8.0);
                ctx.input_mut(|i| i.smooth_scroll_delta = egui::Vec2::ZERO);
            }
        }
        if let Some(tex) = &pv.tex {
            let size = egui::vec2(fit_w * pv.zoom, fit_w * pv.zoom * 9.0 / 16.0);
            let max_h = (ctx.screen_rect().height() - 330.0).max(220.0);
            let out = egui::ScrollArea::both().id_salt("apercu_zoom").max_height(max_h).auto_shrink([false, true]).show(ui, |ui| {
                let resp = ui.image((tex.id(), size));
                if pv.safe {
                    // zone de securite titre : 90 % de la largeur et de la hauteur
                    let r = resp.rect;
                    let inner = r.shrink2(egui::vec2(r.width() * 0.05, r.height() * 0.05));
                    ui.painter().rect_stroke(inner, 0.0, egui::Stroke::new(1.0_f32, egui::Color32::from_rgb(255, 170, 60)));
                }
                resp
            });
            pv.img_rect = out.inner_rect;
        }
        ui.label(egui::RichText::new("Aperçu avec la police et le style de la piste (décalage appliqué, répliques simultanées fusionnées) ; le lecteur affichera le même dessin.").weak());
        egui::CollapsingHeader::new("Modifier le style de la piste").default_open(false).show(ui, |ui| style_ui(ui, &mut t.style, &mut changed));
    });
    (changed, open)
}
