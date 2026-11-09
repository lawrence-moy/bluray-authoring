//! Pistes audio d'une video : source (piste de la video ou fichier audio), langue du projet, decalage ; choix des pistes du disque.
use crate::i18n::tr;
use crate::trf;
use crate::json::Value;
use crate::subtitle::Language;

/// D'ou vient une piste audio.
#[derive(Debug, Clone, PartialEq)]
pub enum AudioSource {
    /// piste audio du fichier video, numerotee parmi les pistes audio (0 = la premiere, comme `-map 0:a:N`)
    Embedded(u32),
    /// fichier audio separe
    File(String),
}

/// Une piste audio d'une video.
#[derive(Debug, Clone, PartialEq)]
pub struct AudioTrackCfg {
    pub source: AudioSource,
    /// Langue de la liste du projet (`Language::id`) ; 0 : non choisie.
    pub lang_id: u32,
    /// Code de langue de 3 lettres ; avec une langue du projet, c'est le code de cette langue.
    pub lang: String,
    /// Decalage a appliquer a la piste (ms, peut etre negatif).
    pub offset_ms: i64,
}

/// Nombre maximal de pistes audio par video.
pub const MAX_AUDIO_TRACKS: usize = 8;

impl AudioTrackCfg {
    pub fn embedded(index: u32, lang: &str) -> Self {
        AudioTrackCfg { source: AudioSource::Embedded(index), lang_id: 0, lang: lang.into(), offset_ms: 0 }
    }
    pub fn from_file(path: &str, lang: &str) -> Self {
        AudioTrackCfg { source: AudioSource::File(path.into()), lang_id: 0, lang: lang.into(), offset_ms: 0 }
    }

    pub fn from_value(v: &Value) -> Result<Self, String> {
        let source = match (v.get("file").and_then(Value::as_str), v.get("track").and_then(Value::as_i64)) {
            (Some(f), _) => AudioSource::File(f.into()),
            (None, Some(n)) if n >= 0 => AudioSource::Embedded(n as u32),
            _ => return Err(tr("audio tracks: “file” or “track” expected").into()),
        };
        Ok(AudioTrackCfg {
            source,
            lang_id: v.get("language_id").and_then(Value::as_i64).filter(|n| *n >= 1).map_or(0, |n| n as u32),
            lang: v.get("language").and_then(Value::as_str).unwrap_or("und").into(),
            offset_ms: v.get("offset_ms").and_then(Value::as_f64).map_or(0, |x| x.round() as i64),
        })
    }

    pub fn to_value(&self) -> Value {
        let mut o = vec![match &self.source {
            AudioSource::File(f) => ("file".to_string(), Value::Str(f.clone())),
            AudioSource::Embedded(n) => ("track".to_string(), Value::Num(*n as f64)),
        }];
        o.push(("language".into(), Value::Str(self.lang.clone())));
        if self.lang_id != 0 { o.push(("language_id".into(), Value::Num(self.lang_id as f64))); }
        if self.offset_ms != 0 { o.push(("offset_ms".into(), Value::Num(self.offset_ms as f64))); }
        Value::Obj(o)
    }

    /// Code de langue sur 3 octets (`und` si invalide).
    pub fn code_bytes(&self) -> [u8; 3] {
        self.lang.as_bytes().try_into().unwrap_or(*b"und")
    }

    /// Problemes de reglage (liste vide si tout va bien).
    pub fn problems(&self) -> Vec<String> {
        let mut p = Vec::new();
        if self.lang.len() != 3 || !self.lang.is_ascii() { p.push(tr("language: 3 letters expected (e.g. fra)").to_string()); }
        if let AudioSource::File(f) = &self.source { if f.trim().is_empty() { p.push(tr("audio file not chosen").to_string()); } }
        p
    }
}

/// Liste de pistes depuis la valeur JSON du champ « pistes_audio » d'une video (absent = aucune piste configuree).
pub fn tracks_from_value(v: Option<&Value>) -> Result<Vec<AudioTrackCfg>, String> {
    match v {
        None | Some(Value::Null) => Ok(vec![]),
        Some(a) => a.as_array().ok_or(tr("“audio_tracks”: list expected"))?.iter().map(AudioTrackCfg::from_value).collect(),
    }
}

pub fn tracks_to_value(t: &[AudioTrackCfg]) -> Option<Value> {
    if t.is_empty() { None } else { Some(Value::Arr(t.iter().map(AudioTrackCfg::to_value).collect())) }
}

/// Pistes audio d'une video telles qu'elles seront sur le disque.
#[derive(Debug, Clone, PartialEq)]
pub struct VideoAudio {
    /// pistes a encoder, dans l'ordre du disque ; vide : la premiere piste audio du fichier (ancien fonctionnement)
    pub tracks: Vec<AudioTrackCfg>,
    /// langue de chaque piste du disque, dans l'ordre
    pub langs: Vec<[u8; 3]>,
    /// langue du projet (`Language::id`) de chaque piste du disque, dans l'ordre
    pub ids: Vec<u32>,
}

/// Pistes audio de tout le projet quand les langues sont utilisees.
#[derive(Debug, Clone, PartialEq)]
pub struct AudioPlan {
    pub videos: Vec<VideoAudio>,
    /// langues ayant de l'audio dans au moins une video, dans l'ordre de la liste du projet
    pub slots: Vec<u32>,
}

/// Pistes audio de chaque video, dans l'ordre de sa liste. `None` : aucune video n'a de piste audio
/// configuree (ancien fonctionnement). Chaque video a ses propres langues, dans l'ordre qu'on lui donne : le
/// numero d'une langue peut donc changer d'une video a l'autre. Les boutons « langue audio » retiennent la langue (et non un
/// numero) ; chaque video choisit ensuite sa piste de cette langue au demarrage (voir `project`).
/// Une video sans piste configuree compte pour sa premiere piste audio, dans la langue du projet `project_lang`.
pub fn plan_audio(languages: &[Language], project_lang: &str, videos: &[Vec<AudioTrackCfg>]) -> Result<Option<AudioPlan>, String> {
    if languages.is_empty() || videos.iter().all(Vec::is_empty) { return Ok(None); }
    let lang_of = |id: u32| languages.iter().find(|l| l.id == id);
    // langue de chaque piste (video, piste), la piste par defaut d'une video sans configuration comprise
    let mut ids: Vec<Vec<u32>> = Vec::new();
    for (i, tracks) in videos.iter().enumerate() {
        if tracks.is_empty() {
            let Some(l) = languages.iter().find(|l| l.code == project_lang) else {
                return Err(trf!("video {}: no audio track configured, and the disc language “{}” is not in the project's language list", i + 1, project_lang));
            };
            ids.push(vec![l.id]);
            continue;
        }
        if tracks.len() > MAX_AUDIO_TRACKS { return Err(trf!("video {}: {} audio tracks at most", i + 1, MAX_AUDIO_TRACKS)); }
        let mut v = Vec::new();
        for (k, t) in tracks.iter().enumerate() {
            let Some(l) = lang_of(t.lang_id) else { return Err(trf!("video {}, audio track {}: language to choose from the project's language list", i + 1, k + 1)) };
            if v.contains(&l.id) { return Err(trf!("video {}, audio track {}: the language “{}” is already used by another audio track of this video", i + 1, k + 1, l.name)); }
            v.push(l.id);
        }
        ids.push(v);
    }
    let slots: Vec<u32> = languages.iter().map(|l| l.id).filter(|id| ids.iter().any(|v| v.contains(id))).collect();
    let mut out = Vec::new();
    for (tracks, v) in videos.iter().zip(&ids) {
        if tracks.is_empty() {
            let l = lang_of(v[0]).unwrap();
            out.push(VideoAudio { tracks: vec![], langs: vec![l.code_bytes()], ids: vec![l.id] });
        } else {
            // l'ordre choisi dans la video est garde (piste 1 = la premiere de sa liste)
            let langs = v.iter().map(|id| lang_of(*id).unwrap().code_bytes()).collect();
            out.push(VideoAudio { tracks: tracks.clone(), langs, ids: v.clone() });
        }
    }
    Ok(Some(AudioPlan { videos: out, slots }))
}
