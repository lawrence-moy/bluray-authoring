//! Modele editable d'un projet (pour l'interface graphique) : lecture/ecriture du JSON de projet et validation.
//! Les champs inconnus du fichier d'origine sont conserves a l'enregistrement.
use crate::i18n::tr;
use crate::trf;
use crate::design::Designs;
use crate::json::{self, Value};
use crate::project::{MAX_CHAPTERS, MAX_VIDEOS};
use crate::subtitle::{Language, SubTrackCfg};
use std::path::Path;

pub const PRESETS: [&str; 9] = ["ultrafast", "superfast", "veryfast", "faster", "fast", "medium", "slow", "slower", "veryslow"];

#[derive(Debug, Clone, PartialEq)]
pub struct ChapterEntry {
    /// Identifiant stable dans la liste de sa video (les vignettes s'y lient par lui, pas par le rang) ; jamais 0.
    pub id: u32,
    pub t: f64,
    pub title: String,
}

/// Plus petite et plus grande taille personnalisee d'un menu.
pub const MENU_SIZE_MIN: (u32, u32) = (640, 360);
pub const MENU_SIZE_MAX: (u32, u32) = (1920, 1080);

/// Taille personnalisee ecrite « LARGEURxHAUTEUR » (valeurs paires, bornees) ; `None` pour « 1080 », « video » ou un texte invalide.
pub fn parse_menu_size(t: &str) -> Option<(u32, u32)> {
    let (w, h) = t.split_once(['x', 'X', '×'])?;
    let (w, h): (u32, u32) = (w.trim().parse().ok()?, h.trim().parse().ok()?);
    Some(clamp_menu_size(w, h))
}

/// Borne et arrondit au pair inferieur une taille de menu.
pub fn clamp_menu_size(w: u32, h: u32) -> (u32, u32) {
    ((w.clamp(MENU_SIZE_MIN.0, MENU_SIZE_MAX.0)) & !1, (h.clamp(MENU_SIZE_MIN.1, MENU_SIZE_MAX.1)) & !1)
}

/// Taille des menus d'apres `menu.taille` (absent ou « 1080 » : 1920x1080 ; « video » : celle de la premiere video ; « LxH » : personnalisee).
pub fn menu_size_for(taille: Option<&str>, video: (u32, u32)) -> (u32, u32) {
    match taille {
        Some("video") => video,
        Some(t) => parse_menu_size(t).unwrap_or((1920, 1080)),
        None => (1920, 1080),
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct VideoEntry {
    pub file: String,
    pub title: String,
    pub thumb_s: Option<f64>,
    pub chapters: Vec<ChapterEntry>,
    pub auto_s: Option<f64>,
    /// Pistes de sous-titres PGS.
    pub subs: Vec<crate::subtitle::SubTrackCfg>,
    /// Pistes audio choisies (vide : la premiere piste audio du fichier, dans la langue du disque).
    pub audio: Vec<crate::audio::AudioTrackCfg>,
    /// Une image cle exacte a chaque chapitre a l'encodage (JSON `chapitres_exacts`) : le chapitre demarre a l'image choisie.
    pub exact_chapters: bool,
    raw: Value,
}

impl VideoEntry {
    /// Chapitres tels que le constructeur les definit, sans calage sur les images d'acces : [(secondes, nom)].
    /// Vide s'il y a moins de 2 chapitres. `duration` (secondes) est necessaire pour les chapitres automatiques ; sans lui ils sont ignores.
    pub fn chapter_list(&self, duration: Option<f64>) -> Vec<(f64, String)> {
        let mut spec: Vec<(f64, String)> = self.chapters.iter().map(|c| (c.t, c.title.clone())).collect();
        if spec.is_empty() {
            if let (Some(a), Some(d)) = (self.auto_s.filter(|a| *a > 0.0), duration) {
                let n = (d / a).floor() as i64 + 1;
                spec = (0..n).map(|k| k as f64 * a).filter(|&t| t < d - a / 2.0).map(|t| (t, String::new())).collect();
            }
        }
        if spec.is_empty() { return vec![]; }
        if spec.iter().all(|(t, _)| *t > 0.0) { spec.insert(0, (0.0, String::new())); }
        spec.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));
        let mut out: Vec<(f64, String)> = Vec::new();
        for (t, name) in spec {
            if duration.map_or(false, |d| t >= d) || out.iter().any(|(u, _)| (*u - t).abs() < 1e-9) { continue; }
            out.push((t, name));
        }
        if out.len() < 2 { return vec![]; }
        out.into_iter().enumerate().map(|(j, (t, n))| (t, if n.is_empty() { format!("Chapitre {}", j + 1) } else { n })).collect()
    }

    /// Identifiant libre pour un nouveau chapitre.
    pub fn next_chapter_id(&self) -> u32 { self.chapters.iter().map(|c| c.id).max().unwrap_or(0) + 1 }

    /// Ajoute un chapitre a l'instant `t` (secondes) et rend son identifiant ; s'il y en a deja un a cet instant (a 10 ms pres),
    /// c'est lui qui est rendu.
    pub fn add_chapter(&mut self, t: f64, title: &str) -> u32 {
        if let Some(c) = self.chapters.iter().find(|c| (c.t - t).abs() < 0.01) { return c.id; }
        let id = self.next_chapter_id();
        self.chapters.push(ChapterEntry { id, t, title: title.into() });
        id
    }

    pub fn chapter(&self, id: u32) -> Option<&ChapterEntry> { self.chapters.iter().find(|c| c.id == id) }

    /// Donne un identifiant a chaque chapitre qui n'en a pas (ou qui reprend celui d'un autre).
    fn fix_chapter_ids(&mut self) {
        let mut seen = std::collections::HashSet::new();
        let mut next = self.chapters.iter().map(|c| c.id).max().unwrap_or(0) + 1;
        for c in &mut self.chapters {
            if c.id == 0 || !seen.insert(c.id) { c.id = next; seen.insert(next); next += 1; }
        }
    }

    pub fn new(file: &str) -> Self {
        VideoEntry { file: file.into(), title: String::new(), thumb_s: None, chapters: vec![], auto_s: None, subs: vec![], audio: vec![], exact_chapters: false, raw: Value::Obj(vec![]) }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct ProjectModel {
    pub title: String,
    pub lang: String,
    pub videos: Vec<VideoEntry>,
    pub crf: u32,
    pub preset: String,
    /// Viser un debit video qui remplit le disque (capacite et marge des reglages de l'image ISO) au lieu du CRF.
    pub fill_disc: bool,
    /// Recopier sans reencodage les videos deja conformes au Blu-ray.
    pub copy_conform: bool,
    pub bg: String, // chemin ; vide = fond genere
    /// Sequence d'ouverture : video jouee a l'insertion du disque (vide = aucune).
    pub intro: String,
    /// Fond de menu anime : video jouee en boucle sous les menus (vide = aucun).
    pub bg_video: String,
    /// Garder le son de la video de fond (quand il n'y a pas de musique).
    pub bg_video_audio: bool,
    /// Musique des menus (vide = aucune).
    pub music: String,
    pub bg_seconds: f64,
    pub menu_1080: bool,
    /// Taille personnalisee des menus (largeur, hauteur) : prend le pas sur `menu_1080` (JSON `menu.taille` « 1280x720 »).
    pub menu_custom: Option<(u32, u32)>,
    pub loop_menu: bool,
    pub cache: bool,
    /// Mises en page personnalisees (cle « mise_en_page »).
    pub designs: Designs,
    /// Langues de sous-titres du projet (cle « langues ») : le rang d'une langue est son numero de piste sur le disque.
    pub languages: Vec<Language>,
    /// Generation de l'image ISO (cle « iso »).
    pub iso: crate::iso::IsoSettings,
    /// Jaquette du boitier (cle « jaquette »).
    pub cover: crate::cover::Cover,
    /// Etiquette de disque (cle « etiquette »).
    pub label: crate::cover::DiscLabel,
    raw: Value,
}

impl Default for ProjectModel {
    fn default() -> Self {
        ProjectModel {
            title: "Mes vidéos".into(), lang: "fra".into(), videos: vec![], crf: 18, preset: "medium".into(), fill_disc: false, copy_conform: false, bg: String::new(), intro: String::new(), bg_video: String::new(), bg_video_audio: false, music: String::new(), bg_seconds: 5.0,
            menu_1080: true, menu_custom: None, loop_menu: false, cache: true, designs: Designs::default(), languages: vec![], iso: Default::default(), cover: Default::default(), label: Default::default(), raw: Value::Obj(vec![]),
        }
    }
}

fn set(obj: &mut Value, key: &str, v: Option<Value>) {
    if !matches!(obj, Value::Obj(_)) { *obj = Value::Obj(vec![]); }
    let Value::Obj(o) = obj else { return };
    match (o.iter().position(|(k, _)| k == key), v) {
        (Some(i), Some(v)) => o[i].1 = v,
        (Some(i), None) => { o.remove(i); }
        (None, Some(v)) => o.push((key.to_string(), v)),
        (None, None) => {}
    }
}
fn s(x: &str) -> Value { Value::Str(x.to_string()) }
fn n(x: f64) -> Value { Value::Num(x) }

impl ProjectModel {
    pub fn from_json(text: &str) -> Result<Self, String> {
        let v = json::parse(text).map_err(|e| e.to_string())?;
        let mut m = ProjectModel { raw: v.clone(), ..Default::default() };
        m.title = v.get("title").and_then(Value::as_str).unwrap_or("Menu").into();
        m.lang = v.get("language").and_then(Value::as_str).unwrap_or("und").into();
        for x in v.get("videos").and_then(Value::as_array).ok_or(tr("“videos” field missing"))? {
            let mut e = VideoEntry::new(x.get("file").and_then(Value::as_str).ok_or(tr("video without “file”"))?);
            e.raw = x.clone();
            e.title = x.get("title").and_then(Value::as_str).unwrap_or("").into();
            e.thumb_s = x.get("thumbnail_s").and_then(Value::as_f64);
            e.auto_s = x.get("auto_chapters_s").and_then(Value::as_f64);
            e.exact_chapters = x.get("exact_chapters").and_then(Value::as_bool).unwrap_or(false);
            e.subs = crate::subtitle::tracks_from_value(x.get("subtitles"))?;
            e.audio = crate::audio::tracks_from_value(x.get("audio_tracks"))?;
            for c in x.get("chapters").and_then(Value::as_array).unwrap_or(&[]) {
                e.chapters.push(match c {
                    Value::Obj(_) => ChapterEntry { id: c.get("id").and_then(Value::as_f64).map_or(0, |x| x.max(0.0) as u32), t: c.get("t").and_then(Value::as_f64).ok_or(tr("chapter without “t”"))?, title: c.get("title").and_then(Value::as_str).unwrap_or("").into() },
                    _ => ChapterEntry { id: 0, t: c.as_f64().ok_or(tr("chapter: number expected"))?, title: String::new() },
                });
            }
            e.fix_chapter_ids();
            m.videos.push(e);
        }
        if let Some(e) = v.get("encoding") {
            if let Some(c) = e.get("crf").and_then(Value::as_i64) { m.crf = c.clamp(0, 51) as u32; }
            if let Some(p) = e.get("preset").and_then(Value::as_str) { m.preset = p.into(); }
            m.fill_disc = e.get("fill_disc").and_then(Value::as_bool).unwrap_or(false);
            m.copy_conform = e.get("copy_if_conforming").and_then(Value::as_bool).unwrap_or(false);
        }
        if let Some(menu) = v.get("menu") {
            m.bg = menu.get("background").and_then(Value::as_str).unwrap_or("").into();
            m.intro = menu.get("intro").and_then(Value::as_str).unwrap_or("").into();
            m.bg_video = menu.get("background_video").and_then(Value::as_str).unwrap_or("").into();
            m.bg_video_audio = menu.get("background_video_audio").and_then(Value::as_bool).unwrap_or(false);
            m.music = menu.get("music").and_then(Value::as_str).unwrap_or("").into();
            m.bg_seconds = menu.get("background_seconds").and_then(Value::as_f64).unwrap_or(5.0);
            let taille = menu.get("size").and_then(Value::as_str);
            m.menu_custom = taille.and_then(parse_menu_size);
            m.menu_1080 = taille != Some("video");
            m.loop_menu = menu.get("loop").and_then(Value::as_bool).unwrap_or(false);
            m.cache = menu.get("use_cache").and_then(Value::as_bool).unwrap_or(true);
        }
        if let Some(d) = v.get("layout") { m.designs = Designs::from_value(d)?; }
        m.languages = crate::subtitle::languages_from_value(v.get("languages"))?;
        m.iso = crate::iso::IsoSettings::from_value(v.get("iso"))?;
        m.cover = crate::cover::Cover::from_value(v.get("cover"))?;
        m.label = crate::cover::DiscLabel::from_value(v.get("label"))?;
        m.sync_chapters();
        Ok(m)
    }

    pub fn to_value(&self) -> Value {
        let mut root = self.raw.clone();
        set(&mut root, "title", Some(s(&self.title)));
        set(&mut root, "language", Some(s(&self.lang)));
        let linked = self.designs.linked_chapters();
        let vids: Vec<Value> = self.videos.iter().enumerate().map(|(vi, e)| {
            let mut o = e.raw.clone();
            set(&mut o, "file", Some(s(&e.file)));
            set(&mut o, "title", if e.title.is_empty() { None } else { Some(s(&e.title)) });
            set(&mut o, "thumbnail_s", e.thumb_s.map(n));
            set(&mut o, "auto_chapters_s", e.auto_s.map(n));
            set(&mut o, "exact_chapters", e.exact_chapters.then_some(Value::Bool(true)));
            let subs: Vec<SubTrackCfg> = e.subs.iter().map(|t| self.synced_track(t)).collect();
            set(&mut o, "subtitles", crate::subtitle::tracks_to_value(&subs));
            let audio: Vec<crate::audio::AudioTrackCfg> = e.audio.iter().map(|t| { let mut t = t.clone(); if let Some(l) = self.language(t.lang_id) { t.lang = l.code.clone(); } t }).collect();
            set(&mut o, "audio_tracks", crate::audio::tracks_to_value(&audio));
            set(&mut o, "chapters", if e.chapters.is_empty() { None } else {
                // l'identifiant n'est ecrit que pour les chapitres auxquels une vignette est liee
                Some(Value::Arr(e.chapters.iter().map(|c| {
                    let used = linked.contains(&(vi, c.id));
                    if c.title.is_empty() && !used { return n(c.t); }
                    let mut o = vec![("t".to_string(), n(c.t))];
                    if !c.title.is_empty() { o.push(("title".into(), s(&c.title))); }
                    if used { o.push(("id".into(), n(c.id as f64))); }
                    Value::Obj(o)
                }).collect()))
            });
            o
        }).collect();
        set(&mut root, "videos", Some(Value::Arr(vids)));
        let mut enc = root.get("encoding").cloned().unwrap_or(Value::Obj(vec![]));
        set(&mut enc, "crf", Some(n(self.crf as f64)));
        set(&mut enc, "preset", Some(s(&self.preset)));
        set(&mut enc, "fill_disc", if self.fill_disc { Some(Value::Bool(true)) } else { None });
        set(&mut enc, "copy_if_conforming", if self.copy_conform { Some(Value::Bool(true)) } else { None });
        set(&mut root, "encoding", Some(enc));
        let mut menu = root.get("menu").cloned().unwrap_or(Value::Obj(vec![]));
        set(&mut menu, "background", if self.bg.is_empty() { None } else { Some(s(&self.bg)) });
        set(&mut menu, "intro", if self.intro.is_empty() { None } else { Some(s(&self.intro)) });
        set(&mut menu, "background_video", if self.bg_video.is_empty() { None } else { Some(s(&self.bg_video)) });
        set(&mut menu, "background_video_audio", if self.bg_video_audio { Some(Value::Bool(true)) } else { None });
        set(&mut menu, "music", if self.music.is_empty() { None } else { Some(s(&self.music)) });
        set(&mut menu, "background_seconds", Some(n(self.bg_seconds)));
        set(&mut menu, "size", Some(s(&match self.menu_custom { Some((w, h)) => format!("{w}x{h}"), None if self.menu_1080 => "1080".to_string(), None => "video".to_string() })));
        set(&mut menu, "loop", Some(Value::Bool(self.loop_menu)));
        // anciens reglages des menus automatiques : retires du fichier
        set(&mut menu, "return_to", None);
        set(&mut menu, "par_page", None);
        set(&mut menu, "use_cache", Some(Value::Bool(self.cache)));
        set(&mut root, "menu", Some(menu));
        set(&mut root, "languages", crate::subtitle::languages_to_value(&self.languages));
        set(&mut root, "iso", self.iso.to_value());
        set(&mut root, "cover", self.cover.to_value());
        set(&mut root, "label", self.label.to_value());
        set(&mut root, "layout", if self.designs.is_empty() { None } else { Some(self.designs.to_value()) });
        root
    }

    pub fn to_json(&self) -> String { self.to_value().to_pretty() }

    /// Images de video posees sur la jaquette et l'etiquette : (cle de `cover::frame_key`, rang de la video, image), sans doublon.
    pub fn cover_frames(&self) -> Vec<(String, usize, crate::design::VideoFrame)> {
        let mut out: Vec<(String, usize, crate::design::VideoFrame)> = Vec::new();
        for it in self.cover.items.iter().chain(self.label.items.iter()) {
            let crate::cover::CoverKind::VideoImage { frame } = &it.kind else { continue };
            let (Some(k), Some(v)) = (crate::cover::frame_key(frame), frame.video) else { continue };
            if v < self.videos.len() && !out.iter().any(|o| o.0 == k) { out.push((k, v, *frame)); }
        }
        out
    }

    /// Valeurs des champs automatiques de la jaquette et de l'etiquette (voir `cover::FIELDS`) ; `durations` : duree de chaque
    /// fichier video connue.
    pub fn cover_vars(&self, durations: &std::collections::HashMap<String, f64>, year: i32) -> std::collections::BTreeMap<String, String> {
        let mut total = 0.0;
        let mut known = true;
        let mut lines = Vec::new();
        for (i, v) in self.videos.iter().enumerate() {
            let t = crate::editor::video_title(self, i);
            match durations.get(&v.file) {
                Some(d) => { total += d; lines.push(format!("{}. {}  —  {}", i + 1, t, crate::cover::clock(*d))); }
                None => { known = false; lines.push(format!("{}. {}", i + 1, t)); }
            }
        }
        let mut m = std::collections::BTreeMap::new();
        m.insert("titre".to_string(), self.title.clone());
        m.insert("liste_videos".to_string(), lines.join("\n"));
        m.insert("nb_videos".to_string(), self.videos.len().to_string());
        m.insert("duree_totale".to_string(), if known && !self.videos.is_empty() { crate::cover::clock(total) } else { String::new() });
        m.insert("nb_chapitres".to_string(), self.videos.iter().map(|v| v.chapters.len()).sum::<usize>().to_string());
        m.insert("annee".to_string(), year.to_string());
        m
    }

    /// Valeurs des tableaux d'informations de la jaquette (voir `cover::InfoTable`), pour chaque video : titre, duree (en minutes),
    /// langues audio, sous-titres, format de l'image sur le disque et son. `durations` et `sizes` (hauteur, images/s) : ce que
    /// l'on sait de chaque fichier video ; une valeur inconnue reste vide. Sans langue ni sous-titre : « — ».
    pub fn cover_info_vars(&self, durations: &std::collections::HashMap<String, f64>, sizes: &std::collections::HashMap<String, (u32, f64)>) -> std::collections::BTreeMap<String, String> {
        use crate::cover::{info_key, InfoField as F, INFO_COUNT_KEY};
        let mut m = std::collections::BTreeMap::new();
        m.insert(INFO_COUNT_KEY.to_string(), self.videos.len().to_string());
        // nom d'une langue : celle de la liste du projet, sinon son code
        let lang = |id: u32, code: &str| -> Option<String> {
            if let Some(l) = self.language(id) { return Some(l.name.clone()); }
            let code = code.trim();
            if code.is_empty() { return None; }
            Some(self.languages.iter().find(|l| l.code.eq_ignore_ascii_case(code)).map_or_else(|| code.to_string(), |l| l.name.clone()))
        };
        let list = |names: Vec<String>| {
            let mut v: Vec<String> = Vec::new();
            for n in names { if !v.contains(&n) { v.push(n); } }
            if v.is_empty() { "—".to_string() } else { v.join(", ") }
        };
        for (i, v) in self.videos.iter().enumerate() {
            m.insert(info_key(i, F::Title), crate::editor::video_title(self, i));
            if let Some(d) = durations.get(&v.file) {
                let d = d.max(0.0);
                m.insert(info_key(i, F::Duration), if d >= 60.0 { format!("{} min", (d / 60.0).round()) } else { format!("{} s", d.round()) });
            }
            // sans piste choisie : la premiere piste du fichier, dans la langue du disque
            let audio: Vec<String> = if v.audio.is_empty() { lang(0, &self.lang).into_iter().collect() } else { v.audio.iter().filter_map(|t| lang(t.lang_id, &t.lang)).collect() };
            m.insert(info_key(i, F::Languages), list(audio));
            m.insert(info_key(i, F::Subtitles), list(v.subs.iter().filter_map(|t| lang(t.lang_id, &t.lang)).collect()));
            if let Some(&(h, fps)) = sizes.get(&v.file) {
                let t = crate::ffmpeg::pick_target(0, h, fps);
                let scan = if matches!(t.scan, crate::ffmpeg::Scan::Tff | crate::ffmpeg::Scan::Bff) { "i" } else { "p" };
                m.insert(info_key(i, F::Format), format!("HD {}{scan} · 16/9", t.height));
            }
            // l'audio du disque est toujours en Dolby Digital (AC3) stereo
            m.insert(info_key(i, F::Sound), "Dolby Digital 2.0".to_string());
        }
        m
    }

    /// Aligne les vignettes liees a un chapitre sur l'instant de ce chapitre (a appeler apres toute modification des chapitres ou
    /// de la video d'une vignette). Rend vrai si une vignette a change.
    pub fn sync_chapters(&mut self) -> bool {
        let videos = &self.videos;
        self.designs.sync_chapters(&|v, id| videos.get(v).and_then(|x| x.chapter(id)).map(|c| c.t))
    }

    /// Lie chaque vignette a instant libre au chapitre de sa video qui tombe a cet instant (a 10 ms pres), s'il y en a un.
    pub fn link_chapters_by_time(&mut self) {
        let videos = &self.videos;
        for d in &mut self.designs.free {
            for it in &mut d.items {
                if let crate::design::Kind::Slot { video: Some(v), start: Some(t), chapter: chapter @ None, .. } = &mut it.kind {
                    if let Some(c) = videos.get(*v).and_then(|x| x.chapters.iter().find(|c| (c.t - *t).abs() < 0.01)) { *chapter = Some(c.id); }
                }
            }
        }
    }

    /// Nombre de vignettes liees au chapitre `id` de la video `video`.
    pub fn chapter_uses(&self, video: usize, id: u32) -> usize { self.designs.chapter_uses(video, id) }

    /// Piste dont le code de langue est celui de sa langue du projet (si elle en a une).
    fn synced_track(&self, t: &SubTrackCfg) -> SubTrackCfg {
        let mut t = t.clone();
        if let Some(l) = self.language(t.lang_id) { t.lang = l.code.clone(); }
        t
    }

    pub fn language(&self, id: u32) -> Option<&Language> {
        if id == 0 { None } else { self.languages.iter().find(|l| l.id == id) }
    }

    /// Rang (= numero de piste, 0-based) d'une langue du projet.
    pub fn language_rank(&self, id: u32) -> Option<usize> {
        self.languages.iter().position(|l| l.id == id)
    }

    /// Nom affiche d'une langue (ou un texte d'avertissement si elle n'existe plus).
    pub fn language_name(&self, id: u32) -> String {
        self.language(id).map_or_else(|| tr("(language removed)").to_string(), |l| l.name.clone())
    }

    /// Ajoute une langue (identifiant neuf) et retourne cet identifiant.
    pub fn add_language(&mut self, name: &str, code: &str) -> u32 {
        let id = crate::subtitle::next_language_id(&self.languages);
        self.languages.push(Language { id, name: name.into(), code: code.into() });
        id
    }

    /// Retire une langue : les pistes qui l'utilisaient n'ont plus de langue, les boutons qui la reglaient ne reglent plus rien.
    pub fn remove_language(&mut self, id: u32) {
        self.languages.retain(|l| l.id != id);
        for v in &mut self.videos {
            for t in &mut v.subs { if t.lang_id == id { t.lang_id = 0; } }
            for t in &mut v.audio { if t.lang_id == id { t.lang_id = 0; } }
        }
        self.for_each_item(&mut |it| {
            if it.sub == crate::design::SubChoice::Lang(id) { it.sub = crate::design::SubChoice::Keep; }
            // destination « regler la langue audio » sur cette langue : a rechoisir
            if let Some(t) = it.targets_mut() {
                if matches!(*t, Some(crate::design::Target::Audio(l, _)) if l == id) { *t = None; }
            }
        });
    }

    /// Change l'ordre des langues (donc les numeros de piste) : la langue `from` passe a la position `to`.
    pub fn move_language(&mut self, from: usize, to: usize) {
        if from < self.languages.len() && to < self.languages.len() {
            let l = self.languages.remove(from);
            self.languages.insert(to, l);
        }
    }

    fn for_each_item(&mut self, f: &mut dyn FnMut(&mut crate::design::Item)) {
        let d = &mut self.designs;
        for pg in d.free.iter_mut() {
            for it in &mut pg.items { f(it); }
        }
    }

    /// Echange deux videos de place (vignettes et destinations suivent leur video).
    pub fn swap_videos(&mut self, a: usize, b: usize) {
        self.videos.swap(a, b);
        self.designs.swap_videos(a, b);
        self.remap_cover_videos(|v| if v == a { b } else if v == b { a } else { v });
    }

    /// Retire une video (ses vignettes et images de jaquette/etiquette restent, sans video).
    pub fn remove_video(&mut self, i: usize) {
        self.videos.remove(i);
        self.designs.remove_video(i);
        self.remap_cover_videos(|v| if v == i { usize::MAX } else if v > i { v - 1 } else { v });
        for it in self.cover.items.iter_mut().chain(self.label.items.iter_mut()) {
            if let crate::cover::CoverKind::VideoImage { frame } = &mut it.kind {
                if frame.video == Some(usize::MAX) { frame.video = None; }
            }
        }
    }

    /// Applique `f` au rang de video de chaque image de video de la jaquette et de l'etiquette.
    fn remap_cover_videos(&mut self, mut f: impl FnMut(usize) -> usize) {
        for it in self.cover.items.iter_mut().chain(self.label.items.iter_mut()) {
            match &mut it.kind {
                crate::cover::CoverKind::VideoImage { frame } => { if let Some(v) = frame.video { frame.video = Some(f(v)); } }
                // tableau d'informations : ses videos suivent, une video retiree disparait de la liste
                crate::cover::CoverKind::InfoTable { table } => {
                    let vids: Vec<usize> = table.videos.iter().map(|&v| f(v)).collect();
                    table.videos = vids.into_iter().filter(|&v| v != usize::MAX).collect();
                }
                _ => {}
            }
        }
    }

    /// Problemes bloquants ou suspects (chemins relatifs resolus depuis `base`, le dossier du fichier de projet).
    pub fn validate(&self, base: &Path) -> Vec<String> {
        let mut p = Vec::new();
        if self.videos.is_empty() { p.push(tr("No video.").into()); }
        if self.videos.len() > MAX_VIDEOS { p.push(trf!("{} videos: {} at most.", self.videos.len(), MAX_VIDEOS)); }
        if self.lang.len() != 3 || !self.lang.is_ascii() { p.push(tr("The language must be 3 letters (e.g. fra, eng, und).").into()); }
        if !PRESETS.contains(&self.preset.as_str()) { p.push(trf!("Unknown x264 preset: {}.", self.preset)); }
        if self.crf > 51 { p.push(tr("CRF: 0 to 51.").into()); }
        let abs = |f: &str| { let q = Path::new(f); if q.is_absolute() { q.to_path_buf() } else { base.join(q) } };
        for (i, v) in self.videos.iter().enumerate() {
            if !abs(&v.file).is_file() { p.push(trf!("Video {}: file not found ({}).", i + 1, v.file)); }
            if v.chapters.len() > MAX_CHAPTERS { p.push(trf!("Video {}: {} chapters at most.", i + 1, MAX_CHAPTERS)); }
            if v.chapters.iter().any(|c| c.t < 0.0) { p.push(trf!("Video {}: negative chapter time.", i + 1)); }
            if v.auto_s.map_or(false, |a| a <= 0.0) { p.push(trf!("Video {}: automatic chapter interval must be > 0.", i + 1)); }
            if v.subs.len() > crate::film::MAX_SUB_TRACKS { p.push(trf!("Video {}: {} subtitle tracks at most.", i + 1, crate::film::MAX_SUB_TRACKS)); }
            for (k, st) in v.subs.iter().enumerate() {
                for m in self.synced_track(st).problems() { p.push(trf!("Video {}, subtitles {}: {}.", i + 1, k + 1, m)); }
                if !self.languages.is_empty() {
                    if self.language(st.lang_id).is_none() {
                        p.push(trf!("Video {}, subtitles {}: language to choose from the language list.", i + 1, k + 1));
                    } else if v.subs[..k].iter().any(|o| o.lang_id == st.lang_id) {
                        p.push(trf!("Video {}, subtitles {}: the language “{}” is already used by another track of this video.", i + 1, k + 1, self.language_name(st.lang_id)));
                    }
                }
                if let crate::subtitle::SubSource::File(f) = &st.source {
                    if !f.trim().is_empty() && !abs(f).is_file() { p.push(trf!("Video {}, subtitles {}: file not found ({}).", i + 1, k + 1, f)); }
                }
            }
        }
        for (i, v) in self.videos.iter().enumerate() {
            for (k, t) in v.audio.iter().enumerate() {
                for m in t.problems() { p.push(trf!("Video {}, audio track {}: {}.", i + 1, k + 1, m)); }
                if let crate::audio::AudioSource::File(f) = &t.source {
                    if !f.trim().is_empty() && !abs(f).is_file() { p.push(trf!("Video {}, audio track {}: file not found ({}).", i + 1, k + 1, f)); }
                }
            }
        }
        {
            let tracks: Vec<Vec<crate::audio::AudioTrackCfg>> = self.videos.iter().map(|v| v.audio.clone()).collect();
            if tracks.iter().any(|t| !t.is_empty()) && self.languages.is_empty() {
                p.push(tr("Audio tracks configured but no language in the project's language list.").into());
            } else if let Err(e) = crate::audio::plan_audio(&self.languages, &self.lang, &tracks) {
                let mut m = e;
                if let Some(c) = m.get_mut(0..1) { c.make_ascii_uppercase(); }
                p.push(m);
            }
        }
        // pages du menu (sans page : les pages de depart sont generees a la construction)
        let np = self.designs.free.len();
        for (k, d) in self.designs.free.iter().enumerate() {
            for pr in d.validate(k, np, self.videos.len()).into_iter().filter(|x| x.error) {
                p.push(trf!("Page {}: {}", k + 1, pr.msg));
            }
        }
        if !self.videos.is_empty() {
            use crate::design::{Kind, NavAction, Target};
            let total = np;
            let d = &self.designs;
            for pg in d.free.iter() {
                for it in &pg.items {
                    if let Kind::Nav { action: NavAction::Goto, text, target } = &it.kind {
                        let bad = match target { None => true, Some(Target::Main(q)) => *q >= total, Some(Target::Video(v)) => *v >= self.videos.len(), Some(Target::Stay(e)) => e.map_or(false, |q| q >= total), Some(Target::Audio(id, e)) => self.language(*id).is_none() || e.map_or(false, |q| q >= total) };
                        if bad { p.push(trf!("Button “{}”: invalid or unchosen destination.", text)); }
                    }
                    let bad_link = match it.link() { Some(Target::Main(q)) => q >= total, Some(Target::Video(v)) => v >= self.videos.len(), Some(Target::Audio(id, e)) => self.language(id).is_none() || e.map_or(false, |q| q >= total), Some(Target::Stay(e)) => e.map_or(false, |q| q >= total), None => false };
                    if bad_link { p.push(tr("Clickable item: invalid destination.").into()); }
                    // reglage de la langue audio : au moins une video doit avoir de l'audio dans cette langue
                    let audio_target = match &it.kind { Kind::Nav { action: NavAction::Goto, target, .. } => *target, _ => it.link() };
                    if let Some(Target::Audio(id, _)) = audio_target {
                        if self.language(id).is_some() && self.videos.iter().all(|ve| ve.audio.iter().all(|t| t.lang_id != id)) {
                            p.push(trf!("Clickable item: no video has an audio track in “{}” (the click will change nothing).", self.language_name(id)));
                        }
                    }
                    if it.fixed_video().map_or(false, |v| v >= self.videos.len()) { p.push(tr("Thumbnail: the chosen video does not exist.").into()); }
                    // animation de l'etat selectionne : image normale requise, fichiers presents
                    let sk = &it.look.skin;
                    if !sk.sel_frames.is_empty() && it.is_button() && !matches!(it.kind, Kind::Slot { .. }) && !sk.has_normal() {
                        p.push(tr("Item animation: also choose a “normal” image (otherwise the animation is ignored).").into());
                    }
                    if it.is_button() && !matches!(it.kind, Kind::Slot { .. }) {
                        for f in sk.sel_frames.iter().chain(sk.normal_frames.iter()).filter(|f| !abs(f).is_file()) { p.push(trf!("Item animation: image not found ({}).", f)); }
                    }
                    match it.sub {
                        crate::design::SubChoice::Lang(id) => {
                            if self.language(id).is_none() {
                                p.push(tr("Clickable item: the chosen subtitle language no longer exists.").into());
                            } else if self.videos.iter().all(|ve| ve.subs.iter().all(|t| t.lang_id != id)) {
                                p.push(trf!("Clickable item: no video has subtitles in “{}”.", self.language_name(id)));
                            }
                        }
                        _ => {}
                    }
                }
            }
        }
        if self.languages.len() > crate::film::MAX_SUB_TRACKS { p.push(trf!("Subtitle languages: {} at most.", crate::film::MAX_SUB_TRACKS)); }
        for (k, l) in self.languages.iter().enumerate() {
            if l.code.len() != 3 || !l.code.is_ascii() { p.push(trf!("Language “{}”: the code must be 3 letters (e.g. fra).", l.name)); }
            if self.languages[..k].iter().any(|o| o.name == l.name) { p.push(trf!("Language “{}”: duplicate name.", l.name)); }
        }
        if !self.bg.is_empty() && !abs(&self.bg).is_file() { p.push(trf!("Background image not found ({}).", self.bg)); }
        // sons des menus (style du projet et styles propres des elements)
        {
            let mut missing: Vec<String> = Vec::new();
            let mut check = |f: &str| if !f.is_empty() && !abs(f).is_file() && !missing.iter().any(|m| m == f) { missing.push(f.to_string()); };
            let st = &self.designs.style;
            check(&st.sound_select);
            check(&st.sound_activate);
            let d = &self.designs;
            for pg in d.free.iter() {
                for it in &pg.items {
                    if let Some(s) = &it.look.sounds { check(&s.select); check(&s.activate); }
                }
            }
            for f in missing { p.push(trf!("Menu sound not found ({}).", f)); }
        }
        if !self.intro.is_empty() && !abs(&self.intro).is_file() { p.push(trf!("Opening sequence not found ({}).", self.intro)); }
        if !self.bg_video.is_empty() && !abs(&self.bg_video).is_file() { p.push(trf!("Background video not found ({}).", self.bg_video)); }
        if !self.music.is_empty() && !abs(&self.music).is_file() { p.push(trf!("Menu music not found ({}).", self.music)); }
        {
            use crate::design::{PageMusic, PageVisual};
            let d = &self.designs;
            for pg in d.free.iter() {
                let files = [match &pg.bg.visual { PageVisual::Image(f) | PageVisual::Video(f, _) => Some(f), _ => None }, match &pg.bg.music { PageMusic::File(f) => Some(f), _ => None }];
                for f in files.into_iter().flatten() {
                    if !abs(f).is_file() { p.push(trf!("Page background: file not found ({}).", f)); }
                }
            }
        }
        if let Some(h) = self.designs.home {
            let total = self.designs.free.len();
            let bad = match h { crate::design::Target::Main(q) => q >= total, crate::design::Target::Video(_) | crate::design::Target::Stay(_) | crate::design::Target::Audio(..) => true };
            if bad { p.push(tr("Invalid home page: choose an existing page.").into()); }
        }
        p
    }
}
