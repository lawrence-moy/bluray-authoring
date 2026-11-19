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

/// Format d'une piste audio sur le disque.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum AudioFormat {
    /// Dolby Digital (AC-3) stereo, au debit choisi pour la piste (sinon celui du projet)
    #[default]
    Ac3,
    /// Dolby Digital (AC-3) 5.1 (sources a plus de canaux mixees en 5.1), 448 kb/s par defaut
    Ac3Surround,
    /// la piste AC-3 de la source, recopiee telle quelle (48 kHz, 640 kb/s au plus, 5.1 au plus)
    Ac3Copy,
    /// la piste DTS de la source (DTS, DTS-HD High Resolution ou DTS-HD Master Audio), recopiee telle quelle (48 kHz)
    DtsCopy,
    /// LPCM sans perte, 16 bits, nombre de canaux de la source (48 kHz, ou 96 kHz : voir `AudioTrackCfg::sample_rate`)
    Lpcm16,
    /// LPCM sans perte, 24 bits, nombre de canaux de la source (48 ou 96 kHz)
    Lpcm24,
}

/// Canaux d'une piste LPCM : ceux de la source, ou un plafond (une source qui a deja moins de canaux n'est pas touchee).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum LpcmChannels {
    #[default]
    Source,
    /// 5.1 au plus (une source 7.1, 7.0 ou 6.1 est mixee en 5.1)
    Max51,
    /// stereo (mixage standard, caisson de basses garde)
    Stereo,
    /// stereo Dolby Pro Logic II (un ampli home-cinema peut en retirer un effet surround)
    StereoDpl2,
}

impl LpcmChannels {
    pub const ALL: [LpcmChannels; 4] = [LpcmChannels::Source, LpcmChannels::Max51, LpcmChannels::Stereo, LpcmChannels::StereoDpl2];
    pub fn name(self) -> &'static str {
        match self { LpcmChannels::Source => "source", LpcmChannels::Max51 => "5.1", LpcmChannels::Stereo => "stereo", LpcmChannels::StereoDpl2 => "stereo_dpl2" }
    }
    pub fn from_name(s: &str) -> Option<LpcmChannels> { LpcmChannels::ALL.into_iter().find(|c| c.name() == s) }
    /// Plafond de canaux (`None` : ceux de la source).
    pub fn max(self) -> Option<u32> {
        match self { LpcmChannels::Source => None, LpcmChannels::Max51 => Some(6), LpcmChannels::Stereo | LpcmChannels::StereoDpl2 => Some(2) }
    }
    /// Canaux sur le disque pour une source de `src` canaux.
    pub fn out(self, src: u32) -> u32 { self.max().map_or(src, |m| src.min(m)) }
    /// Filtre ffmpeg du mixage. La normalisation de ffmpeg (aucune saturation possible) baisse beaucoup le niveau : dialogues
    /// au centre -4,7 dB en 5.1 et -11 dB en stereo ; avec `rematrix_maxval=2`, 0 dB en 5.1 et -5 dB en stereo (le mixage de
    /// reference donnerait -3 dB, mais pourrait saturer quand tous les canaux sont forts).
    pub fn filter(self) -> Option<&'static str> {
        match self {
            LpcmChannels::Source => None,
            LpcmChannels::Max51 => Some("aresample=ochl=5.1(side):rematrix_maxval=2"),
            LpcmChannels::Stereo => Some("aresample=ochl=stereo:lfe_mix_level=0.7071:rematrix_maxval=2"),
            LpcmChannels::StereoDpl2 => Some("aresample=ochl=stereo:matrix_encoding=dplii:lfe_mix_level=0.7071:rematrix_maxval=2"),
        }
    }
}

impl AudioFormat {
    pub const ALL: [AudioFormat; 6] =
        [AudioFormat::Ac3, AudioFormat::Ac3Surround, AudioFormat::Ac3Copy, AudioFormat::DtsCopy, AudioFormat::Lpcm16, AudioFormat::Lpcm24];
    /// Nom dans le fichier de projet.
    pub fn name(self) -> &'static str {
        match self {
            AudioFormat::Ac3 => "ac3", AudioFormat::Ac3Surround => "ac3_51", AudioFormat::Ac3Copy => "ac3_copy", AudioFormat::DtsCopy => "dts_copy",
            AudioFormat::Lpcm16 => "lpcm16", AudioFormat::Lpcm24 => "lpcm24",
        }
    }
    pub fn from_name(s: &str) -> Option<AudioFormat> { AudioFormat::ALL.into_iter().find(|f| f.name() == s) }
    pub fn label(self) -> &'static str {
        match self {
            AudioFormat::Ac3 => "Dolby Digital 2.0", AudioFormat::Ac3Surround => "Dolby Digital 5.1", AudioFormat::Ac3Copy => "Copy of the source AC-3 track",
            AudioFormat::DtsCopy => "Copy of the source DTS / DTS-HD track",
            AudioFormat::Lpcm16 => "LPCM 16 bits", AudioFormat::Lpcm24 => "LPCM 24 bits",
        }
    }
    pub fn is_lpcm(self) -> bool { matches!(self, AudioFormat::Lpcm16 | AudioFormat::Lpcm24) }
    /// AC-3 encode par le programme (le debit est alors un reglage de la piste).
    pub fn is_encoded_ac3(self) -> bool { matches!(self, AudioFormat::Ac3 | AudioFormat::Ac3Surround) }
    /// Flux lourd (sans perte ou haute definition) : le debit de la video doit lui laisser de la place.
    pub fn is_heavy(self) -> bool { matches!(self, AudioFormat::Lpcm16 | AudioFormat::Lpcm24 | AudioFormat::DtsCopy) }
    /// Profondeur d'un echantillon LPCM (0 pour AC-3).
    pub fn bits(self) -> u32 { match self { AudioFormat::Lpcm16 => 16, AudioFormat::Lpcm24 => 24, _ => 0 } }
    /// Debit du flux audio en kb/s pour `channels` canaux de la source ; `ac3_kbps` : debit AC-3 du projet. Le LPCM de Blu-ray
    /// range les canaux par paires (5 canaux : 6 places) et 48 kHz.
    pub fn kbps(self, channels: u32, ac3_kbps: u32) -> u32 {
        match self {
            AudioFormat::Ac3 | AudioFormat::Ac3Copy => ac3_kbps,
            AudioFormat::DtsCopy => DTS_HD_KBPS,
            AudioFormat::Ac3Surround => AC3_SURROUND_KBPS,
            _ => (channels.max(1) + 1) / 2 * 2 * self.bits() * 48,
        }
    }
}

/// Debits AC-3 permis (kb/s).
pub const AC3_RATES: [u32; 19] = [32, 40, 48, 56, 64, 80, 96, 112, 128, 160, 192, 224, 256, 320, 384, 448, 512, 576, 640];
/// Debit AC-3 5.1 par defaut (kb/s).
pub const AC3_SURROUND_KBPS: u32 = 448;
/// Debit maximal d'un flux AC-3 sur Blu-ray (kb/s).
pub const AC3_MAX_KBPS: u32 = 640;
/// Debit suppose d'une piste DTS-HD MA recopiee dont le debit est inconnu (kb/s ; un DTS-HD MA 5.1 tourne autour de 3 a 5 Mb/s).
pub const DTS_HD_KBPS: u32 = 6500;

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
    /// Format sur le disque (AC-3 par defaut).
    pub format: AudioFormat,
    /// Debit AC-3 (kb/s) des formats encodes ; 0 : celui du projet (stereo) ou 448 (5.1).
    pub bitrate: u32,
    /// Frequence du LPCM (Hz) : 96000, ou 0 pour 48 kHz ; sans effet sur les autres formats (toujours 48 kHz).
    pub sample_rate: u32,
    /// Canaux du LPCM (plafond) ; sans effet sur les autres formats.
    pub lpcm_channels: LpcmChannels,
}

/// Frequences LPCM proposees (Hz). Le Blu-ray accepte aussi 192 kHz, au plus en 5.1.
pub const LPCM_RATES: [u32; 2] = [48_000, 96_000];

/// Nombre maximal de pistes audio par video.
pub const MAX_AUDIO_TRACKS: usize = 8;

impl AudioTrackCfg {
    pub fn embedded(index: u32, lang: &str) -> Self {
        AudioTrackCfg { source: AudioSource::Embedded(index), lang_id: 0, lang: lang.into(), offset_ms: 0, format: AudioFormat::Ac3, bitrate: 0, sample_rate: 0, lpcm_channels: LpcmChannels::Source }
    }
    pub fn from_file(path: &str, lang: &str) -> Self {
        AudioTrackCfg { source: AudioSource::File(path.into()), lang_id: 0, lang: lang.into(), offset_ms: 0, format: AudioFormat::Ac3, bitrate: 0, sample_rate: 0, lpcm_channels: LpcmChannels::Source }
    }

    /// Canaux sur le disque d'une piste LPCM dont la source a `src` canaux.
    pub fn lpcm_out_channels(&self, src: u32) -> u32 { if self.format.is_lpcm() { self.lpcm_channels.out(src) } else { src } }

    /// Frequence de la piste sur le disque (Hz) : 96 kHz pour un LPCM ainsi regle, 48 kHz sinon.
    pub fn rate(&self) -> u32 { if self.format.is_lpcm() && self.sample_rate == 96_000 { 96_000 } else { 48_000 } }

    /// Debit du flux sur le disque (kb/s) ; `channels` / `source_kbps` : canaux et debit de la piste source (0 si inconnus),
    /// `project_kbps` : debit AC-3 du projet.
    pub fn kbps(&self, channels: u32, source_kbps: u32, project_kbps: u32) -> u32 {
        match self.format {
            AudioFormat::Ac3 => if self.bitrate != 0 { self.bitrate } else { project_kbps },
            AudioFormat::Ac3Surround => if self.bitrate != 0 { self.bitrate } else { AC3_SURROUND_KBPS },
            AudioFormat::Ac3Copy => if source_kbps != 0 { source_kbps } else { AC3_MAX_KBPS },
            AudioFormat::DtsCopy => if source_kbps != 0 { source_kbps } else { DTS_HD_KBPS },
            f => f.kbps(self.lpcm_out_channels(channels), project_kbps) * (self.rate() / 48_000),
        }
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
            format: match v.get("format").and_then(Value::as_str) {
                None => AudioFormat::Ac3,
                Some(s) => AudioFormat::from_name(s).ok_or_else(|| trf!("audio tracks: unknown format “{}”", s))?,
            },
            bitrate: v.get("bitrate_kbps").and_then(Value::as_f64).map_or(0, |x| x.round().max(0.0) as u32),
            lpcm_channels: v.get("lpcm_channels").and_then(Value::as_str).and_then(LpcmChannels::from_name).unwrap_or_default(),
            sample_rate: v.get("sample_rate_hz").and_then(Value::as_f64).map_or(0, |x| if x.round() as u32 == 48_000 { 0 } else { x.round().max(0.0) as u32 }),
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
        if self.format != AudioFormat::Ac3 { o.push(("format".into(), Value::Str(self.format.name().into()))); }
        if self.bitrate != 0 { o.push(("bitrate_kbps".into(), Value::Num(self.bitrate as f64))); }
        if self.sample_rate != 0 { o.push(("sample_rate_hz".into(), Value::Num(self.sample_rate as f64))); }
        if self.lpcm_channels != LpcmChannels::Source { o.push(("lpcm_channels".into(), Value::Str(self.lpcm_channels.name().into()))); }
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
        if self.bitrate != 0 && (!self.format.is_encoded_ac3() || !AC3_RATES.contains(&self.bitrate)) {
            p.push(tr("audio bitrate: only the Dolby Digital rates 32 to 640 kb/s are allowed, and only for encoded Dolby Digital").to_string());
        }
        if self.lpcm_channels != LpcmChannels::Source && !self.format.is_lpcm() {
            p.push(tr("channels: the choice of channels is only available for LPCM").to_string());
        }
        if self.sample_rate != 0 && (self.sample_rate != 96_000 || !self.format.is_lpcm()) {
            p.push(tr("sample rate: 96 kHz is only available for LPCM").to_string());
        }
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
