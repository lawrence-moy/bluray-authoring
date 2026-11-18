//! Encodage video materiel (Intel Quick Sync, NVIDIA NVENC, AMD AMF, VAAPI sous Linux, Media Foundation sous Windows) : options
//! ffmpeg de chaque encodeur, choix courant et detection. Apple VideoToolbox n'est pas propose : il n'a aucun reglage pour les
//! parametres HRD et les messages de temps qu'exige le Blu-ray (et personne ici ne peut l'essayer).
//!
//! La detection ne reconnait pas un modele de carte : elle essaie chaque encodeur que ffmpeg connait, avec les reglages
//! Blu-ray, sur quelques secondes de mire, puis juge le flux produit avec `h264check`. Elle n'est lancee qu'a la demande.
//! A chaque vrai encodage, le flux est verifie de nouveau ; s'il n'est pas conforme, la video est reencodee avec x264.
use crate::ffmpeg::{no_window, Error, Result, Scan, Tools};
use crate::i18n::tr;
use crate::trf;
use std::io::Read;
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::cell::Cell;
use std::time::{Duration, Instant};

/// Encodeur video.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum VideoEncoder {
    /// logiciel (libx264), la reference
    #[default]
    X264,
    Qsv,
    Nvenc,
    Amf,
    /// Linux (Intel, AMD)
    Vaapi,
    /// Windows, toute carte ayant un pilote video (secours, peu de reglages)
    Mf,
}

impl VideoEncoder {
    pub const HARDWARE: [VideoEncoder; 5] = [VideoEncoder::Qsv, VideoEncoder::Nvenc, VideoEncoder::Amf, VideoEncoder::Vaapi, VideoEncoder::Mf];
    pub const ALL: [VideoEncoder; 6] = [VideoEncoder::X264, VideoEncoder::Qsv, VideoEncoder::Nvenc, VideoEncoder::Amf, VideoEncoder::Vaapi, VideoEncoder::Mf];

    /// L'encodeur existe sur ce systeme (VAAPI : Linux, Media Foundation : Windows).
    pub fn on_this_system(self) -> bool {
        match self {
            VideoEncoder::Vaapi => cfg!(target_os = "linux"),
            VideoEncoder::Mf => cfg!(windows),
            _ => true,
        }
    }

    /// Nom enregistre dans la configuration.
    pub fn name(self) -> &'static str {
        match self {
            VideoEncoder::X264 => "x264", VideoEncoder::Qsv => "qsv", VideoEncoder::Nvenc => "nvenc", VideoEncoder::Amf => "amf",
            VideoEncoder::Vaapi => "vaapi", VideoEncoder::Mf => "mf",
        }
    }
    pub fn from_name(s: &str) -> Option<VideoEncoder> { VideoEncoder::ALL.into_iter().find(|e| e.name() == s) }
    /// Nom de l'encodeur dans ffmpeg.
    pub fn codec(self) -> &'static str {
        match self {
            VideoEncoder::X264 => "libx264", VideoEncoder::Qsv => "h264_qsv", VideoEncoder::Nvenc => "h264_nvenc", VideoEncoder::Amf => "h264_amf",
            VideoEncoder::Vaapi => "h264_vaapi", VideoEncoder::Mf => "h264_mf",
        }
    }
    /// Libelle (anglais, a traduire).
    pub fn label(self) -> &'static str {
        match self {
            VideoEncoder::X264 => "Software (x264)",
            VideoEncoder::Qsv => "Intel Quick Sync",
            VideoEncoder::Nvenc => "NVIDIA NVENC",
            VideoEncoder::Amf => "AMD AMF",
            VideoEncoder::Vaapi => "VAAPI (Linux)",
            VideoEncoder::Mf => "Windows Media Foundation",
        }
    }

    /// Options a placer avant la source (ouverture du peripherique) : VAAPI seulement.
    pub fn input_args(self) -> Vec<String> {
        match self {
            VideoEncoder::Vaapi => vec!["-vaapi_device".into(), vaapi_device()],
            _ => Vec::new(),
        }
    }

    /// Fin du filtre video : format des images remises a l'encodeur (NV12), puis envoi dans la memoire de la carte (VAAPI).
    pub fn filter_tail(self) -> &'static str {
        match self {
            VideoEncoder::X264 => ",format=yuv420p",
            VideoEncoder::Vaapi => ",format=nv12,hwupload",
            _ => ",format=nv12",
        }
    }
}

/// Premier peripherique de rendu de Linux (`/dev/dri/renderD128` le plus souvent).
fn vaapi_device() -> String {
    let mut nodes: Vec<String> = std::fs::read_dir("/dev/dri").into_iter().flatten().flatten()
        .map(|e| e.path().display().to_string()).filter(|p| p.contains("renderD")).collect();
    nodes.sort();
    nodes.into_iter().next().unwrap_or_else(|| "/dev/dri/renderD128".into())
}

thread_local! {
    /// Encodeur choisi pour les videos du disque et s'il sait coder l'entrelace (reglage de l'application, pas du projet) :
    /// propre au fil qui genere le disque, comme `ffmpeg::set_command_trace`.
    static CURRENT: Cell<(VideoEncoder, bool)> = const { Cell::new((VideoEncoder::X264, false)) };
}

/// Fixe l'encodeur des generations de ce fil ; `interlaced` : il code l'entrelace de facon conforme (vu par la detection).
pub fn set_video_encoder(e: VideoEncoder, interlaced: bool) { CURRENT.with(|c| c.set((e, interlaced))); }

/// Encodeur choisi dans ce fil et sa capacite a coder l'entrelace.
pub fn video_encoder() -> (VideoEncoder, bool) { CURRENT.with(Cell::get) }

/// Preset x264 -> preset de l'encodeur materiel (meme compromis vitesse / qualite, a peu pres).
fn preset_for(e: VideoEncoder, x264: &str) -> &'static str {
    let rank = ["ultrafast", "superfast", "veryfast", "faster", "fast", "medium", "slow", "slower", "veryslow", "placebo"]
        .iter().position(|p| *p == x264).unwrap_or(5);
    match e {
        VideoEncoder::Nvenc => ["p1", "p2", "p3", "p4", "p5", "p5", "p6", "p7", "p7", "p7"][rank],
        VideoEncoder::Qsv => ["veryfast", "veryfast", "veryfast", "faster", "fast", "medium", "slow", "slower", "veryslow", "veryslow"][rank],
        VideoEncoder::Amf => ["speed", "speed", "speed", "balanced", "balanced", "balanced", "quality", "quality", "quality", "quality"][rank],
        _ => "",
    }
}

/// Options ffmpeg de l'encodeur materiel `e` (video seulement). Le materiel n'a pas de CRF comparable a celui de x264 : le
/// CRF est converti en debit moyen (meme estimation que la jauge du disque), en debit variable plafonne par `maxrate` et un
/// tampon de 30 Mbit. `scan` : `Progressive`, ou `Tff` / `Bff` pour un codage entrelace (un PsF se code en `Tff`).
pub fn hw_args(e: VideoEncoder, crf: u32, kbps: Option<u32>, maxrate: u32, keyint: u32, preset: &str, scan: Scan, height: u32) -> Vec<String> {
    let avg = kbps.unwrap_or_else(|| crate::occupancy::crf_kbps(crf, height).round() as u32).clamp(1000, maxrate);
    let v = |a: &[&str]| a.iter().map(|s| s.to_string()).collect::<Vec<_>>();
    // Media Foundation n'a pas de noms de profils : valeur numerique de High (100)
    let mut out = v(&["-c:v", e.codec(), "-profile:v", if e == VideoEncoder::Mf { "100" } else { "high" }]);
    let (b, m) = (format!("{avg}k"), format!("{maxrate}k"));
    let g = keyint.to_string();
    match e {
        VideoEncoder::Qsv => out.extend(v(&[
            "-preset", preset_for(e, preset), "-level", "41", "-b:v", &b, "-maxrate", &m, "-bufsize", "30000k",
            "-g", &g, "-bf", "3", "-refs", "4", "-idr_interval", "0", "-look_ahead", "0",
            "-aud", "1", "-pic_timing_sei", "1", "-strict:v", "strict",
        ])),
        VideoEncoder::Nvenc => out.extend(v(&[
            "-preset", preset_for(e, preset), "-tune", "hq", "-level", "4.1", "-rc", "vbr", "-b:v", &b, "-maxrate", &m, "-bufsize", "30000k",
            "-g", &g, "-bf", "3", "-refs", "4", "-b_ref_mode", "disabled", "-bluray-compat", "1", "-aud", "1", "-strict_gop", "1",
            "-forced-idr", "1", "-spatial-aq", "1",
        ])),
        VideoEncoder::Amf => out.extend(v(&[
            "-usage", "transcoding", "-quality", preset_for(e, preset), "-level", "41", "-rc", "vbr_peak", "-b:v", &b, "-maxrate", &m,
            "-bufsize", "30000k", "-g", &g, "-bf", "3", "-refs", "4", "-enforce_hrd", "1", "-aud", "1", "-header_insertion_mode", "idr",
        ])),
        // VAAPI : messages de temps (buffering period, picture timing) demandes, toutes les images I sont IDR, B sans pyramide
        VideoEncoder::Vaapi => out.extend(v(&[
            "-level", "41", "-rc_mode", "VBR", "-b:v", &b, "-maxrate", &m, "-bufsize", "30000k", "-g", &g, "-bf", "3", "-b_depth", "1",
            "-idr_interval", "0", "-refs", "4", "-aud", "1", "-sei", "timing",
        ])),
        // Media Foundation : debit variable plafonne, scenario d'archivage, encodage materiel exige
        VideoEncoder::Mf => out.extend(v(&[
            "-hw_encoding", "1", "-rate_control", "pc_vbr", "-scenario", "archive", "-b:v", &b, "-maxrate", &m, "-bufsize", "30000k", "-g", &g, "-bf", "2",
        ])),
        VideoEncoder::X264 => {}
    }
    out.extend(v(&["-color_primaries", "bt709", "-color_trc", "bt709", "-colorspace", "bt709", "-color_range", "tv"]));
    // GOP fermes ; en entrelace, codage par trames (une seule option -flags : une seconde remplacerait la premiere)
    match scan {
        Scan::Tff | Scan::PsF => out.extend(v(&["-flags", "+cgop+ildct+ilme", "-field_order", "tt"])),
        Scan::Bff => out.extend(v(&["-flags", "+cgop+ildct+ilme", "-field_order", "bb"])),
        Scan::Progressive => out.extend(v(&["-flags", "+cgop"])),
    }
    out
}

// ---------------------------------------------------------------- detection

/// Resultat d'un essai.
#[derive(Debug, Clone, PartialEq)]
pub enum Status {
    /// conforme Blu-ray
    Ok,
    /// absent de ce ffmpeg
    NotInFfmpeg,
    /// present dans ffmpeg mais inutilisable ici (pas de carte, pas de pilote) : message de ffmpeg
    Unavailable(String),
    /// l'encodeur a refuse les reglages Blu-ray : message de ffmpeg
    Refused(String),
    /// le flux produit n'est pas conforme : defauts trouves
    NotCompliant(Vec<String>),
}

/// Essai d'un encodeur.
#[derive(Debug, Clone, PartialEq)]
pub struct Probe {
    pub encoder: VideoEncoder,
    /// progressif (1080p 23,976)
    pub status: Status,
    /// images par seconde de l'essai progressif (0 si echec)
    pub fps: f64,
    /// entrelace (1080i 25, aussi utilise pour le 1080p 25 des sources PAL) : `None` si l'essai progressif a echoue
    pub interlaced: Option<Status>,
}

impl Probe {
    pub fn usable(&self) -> bool { self.status == Status::Ok }
    pub fn interlaced_ok(&self) -> bool { self.interlaced == Some(Status::Ok) }
}

/// Resultat de la detection : chaque encodeur materiel connu, et la vitesse de x264 (preset medium) sur le meme essai.
#[derive(Debug, Clone, PartialEq)]
pub struct Detection {
    pub probes: Vec<Probe>,
    pub x264_fps: f64,
}

/// Encodeurs H.264 que ce ffmpeg connait (sortie de `ffmpeg -encoders`).
pub fn listed_encoders(encoders_output: &str) -> Vec<VideoEncoder> {
    VideoEncoder::HARDWARE.into_iter()
        .filter(|e| encoders_output.lines().any(|l| l.split_whitespace().nth(1) == Some(e.codec())))
        .collect()
}

/// Classe l'echec d'un essai d'apres les messages de ffmpeg : materiel ou pilote absent, ou reglage refuse.
pub fn classify_failure(stderr: &str) -> Status {
    let low = stderr.to_ascii_lowercase();
    // message de l'encodeur lui-meme (« [h264_qsv @ …] … ») ou d'une option refusee, sinon les dernieres lignes
    let own: Vec<&str> = stderr.lines().map(str::trim)
        .filter(|l| l.starts_with("[h264_") || l.starts_with("[AVHWDeviceContext") || l.contains("Unrecognized option") || l.contains("Error setting option") || l.contains("Option not found"))
        .map(|l| l.split_once("] ").map_or(l, |(_, m)| m)).collect();
    let last: String = if own.is_empty() {
        stderr.lines().rev().map(str::trim).filter(|l| !l.is_empty()).take(3).collect::<Vec<_>>().into_iter().rev().collect::<Vec<_>>().join(" / ")
    } else {
        own.join(" / ")
    };
    if ["unrecognized option", "error setting option", "option not found"].iter().any(|k| low.contains(k)) {
        return Status::Refused(last);
    }
    let device = [
        "no device", "device not found", "cannot load", "failed to load", "could not load", "no capable devices", "no nvenc capable",
        "mfx session", "error creating a mfx", "unsupported device", "driver", "dll", "cuda", "d3d11", "dxva", "failed to initialise",
        "failed to initialize", "not available", "no va display", "openencodesessionex failed", "no qsv",
    ];
    // « invalid level », « invalid param » : le materiel est la, c'est un reglage qui ne passe pas
    if ["invalid param", "invalid level", "not supported by", "unsupported ratecontrol", "selected ratecontrol mode is not supported", "invalid value"].iter().any(|k| low.contains(k)) {
        return Status::Refused(last);
    }
    if device.iter().any(|k| low.contains(k)) { Status::Unavailable(last) } else { Status::Refused(last) }
}

/// Lance ffmpeg avec un delai maximal ; rend (reussi, sortie d'erreur, duree).
fn run_limited(tools: &Tools, args: &[String], limit: Duration, cancel: &AtomicBool) -> Result<(bool, String, Duration)> {
    let mut c = Command::new(&tools.ffmpeg);
    c.args(["-hide_banner", "-nostdin", "-y"]).args(args).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::piped());
    no_window(&mut c);
    let t0 = Instant::now();
    let mut child = c.spawn()?;
    let mut err = child.stderr.take();
    let reader = std::thread::spawn(move || {
        let mut s = String::new();
        if let Some(e) = err.as_mut() { let _ = e.read_to_string(&mut s); }
        s
    });
    let ok = loop {
        if let Some(st) = child.try_wait()? { break st.success(); }
        if cancel.load(Ordering::Relaxed) || t0.elapsed() > limit {
            let _ = child.kill();
            let _ = child.wait();
            if cancel.load(Ordering::Relaxed) { return Err(Error::Cancelled); }
            let s = reader.join().unwrap_or_default();
            return Ok((false, format!("{} {}", tr("no answer from the encoder within the time limit"), s.trim()), t0.elapsed()));
        }
        std::thread::sleep(Duration::from_millis(30));
    };
    let el = t0.elapsed();
    Ok((ok, reader.join().unwrap_or_default(), el))
}

/// Mire de l'essai : (filtre source, duree, cadence, balayage, ecart entre images cles).
const PROGRESSIVE_TRIAL: (&str, u32, Scan, u32) = ("testsrc2=size=1920x1080:rate=24000/1001", 4, Scan::Progressive, 24);
const INTERLACED_TRIAL: (&str, u32, Scan, u32) = ("testsrc2=size=1920x1080:rate=25,setfield=tff", 4, Scan::Tff, 25);

/// Un essai : encode la mire en TS dans `dir`, puis verifie le flux. Rend le statut et les images par seconde.
fn trial(tools: &Tools, e: VideoEncoder, t: (&str, u32, Scan, u32), dir: &Path, cancel: &AtomicBool) -> Result<(Status, f64)> {
    let out = dir.join(format!("essai_{}.ts", e.name()));
    let mut a: Vec<String> = e.input_args();
    a.extend(["-f", "lavfi", "-i", t.0, "-t", &t.1.to_string(), "-an", "-vf", &e.filter_tail()[1..]].iter().map(|s| s.to_string()));
    if e == VideoEncoder::X264 {
        a.truncate(a.len() - 2);
        a.extend(["-c:v", "libx264", "-preset", "medium", "-crf", "18", "-x264-params", "bluray-compat=1:nal-hrd=vbr:vbv-maxrate=30000:vbv-bufsize=30000:level=4.1:keyint=24"].iter().map(|s| s.to_string()));
    } else {
        a.extend(hw_args(e, 18, None, 30000, t.3, "slow", t.2, 1080));
    }
    a.extend(["-f", "mpegts"].iter().map(|s| s.to_string()));
    a.push(out.display().to_string());
    let (ok, stderr, el) = run_limited(tools, &a, Duration::from_secs(90), cancel)?;
    if !ok {
        let _ = std::fs::remove_file(&out);
        return Ok((classify_failure(&stderr), 0.0));
    }
    let data = std::fs::read(&out).unwrap_or_default();
    let _ = std::fs::remove_file(&out);
    let enc = match crate::film::load_encoded(&data) {
        Ok(enc) => enc,
        Err(err) => return Ok((Status::NotCompliant(vec![err.to_string()]), 0.0)),
    };
    let fps = enc.video.len() as f64 / el.as_secs_f64().max(0.001);
    if e == VideoEncoder::X264 { return Ok((Status::Ok, fps)); }
    let mut r = crate::h264check::check(&enc.video, t.3);
    if t.2 != Scan::Progressive && r.sps.as_ref().is_some_and(|s| s.frame_mbs_only) {
        r.issues.push(tr("the encoder did not produce interlaced pictures").to_string());
    }
    Ok((if r.ok() { Status::Ok } else { Status::NotCompliant(r.issues) }, fps))
}

/// Essaie chaque encodeur materiel que ffmpeg connait (progressif puis entrelace), puis x264 pour comparer. `step` recoit
/// l'avancement (0 a 1) et l'essai en cours. Les fichiers d'essai vont dans un dossier temporaire, efface ensuite.
pub fn detect(tools: &Tools, step: &mut dyn FnMut(f32, &str), cancel: &AtomicBool) -> Result<Detection> {
    let listed = {
        let mut c = Command::new(&tools.ffmpeg);
        c.args(["-hide_banner", "-encoders"]).stdin(Stdio::null()).stderr(Stdio::null());
        no_window(&mut c);
        String::from_utf8_lossy(&c.output()?.stdout).into_owned()
    };
    let present = listed_encoders(&listed);
    let dir = std::env::temp_dir().join(format!("bdauthor_hwenc_{}", std::process::id()));
    std::fs::create_dir_all(&dir)?;
    let total = (present.iter().filter(|e| e.on_this_system()).count() * 2 + 1) as f32;
    let mut done = 0.0f32;
    let mut probes = Vec::new();
    let result = (|| -> Result<f64> {
        for e in VideoEncoder::HARDWARE.into_iter().filter(|e| e.on_this_system()) {
            if !present.contains(&e) {
                probes.push(Probe { encoder: e, status: Status::NotInFfmpeg, fps: 0.0, interlaced: None });
                continue;
            }
            step(done / total, &trf!("{}: progressive test", tr(e.label())));
            let (status, fps) = trial(tools, e, PROGRESSIVE_TRIAL, &dir, cancel)?;
            done += 1.0;
            let interlaced = if status == Status::Ok {
                step(done / total, &trf!("{}: interlaced test", tr(e.label())));
                Some(trial(tools, e, INTERLACED_TRIAL, &dir, cancel)?.0)
            } else {
                None
            };
            done += 1.0;
            probes.push(Probe { encoder: e, status, fps, interlaced });
        }
        step(done / total, tr("x264: reference speed"));
        let (_, x264_fps) = trial(tools, VideoEncoder::X264, PROGRESSIVE_TRIAL, &dir, cancel)?;
        step(1.0, "");
        Ok(x264_fps)
    })();
    let _ = std::fs::remove_dir_all(&dir);
    Ok(Detection { probes, x264_fps: result? })
}

// ---------------------------------------------------------------- memorisation

impl Detection {
    /// Texte d'une ligne pour la configuration : « qsv=ok,120.5,i-ok;nvenc=absent;… ;x264=8.2 ».
    pub fn to_config(&self) -> String {
        let st = |s: &Status| match s {
            Status::Ok => "ok", Status::NotInFfmpeg => "absent", Status::Unavailable(_) => "unavailable", Status::Refused(_) => "refused", Status::NotCompliant(_) => "noncompliant",
        };
        let mut parts: Vec<String> = self.probes.iter().map(|p| {
            let i = match &p.interlaced { Some(Status::Ok) => ",i-ok", Some(_) => ",i-no", None => "" };
            format!("{}={},{:.1}{}", p.encoder.name(), st(&p.status), p.fps, i)
        }).collect();
        parts.push(format!("x264={:.1}", self.x264_fps));
        parts.join(";")
    }

    /// Relit `to_config` (les messages detailles ne sont pas gardes).
    pub fn from_config(s: &str) -> Option<Detection> {
        let mut d = Detection { probes: Vec::new(), x264_fps: 0.0 };
        for part in s.split(';').filter(|p| !p.is_empty()) {
            let (k, v) = part.split_once('=')?;
            if k == "x264" { d.x264_fps = v.parse().ok()?; continue; }
            let e = VideoEncoder::from_name(k)?;
            let mut f = v.split(',');
            let status = match f.next()? {
                "ok" => Status::Ok, "absent" => Status::NotInFfmpeg, "unavailable" => Status::Unavailable(String::new()),
                "refused" => Status::Refused(String::new()), _ => Status::NotCompliant(Vec::new()),
            };
            let fps = f.next().and_then(|x| x.parse().ok()).unwrap_or(0.0);
            let interlaced = match f.next() { Some("i-ok") => Some(Status::Ok), Some(_) => Some(Status::NotCompliant(Vec::new())), None => None };
            d.probes.push(Probe { encoder: e, status, fps, interlaced });
        }
        Some(d)
    }

    pub fn probe(&self, e: VideoEncoder) -> Option<&Probe> { self.probes.iter().find(|p| p.encoder == e) }
}

/// L'encodeur choisi s'applique-t-il a une video de balayage `scan` ? (le materiel ne fait pas le PsF de x264 : il le code en
/// entrelace, seulement s'il sait le faire ; idem pour l'entrelace vrai). Le mode « remplir le disque » (deux passes) reste en x264.
pub fn applies(e: VideoEncoder, interlaced_ok: bool, scan: Scan, two_pass: bool) -> bool {
    e != VideoEncoder::X264 && !two_pass && (scan == Scan::Progressive || interlaced_ok)
}
