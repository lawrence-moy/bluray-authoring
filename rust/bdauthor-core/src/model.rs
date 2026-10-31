//! Modele editable d'un projet (pour l'interface graphique) : lecture/ecriture du JSON de projet et validation.
//! Les champs inconnus du fichier d'origine sont conserves a l'enregistrement.
use crate::design::Designs;
use crate::json::{self, Value};
use crate::project::{MAX_CHAPTERS, MAX_VIDEOS};
use crate::subtitle::{Language, SubTrackCfg};
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
    /// Pistes audio choisies (vide : la premiere piste audio du fichier, dans la langue du disque).
    pub audio: Vec<crate::audio::AudioTrackCfg>,
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
        VideoEntry { file: file.into(), title: String::new(), thumb_s: None, chapters: vec![], auto_s: None, subs: vec![], audio: vec![], raw: Value::Obj(vec![]) }
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
    pub loop_menu: bool,
    pub cache: bool,
    /// Mises en page personnalisees (cle « mise_en_page »).
    pub designs: Designs,
    /// Langues de sous-titres du projet (cle « langues ») : le rang d'une langue est son numero de piste sur le disque.
    pub languages: Vec<Language>,
    raw: Value,
}

impl Default for ProjectModel {
    fn default() -> Self {
        ProjectModel {
            title: "Mes vidéos".into(), lang: "fra".into(), videos: vec![], crf: 18, preset: "medium".into(), bg: String::new(), intro: String::new(), bg_video: String::new(), bg_video_audio: false, music: String::new(), bg_seconds: 5.0,
            menu_1080: false, loop_menu: false, cache: true, designs: Designs::default(), languages: vec![], raw: Value::Obj(vec![]),
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
            e.audio = crate::audio::tracks_from_value(x.get("pistes_audio"))?;
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
            m.intro = menu.get("intro").and_then(Value::as_str).unwrap_or("").into();
            m.bg_video = menu.get("fond_video").and_then(Value::as_str).unwrap_or("").into();
            m.bg_video_audio = menu.get("son_fond_video").and_then(Value::as_bool).unwrap_or(false);
            m.music = menu.get("musique").and_then(Value::as_str).unwrap_or("").into();
            m.bg_seconds = menu.get("fond_secondes").and_then(Value::as_f64).unwrap_or(5.0);
            m.menu_1080 = menu.get("taille").and_then(Value::as_str) == Some("1080");
            m.loop_menu = menu.get("boucle").and_then(Value::as_bool).unwrap_or(false);
            m.cache = menu.get("cache").and_then(Value::as_bool).unwrap_or(true);
        }
        if let Some(d) = v.get("mise_en_page") { m.designs = Designs::from_value(d)?; }
        m.languages = crate::subtitle::languages_from_value(v.get("langues"))?;
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
            let subs: Vec<SubTrackCfg> = e.subs.iter().map(|t| self.synced_track(t)).collect();
            set(&mut o, "soustitres", crate::subtitle::tracks_to_value(&subs));
            let audio: Vec<crate::audio::AudioTrackCfg> = e.audio.iter().map(|t| { let mut t = t.clone(); if let Some(l) = self.language(t.lang_id) { t.lang = l.code.clone(); } t }).collect();
            set(&mut o, "pistes_audio", crate::audio::tracks_to_value(&audio));
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
        set(&mut menu, "intro", if self.intro.is_empty() { None } else { Some(s(&self.intro)) });
        set(&mut menu, "fond_video", if self.bg_video.is_empty() { None } else { Some(s(&self.bg_video)) });
        set(&mut menu, "son_fond_video", if self.bg_video_audio { Some(Value::Bool(true)) } else { None });
        set(&mut menu, "musique", if self.music.is_empty() { None } else { Some(s(&self.music)) });
        set(&mut menu, "fond_secondes", Some(n(self.bg_seconds)));
        set(&mut menu, "taille", if self.menu_1080 { Some(s("1080")) } else { None });
        set(&mut menu, "boucle", Some(Value::Bool(self.loop_menu)));
        // anciens reglages des menus automatiques : retires du fichier
        set(&mut menu, "retour", None);
        set(&mut menu, "par_page", None);
        set(&mut menu, "cache", Some(Value::Bool(self.cache)));
        set(&mut root, "menu", Some(menu));
        set(&mut root, "langues", crate::subtitle::languages_to_value(&self.languages));
        set(&mut root, "mise_en_page", if self.designs.is_empty() { None } else { Some(self.designs.to_value()) });
        root
    }

    pub fn to_json(&self) -> String { self.to_value().to_pretty() }

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
        self.language(id).map_or_else(|| "(langue supprimée)".to_string(), |l| l.name.clone())
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
    }

    /// Retire une video (ses vignettes restent, sans video).
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
        let abs = |f: &str| { let q = Path::new(f); if q.is_absolute() { q.to_path_buf() } else { base.join(q) } };
        for (i, v) in self.videos.iter().enumerate() {
            if !abs(&v.file).is_file() { p.push(format!("Vidéo {} : fichier introuvable ({}).", i + 1, v.file)); }
            if v.chapters.len() > MAX_CHAPTERS { p.push(format!("Vidéo {} : {MAX_CHAPTERS} chapitres au maximum.", i + 1)); }
            if v.chapters.iter().any(|c| c.t < 0.0) { p.push(format!("Vidéo {} : temps de chapitre négatif.", i + 1)); }
            if v.auto_s.map_or(false, |a| a <= 0.0) { p.push(format!("Vidéo {} : intervalle de chapitres automatiques > 0.", i + 1)); }
            if v.subs.len() > crate::film::MAX_SUB_TRACKS { p.push(format!("Vidéo {} : {} pistes de sous-titres au maximum.", i + 1, crate::film::MAX_SUB_TRACKS)); }
            for (k, st) in v.subs.iter().enumerate() {
                for m in self.synced_track(st).problems() { p.push(format!("Vidéo {}, sous-titres {} : {m}.", i + 1, k + 1)); }
                if !self.languages.is_empty() {
                    if self.language(st.lang_id).is_none() {
                        p.push(format!("Vidéo {}, sous-titres {} : langue à choisir dans la liste des langues.", i + 1, k + 1));
                    } else if v.subs[..k].iter().any(|o| o.lang_id == st.lang_id) {
                        p.push(format!("Vidéo {}, sous-titres {} : la langue « {} » est déjà utilisée par une autre piste de cette vidéo.", i + 1, k + 1, self.language_name(st.lang_id)));
                    }
                }
                if let crate::subtitle::SubSource::File(f) = &st.source {
                    if !f.trim().is_empty() && !abs(f).is_file() { p.push(format!("Vidéo {}, sous-titres {} : fichier introuvable ({f}).", i + 1, k + 1)); }
                }
            }
        }
        for (i, v) in self.videos.iter().enumerate() {
            for (k, t) in v.audio.iter().enumerate() {
                for m in t.problems() { p.push(format!("Vidéo {}, piste audio {} : {m}.", i + 1, k + 1)); }
                if let crate::audio::AudioSource::File(f) = &t.source {
                    if !f.trim().is_empty() && !abs(f).is_file() { p.push(format!("Vidéo {}, piste audio {} : fichier introuvable ({f}).", i + 1, k + 1)); }
                }
            }
        }
        {
            let tracks: Vec<Vec<crate::audio::AudioTrackCfg>> = self.videos.iter().map(|v| v.audio.clone()).collect();
            if tracks.iter().any(|t| !t.is_empty()) && self.languages.is_empty() {
                p.push("Pistes audio configurées mais aucune langue dans la liste des langues du projet.".into());
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
                p.push(format!("Page {} : {}", k + 1, pr.msg));
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
                        if bad { p.push(format!("Bouton « {text} » : destination invalide ou non choisie.")); }
                    }
                    let bad_link = match it.link() { Some(Target::Main(q)) => q >= total, Some(Target::Video(v)) => v >= self.videos.len(), Some(Target::Audio(id, e)) => self.language(id).is_none() || e.map_or(false, |q| q >= total), Some(Target::Stay(e)) => e.map_or(false, |q| q >= total), None => false };
                    if bad_link { p.push("Élément cliquable : destination invalide.".into()); }
                    // reglage de la langue audio : au moins une video doit avoir de l'audio dans cette langue
                    let audio_target = match &it.kind { Kind::Nav { action: NavAction::Goto, target, .. } => *target, _ => it.link() };
                    if let Some(Target::Audio(id, _)) = audio_target {
                        if self.language(id).is_some() && self.videos.iter().all(|ve| ve.audio.iter().all(|t| t.lang_id != id)) {
                            p.push(format!("Élément cliquable : aucune vidéo n'a de piste audio en « {} » (le clic ne changera rien).", self.language_name(id)));
                        }
                    }
                    if it.fixed_video().map_or(false, |v| v >= self.videos.len()) { p.push("Vignette : la vidéo choisie n'existe pas.".into()); }
                    // animation de l'etat selectionne : image normale requise, fichiers presents
                    let sk = &it.look.skin;
                    if !sk.sel_frames.is_empty() && it.is_button() && !matches!(it.kind, Kind::Slot { .. }) && !sk.has_normal() {
                        p.push("Animation d'un élément : choisissez aussi une image « normale » (sinon l'animation est ignorée).".into());
                    }
                    if it.is_button() && !matches!(it.kind, Kind::Slot { .. }) {
                        for f in sk.sel_frames.iter().chain(sk.normal_frames.iter()).filter(|f| !abs(f).is_file()) { p.push(format!("Animation d'un élément : image introuvable ({f}).")); }
                    }
                    match it.sub {
                        crate::design::SubChoice::Lang(id) => {
                            if self.language(id).is_none() {
                                p.push("Élément cliquable : la langue de sous-titres choisie n'existe plus.".into());
                            } else if self.videos.iter().all(|ve| ve.subs.iter().all(|t| t.lang_id != id)) {
                                p.push(format!("Élément cliquable : aucune vidéo n'a de sous-titres en « {} ».", self.language_name(id)));
                            }
                        }
                        _ => {}
                    }
                }
            }
        }
        if self.languages.len() > crate::film::MAX_SUB_TRACKS { p.push(format!("Langues de sous-titres : {} au maximum.", crate::film::MAX_SUB_TRACKS)); }
        for (k, l) in self.languages.iter().enumerate() {
            if l.code.len() != 3 || !l.code.is_ascii() { p.push(format!("Langue « {} » : le code doit faire 3 lettres (ex. fra).", l.name)); }
            if self.languages[..k].iter().any(|o| o.name == l.name) { p.push(format!("Langue « {} » : nom en double.", l.name)); }
        }
        if !self.bg.is_empty() && !abs(&self.bg).is_file() { p.push(format!("Image de fond introuvable ({}).", self.bg)); }
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
            for f in missing { p.push(format!("Son de menu introuvable ({f}).")); }
        }
        if !self.intro.is_empty() && !abs(&self.intro).is_file() { p.push(format!("Séquence d'ouverture introuvable ({}).", self.intro)); }
        if !self.bg_video.is_empty() && !abs(&self.bg_video).is_file() { p.push(format!("Vidéo de fond introuvable ({}).", self.bg_video)); }
        if !self.music.is_empty() && !abs(&self.music).is_file() { p.push(format!("Musique du menu introuvable ({}).", self.music)); }
        {
            use crate::design::{PageMusic, PageVisual};
            let d = &self.designs;
            for pg in d.free.iter() {
                let files = [match &pg.bg.visual { PageVisual::Image(f) | PageVisual::Video(f, _) => Some(f), _ => None }, match &pg.bg.music { PageMusic::File(f) => Some(f), _ => None }];
                for f in files.into_iter().flatten() {
                    if !abs(f).is_file() { p.push(format!("Fond d'une page : fichier introuvable ({f}).")); }
                }
            }
        }
        if let Some(h) = self.designs.home {
            let total = self.designs.free.len();
            let bad = match h { crate::design::Target::Main(q) => q >= total, crate::design::Target::Video(_) | crate::design::Target::Stay(_) | crate::design::Target::Audio(..) => true };
            if bad { p.push("Page d'accueil invalide : choisissez une page existante.".into()); }
        }
        p
    }
}
