//! Panneau « Sous-titres » d'une video : pistes PGS issues de fichiers .srt/.ass ou de pistes texte integrees a la video.
use crate::i18n::{tr, trf};
use bdauthor_core::ffmpeg::{extract_subtitle_srt, list_subtitle_streams, FrameStream, SubStreamInfo, Tools};
use bdauthor_core::film::MAX_SUB_TRACKS;
use bdauthor_core::framesrc::{probe_async, Fit, FrameLoader, VideoMeta};
use crate::timeline_ui::{Timeline, TimelineCfg};
use bdauthor_core::model::{ProjectModel, VideoEntry};
use bdauthor_core::palette::RgbaImage;
use bdauthor_core::project::Renderer;
use bdauthor_core::charset::Charset;
use bdauthor_core::subtitle::{check_quality, clear_positions, default_language_name, load_cues, load_cues_enc, merge, next_language_id, plain_text, shift, strip_format, strip_notes, Cue, CuePos, HAlign, Language, QualityIssue, QualityLimits, SubSource, SubStyle, SubTrackCfg, VPos};
use bdauthor_render::FontRenderer;
use eframe::egui;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

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
pub fn panel(ui: &mut egui::Ui, v: &mut VideoEntry, langs: &mut Vec<Language>, probes: &mut Probes, base: &Path, preview: &mut Option<usize>, auto_fps: Option<f64>) -> bool {
    let mut changed = false;
    ui.label(tr("Subtitles (displayed by the player, selectable with the remote control):"));
    let mut del = None;
    for (k, t) in v.subs.iter_mut().enumerate() {
        ui.push_id(k, |ui| {
            let what = match &t.source {
                SubSource::File(f) => Path::new(f).file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| f.clone()),
                SubSource::Embedded(n) => trf!("embedded track no. {}", n + 1),
            };
            // ligne de la piste : titre a gauche ; a droite, la langue, le bouton de reglages et le retrait
            let open_id = ui.id().with("reglages_ouverts");
            let mut open: bool = ui.data_mut(|d| d.get_persisted(open_id)).unwrap_or(false);
            ui.horizontal(|ui| {
                ui.add(egui::Label::new(format!("{} — {}", k + 1, what)).truncate());
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.button(egui::RichText::new("🗑").size(17.0)).on_hover_text(tr("Remove this track")).clicked() { del = Some(k); }
                    if ui.button(egui::RichText::new("✏").size(17.0)).on_hover_text(tr("Edit this track's settings")).clicked() { open = !open; }
                    let cur = langs.iter().find(|l| l.id == t.lang_id).map(|l| l.name.clone());
                    let shown = cur.clone().unwrap_or_else(|| if langs.is_empty() { t.lang.clone() } else { tr("(to choose)").to_string() });
                    egui::ComboBox::from_id_salt("langue_piste_sous_titres").selected_text(shown).show_ui(ui, |ui| {
                        for l in langs.iter() {
                            if ui.selectable_label(t.lang_id == l.id, &l.name).clicked() { t.lang_id = l.id; t.lang = l.code.clone(); changed = true; }
                        }
                    }).response.on_hover_text(tr("Language of this track"));
                    if cur.is_none() && !langs.is_empty() { ui.colored_label(egui::Color32::from_rgb(230, 100, 90), tr("to choose")); }
                });
            });
            ui.data_mut(|d| d.insert_persisted(open_id, open));
            if open { ui.indent("reglages_piste", |ui| {
                if let SubSource::File(f) = &mut t.source {
                    ui.horizontal(|ui| {
                        ui.label(tr("File:"));
                        changed |= ui.text_edit_singleline(f).changed();
                        if ui.button(tr("Choose…")).clicked() {
                            if let Some(p) = rfd::FileDialog::new().add_filter(tr("Subtitles"), &["srt", "ass", "ssa", "vtt", "smi", "sami", "sub", "txt", "sbv", "ttml", "dfxp", "xml", "lrc", "stl"]).pick_file() {
                                *f = clean(&p);
                                changed = true;
                            }
                        }
                    });
                    if !f.trim().is_empty() && !abs(base, f).is_file() {
                        ui.colored_label(egui::Color32::from_rgb(230, 100, 90), tr("File not found."));
                    }
                }
                if let SubSource::File(_) = &t.source {
                    let auto_cs = Charset::for_language(&t.lang);
                    ui.horizontal(|ui| {
                        ui.label(tr("File encoding:"));
                        let shown = if t.charset == Charset::Auto { trf!("Automatic ({})", auto_cs.label()) } else { t.charset.label().to_string() };
                        egui::ComboBox::from_id_salt("encodage_sous_titres").selected_text(shown).show_ui(ui, |ui| {
                            for c in Charset::ALL {
                                let l = if c == Charset::Auto { trf!("Automatic (UTF-8, otherwise {})", auto_cs.label()) } else { c.label().to_string() };
                                if ui.selectable_label(t.charset == c, l).clicked() { t.charset = c; changed = true; }
                            }
                        });
                    }).response.on_hover_text(tr("Recent files are in UTF-8. Older ones use a code page specific to the language (Windows-1251 for Russian, Shift-JIS for Japanese…): “Automatic” guesses it from the track's language. If accents or letters display wrongly, choose the encoding by hand."));
                }
                // cadence : seulement pour les fichiers dont les temps sont en images (MicroDVD : .sub, .txt)
                if let SubSource::File(f) = &t.source {
                    let ext = Path::new(f).extension().and_then(|e| e.to_str()).unwrap_or("").to_ascii_lowercase();
                    if ext == "sub" || ext == "txt" {
                        ui.horizontal(|ui| {
                            ui.label(tr("Frame rate (if the file is frame-based):"));
                            let auto = auto_fps.map_or(tr("Automatic (23.976 fps)").to_string(), |v| trf!("Automatic ({} fps, from the video)", format!("{:.3}", v)));
                            let shown = t.fps.map_or(auto.clone(), |v| trf!("{} fps", v));
                            egui::ComboBox::from_id_salt("cadence_sous_titres").selected_text(shown).show_ui(ui, |ui| {
                                if ui.selectable_label(t.fps.is_none(), auto.as_str()).clicked() { t.fps = None; changed = true; }
                                for v in [23.976, 24.0, 25.0, 29.97, 30.0, 50.0, 59.94] {
                                    if ui.selectable_label(t.fps == Some(v), trf!("{} fps", v)).clicked() { t.fps = Some(v); changed = true; }
                                }
                            });
                            if let Some(v) = &mut t.fps {
                                changed |= ui.add(egui::DragValue::new(v).speed(0.01).range(1.0..=240.0).suffix(tr(" fps"))).changed();
                            }
                        }).response.on_hover_text(tr("MicroDVD files count in frames, not seconds: choose the frame rate of the original video (a first line {1}{1}25 in the file already sets it). A wrong choice stretches or compresses the timing."));
                    }
                }
                ui.horizontal(|ui| {
                    ui.label(tr("Offset:"));
                    changed |= ui.add(egui::DragValue::new(&mut t.offset_ms).speed(10.0).range(-600_000..=600_000).suffix(" ms")).changed();
                });
                changed |= ui.checkbox(&mut t.strip_notes, tr("Remove [bracketed] and (parenthesized) annotations"))
                    .on_hover_text(tr("For subtitles meant for the deaf and hard of hearing: [Music], (laughs), (sighs)… are removed; a cue that contains only that disappears.")).changed();
                changed |= ui.checkbox(&mut t.keep_format, tr("Keep the file's italics, bold and colors"))
                    .on_hover_text(tr("The tags <i>, <b>, <u>, <font color> (SRT, WebVTT, SAMI) and {\\i1}, {\\b1}, {\\c&H…&} (ASS) are drawn. Unchecked: all the text takes the track's style. The text color set below is used for text with no color of its own.")).changed();
                changed |= ui.checkbox(&mut t.file_positions, tr("Respect the positions in the file (ASS: \\an1 to \\an9)"))
                    .on_hover_text(tr("Some files place cues at the top, on the left, etc. (for example so as not to hide text burned into the picture). Unchecked: all cues take the position set below. Simultaneous cues are displayed together, at the position of the first one that asks for one.")).changed();
                // le style du texte (police, couleurs, position...) se regle dans la fenetre d'apercu du rendu, image a l'appui
                if ui.button(tr("Rendering preview…")).on_hover_text(tr("See this track's cues drawn with its style, and set the text style (font, colors, position…)")).clicked() { *preview = Some(k); }
            }); }
        });
    }
    if let Some(k) = del { v.subs.remove(k); changed = true; }

    let full = v.subs.len() >= MAX_SUB_TRACKS;
    ui.add_enabled_ui(!full, |ui| {
        ui.horizontal(|ui| {
            if ui.button(tr("Add a subtitle file…")).clicked() {
                if let Some(p) = rfd::FileDialog::new().add_filter(tr("Subtitles"), &["srt", "ass", "ssa", "vtt", "smi", "sami", "sub", "txt", "sbv", "ttml", "dfxp", "xml", "lrc", "stl"]).pick_file() {
                    let mut t = SubTrackCfg::from_file(&clean(&p), "fra");
                    // premiere langue du projet : le francais par defaut ; ensuite, a choisir
                    if langs.is_empty() { t.lang_id = language_for_code(langs, "fra"); }
                    v.subs.push(t);
                    changed = true;
                }
            }
            if ui.button(tr("Tracks embedded in the video…")).clicked() {
                let r = match Tools::locate() {
                    Ok(t) => list_subtitle_streams(&t, &abs(base, &v.file)).map_err(|e| e.to_string()),
                    Err(e) => Err(e.to_string()),
                };
                probes.insert(v.file.clone(), r);
            }
        });
    });
    if full {
        ui.label(egui::RichText::new(trf!("{} tracks maximum per video.", MAX_SUB_TRACKS)).weak());
    }
    match probes.get(&v.file) {
        Some(Err(e)) => { ui.colored_label(egui::Color32::from_rgb(230, 100, 90), e.as_str()); }
        Some(Ok(list)) if list.is_empty() => { ui.label(egui::RichText::new(tr("No subtitle track in this video.")).weak()); }
        Some(Ok(list)) => {
            for s in list {
                ui.horizontal(|ui| {
                    let name = trf!("Track {} : {} {} {}", s.index + 1, s.codec, if s.lang.is_empty() { "?" } else { &s.lang }, s.title);
                    ui.label(name);
                    if s.is_text() {
                        let used = v.subs.iter().any(|t| t.source == SubSource::Embedded(s.index));
                        if ui.add_enabled(!used && !full, egui::Button::new(tr("Add"))).clicked() {
                            let lang = if s.lang.len() == 3 { s.lang.as_str() } else { "und" };
                            let mut t = SubTrackCfg::embedded(s.index, lang);
                            // la langue annoncee par la piste est ajoutee a la liste du projet si elle n'y est pas
                            t.lang_id = language_for_code(langs, lang);
                            v.subs.push(t);
                            changed = true;
                        }
                    } else {
                        ui.label(egui::RichText::new(tr("(image-based subtitles: not supported)")).weak());
                    }
                });
            }
        }
        None => {}
    }
    crate::help(ui, tr("The project's language list is set in the menu “Edit > Project…”, tab “Subtitle languages”: each language keeps the same track number in all videos.\n\nFormats read: SRT, ASS/SSA, WebVTT, MicroDVD (23.976 fps unless a {1}{1}rate line says otherwise), SubViewer, SBV, SAMI; TTML, LRC, STL and others via ffmpeg.\n\nThe offset corrects a delay or an advance. Subtitles are drawn as images (Blu-ray PGS format); the remote control turns them on and chooses the track."));
    changed
}

fn style_ui(ui: &mut egui::Ui, s: &mut SubStyle, changed: &mut bool) {
    ui.horizontal(|ui| {
        ui.label(tr("Size:"));
        *changed |= ui.add(egui::DragValue::new(&mut s.size).range(8..=400).suffix(" px")).changed();
        ui.label(tr("Color:"));
        *changed |= ui.color_edit_button_srgb(&mut s.color).changed();
    });
    ui.horizontal(|ui| {
        ui.label(tr("Outline:"));
        *changed |= ui.add(egui::DragValue::new(&mut s.outline).range(0..=30).suffix(" px")).changed();
        ui.label(tr("Outline color:"));
        *changed |= ui.color_edit_button_srgb(&mut s.outline_color).changed();
    });
    ui.horizontal(|ui| {
        ui.label(tr("Position:"));
        for p in [VPos::Top, VPos::Middle, VPos::Bottom] {
            if ui.selectable_label(s.pos == p, p.name()).clicked() && s.pos != p { s.pos = p; *changed = true; }
        }
        ui.label(tr("Margin:"));
        *changed |= ui.add(egui::DragValue::new(&mut s.margin).range(0..=540).suffix(" px")).changed();
    });
    ui.horizontal(|ui| {
        ui.label(tr("Alignment:"));
        for a in HAlign::ALL {
            if ui.selectable_label(s.halign == a, a.name()).clicked() && s.halign != a { s.halign = a; *changed = true; }
        }
        ui.add_enabled_ui(s.halign != HAlign::Center, |ui| {
            ui.label(tr("Side margin:"));
            *changed |= ui.add(egui::DragValue::new(&mut s.margin_x).range(0..=960).suffix(" px")).changed();
        });
    });
    ui.horizontal(|ui| {
        ui.label(tr("Maximum width:"));
        *changed |= ui.add(egui::DragValue::new(&mut s.max_width).range(30..=100).suffix(" %")).on_hover_text(tr("Share of the image width the text may occupy")).changed();
        *changed |= ui.checkbox(&mut s.wrap, tr("Wrap long lines")).on_hover_text(tr("Wraps at spaces and balances the lines; otherwise the text size is reduced so everything fits")).changed();
    });
    ui.horizontal(|ui| {
        ui.label(tr("Shadow:"));
        *changed |= ui.add(egui::DragValue::new(&mut s.shadow).range(0..=30).suffix(" px")).on_hover_text(tr("0: no shadow")).changed();
        ui.add_enabled_ui(s.shadow > 0, |ui| {
            *changed |= ui.color_edit_button_srgb(&mut s.shadow_color).changed();
            ui.label(tr("Opacity:"));
            *changed |= ui.add(egui::DragValue::new(&mut s.shadow_opacity).range(0..=100).suffix(" %")).changed();
        });
    });
    ui.horizontal(|ui| {
        ui.label(tr("Box:"));
        *changed |= ui.add(egui::DragValue::new(&mut s.bg_opacity).range(0..=100).suffix(" %")).on_hover_text(tr("Opacity of the rectangle behind the text; 0: no box")).changed();
        ui.add_enabled_ui(s.bg_opacity > 0, |ui| {
            *changed |= ui.color_edit_button_srgb(&mut s.bg_color).changed();
            ui.label(tr("Margin:"));
            *changed |= ui.add(egui::DragValue::new(&mut s.bg_pad).range(0..=100).suffix(" px")).changed();
            ui.label(tr("Corners:"));
            *changed |= ui.add(egui::DragValue::new(&mut s.bg_radius).range(0..=100).suffix(" px")).changed();
        });
    });
    let shown = if s.font.is_empty() { tr("Default font").to_string() } else {
        bdauthor_render::common_fonts().into_iter().find(|(_, f)| *f == s.font).map(|(l, _)| l.to_string()).unwrap_or_else(|| s.font.clone())
    };
    let mut pick = false;
    egui::ComboBox::from_label(tr("Font")).selected_text(shown).show_ui(ui, |ui| {
        if ui.selectable_label(s.font.is_empty(), tr("Default font")).clicked() { s.font.clear(); *changed = true; }
        for (label, file) in bdauthor_render::common_fonts() {
            if ui.selectable_label(s.font == file, label).clicked() { s.font = file.to_string(); *changed = true; }
        }
        if ui.button(tr("Other file…")).clicked() { pick = true; ui.close_menu(); }
    });
    if pick {
        if let Some(p) = rfd::FileDialog::new().add_filter(tr("Fonts"), &["ttf", "otf"]).pick_file() {
            s.font = clean(&p);
            *changed = true;
        }
    }
    crate::help(ui, tr("Sizes given for a 1080-line video; they adapt automatically to 720."));
}

/// Onglet « Langues » de la fenetre Projet (audio et sous-titres) : liste des langues du projet (le rang d'une langue est son numero de
/// piste dans toutes les videos). Retourne vrai si quelque chose a change.
pub fn languages_ui(ui: &mut egui::Ui, m: &mut ProjectModel) -> bool {
    let mut changed = false;
    {
        ui.label(tr("Languages of the audio and subtitle tracks. Subtitles: the rank in the list sets the track number, identical in all videos."));
        let mut op: Option<(usize, i32)> = None; // (rang, -1 haut / 1 bas / 0 retirer)
        let n = m.languages.len();
        for (k, l) in m.languages.iter_mut().enumerate() {
            ui.push_id(("langue", l.id), |ui| {
                ui.horizontal(|ui| {
                    ui.label(format!("{}.", k + 1));
                    changed |= ui.add(egui::TextEdit::singleline(&mut l.name).desired_width(150.0)).changed();
                    ui.label(tr("code:"));
                    changed |= ui.add(egui::TextEdit::singleline(&mut l.code).desired_width(40.0)).changed();
                    if ui.add_enabled(k > 0, egui::Button::new("▲").small()).clicked() { op = Some((k, -1)); }
                    if ui.add_enabled(k + 1 < n, egui::Button::new("▼").small()).clicked() { op = Some((k, 1)); }
                    if ui.small_button(tr("Remove")).clicked() { op = Some((k, 0)); }
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
            ui.menu_button(tr("Add a language ▾"), |ui| {
                for code in ["fra", "eng", "deu", "spa", "ita", "por", "nld", "jpn", "zho", "rus", "ara", "kor"] {
                    let name = default_language_name(code);
                    if ui.button(&name).clicked() {
                        m.add_language(&name, code);
                        changed = true;
                        ui.close_menu();
                    }
                }
                if ui.button(tr("Other…")).clicked() { m.add_language(tr("New language"), "und"); changed = true; ui.close_menu(); }
            });
        });
        if m.languages.len() >= MAX_SUB_TRACKS {
            ui.label(egui::RichText::new(trf!("{} languages maximum.", MAX_SUB_TRACKS)).weak());
        }
        ui.separator();
        if m.languages.is_empty() {
            ui.label(egui::RichText::new(tr("No language: add some to choose the language of audio and subtitle tracks, and for the “Set…” buttons.")).weak());
        } else {
            crate::help(ui, tr("Each audio or subtitle track of a video chooses its language from this list. Subtitles: a video that lacks a language keeps an empty track at that place. Audio: each video has its own languages, in the order of its own track list, and an “audio language” button selects, in each video, its track in that language.\n\nChanging the order changes the track numbers: buttons follow their language."));
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
    /// encodage et langue avec lesquels le fichier a ete lu (relu s'ils changent), et cadence automatique
    charset: Charset,
    lang: String,
    auto_fps: Option<f64>,
    /// controle de qualite : seuils et problemes trouves (recalcules quand les repliques ou les seuils changent)
    lim: QualityLimits,
    issues: Option<Vec<QualityIssue>>,
    cue: usize,
    bg: u8,
    vh: u32,
    safe: bool,
    key: String,
    tex: Option<egui::TextureHandle>,
    /// fond « image de la video » : fichier video, extracteur d'images (cree au premier besoin) et image utilisee (instant en ms, taille)
    video_file: PathBuf,
    loader: Option<FrameLoader>,
    /// derniere image de la video affichee (gardee pendant que la suivante est extraite)
    last_img: Option<Arc<RgbaImage>>,
    /// video : sondage en cours et resultat (duree, cadence), frise de vignettes
    meta_rx: Option<std::sync::mpsc::Receiver<Result<VideoMeta, String>>>,
    meta: Option<VideoMeta>,
    tl: Timeline,
    /// lecture : (flux, instant de depart en ms, images recues)
    playing: Option<(FrameStream, i64, i64)>,
    /// instant affiche (ms) : libre (frise, lecture) ou lie a la replique choisie ; replique vue a l'image precedente
    t_ms: i64,
    free: bool,
    last_cue: usize,
    /// dessin de la replique en cours pendant la lecture : (cle, image)
    sprite: Option<(String, Option<Sprite>)>,
    /// zoom de l'image (1 = largeur de la fenetre)
    zoom: f32,
    /// style de la piste dans une fenetre a part (sinon en bas de l'apercu)
    style_detached: bool,
    /// zone de l'image a l'image precedente (pour savoir si la molette vise l'image)
    img_rect: egui::Rect,
}

impl SubPreview {
    /// Charge les repliques : un fichier tout de suite, une piste integree par ffmpeg dans un fil separe.
    pub fn new(video: usize, track: usize, t: &SubTrackCfg, video_file: &Path, base: &Path, auto_fps: Option<f64>) -> SubPreview {
        let mut p = SubPreview { video, track, raw: None, rx: None, shown: Vec::new(), shown_offset: None, charset: t.charset, lang: t.lang.clone(), auto_fps, lim: QualityLimits::default(), issues: None, cue: 0, bg: 0, vh: 1080, safe: false, key: String::new(), tex: None, video_file: video_file.to_path_buf(), loader: None, last_img: None, meta_rx: Some(probe_async(video_file, 960)), meta: None, tl: Timeline::new(48, "frise_sous_titres"), playing: None, t_ms: 0, free: false, last_cue: 0, sprite: None, zoom: 1.0, img_rect: egui::Rect::NOTHING, style_detached: false };
        match &t.source {
            SubSource::File(f) => p.raw = Some(load_cues_enc(&abs(base, f), t.fps.or(auto_fps), t.charset, &t.lang)),
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

impl SubPreview {
    /// Lecture / pause : la lecture demarre a l'instant affiche (au debut si l'on est a la fin), sur le fond « image de la video ».
    fn toggle_play(&mut self, vh: u32) {
        if self.playing.is_some() { self.playing = None; return; }
        let Some(m) = &self.meta else { return };
        let start = if self.t_ms as f64 / 1000.0 >= m.duration - 0.2 { 0 } else { self.t_ms.max(0) };
        if let Ok(tools) = Tools::locate() {
            if let Ok(fs) = FrameStream::start_fit(&tools, &self.video_file, start as f64 / 1000.0, (vh * 16 / 9, vh), true) {
                self.playing = Some((fs, start, 0));
                self.bg = 3;
                self.free = true;
                self.last_img = None;
            }
        }
    }
}

fn clock(ms: i64) -> String {
    let ms = ms.max(0);
    format!("{}:{:02}:{:02}.{:03}", ms / 3_600_000, ms / 60_000 % 60, ms / 1000 % 60, ms % 1000)
}

/// Dessin d'une replique (image rognee) et sa place sur une video de `vh` lignes.
struct Sprite { img: RgbaImage, x: u32, y: u32 }

/// La replique dessinee avec le style de la piste, placee comme dans la piste PGS finale (rien si elle est vide).
fn sub_sprite(r: &FontRenderer, text: &str, style: &SubStyle, pos: Option<CuePos>, base: &Path, vh: u32) -> Option<Sprite> {
    let vw = vh * 16 / 9;
    let mut st = bdauthor_core::pgs::scaled_style(&bdauthor_core::pgs::with_pos(style, pos), vh);
    if !st.font.is_empty() {
        let c = abs(base, &st.font);
        if c.is_file() { st.font = c.to_string_lossy().into_owned(); }
    }
    let img = r.subtitle(text, &st, vw)?;
    let (img, _, _) = bdauthor_core::pgs::crop_visible(&img, 12)?;
    let (w, h) = (img.width.min(vw), img.height.min(vh));
    let (x, y) = bdauthor_core::pgs::place(&st, (vw, vh), w, h);
    Some(Sprite { img, x, y })
}

/// Fond uni (16/9, `vh` lignes) : noir, gris, clair ; sombre derriere une image de video absente.
fn blank(vh: u32, bg: u8) -> RgbaImage {
    let vw = vh * 16 / 9;
    let g = if bg >= 3 { 70 } else { [0u8, 110, 225][bg.min(2) as usize] };
    let mut c = RgbaImage::new(vw, vh);
    for p in c.data.chunks_exact_mut(4) { p.copy_from_slice(&[g, g, g, 255]); }
    c
}

/// Pose la replique sur l'image.
fn blend(canvas: &mut RgbaImage, s: &Sprite) {
    let (w, h) = (s.img.width.min(canvas.width.saturating_sub(s.x)), s.img.height.min(canvas.height.saturating_sub(s.y)));
    for y in 0..h {
        for x in 0..w {
            let p = s.img.pixel(x, y);
            if p[3] == 0 { continue; }
            let a = p[3] as f32 / 255.0;
            let mut d = canvas.pixel(s.x + x, s.y + y);
            for c in 0..3 { d[c] = (p[c] as f32 * a + d[c] as f32 * (1.0 - a)).round() as u8; }
            canvas.set(s.x + x, s.y + y, d);
        }
    }
}

/// Une replique (aucune si `text` est vide) sur un fond uni ou sur une image de la video, de `vh` lignes (16/9).
fn render_preview(r: &FontRenderer, text: Option<&str>, style: &SubStyle, pos: Option<CuePos>, base: &Path, vh: u32, bg: u8, frame: Option<&RgbaImage>) -> egui::ColorImage {
    let vw = vh * 16 / 9;
    let mut canvas = match frame {
        Some(f) if f.width == vw && f.height == vh => f.clone(),
        _ => blank(vh, bg),
    };
    if let Some(sp) = text.and_then(|t| sub_sprite(r, t, style, pos, base, vh)) { blend(&mut canvas, &sp); }
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
    if let Some(rx) = &pv.meta_rx {
        match rx.try_recv() {
            Ok(r) => {
                pv.meta_rx = None;
                if let Ok(m) = r { pv.tl.start(&pv.video_file, m.duration, m.size); pv.meta = Some(m); }
            }
            Err(_) => ctx.request_repaint_after(std::time::Duration::from_millis(150)),
        }
    }
    if pv.tl.poll(ctx) { ctx.request_repaint_after(std::time::Duration::from_millis(100)); }
    let Some(t) = v.subs.get_mut(pv.track) else { return (false, false) };
    // encodage ou langue changes : le fichier est relu
    if let SubSource::File(f) = &t.source {
        if pv.charset != t.charset || pv.lang != t.lang {
            pv.charset = t.charset;
            pv.lang = t.lang.clone();
            pv.raw = Some(load_cues_enc(&abs(base, f), t.fps.or(pv.auto_fps), t.charset, &t.lang));
            pv.shown_offset = None;
        }
    }
    if let Some(Ok(raw)) = &pv.raw {
        let skey = t.offset_ms * 8 + t.strip_notes as i64 * 4 + t.file_positions as i64 * 2 + t.keep_format as i64;
        if pv.shown_offset != Some(skey) {
            let src = if t.strip_notes { strip_notes(raw) } else { raw.clone() };
            let src = if t.file_positions { src } else { clear_positions(&src) };
            let src = if t.keep_format { src } else { strip_format(&src) };
            pv.shown = merge(&shift(&src, t.offset_ms));
            pv.shown_offset = Some(skey);
            pv.issues = None;
            pv.cue = pv.cue.min(pv.shown.len().saturating_sub(1));
            pv.key.clear();
        }
    }
    let name = match &t.source {
        SubSource::File(f) => Path::new(f).file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| f.clone()),
        SubSource::Embedded(n) => trf!("embedded track no. {}", n + 1),
    };
    let mut open = true;
    // style de la piste : a droite de l'apercu ou dans sa propre fenetre
    let mut detach = pv.style_detached;
    if detach {
        let mut keep = true;
        crate::os_window(ctx, "style_piste_sous_titres", &trf!("Track style — {}", name), [520.0, 560.0], &mut keep, &mut |ui| {
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new(tr("Track style")).strong());
                if ui.button(tr("Dock in the preview")).on_hover_text(tr("Puts these settings back on the right of the preview window")).clicked() { detach = false; }
            });
            ui.separator();
            style_ui(ui, &mut t.style, &mut changed);
        });
        if !keep { detach = false; }
    }
    let docked = !detach;
    // par defaut, le style est a droite de l'apercu (ajustable en glissant son bord gauche)
    crate::os_window_docked(ctx, "apercu_sous_titres", tr("Subtitle preview"), [1240.0, 760.0], &mut open, docked.then_some(crate::Dock::Right(380.0)), &mut |ui, is_bottom| {
        if is_bottom {
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new(tr("Track style")).strong());
                if ui.button(tr("Undock")).on_hover_text(tr("Shows these settings in their own window, which you can place where you like")).clicked() { detach = true; }
            });
            style_ui(ui, &mut t.style, &mut changed);
            return;
        }
        ui.label(egui::RichText::new(name.as_str()).strong());
        match &pv.raw {
            None => { ui.horizontal(|ui| { ui.spinner(); ui.label(tr("Extracting the track…")); }); return; }
            Some(Err(e)) => { ui.colored_label(egui::Color32::from_rgb(230, 100, 90), e.as_str()); return; }
            Some(Ok(_)) => {}
        }
        let n = pv.shown.len();
        if n == 0 {
            ui.label(tr("No displayable cue (check the offset)."));
            return;
        }
        ui.horizontal(|ui| {
            if ui.button("<").on_hover_text(tr("Previous cue")).clicked() { pv.cue = pv.cue.saturating_sub(1); }
            ui.add(egui::Slider::new(&mut pv.cue, 0..=n - 1).custom_formatter(|v, _| format!("{} / {}", v as usize + 1, n)).custom_parser(|s| s.split('/').next().and_then(|a| a.trim().parse::<f64>().ok()).map(|v| v - 1.0)));
            if ui.button(">").on_hover_text(tr("Next cue")).clicked() { pv.cue = (pv.cue + 1).min(n - 1); }
            if ui.button(tr("Longest")).on_hover_text(tr("Goes to the longest cue, the most demanding for layout")).clicked() {
                pv.cue = (0..n).max_by_key(|&i| plain_text(&pv.shown[i].text).lines().map(|l| l.chars().count()).max().unwrap_or(0) + pv.shown[i].text.lines().count() * 8).unwrap_or(0);
            }
        });
        pv.cue = pv.cue.min(n - 1);
        if pv.issues.is_none() { pv.issues = Some(check_quality(&pv.shown, &pv.lim)); }
        let n_issues = pv.issues.as_ref().map_or(0, |v| v.len());
        egui::CollapsingHeader::new(if n_issues == 0 { tr("Quality check: no problem").to_string() } else { trf!("Quality check: {} problem(s)", n_issues) })
            .id_salt("controle_qualite").default_open(false).show(ui, |ui| {
                let mut lim = pv.lim;
                ui.horizontal_wrapped(|ui| {
                    ui.label(tr("Max reading speed:"));
                    ui.add(egui::DragValue::new(&mut lim.max_cps).range(5.0..=60.0).speed(0.5).suffix(tr(" chars/s")));
                    ui.label(tr("Max line length:"));
                    ui.add(egui::DragValue::new(&mut lim.max_line_chars).range(10..=100).suffix(tr(" chars")));
                    ui.label(tr("Max lines:"));
                    ui.add(egui::DragValue::new(&mut lim.max_lines).range(1..=6));
                    ui.label(tr("Duration:"));
                    ui.add(egui::DragValue::new(&mut lim.min_ms).range(0..=5000).speed(10.0).suffix(" ms"));
                    ui.label(tr("to"));
                    ui.add(egui::DragValue::new(&mut lim.max_ms).range(1000..=30000).speed(50.0).suffix(" ms"));
                    ui.label(tr("Min gap:"));
                    ui.add(egui::DragValue::new(&mut lim.min_gap_ms).range(0..=1000).speed(5.0).suffix(" ms"));
                    if ui.small_button(tr("Usual values")).clicked() { lim = QualityLimits::default(); }
                });
                if lim != pv.lim { pv.lim = lim; pv.issues = None; }
                if let Some(list) = &pv.issues {
                    egui::ScrollArea::vertical().id_salt("liste_problemes").max_height(130.0).show(ui, |ui| {
                        for i in list.iter().take(500) {
                            let at = pv.shown.get(i.index).map_or(0, |c| c.start_ms);
                            if ui.selectable_label(pv.cue == i.index, format!("{} · {} — {}", clock(at), i.kind.label(), i.detail)).clicked() { pv.cue = i.index; }
                        }
                        if list.len() > 500 { ui.label(egui::RichText::new(trf!("… and {} more", list.len() - 500)).weak()); }
                    });
                }
                crate::help(ui, tr("Checks the displayed cues (offset applied, simultaneous ones merged): reading speed, length and number of lines, display duration and gap with the next one. Click a problem to see the cue. The thresholds are the usual ones; they do not change the subtitles."));
            });
        let c = pv.shown[pv.cue].clone();
        ui.label(egui::RichText::new(format!("{} → {}{}", clock(c.start_ms), clock(c.end_ms), c.pos.map_or(String::new(), |p| trf!(" · position from file: {}", p.label())))).weak());
        ui.horizontal(|ui| {
            ui.label(tr("Background:"));
            for (i, l) in [tr("Black"), tr("Gray"), tr("Light"), tr("Video frame")].iter().enumerate() { if ui.selectable_label(pv.bg == i as u8, *l).clicked() { pv.bg = i as u8; } }
            ui.separator();
            ui.label(tr("Video:"));
            for h in [1080u32, 720] { if ui.selectable_label(pv.vh == h, format!("{h}p")).clicked() { pv.vh = h; pv.playing = None; } }
            ui.separator();
            ui.checkbox(&mut pv.safe, tr("Safe area"));
        });
        let play_vh = pv.vh.min(720);
        // les commandes de replique du haut ramenent a l'instant de la replique choisie
        if pv.cue != pv.last_cue { pv.free = false; pv.playing = None; pv.last_cue = pv.cue; }
        // instant lie a la replique choisie : un peu apres son apparition
        if !pv.free {
            let m = { let c0 = &pv.shown[pv.cue]; c0.start_ms + ((c0.end_ms - c0.start_ms) / 2).min(300) };
            pv.t_ms = m;
        }
        // lecture : la derniere image recue est affichee, l'instant suit le nombre d'images
        let mut play_img: Option<RgbaImage> = None;
        let mut ended = false;
        if let Some((fs, start, got)) = &mut pv.playing {
            loop {
                match fs.rx.try_recv() {
                    Ok(img) => { *got += 1; play_img = Some(img); }
                    Err(std::sync::mpsc::TryRecvError::Empty) => break,
                    Err(_) => { ended = true; break; }
                }
            }
            if play_img.is_some() {
                let fps = pv.meta.as_ref().map_or(25.0, |m| m.fps);
                pv.t_ms = *start + ((*got - 1) as f64 * 1000.0 / fps).round() as i64;
            }
        }
        if ended { pv.playing = None; }
        if pv.playing.is_some() { ctx.request_repaint_after(std::time::Duration::from_millis(8)); }
        // replique vue a cet instant : en temps libre, celle qui est affichee (aucune entre deux repliques)
        let at = pv.t_ms;
        let active = { let i = pv.shown.partition_point(|c| c.end_ms <= at); pv.shown.get(i).filter(|c| c.start_ms <= at).map(|_| i) };
        if pv.free { if let Some(i) = active { pv.cue = i; pv.last_cue = i; } }
        let c = pv.shown[pv.cue].clone();
        let cue_text: Option<(String, Option<CuePos>)> = if pv.free { active.map(|_| (c.text.clone(), c.pos)) } else { Some((c.text.clone(), c.pos)) };
        // fond « image de la video » : l'image reelle a l'instant affiche (extraite par ffmpeg en arriere-plan, gardee en memoire)
        let mut frame_note = None;
        let vh_eff = if pv.playing.is_some() { play_vh } else { pv.vh };
        let size = (vh_eff * 16 / 9, vh_eff);
        let mut frame_now: Option<Arc<RgbaImage>> = None;
        if pv.bg == 3 && pv.playing.is_none() {
            let ms = pv.t_ms;
            let loader = pv.loader.get_or_insert_with(|| FrameLoader::start(&pv.video_file, Fit::Cover, 6));
            let arrived = loader.poll();
            loader.want(ms, ms as f64 / 1000.0, size);
            if let Some(e) = loader.error(ms, size) {
                frame_note = Some(trf!("Frame unavailable: {}", e));
            } else if loader.get(ms, size).is_none() {
                frame_note = Some(tr("Extracting the video frame…").to_string());
                ctx.request_repaint_after(std::time::Duration::from_millis(150));
            }
            frame_now = loader.get(ms, size);
            if arrived { pv.key.clear(); }
            if let Some(f) = &frame_now { pv.last_img = Some(f.clone()); }
        }
        let shown_frame = if pv.bg == 3 { frame_now.clone().or_else(|| pv.last_img.clone().filter(|f| (f.width, f.height) == size)) } else { None };
        if let Some(img) = play_img {
            // lecture : l'image recue et la replique en cours, dessinees tout de suite (sans passer par la memoire d'images)
            let mut canvas = if pv.bg == 3 && (img.width, img.height) == size { img } else { blank(vh_eff, pv.bg) };
            if let Some((text, pos)) = &cue_text {
                let skey = format!("{}|{}|{:?}|{}|{:?}", pv.cue, vh_eff, t.style, text, pos);
                if pv.sprite.as_ref().map_or(true, |s| s.0 != skey) { pv.sprite = Some((skey, sub_sprite(renderer, text, &t.style, *pos, base, vh_eff))); }
                if let Some((_, Some(sp))) = &pv.sprite { blend(&mut canvas, sp); }
            }
            let ci = egui::ColorImage::from_rgba_unmultiplied([canvas.width as usize, canvas.height as usize], &canvas.data);
            match &mut pv.tex {
                Some(tx) => tx.set(ci, egui::TextureOptions::LINEAR),
                None => pv.tex = Some(ctx.load_texture("apercu_sous_titres", ci, egui::TextureOptions::LINEAR)),
            }
            pv.key = "lecture".to_string();
        } else if pv.playing.is_none() {
            let key = format!("{}|{}|{}|{:?}|{:?}|{}|{}", pv.t_ms, pv.bg, pv.vh, t.style, cue_text, frame_now.is_some(), shown_frame.is_some());
            if key != pv.key || pv.tex.is_none() {
                let img = render_preview(renderer, cue_text.as_ref().map(|c| c.0.as_str()), &t.style, cue_text.as_ref().and_then(|c| c.1), base, pv.vh, pv.bg, shown_frame.as_deref());
                pv.tex = Some(ctx.load_texture("apercu_sous_titres", img, egui::TextureOptions::LINEAR));
                pv.key = key;
            }
        }
        if let Some(n) = &frame_note { ui.label(egui::RichText::new(n).weak()); }
        // zoom : boutons, molette (ou Ctrl + molette) sur l'image ; glisser l'image pour la deplacer
        // largeur de la fenetre (le contenu est dans une zone defilante, donc sa largeur disponible ne veut rien dire)
        let fit_w = (ui.available_width() - 4.0).clamp(200.0, 2400.0);
        let ppp = ctx.pixels_per_point();
        ui.horizontal(|ui| {
            if ui.small_button("−").on_hover_text(tr("Zoom out")).clicked() { pv.zoom = (pv.zoom / 1.25).max(0.25); }
            if ui.small_button("+").on_hover_text(tr("Zoom in")).clicked() { pv.zoom = (pv.zoom * 1.25).min(8.0); }
            if ui.small_button(tr("Fit")).on_hover_text(tr("Whole image across the window width")).clicked() { pv.zoom = 1.0; }
            if ui.small_button("1:1").on_hover_text(tr("One video pixel = one screen pixel")).clicked() { pv.zoom = ((pv.vh * 16 / 9) as f32 / ppp / fit_w).clamp(0.25, 8.0); }
            ui.label(format!("{:.0} %", pv.zoom * fit_w * ppp / (pv.vh * 16 / 9) as f32 * 100.0)).on_hover_text(tr("100% = actual video size"));
            ui.label(egui::RichText::new(tr("Wheel: zoom · drag: pan")).weak());
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
            // l'image occupe toute la hauteur laissee par la zone du bas (que l'on regle en glissant son bord)
            let max_h = (ui.available_height() - 28.0 - if pv.meta.is_some() { 200.0 } else { 40.0 }).max(80.0);
            let out = egui::ScrollArea::both().id_salt("apercu_zoom").max_height(max_h).auto_shrink([false, false]).show(ui, |ui| {
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
        // lecture et frise de la video, sous l'image : « Lecture » ou un clic sur la frise passent au fond « image de la video »
        if !ctx.wants_keyboard_input() && ctx.input(|i| i.key_pressed(egui::Key::Space)) { pv.toggle_play(play_vh); }
        ui.horizontal(|ui| {
            let label = if pv.playing.is_some() { tr("■ Pause") } else { tr("▶ Play") };
            let ready = pv.meta.is_some();
            if ui.add_enabled(ready, egui::Button::new(label).min_size(egui::vec2(70.0, 26.0)).fill(egui::Color32::from_rgb(44, 93, 135))).clicked() { pv.toggle_play(play_vh); }
            let total = pv.meta.as_ref().map_or(String::new(), |m| format!(" / {}", clock((m.duration * 1000.0) as i64)));
            ui.label(egui::RichText::new(format!("{}{}", clock(pv.t_ms), total)).monospace());
            if ready { ui.label(egui::RichText::new(tr("Space: play / pause · playback without sound")).small().weak()); }
            else if pv.meta_rx.is_some() { ui.spinner(); ui.label(tr("Reading the video…")); }
        });
        if let Some(dur) = pv.meta.as_ref().map(|m| m.duration) {
            let cfg = TimelineCfg { hint: tr("Wheel: zoom · Shift + wheel: scroll · click or drag: choose the time"), below: 8.0, follow: pv.playing.is_some() };
            let head = pv.t_ms as f64 / 1000.0;
            let (shown, sel) = (&pv.shown, pv.cue);
            // les repliques de la piste en reperes sous les vignettes (la replique choisie en orange)
            let out = pv.tl.show(ui, dur, head, &cfg, |p, g| {
                let (t0, span) = (g.start(), g.span());
                let first = shown.partition_point(|c| (c.end_ms as f64 / 1000.0) < t0);
                for (i, c) in shown.iter().enumerate().skip(first) {
                    let (s0, e0) = (c.start_ms as f64 / 1000.0, c.end_ms as f64 / 1000.0);
                    if s0 > t0 + span { break; }
                    let (x0, x1) = (g.x_of(s0), g.x_of(e0).max(g.x_of(s0) + 1.5));
                    let r = egui::Rect::from_min_max(egui::pos2(x0, g.strip.max.y - 6.0), egui::pos2(x1, g.strip.max.y));
                    p.rect_filled(r, 0.0, if i == sel { egui::Color32::from_rgb(255, 170, 60) } else { egui::Color32::from_rgb(120, 200, 255) });
                }
            });
            if let Some((resp, geo)) = out {
                if let Some(q) = resp.interact_pointer_pos() {
                    if resp.clicked() || resp.dragged() {
                        pv.playing = None;
                        pv.bg = 3;
                        pv.free = true;
                        pv.t_ms = (geo.t_at(q.x) * 1000.0).round() as i64;
                        ctx.request_repaint();
                    }
                }
            }
        }
        crate::help(ui, tr("Preview with the track's font and style (offset applied, simultaneous cues merged); the player will show the same drawing."));
    });
    pv.style_detached = detach;
    (changed, open)
}
