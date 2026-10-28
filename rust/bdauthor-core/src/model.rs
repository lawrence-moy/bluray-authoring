//! Modele editable d'un projet (pour l'interface graphique) : lecture/ecriture du JSON de projet et validation.
//! Les champs inconnus du fichier d'origine sont conserves a l'enregistrement.
use crate::design::Designs;
use crate::json::{self, Value};
use crate::project::{MAX_CHAPTERS, MAX_PER_PAGE, MAX_VIDEOS};
use std::path::Path;

pub const PRESETS: [&str; 9] = ["ultrafast", "superfast", "veryfast", "faster", "fast", "medium", "slow", "slower", "veryslow"];

#[derive(Debug, Clone, PartialEq)]
pub struct ChapterEntry { pub t: f64, pub title: String }

#[derive(Debug, Clone, PartialEq)]
pub struct VideoEntry {
    pub file: String,
    pub title: String,
    pub thumb_s: Option<f64>,
    pub chapters: Vec<ChapterEntry>,
    pub auto_s: Option<f64>,
    /// Pistes de sous-titres PGS.
    pub subs: Vec<crate::subtitle::SubTrackCfg>,
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

    pub fn new(file: &str) -> Self {
        VideoEntry { file: file.into(), title: String::new(), thumb_s: None, chapters: vec![], auto_s: None, subs: vec![], raw: Value::Obj(vec![]) }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct ProjectModel {
    pub title: String,
    pub lang: String,
    pub videos: Vec<VideoEntry>,
    pub crf: u32,
    pub preset: String,
    pub bg: String, // chemin ; vide = fond genere
    pub bg_seconds: f64,
    pub menu_1080: bool,
    pub loop_menu: bool,
    pub back_to_chapters: bool,
    pub par_page: Option<usize>,
    pub cache: bool,
    /// Mises en page personnalisees (cle « mise_en_page »).
    pub designs: Designs,
    raw: Value,
}

impl Default for ProjectModel {
    fn default() -> Self {
        ProjectModel {
            title: "Mes vidéos".into(), lang: "fra".into(), videos: vec![], crf: 18, preset: "medium".into(), bg: String::new(), bg_seconds: 5.0,
            menu_1080: false, loop_menu: false, back_to_chapters: true, par_page: None, cache: true, designs: Designs::default(), raw: Value::Obj(vec![]),
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
        m.title = v.get("titre").and_then(Value::as_str).unwrap_or("Menu").into();
        m.lang = v.get("langue").and_then(Value::as_str).unwrap_or("und").into();
        for x in v.get("videos").and_then(Value::as_array).ok_or("champ « videos » absent")? {
            let mut e = VideoEntry::new(x.get("fichier").and_then(Value::as_str).ok_or("video sans « fichier »")?);
            e.raw = x.clone();
            e.title = x.get("titre").and_then(Value::as_str).unwrap_or("").into();
            e.thumb_s = x.get("vignette_s").and_then(Value::as_f64);
            e.auto_s = x.get("chapitres_auto_s").and_then(Value::as_f64);
            e.subs = crate::subtitle::tracks_from_value(x.get("soustitres"))?;
            for c in x.get("chapitres").and_then(Value::as_array).unwrap_or(&[]) {
                e.chapters.push(match c {
                    Value::Obj(_) => ChapterEntry { t: c.get("t").and_then(Value::as_f64).ok_or("chapitre sans « t »")?, title: c.get("titre").and_then(Value::as_str).unwrap_or("").into() },
                    _ => ChapterEntry { t: c.as_f64().ok_or("chapitre : nombre attendu")?, title: String::new() },
                });
            }
            m.videos.push(e);
        }
        if let Some(e) = v.get("encodage") {
            if let Some(c) = e.get("crf").and_then(Value::as_i64) { m.crf = c.clamp(0, 51) as u32; }
            if let Some(p) = e.get("preset").and_then(Value::as_str) { m.preset = p.into(); }
        }
        if let Some(menu) = v.get("menu") {
            m.bg = menu.get("fond").and_then(Value::as_str).unwrap_or("").into();
            m.bg_seconds = menu.get("fond_secondes").and_then(Value::as_f64).unwrap_or(5.0);
            m.menu_1080 = menu.get("taille").and_then(Value::as_str) == Some("1080");
            m.loop_menu = menu.get("boucle").and_then(Value::as_bool).unwrap_or(false);
            m.back_to_chapters = menu.get("retour").and_then(Value::as_str).unwrap_or("chapitres") == "chapitres";
            m.par_page = menu.get("par_page").and_then(Value::as_i64).map(|p| p.max(0) as usize);
            m.cache = menu.get("cache").and_then(Value::as_bool).unwrap_or(true);
        }
        if let Some(d) = v.get("mise_en_page") { m.designs = Designs::from_value(d)?; }
        Ok(m)
    }

    pub fn to_value(&self) -> Value {
        let mut root = self.raw.clone();
        set(&mut root, "titre", Some(s(&self.title)));
        set(&mut root, "langue", Some(s(&self.lang)));
        let vids: Vec<Value> = self.videos.iter().map(|e| {
            let mut o = e.raw.clone();
            set(&mut o, "fichier", Some(s(&e.file)));
            set(&mut o, "titre", if e.title.is_empty() { None } else { Some(s(&e.title)) });
            set(&mut o, "vignette_s", e.thumb_s.map(n));
            set(&mut o, "chapitres_auto_s", e.auto_s.map(n));
            set(&mut o, "soustitres", crate::subtitle::tracks_to_value(&e.subs));
            set(&mut o, "chapitres", if e.chapters.is_empty() { None } else {
                Some(Value::Arr(e.chapters.iter().map(|c| if c.title.is_empty() { n(c.t) } else { Value::Obj(vec![("t".into(), n(c.t)), ("titre".into(), s(&c.title))]) }).collect()))
            });
            o
        }).collect();
        set(&mut root, "videos", Some(Value::Arr(vids)));
        let mut enc = root.get("encodage").cloned().unwrap_or(Value::Obj(vec![]));
        set(&mut enc, "crf", Some(n(self.crf as f64)));
        set(&mut enc, "preset", Some(s(&self.preset)));
        set(&mut root, "encodage", Some(enc));
        let mut menu = root.get("menu").cloned().unwrap_or(Value::Obj(vec![]));
        set(&mut menu, "fond", if self.bg.is_empty() { None } else { Some(s(&self.bg)) });
        set(&mut menu, "fond_secondes", Some(n(self.bg_seconds)));
        set(&mut menu, "taille", if self.menu_1080 { Some(s("1080")) } else { None });
        set(&mut menu, "boucle", Some(Value::Bool(self.loop_menu)));
        set(&mut menu, "retour", Some(s(if self.back_to_chapters { "chapitres" } else { "principal" })));
        set(&mut menu, "par_page", self.par_page.map(|p| n(p as f64)));
        set(&mut menu, "cache", Some(Value::Bool(self.cache)));
        set(&mut root, "menu", Some(menu));
        set(&mut root, "mise_en_page", if self.designs.is_empty() { None } else { Some(self.designs.to_value()) });
        root
    }

    pub fn to_json(&self) -> String { self.to_value().to_pretty() }

    /// Echange deux videos de place (les mises en page de leurs menus de chapitres suivent).
    pub fn swap_videos(&mut self, a: usize, b: usize) {
        self.videos.swap(a, b);
        self.designs.swap_videos(a, b);
    }

    /// Retire une video et ses mises en page de chapitres.
    pub fn remove_video(&mut self, i: usize) {
        self.videos.remove(i);
        self.designs.remove_video(i);
    }

    /// Problemes bloquants ou suspects (chemins relatifs resolus depuis `base`, le dossier du fichier de projet).
    pub fn validate(&self, base: &Path) -> Vec<String> {
        let mut p = Vec::new();
        if self.videos.is_empty() { p.push("Aucune vidéo.".into()); }
        if self.videos.len() > MAX_VIDEOS { p.push(format!("{} vidéos : {MAX_VIDEOS} au maximum.", self.videos.len())); }
        if self.lang.len() != 3 || !self.lang.is_ascii() { p.push("La langue doit faire 3 lettres (ex. fra, eng, und).".into()); }
        if !PRESETS.contains(&self.preset.as_str()) { p.push(format!("Préréglage x264 inconnu : {}.", self.preset)); }
        if self.crf > 51 { p.push("CRF : 0 à 51.".into()); }
        if let Some(pp) = self.par_page { if !(1..=MAX_PER_PAGE).contains(&pp) { p.push(format!("Éléments par page : 1 à {MAX_PER_PAGE}.")); } }
        let abs = |f: &str| { let q = Path::new(f); if q.is_absolute() { q.to_path_buf() } else { base.join(q) } };
        for (i, v) in self.videos.iter().enumerate() {
            if !abs(&v.file).is_file() { p.push(format!("Vidéo {} : fichier introuvable ({}).", i + 1, v.file)); }
            if v.chapters.len() > MAX_CHAPTERS { p.push(format!("Vidéo {} : {MAX_CHAPTERS} chapitres au maximum.", i + 1)); }
            if v.chapters.iter().any(|c| c.t < 0.0) { p.push(format!("Vidéo {} : temps de chapitre négatif.", i + 1)); }
            if v.auto_s.map_or(false, |a| a <= 0.0) { p.push(format!("Vidéo {} : intervalle de chapitres automatiques > 0.", i + 1)); }
            if v.subs.len() > crate::film::MAX_SUB_TRACKS { p.push(format!("Vidéo {} : {} pistes de sous-titres au maximum.", i + 1, crate::film::MAX_SUB_TRACKS)); }
            for (k, st) in v.subs.iter().enumerate() {
                for m in st.problems() { p.push(format!("Vidéo {}, sous-titres {} : {m}.", i + 1, k + 1)); }
                if let crate::subtitle::SubSource::File(f) = &st.source {
                    if !f.trim().is_empty() && !abs(f).is_file() { p.push(format!("Vidéo {}, sous-titres {} : fichier introuvable ({f}).", i + 1, k + 1)); }
                }
            }
        }
        if !self.designs.free.is_empty() && !self.videos.is_empty() {
            let auto = crate::project::paginate(self.videos.len(), self.par_page).map(|g| g.len()).unwrap_or(1);
            let total = auto + self.designs.free.len();
            for (f, d) in self.designs.free.iter().enumerate() {
                for pr in d.validate_free(auto + f, total, self.videos.len()).into_iter().filter(|x| x.error) {
                    p.push(format!("Page libre {} : {}", f + 1, pr.msg));
                }
            }
        }
        if !self.videos.is_empty() {
            use crate::design::{Kind, NavAction, Target};
            let total = crate::project::paginate(self.videos.len(), self.par_page).map(|g| g.len()).unwrap_or(1) + self.designs.free.len();
            let d = &self.designs;
            for pg in d.main.values().chain(d.chapters.values()).chain(d.free.iter()) {
                for it in &pg.items {
                    if let Kind::Nav { action: NavAction::Goto, text, target } = &it.kind {
                        let bad = match target { None => true, Some(Target::Main(q)) => *q >= total, Some(Target::Chapters(v, _)) | Some(Target::Video(v)) => *v >= self.videos.len() };
                        if bad { p.push(format!("Bouton « {text} » : destination invalide ou non choisie.")); }
                    }
                    let bad_link = match it.link() { Some(Target::Main(q)) => q >= total, Some(Target::Chapters(v, _)) | Some(Target::Video(v)) => v >= self.videos.len(), None => false };
                    if bad_link { p.push("Texte ou image cliquable : destination invalide.".into()); }
                }
            }
        }
        if !self.bg.is_empty() && !abs(&self.bg).is_file() { p.push(format!("Image de fond introuvable ({}).", self.bg)); }
        p
    }
}
