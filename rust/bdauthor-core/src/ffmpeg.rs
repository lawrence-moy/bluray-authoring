//! Pilotage de ffmpeg / ffprobe : meme logique et memes commandes que `bdauthor/encode.py`.
//!
//! * `Tools::locate` trouve ffmpeg (variable `BDAUTHOR_FFMPEG_DIR`, dossier de l'executable, puis PATH).
//! * `probe` lit les caracteristiques d'une source.
//! * `encode_to_ts` produit le flux MPEG-TS H.264 (High@4.1, GOP ferme, AC3 48 kHz) que le muxeur transformera en .m2ts.
//! * `make_still_ts` : image fixe -> clip video seul. `extract_thumbnail` : une image de la video.
//! La progression est rapportee par `-progress pipe:1` ; l'annulation tue le processus ffmpeg.

use crate::i18n::tr;
use crate::trf;
use std::collections::BTreeMap;
use std::fmt;
use std::io::{BufRead, BufReader, Read};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};

#[derive(Debug)]
pub enum Error {
    Io(std::io::Error),
    /// ffmpeg ou ffprobe introuvable : message a afficher a l'utilisateur.
    NotFound(String),
    Probe(String),
    /// ffmpeg a echoue : code de sortie et dernieres lignes de stderr.
    Ffmpeg { code: Option<i32>, stderr: String },
    Cancelled,
    /// Donnees mal formees (flux TS, PES, AC3...) ou muxage impossible.
    Format(String),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Io(e) => f.write_str(&trf!("I/O error: {}", e)),
            Error::NotFound(m) => write!(f, "{m}"),
            Error::Probe(m) => f.write_str(&trf!("unreadable source: {}", m)),
            Error::Ffmpeg { code, stderr } => f.write_str(&trf!("ffmpeg failed (code {}): {}", format!("{:?}", code), stderr.trim())),
            Error::Cancelled => f.write_str(tr("operation cancelled")),
            Error::Format(m) => f.write_str(&trf!("invalid data: {}", m)),
        }
    }
}
impl std::error::Error for Error {}
impl From<std::io::Error> for Error {
    fn from(e: std::io::Error) -> Self {
        Error::Io(e)
    }
}
impl From<crate::reader::ParseError> for Error {
    fn from(e: crate::reader::ParseError) -> Self {
        Error::Format(e.0)
    }
}
pub type Result<T> = std::result::Result<T, Error>;

// ---------------------------------------------------------------- outils

#[derive(Debug, Clone)]
pub struct Tools {
    pub ffmpeg: PathBuf,
    pub ffprobe: PathBuf,
}

fn exe_name(base: &str) -> String {
    if cfg!(windows) { format!("{base}.exe") } else { base.to_string() }
}

fn works(p: &Path) -> bool {
    let mut c = Command::new(p);
    c.arg("-version").stdout(Stdio::null()).stderr(Stdio::null());
    no_window(&mut c);
    c.status().map(|s| s.success()).unwrap_or(false)
}

static CUSTOM_DIR: std::sync::Mutex<Option<PathBuf>> = std::sync::Mutex::new(None);

impl Tools {
    /// Chemin choisi par l'utilisateur (dossier contenant ffmpeg et ffprobe, ou chemin de ffmpeg lui-meme), essaye avant tout le reste.
    /// Chaine vide = aucun.
    pub fn set_custom_path(p: &str) {
        let p = p.trim();
        if let Ok(mut g) = CUSTOM_DIR.lock() {
            *g = if p.is_empty() { None } else { Some(PathBuf::from(p)) };
        }
    }

    /// Cherche ffmpeg et ffprobe : `BDAUTHOR_FFMPEG_DIR`, dossier de l'executable (et son sous-dossier `ffmpeg`/`bin`), puis PATH.
    pub fn locate() -> Result<Tools> {
        let mut dirs: Vec<PathBuf> = Vec::new();
        if let Some(c) = CUSTOM_DIR.lock().ok().and_then(|g| g.clone()) {
            // un fichier (ffmpeg.exe) designe son dossier ; ffprobe est cherche a cote
            dirs.push(if c.is_file() { c.parent().map(Path::to_path_buf).unwrap_or(c) } else { c });
        }
        if let Some(d) = std::env::var_os("BDAUTHOR_FFMPEG_DIR") {
            dirs.push(PathBuf::from(d));
        }
        if let Ok(exe) = std::env::current_exe() {
            if let Some(d) = exe.parent() {
                dirs.push(d.to_path_buf());
                dirs.push(d.join("ffmpeg"));
                dirs.push(d.join("ffmpeg").join("bin"));
                dirs.push(d.join("bin"));
            }
        }
        for d in dirs {
            let (f, p) = (d.join(exe_name("ffmpeg")), d.join(exe_name("ffprobe")));
            if works(&f) && works(&p) {
                return Ok(Tools { ffmpeg: f, ffprobe: p });
            }
        }
        let (f, p) = (PathBuf::from(exe_name("ffmpeg")), PathBuf::from(exe_name("ffprobe")));
        if works(&f) && works(&p) {
            return Ok(Tools { ffmpeg: f, ffprobe: p });
        }
        Err(Error::NotFound(
            tr("ffmpeg/ffprobe not found: put ffmpeg.exe and ffprobe.exe next to the program, or give their folder in the BDAUTHOR_FFMPEG_DIR variable, or add them to the PATH.")
                .into(),
        ))
    }
}

#[cfg(windows)]
fn no_window(c: &mut Command) {
    use std::os::windows::process::CommandExt;
    c.creation_flags(0x0800_0000); // CREATE_NO_WINDOW : pas de fenetre console qui clignote
}
#[cfg(not(windows))]
fn no_window(_c: &mut Command) {}

// ---------------------------------------------------------------- sondage

#[derive(Debug, Clone, PartialEq, Default)]
pub struct VideoInfo {
    pub width: u32,
    pub height: u32,
    pub fps_num: u32,
    pub fps_den: u32,
    pub pix_fmt: String,
    pub full_range: bool,
    pub codec: String,
    pub duration_s: f64,
    pub has_audio: bool,
    /// Ordre des trames d'une source entrelacee (« tt », « bb », « tb », « bt ») ; vide ou « progressive » sinon.
    pub field_order: String,
    /// Profil H.264 (« High », « Main »...), vide si inconnu.
    pub profile: String,
    /// Niveau H.264 sur 2 chiffres (41 pour 4.1), 0 si inconnu.
    pub level: u32,
    /// Nombre d'images de reference annonce dans le SPS, 0 si inconnu.
    pub refs: u32,
    /// Cadence constante (cadence de base == cadence moyenne) ?
    pub cfr: bool,
}

/// Fraction « n/d » en nombre (0 si invalide).
fn frac(s: &str) -> f64 {
    match s.split_once('/') {
        Some((a, b)) => { let (a, b) = (a.trim().parse::<f64>().unwrap_or(0.0), b.trim().parse::<f64>().unwrap_or(0.0)); if b > 0.0 { a / b } else { 0.0 } }
        None => s.trim().parse().unwrap_or(0.0),
    }
}

impl VideoInfo {
    pub fn fps(&self) -> f64 {
        self.fps_num as f64 / self.fps_den.max(1) as f64
    }
    /// Source entrelacee ?
    pub fn interlaced(&self) -> bool {
        matches!(self.field_order.as_str(), "tt" | "bb" | "tb" | "bt")
    }
    /// Trame du haut en premier (sinon trame du bas) ?
    pub fn top_first(&self) -> bool {
        !matches!(self.field_order.as_str(), "bb" | "bt")
    }
}

/// Analyse la sortie `-of flat` de ffprobe : cles `streams.stream.N.champ` et `format.champ`.
pub fn parse_probe_flat(text: &str) -> Result<VideoInfo> {
    let mut streams: BTreeMap<u32, BTreeMap<String, String>> = BTreeMap::new();
    let mut format: BTreeMap<String, String> = BTreeMap::new();
    for line in text.lines() {
        let Some((k, v)) = line.split_once('=') else { continue };
        let v = v.trim().trim_matches('"').to_string();
        if let Some(rest) = k.strip_prefix("streams.stream.") {
            if let Some((idx, field)) = rest.split_once('.') {
                if let Ok(i) = idx.parse::<u32>() {
                    streams.entry(i).or_default().insert(field.to_string(), v);
                }
            }
        } else if let Some(f) = k.strip_prefix("format.") {
            format.insert(f.to_string(), v);
        }
    }
    let video = streams
        .values()
        .find(|s| s.get("codec_type").map(String::as_str) == Some("video"))
        .ok_or_else(|| Error::Probe(tr("no video stream").into()))?;
    let get = |m: &BTreeMap<String, String>, k: &str| m.get(k).cloned().unwrap_or_default();
    let (n, d) = get(video, "r_frame_rate")
        .split_once('/')
        .map(|(a, b)| (a.parse::<u32>().unwrap_or(0), b.parse::<u32>().unwrap_or(1)))
        .unwrap_or((0, 1));
    if n == 0 {
        return Err(Error::Probe(tr("unreadable frame rate").into()));
    }
    let pix_fmt = get(video, "pix_fmt");
    Ok(VideoInfo {
        width: get(video, "width").parse().map_err(|_| Error::Probe("largeur".into()))?,
        height: get(video, "height").parse().map_err(|_| Error::Probe("hauteur".into()))?,
        fps_num: n,
        fps_den: d.max(1),
        full_range: pix_fmt.starts_with("yuvj") || get(video, "color_range") == "pc",
        pix_fmt,
        codec: get(video, "codec_name"),
        duration_s: get(&format, "duration").parse().unwrap_or(0.0),
        has_audio: streams.values().any(|s| s.get("codec_type").map(String::as_str) == Some("audio")),
        field_order: get(video, "field_order"),
        profile: get(video, "profile"),
        level: get(video, "level").parse::<i64>().ok().filter(|l| *l > 0).map_or(0, |l| l as u32),
        refs: get(video, "refs").parse().unwrap_or(0),
        cfr: {
            let (r, a) = (frac(&get(video, "r_frame_rate")), frac(&get(video, "avg_frame_rate")));
            // ffprobe ne donne pas d'avg_frame_rate pour certains conteneurs : on le tient alors pour constant
            a == 0.0 || (r - a).abs() <= r * 0.002
        },
    })
}

pub fn probe(tools: &Tools, src: &Path) -> Result<VideoInfo> {
    let mut c = Command::new(&tools.ffprobe);
    c.args(["-v", "error", "-of", "flat", "-show_entries"])
        .arg("stream=index,codec_type,codec_name,profile,level,refs,width,height,r_frame_rate,avg_frame_rate,pix_fmt,color_range,field_order:format=duration")
        .arg(src);
    no_window(&mut c);
    let out = c.output()?;
    if !out.status.success() {
        return Err(Error::Probe(String::from_utf8_lossy(&out.stderr).trim().to_string()));
    }
    parse_probe_flat(&String::from_utf8_lossy(&out.stdout))
}

// ---------------------------------------------------------------- cibles et options

#[derive(Debug, Clone, PartialEq)]
pub struct Target {
    pub width: u32,
    pub height: u32,
    pub fps: &'static str,
    pub label: &'static str,
    /// Codage entrelace de la sortie.
    pub scan: Scan,
    /// Desentrelacer la source en doublant la cadence (une image par trame).
    pub deinterlace: bool,
}

/// Mode de balayage de la video encodee.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Scan {
    Progressive,
    /// Images progressives transportees en 1080i (« PsF ») : seule facon admise par le Blu-ray pour 1080p25 et 1080p29,97.
    PsF,
    /// Vrai entrelace, trame du haut / du bas en premier.
    Tff,
    Bff,
}

fn film_fps(fps: f64) -> &'static str {
    if (fps - 23.976).abs() < (fps - 24.0).abs() { "24000/1001" } else { "24" }
}

/// Cible Blu-ray la plus proche de la source (cadence changee par doublement/suppression d'images, jamais par interpolation).
pub fn pick_target(width: u32, height: u32, fps: f64) -> Target {
    pick_target_scan(width, height, fps, None)
}

/// Comme `pick_target`, en tenant compte d'une source entrelacee (`Some(trame du haut en premier ?)`).
/// Formats Blu-ray : 1080p23,976/24, 1080i50/59,94 (dont PsF pour 25p/29,97p), 720p23,976/24/50/59,94.
pub fn pick_target_scan(width: u32, height: u32, fps: f64, interlaced: Option<bool>) -> Target {
    let _ = width;
    let t = |w, h, f, l, scan, deinterlace| Target { width: w, height: h, fps: f, label: l, scan, deinterlace };
    let p = Scan::Progressive;
    // une source entrelacee annoncee en trames par seconde (50, 59,94) a la moitie en images
    let fps = if interlaced.is_some() && fps >= 45.0 { fps / 2.0 } else { fps };
    let near = |a: f64| (fps - a).abs() < 0.6;
    let hd = height > 800;
    if let Some(top) = interlaced {
        let sc = if top { Scan::Tff } else { Scan::Bff };
        if hd && near(25.0) { return t(1920, 1080, "25", "1080i50", sc, false); }
        if hd && (near(29.97) || near(30.0)) { return t(1920, 1080, "30000/1001", "1080i59.94", sc, false); }
        // definition standard entrelacee : desentrelacee en 50 ou 59,94 images par seconde
        if near(25.0) { return t(1280, 720, "50", "720p50 (desentrelace)", p, true); }
        if near(29.97) || near(30.0) { return t(1280, 720, "60000/1001", "720p59.94 (desentrelace)", p, true); }
    }
    if hd && near(25.0) {
        return t(1920, 1080, "25", "1080p25 (PsF)", Scan::PsF, false);
    }
    if hd && (near(29.97) || near(30.0)) {
        return t(1920, 1080, "30000/1001", "1080p29.97 (PsF)", Scan::PsF, false);
    }
    if hd && fps < 26.0 {
        return t(1920, 1080, film_fps(fps), "1080p", p, false);
    }
    if fps >= 55.0 {
        return t(1280, 720, "60000/1001", "720p59.94", p, false);
    }
    if fps >= 45.0 {
        return t(1280, 720, "50", "720p50", p, false);
    }
    if near(25.0) {
        return t(1280, 720, "50", "720p50 (cadence doublee)", p, false);
    }
    if fps >= 26.0 {
        return t(1280, 720, "60000/1001", "720p59.94 (cadence doublee)", p, false);
    }
    t(1280, 720, film_fps(fps), "720p24", p, false)
}

/// Balayage impose pour une image fixe ou un fond de menu de `size` a la cadence `fps` (1080 a 25 ou 29,97 i/s : PsF).
pub fn scan_for(size: (u32, u32), fps: &str) -> Scan {
    if size.1 == 1080 && matches!(fps, "25" | "30000/1001") { Scan::PsF } else { Scan::Progressive }
}

#[derive(Debug, Clone)]
pub struct EncodeOpts {
    pub crf: u32,
    pub maxrate: u32,
    pub keyint: u32,
    pub audio_bitrate: String,
    pub preset: String,
    /// Instants (secondes depuis le debut) ou placer une image cle IDR : chapitres exacts. Vide : GOP reguliers seulement.
    pub force_keys: Vec<f64>,
    /// Copier le flux video tel quel (sans le reencoder) : seulement pour une source deja conforme (voir `copy_blockers`).
    pub copy_video: bool,
    /// Debit video moyen vise (kb/s) : encodage en deux passes au lieu du CRF (remplir le disque). `None` : CRF.
    pub video_kbps: Option<u32>,
}

impl Default for EncodeOpts {
    fn default() -> Self {
        EncodeOpts { crf: 18, maxrate: 30000, keyint: 30, audio_bitrate: "192k".into(), preset: "medium".into(), force_keys: Vec::new(), copy_video: false, video_kbps: None }
    }
}

fn x264_args_scan(crf: u32, maxrate: u32, keyint: u32, preset: &str, scan: Scan) -> Vec<String> {
    x264_args_rate(crf, None, maxrate, keyint, preset, scan)
}

/// Comme `x264_args_scan`, avec un debit moyen vise (`kbps`) a la place du CRF.
fn x264_args_rate(crf: u32, kbps: Option<u32>, maxrate: u32, keyint: u32, preset: &str, scan: Scan) -> Vec<String> {
    let mut x264 = format!(
        "bluray-compat=1:keyint={keyint}:open-gop=0:aud=1:nal-hrd=vbr:vbv-maxrate={maxrate}:vbv-bufsize=30000:level=4.1:bframes=3"
    );
    match scan {
        Scan::Progressive => {}
        Scan::PsF => x264.push_str(":fake-interlaced=1:pic-struct=1"),
        Scan::Tff => x264.push_str(":tff=1"),
        Scan::Bff => x264.push_str(":bff=1"),
    }
    let v = |a: &[&str]| a.iter().map(|s| s.to_string()).collect::<Vec<_>>();
    let mut out = v(&["-c:v", "libx264", "-profile:v", "high", "-preset", preset]);
    match kbps {
        Some(k) => { out.push("-b:v".into()); out.push(format!("{k}k")); }
        None => { out.push("-crf".into()); out.push(crf.to_string()); }
    }
    out.extend(v(&["-x264-params"]));
    out.push(x264);
    out.extend(v(&["-color_primaries", "bt709", "-color_trc", "bt709", "-colorspace", "bt709", "-color_range", "tv"]));
    match scan {
        Scan::Tff => out.extend(v(&["-flags", "+ildct+ilme", "-field_order", "tt"])),
        Scan::Bff => out.extend(v(&["-flags", "+ildct+ilme", "-field_order", "bb"])),
        _ => {}
    }
    out
}

// ---------------------------------------------------------------- execution

/// Lance ffmpeg avec `-progress pipe:1`, rapporte la progression (0.0..=1.0 d'apres `duration_s`) et gere l'annulation.
/// Avancement detaille d'un encodage ffmpeg en cours (voir `set_progress_detail`).
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct FfmpegProgress {
    /// position atteinte dans la source (s)
    pub time_s: f64,
    /// duree a traiter (s) ; 0 si inconnue
    pub duration_s: f64,
    /// images par seconde encodees
    pub fps: f64,
    /// vitesse par rapport au temps reel (2.0 : deux fois plus vite)
    pub speed: f64,
}

impl FfmpegProgress {
    /// Temps restant estime (s) d'apres la vitesse.
    pub fn remaining_s(&self) -> Option<f64> {
        (self.speed > 0.0 && self.duration_s > 0.0).then(|| (self.duration_s - self.time_s).max(0.0) / self.speed)
    }
}

thread_local! {
    static DETAIL: std::cell::RefCell<Option<Box<dyn FnMut(FfmpegProgress)>>> = const { std::cell::RefCell::new(None) };
}

/// Recoit, dans le fil qui appelle ffmpeg, l'avancement detaille de chaque encodage (environ deux fois par seconde) ; `None`
/// pour arreter. Propre au fil appelant : les autres fils (apercus, vignettes) ne sont pas concernes.
pub fn set_progress_detail(f: Option<Box<dyn FnMut(FfmpegProgress)>>) {
    DETAIL.with(|d| *d.borrow_mut() = f);
}

/// Analyse une ligne de `-progress` ; rend vrai a la fin d'un bloc (ligne `progress=`).
fn parse_progress_line(line: &str, cur: &mut FfmpegProgress) -> bool {
    let Some((k, v)) = line.split_once('=') else { return false };
    let v = v.trim();
    match k {
        "out_time_us" | "out_time_ms" => { if let Ok(us) = v.parse::<i64>() { cur.time_s = us.max(0) as f64 / 1e6; } }
        "fps" => { if let Ok(f) = v.parse::<f64>() { cur.fps = f; } }
        "speed" => { if let Ok(x) = v.trim_end_matches('x').trim().parse::<f64>() { cur.speed = x; } }
        "progress" => return true,
        _ => {}
    }
    false
}

fn run_ffmpeg(
    tools: &Tools,
    args: &[String],
    duration_s: f64,
    progress: &mut dyn FnMut(f64),
    cancel: &AtomicBool,
) -> Result<()> {
    let mut c = Command::new(&tools.ffmpeg);
    c.args(["-y", "-v", "error", "-nostats", "-progress", "pipe:1"]).args(args);
    c.stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped());
    no_window(&mut c);
    let mut child: Child = c.spawn()?;
    let mut stderr = child.stderr.take().expect("stderr");
    let err_thread = std::thread::spawn(move || {
        let mut s = String::new();
        let _ = stderr.read_to_string(&mut s);
        s
    });
    let stdout = child.stdout.take().expect("stdout");
    let mut cur = FfmpegProgress { duration_s: duration_s.max(0.0), ..Default::default() };
    for line in BufReader::new(stdout).lines() {
        if cancel.load(Ordering::Relaxed) {
            let _ = child.kill();
            let _ = child.wait();
            let _ = err_thread.join();
            return Err(Error::Cancelled);
        }
        let Ok(line) = line else { break };
        if line.starts_with("out_time_us=") && duration_s > 0.0 {
            if let Ok(us) = line["out_time_us=".len()..].trim().parse::<i64>() { progress((us.max(0) as f64 / 1e6 / duration_s).min(1.0)); }
        }
        if parse_progress_line(&line, &mut cur) {
            DETAIL.with(|d| if let Some(f) = d.borrow_mut().as_mut() { f(cur) });
        }
    }
    let status = child.wait()?;
    let err = err_thread.join().unwrap_or_default();
    if cancel.load(Ordering::Relaxed) {
        return Err(Error::Cancelled);
    }
    if !status.success() {
        let tail: Vec<&str> = err.lines().rev().take(8).collect();
        return Err(Error::Ffmpeg { code: status.code(), stderr: tail.into_iter().rev().collect::<Vec<_>>().join("\n") });
    }
    progress(1.0);
    Ok(())
}

fn s(p: &Path) -> String {
    p.to_string_lossy().into_owned()
}

/// Construit les arguments ffmpeg (sans les options communes) pour le transcodage d'une video. Public pour les tests.
/// Une source sans piste audio recoit une piste muette (le disque reste homogene : chaque video a son audio).
pub fn encode_args(src: &Path, dst: &Path, info: &VideoInfo, o: &EncodeOpts) -> (Vec<String>, Target) {
    encode_args_tracks(src, dst, info, o, &[])
}

/// Comme `encode_args`, avec des pistes audio choisies (dans l'ordre du disque) ; `tracks` vide : la premiere piste audio du fichier.
/// Les fichiers audio doivent avoir un chemin utilisable tel quel.
pub fn encode_args_tracks(src: &Path, dst: &Path, info: &VideoInfo, o: &EncodeOpts, tracks: &[crate::audio::AudioTrackCfg]) -> (Vec<String>, Target) {
    encode_args_full(src, dst, info, o, tracks, None)
}

fn target_of(info: &VideoInfo) -> Target {
    pick_target_scan(info.width, info.height, info.fps(), info.interlaced().then(|| info.top_first()))
}

/// Filtre video qui amene la source a la cible Blu-ray `tg`.
fn video_filter(info: &VideoInfo, tg: &Target) -> String {
    let interl = matches!(tg.scan, Scan::Tff | Scan::Bff);
    let pre = if tg.deinterlace { "bwdif=mode=send_field," } else { "" };
    format!(
        "{pre}fps={},scale={}:{}:in_range={}:in_color_matrix=bt709:out_range=tv:out_color_matrix=bt709{},format=yuv420p",
        tg.fps, tg.width, tg.height, if info.full_range { "pc" } else { "tv" }, if interl { ":interl=1" } else { "" }
    )
}

/// Arguments de la premiere passe d'un encodage a debit cible : video seule, sortie jetee, statistiques dans `log`.
pub fn first_pass_args(src: &Path, info: &VideoInfo, o: &EncodeOpts, log: &Path) -> (Vec<String>, Target) {
    let tg = target_of(info);
    let mut a: Vec<String> = vec!["-i".into(), s(src), "-map".into(), "0:v:0".into(), "-an".into(), "-vf".into(), video_filter(info, &tg)];
    a.extend(x264_args_rate(o.crf, o.video_kbps, o.maxrate, o.keyint, &o.preset, tg.scan));
    a.extend(force_keys_args(&o.force_keys));
    a.extend(["-pass".to_string(), "1".into(), "-passlogfile".into(), s(log), "-f".into(), "null".into(), if cfg!(windows) { "NUL".into() } else { "/dev/null".into() }]);
    (a, tg)
}

/// `pass` : `Some((2, journal))` pour la seconde passe d'un encodage a debit cible.
fn encode_args_full(src: &Path, dst: &Path, info: &VideoInfo, o: &EncodeOpts, tracks: &[crate::audio::AudioTrackCfg], pass: Option<(u8, &Path)>) -> (Vec<String>, Target) {
    use crate::audio::AudioSource;
    let tg = target_of(info);
    let vf = video_filter(info, &tg);
    let mut a: Vec<String> = ["-i"].iter().map(|x| x.to_string()).collect();
    a.push(s(src));
    if !tracks.is_empty() {
        // une entree de plus pour chaque fichier audio et pour chaque piste decalee de la video ; les autres pistes viennent de l'entree 0
        let mut maps: Vec<String> = vec!["0:v:0".into()];
        let mut next_input = 1usize;
        for t in tracks {
            let shifted = t.offset_ms != 0;
            let (path, map_tail) = match &t.source {
                AudioSource::Embedded(n) => (src.to_path_buf(), format!("a:{n}")),
                AudioSource::File(f) => (std::path::PathBuf::from(f), "a:0".to_string()),
            };
            if matches!(t.source, AudioSource::Embedded(_)) && !shifted {
                maps.push(format!("0:{map_tail}"));
            } else {
                if shifted { a.extend(["-itsoffset".to_string(), format!("{:.3}", t.offset_ms as f64 / 1000.0)]); }
                a.extend(["-i".to_string(), s(&path)]);
                maps.push(format!("{next_input}:{map_tail}"));
                next_input += 1;
            }
        }
        for m in maps { a.extend(["-map".to_string(), m]); }
        a.push("-vf".into());
    } else if info.has_audio {
        a.extend(["-map", "0:v:0", "-map", "0:a:0", "-vf"].iter().map(|x| x.to_string()));
    } else {
        a.extend(["-f", "lavfi", "-i", "anullsrc=channel_layout=stereo:sample_rate=48000", "-map", "0:v:0", "-map", "1:a:0", "-shortest", "-vf"].iter().map(|x| x.to_string()));
    }
    if o.copy_video {
        // flux video recopie tel quel (pas de filtre) ; le muxeur MPEG-TS ajoute les delimiteurs d'unite d'acces et passe en Annex B
        a.pop();
        a.extend(["-c:v", "copy"].iter().map(|x| x.to_string()));
    } else {
        a.push(vf);
        a.extend(x264_args_rate(o.crf, o.video_kbps, o.maxrate, o.keyint, &o.preset, tg.scan));
        a.extend(force_keys_args(&o.force_keys));
        if let Some((n, log)) = pass { a.extend(["-pass".to_string(), n.to_string(), "-passlogfile".into(), s(log)]); }
    }
    a.extend(["-c:a", "ac3", "-b:a"].iter().map(|x| x.to_string()));
    a.push(o.audio_bitrate.clone());
    a.extend(["-ar", "48000", "-ac", "2", "-f", "mpegts", "-pes_payload_size", "0"].iter().map(|x| x.to_string()));
    a.push(s(dst));
    (a, tg)
}

/// Video quelconque -> MPEG-TS Blu-ray (H.264 + AC3). Retourne la cible choisie.
pub fn encode_to_ts(
    tools: &Tools,
    src: &Path,
    dst: &Path,
    info: &VideoInfo,
    opts: &EncodeOpts,
    progress: &mut dyn FnMut(f64),
    cancel: &AtomicBool,
) -> Result<Target> {
    encode_to_ts_tracks(tools, src, dst, info, opts, &[], progress, cancel)
}

/// Comme `encode_to_ts`, avec des pistes audio choisies (voir `encode_args_tracks`).
#[allow(clippy::too_many_arguments)]
pub fn encode_to_ts_tracks(
    tools: &Tools,
    src: &Path,
    dst: &Path,
    info: &VideoInfo,
    opts: &EncodeOpts,
    tracks: &[crate::audio::AudioTrackCfg],
    progress: &mut dyn FnMut(f64),
    cancel: &AtomicBool,
) -> Result<Target> {
    if opts.video_kbps.is_some() && !opts.copy_video {
        // debit cible : premiere passe (analyse, 30 % du temps annonce), puis seconde passe (le TS definitif)
        let log = dst.with_extension("x264");
        let (a1, _) = first_pass_args(src, info, opts, &log);
        run_ffmpeg(tools, &a1, info.duration_s, &mut |f| progress(f * 0.3), cancel)?;
        let (a2, tg) = encode_args_full(src, dst, info, opts, tracks, Some((2, &log)));
        let r = run_ffmpeg(tools, &a2, info.duration_s, &mut |f| progress(0.3 + f * 0.7), cancel);
        for suffix in ["-0.log", "-0.log.mbtree"] { let _ = std::fs::remove_file(format!("{}{suffix}", log.display())); }
        r?;
        return Ok(tg);
    }
    let (args, tg) = encode_args_tracks(src, dst, info, opts, tracks);
    run_ffmpeg(tools, &args, info.duration_s, progress, cancel)?;
    Ok(tg)
}

/// Image fixe -> clip video seul (sans audio) de `seconds` secondes au format `size`.
pub fn make_still_ts(
    tools: &Tools,
    image: &Path,
    dst: &Path,
    seconds: f64,
    size: (u32, u32),
    fps: &str,
    opts: &EncodeOpts,
    cancel: &AtomicBool,
) -> Result<()> {
    let vf = format!("scale={}:{}:out_color_matrix=bt709:out_range=tv,format=yuv420p", size.0, size.1);
    let mut a: Vec<String> = ["-loop", "1", "-framerate"].iter().map(|x| x.to_string()).collect();
    a.push(fps.to_string());
    a.push("-t".into());
    a.push(format!("{seconds}"));
    a.push("-i".into());
    a.push(s(image));
    a.push("-vf".into());
    a.push(vf);
    a.extend(x264_args_scan(opts.crf, 30000, 30, &opts.preset, scan_for(size, fps)));
    a.extend(["-an", "-f", "mpegts"].iter().map(|x| x.to_string()));
    a.push(s(dst));
    run_ffmpeg(tools, &a, seconds, &mut |_| {}, cancel)
}

/// Duree (s) d'un fichier audio ou video.
pub fn probe_duration(tools: &Tools, src: &Path) -> Result<f64> {
    let mut c = Command::new(&tools.ffprobe);
    c.args(["-v", "error", "-of", "flat", "-show_entries", "format=duration"]).arg(src);
    no_window(&mut c);
    let out = c.output()?;
    if !out.status.success() {
        return Err(Error::Probe(String::from_utf8_lossy(&out.stderr).trim().to_string()));
    }
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .find_map(|l| l.strip_prefix("format.duration=").map(|v| v.trim().trim_matches('"').parse::<f64>()))
        .and_then(|r| r.ok())
        .filter(|d| *d > 0.0)
        .ok_or_else(|| Error::Probe(trf!("{}: unreadable duration", src.display())))
}

/// Visuel d'un fond de menu anime ou sonore.
#[derive(Debug, Clone, Copy)]
pub enum BgVisual<'a> {
    Image(&'a Path),
    /// Video (et faut-il garder son propre son, a defaut de musique ?)
    Video(&'a Path, bool),
}

/// Fond de menu -> MPEG-TS de `seconds` secondes au format `size` / `fps` : visuel (image fixe ou video recadree sans
/// deformation), calque transparent `layer` pose par-dessus (textes et images de la page), son (`music`, sinon celui de la video).
#[allow(clippy::too_many_arguments)]
pub fn make_menu_bg_ts(
    tools: &Tools, visual: BgVisual, layer: Option<&Path>, music: Option<&Path>, dst: &Path, seconds: f64, size: (u32, u32), fps: &str,
    opts: &EncodeOpts, cancel: &AtomicBool,
) -> Result<()> {
    let st = |x: &str| x.to_string();
    let t = format!("{seconds:.3}");
    let mut a: Vec<String> = Vec::new();
    match visual {
        BgVisual::Image(p) => a.extend([st("-loop"), st("1"), st("-framerate"), st(fps), st("-t"), t.clone(), st("-i"), s(p)]),
        BgVisual::Video(p, _) => a.extend([st("-t"), t.clone(), st("-i"), s(p)]),
    }
    let mut next = 1;
    let layer_idx = layer.map(|l| {
        a.extend([st("-loop"), st("1"), st("-framerate"), st(fps), st("-t"), t.clone(), st("-i"), s(l)]);
        next += 1;
        next - 1
    });
    let music_idx = music.map(|m| {
        a.extend([st("-t"), t.clone(), st("-i"), s(m)]);
        next += 1;
        next - 1
    });
    let mut fc = format!(
        "[0:v]fps={fps},scale={w}:{h}:force_original_aspect_ratio=increase:out_color_matrix=bt709:out_range=tv,crop={w}:{h},setsar=1[b]",
        w = size.0, h = size.1
    );
    let mut last = "b".to_string();
    if let Some(i) = layer_idx {
        fc.push_str(&format!(";[{last}][{i}:v]overlay=0:0[c]"));
        last = "c".into();
    }
    fc.push_str(&format!(";[{last}]format=yuv420p[v]"));
    a.extend([st("-filter_complex"), fc, st("-map"), st("[v]")]);
    let audio = match (music_idx, visual) {
        (Some(i), _) => Some(format!("{i}:a:0")),
        (None, BgVisual::Video(_, true)) => Some("0:a:0".to_string()),
        _ => None,
    };
    a.extend(x264_args_scan(opts.crf, 30000, 30, &opts.preset, scan_for(size, fps)));
    match audio {
        Some(m) => {
            a.extend([st("-map"), m, st("-c:a"), st("ac3"), st("-b:a"), opts.audio_bitrate.clone(), st("-ar"), st("48000"), st("-ac"), st("2")]);
        }
        None => a.push(st("-an")),
    }
    a.extend([st("-t"), t, st("-f"), st("mpegts"), st("-pes_payload_size"), st("0"), s(dst)]);
    run_ffmpeg(tools, &a, seconds, &mut |_| {}, cancel)
}

/// Une image de la video, redimensionnee et rognee (sans deformation) a `size`. `at` en secondes (defaut : 25 % de la duree, 1 s au plus).
pub fn extract_thumbnail(tools: &Tools, src: &Path, dst_png: &Path, size: (u32, u32), at: Option<f64>, duration_s: f64) -> Result<()> {
    let t = at.unwrap_or_else(|| (duration_s * 0.25).min(1.0));
    let vf = format!("scale={0}:{1}:force_original_aspect_ratio=increase,crop={0}:{1}", size.0, size.1);
    let a: Vec<String> = vec![
        "-ss".into(), format!("{t:.3}"), "-i".into(), s(src), "-frames:v".into(), "1".into(), "-vf".into(), vf, s(dst_png),
    ];
    run_ffmpeg(tools, &a, 0.0, &mut |_| {}, &AtomicBool::new(false))
}

/// Comme `extract_thumbnail`, mais retourne les pixels RGBA (aucun fichier image intermediaire a decoder).
pub fn extract_thumbnail_rgba(tools: &Tools, src: &Path, size: (u32, u32), at: Option<f64>, duration_s: f64) -> Result<crate::palette::RgbaImage> {
    let t = at.unwrap_or_else(|| (duration_s * 0.25).min(1.0));
    let vf = format!("scale={0}:{1}:force_original_aspect_ratio=increase,crop={0}:{1}", size.0, size.1);
    let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0);
    let tmp = std::env::temp_dir().join(format!("bdauthor_thumb_{}_{nanos}.rgba", std::process::id()));
    let a: Vec<String> = vec![
        "-ss".into(), format!("{t:.3}"), "-i".into(), s(src), "-frames:v".into(), "1".into(), "-vf".into(), vf,
        "-f".into(), "rawvideo".into(), "-pix_fmt".into(), "rgba".into(), s(&tmp),
    ];
    let r = run_ffmpeg(tools, &a, 0.0, &mut |_| {}, &AtomicBool::new(false));
    let data = std::fs::read(&tmp);
    let _ = std::fs::remove_file(&tmp);
    r?;
    crate::palette::RgbaImage::from_raw(size.0, size.1, data?).map_err(|_| Error::Format(tr("thumbnail: unexpected size (video too short?)").into()))
}

/// Arguments `-force_key_frames` (images cles IDR aux instants donnes) ; aucun si la liste est vide. Chaque instant est avance
/// d'une demi-milliseconde : ffmpeg force la premiere image dont l'horodatage atteint l'instant, et un chapitre enregistre au
/// dix-millieme pres pourrait sinon tomber juste apres son image et forcer la suivante.
pub fn force_keys_args(times: &[f64]) -> Vec<String> {
    let t: Vec<String> = times.iter().filter(|t| **t > 0.0).map(|t| format!("{:.4}", (t - 0.0005).max(0.0))).collect();
    if t.is_empty() { return Vec::new(); }
    vec!["-force_key_frames".into(), t.join(","), "-forced-idr".into(), "1".into()]
}

/// Arguments ffmpeg d'une image de la video a l'instant `t`, en pixels RGBA de `size` sur la sortie standard.
/// `exact` : l'image exacte (decodage depuis l'image cle qui precede) ; sinon l'image cle la plus proche (bien plus rapide).
pub fn frame_args(src: &Path, t: f64, size: (u32, u32), exact: bool) -> Vec<String> {
    let mut a: Vec<String> = vec!["-v".into(), "error".into(), "-nostdin".into(), "-ss".into(), format!("{:.4}", t.max(0.0))];
    if !exact { a.push("-noaccurate_seek".into()); }
    // vignette : un seul fil de decodage par ffmpeg (plusieurs tournent en parallele) et mise a l'echelle rapide
    let thumb = size.0 <= 320;
    if thumb { a.extend(["-threads".to_string(), "1".into()]); }
    a.extend(["-i".to_string(), s(src), "-frames:v".into(), "1".into(), "-an".into(), "-sn".into(), "-vf".into(),
        format!("scale={}:{}{}", size.0, size.1, if thumb { ":flags=fast_bilinear" } else { "" })]);
    a.extend(["-f", "rawvideo", "-pix_fmt", "rgba", "pipe:1"].iter().map(|x| x.to_string()));
    a
}

/// Une image de la video a l'instant `t` (voir `frame_args`), sans fichier intermediaire.
pub fn extract_frame_rgba(tools: &Tools, src: &Path, t: f64, size: (u32, u32), exact: bool) -> Result<crate::palette::RgbaImage> {
    let mut c = Command::new(&tools.ffmpeg);
    c.args(frame_args(src, t, size, exact)).stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped());
    no_window(&mut c);
    let out = c.output()?;
    let need = size.0 as usize * size.1 as usize * 4;
    if !out.status.success() || out.stdout.len() < need {
        let err = String::from_utf8_lossy(&out.stderr);
        if out.status.success() { return Err(Error::Format(tr("no image at this time (beyond the end of the video?)").into())); }
        return Err(Error::Ffmpeg { code: out.status.code(), stderr: err.lines().rev().take(4).collect::<Vec<_>>().into_iter().rev().collect::<Vec<_>>().join("\n") });
    }
    let mut data = out.stdout;
    data.truncate(need);
    crate::palette::RgbaImage::from_raw(size.0, size.1, data).map_err(|_| Error::Format(tr("thumbnail: unexpected size (video too short?)").into()))
}

/// Arguments ffmpeg pour `n` images regulieres de la plage [`from`, `to`] en une seule passe, en ne decodant que les images cles
/// (la plage n'est lue qu'une fois) : une image par tranche de `(to - from) / n` secondes, la premiere au milieu de la premiere
/// tranche. Les tranches sans image cle propre reprennent l'image cle precedente (filtre `fps` : `start_time=0` complete le debut).
pub fn range_keyframes_args(src: &Path, from: f64, to: f64, n: usize, size: (u32, u32)) -> Vec<String> {
    let cell = ((to - from) / n.max(1) as f64).max(0.001);
    let mut a: Vec<String> = vec!["-v".into(), "error".into(), "-nostdin".into(), "-skip_frame".into(), "nokey".into(), "-ss".into(), format!("{:.4}", (from + cell / 2.0).max(0.0))];
    a.extend(["-i".to_string(), s(src), "-an".into(), "-sn".into(), "-frames:v".into(), n.to_string(), "-vf".into(),
        format!("fps=fps={:.9}:start_time=0:round=near,scale={}:{}", 1.0 / cell, size.0, size.1)]);
    a.extend(["-f", "rawvideo", "-pix_fmt", "rgba", "pipe:1"].iter().map(|x| x.to_string()));
    a
}

/// Images regulieres d'une plage en une seule passe (voir `range_keyframes_args`), envoyees a `on_frame(rang, image)` des qu'elles
/// sont pretes ; `on_frame` rend faux pour arreter (ffmpeg est alors interrompu). Si ffmpeg s'arrete avant `n` images (fin de la
/// video), les rangs manquants reprennent la derniere image. Rend le nombre d'images recues de ffmpeg.
pub fn stream_range_keyframes(tools: &Tools, src: &Path, from: f64, to: f64, n: usize, size: (u32, u32), mut on_frame: impl FnMut(usize, crate::palette::RgbaImage) -> bool) -> Result<usize> {
    let mut c = Command::new(&tools.ffmpeg);
    c.args(range_keyframes_args(src, from, to, n, size)).stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::null());
    no_window(&mut c);
    let mut child = c.spawn()?;
    let mut out = child.stdout.take().expect("stdout");
    let need = size.0 as usize * size.1 as usize * 4;
    let mut got = 0usize;
    let mut last: Option<crate::palette::RgbaImage> = None;
    let mut stop = false;
    while got < n {
        let mut buf = vec![0u8; need];
        if out.read_exact(&mut buf).is_err() { break; }
        let Ok(img) = crate::palette::RgbaImage::from_raw(size.0, size.1, buf) else { break };
        last = Some(img.clone());
        if !on_frame(got, img) { stop = true; break; }
        got += 1;
    }
    drop(out);
    let _ = child.kill();
    let _ = child.wait();
    if !stop && got > 0 {
        if let Some(l) = last { for i in got..n { if !on_frame(i, l.clone()) { break; } } }
    }
    Ok(got)
}

/// Lecture d'une video en temps reel, sans le son : images RGBA de `size` envoyees au fil de l'eau. Arretee a la destruction.
pub struct FrameStream {
    child: Child,
    /// images dans l'ordre, au rythme de la video
    pub rx: std::sync::mpsc::Receiver<crate::palette::RgbaImage>,
}

impl FrameStream {
    pub fn start(tools: &Tools, src: &Path, t: f64, size: (u32, u32)) -> Result<FrameStream> {
        FrameStream::start_fit(tools, src, t, size, false)
    }

    /// Comme `start` ; `cover` : l'image remplit `size` et l'excedent est rogne (sans deformation), comme `extract_thumbnail_rgba`.
    pub fn start_fit(tools: &Tools, src: &Path, t: f64, size: (u32, u32), cover: bool) -> Result<FrameStream> {
        let vf = if cover { format!("scale={0}:{1}:force_original_aspect_ratio=increase,crop={0}:{1}", size.0, size.1) } else { format!("scale={}:{}", size.0, size.1) };
        let mut a: Vec<String> = vec!["-v".into(), "error".into(), "-nostdin".into(), "-re".into(), "-ss".into(), format!("{:.4}", t.max(0.0))];
        a.extend(["-i".to_string(), s(src), "-an".into(), "-sn".into(), "-vf".into(), vf]);
        a.extend(["-f", "rawvideo", "-pix_fmt", "rgba", "pipe:1"].iter().map(|x| x.to_string()));
        let mut c = Command::new(&tools.ffmpeg);
        c.args(a).stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::null());
        no_window(&mut c);
        let mut child = c.spawn()?;
        let mut out = child.stdout.take().expect("stdout");
        // file courte : si l'affichage prend du retard, ffmpeg attend au lieu d'accumuler des images en memoire
        let (tx, rx) = std::sync::mpsc::sync_channel(3);
        let need = size.0 as usize * size.1 as usize * 4;
        std::thread::spawn(move || loop {
            let mut buf = vec![0u8; need];
            if out.read_exact(&mut buf).is_err() { break; }
            match crate::palette::RgbaImage::from_raw(size.0, size.1, buf) {
                Ok(img) => { if tx.send(img).is_err() { break; } }
                Err(_) => break,
            }
        });
        Ok(FrameStream { child, rx })
    }
}

impl Drop for FrameStream {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// Instants des images cles de la premiere piste video (sortie `-of compact` de ffprobe), depuis le debut du fichier, croissants.
pub fn parse_keyframes_compact(text: &str) -> Vec<f64> {
    let mut start = None;
    let mut keys = Vec::new();
    for l in text.lines() {
        let l = l.trim();
        if let Some(r) = l.strip_prefix("frame|") {
            let num = |k: &str| r.split('|').find_map(|kv| kv.strip_prefix(k).and_then(|v| v.parse::<f64>().ok()));
            if let Some(t) = num("pts_time=").or_else(|| num("best_effort_timestamp_time=")) { keys.push(t); }
        } else if let Some(r) = l.strip_prefix("format|").or_else(|| l.strip_prefix("stream|")) {
            if let Some(v) = r.split('|').find_map(|kv| kv.strip_prefix("start_time=")) {
                if let Ok(t) = v.parse::<f64>() { start.get_or_insert(t); }
            }
        }
    }
    let s0 = start.unwrap_or_else(|| keys.first().copied().unwrap_or(0.0));
    let mut out: Vec<f64> = keys.into_iter().map(|t| (t - s0).max(0.0)).collect();
    out.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    out.dedup_by(|a, b| (*a - *b).abs() < 1e-6);
    out
}

/// Images cles de la video (lecture des seules images cles : quelques secondes pour un film).
pub fn list_keyframes(tools: &Tools, src: &Path) -> Result<Vec<f64>> {
    let mut c = Command::new(&tools.ffprobe);
    c.args(["-v", "error", "-select_streams", "v:0", "-skip_frame", "nokey", "-show_entries", "frame=pts_time,best_effort_timestamp_time:format=start_time", "-of", "compact"]).arg(src);
    c.stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped());
    no_window(&mut c);
    let out = c.output()?;
    if !out.status.success() {
        return Err(Error::Probe(String::from_utf8_lossy(&out.stderr).trim().to_string()));
    }
    Ok(parse_keyframes_compact(&String::from_utf8_lossy(&out.stdout)))
}

/// Son de menu : echantillons 16 bits gros-boutistes, stereo, 48 kHz (10 s au plus), pour sound.bdmv.
pub fn decode_menu_sound(tools: &Tools, src: &Path) -> Result<crate::sound::Sound> {
    let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0);
    let tmp = std::env::temp_dir().join(format!("bdauthor_son_{}_{nanos}.pcm", std::process::id()));
    let a: Vec<String> = vec!["-i".into(), s(src), "-vn".into(), "-t".into(), "10".into(), "-ac".into(), "2".into(), "-ar".into(), "48000".into(),
        "-f".into(), "s16be".into(), s(&tmp)];
    let r = run_ffmpeg(tools, &a, 0.0, &mut |_| {}, &AtomicBool::new(false));
    let data = std::fs::read(&tmp);
    let _ = std::fs::remove_file(&tmp);
    r?;
    let samples = data?;
    if samples.is_empty() { return Err(Error::Format(trf!("empty menu sound: {}", src.display()))); }
    Ok(crate::sound::Sound { channels: 2, samples })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cibles() {
        assert_eq!(pick_target(1280, 720, 59.94).label, "720p59.94");
        assert_eq!(pick_target(1920, 1080, 50.0).label, "720p50");
        assert_eq!(pick_target(1920, 1080, 23.976).fps, "24000/1001");
        assert_eq!(pick_target(1920, 1080, 24.0).fps, "24");
        assert_eq!(pick_target(1280, 720, 30.0).label, "720p59.94 (cadence doublee)");
        assert_eq!(pick_target(640, 480, 15.0).label, "720p24");
        // PAL et 29,97 : 1080 en PsF pour une source HD, 720p a cadence doublee sinon
        let t = pick_target(1920, 1080, 25.0);
        assert_eq!((t.height, t.fps, t.scan), (1080, "25", Scan::PsF));
        assert_eq!(pick_target(1920, 1080, 29.97).scan, Scan::PsF);
        assert_eq!(pick_target(1280, 720, 25.0).label, "720p50 (cadence doublee)");
        // entrelace : 1080i conserve (trame du haut ou du bas), 50 trames/s annoncees = 25 images/s ; SD desentrelacee
        assert_eq!(pick_target_scan(1920, 1080, 25.0, Some(true)).scan, Scan::Tff);
        assert_eq!(pick_target_scan(1920, 1080, 50.0, Some(false)).label, "1080i50");
        assert_eq!(pick_target_scan(1920, 1080, 29.97, Some(true)).fps, "30000/1001");
        let sd = pick_target_scan(720, 576, 25.0, Some(true));
        assert!(sd.deinterlace && sd.fps == "50" && sd.height == 720);
        assert_eq!(scan_for((1920, 1080), "25"), Scan::PsF);
        assert_eq!(scan_for((1280, 720), "50"), Scan::Progressive);
    }

    #[test]
    fn sondage_plat() {
        let t = "streams.stream.0.index=0\nstreams.stream.0.codec_name=\"h264\"\nstreams.stream.0.codec_type=\"video\"\n\
                 streams.stream.0.width=1280\nstreams.stream.0.height=720\nstreams.stream.0.pix_fmt=\"yuvj420p\"\n\
                 streams.stream.0.r_frame_rate=\"60000/1001\"\nstreams.stream.1.codec_type=\"audio\"\nformat.duration=\"32.315611\"\n";
        let i = parse_probe_flat(t).unwrap();
        assert_eq!((i.width, i.height, i.fps_num, i.fps_den), (1280, 720, 60000, 1001));
        assert!(i.full_range && i.has_audio);
        assert!((i.duration_s - 32.315611).abs() < 1e-9);
        assert!(parse_probe_flat("format.duration=\"1\"").is_err());
    }

    #[test]
    fn arguments_identiques_a_python() {
        let info = VideoInfo { width: 1280, height: 720, fps_num: 60000, fps_den: 1001, pix_fmt: "yuv420p".into(), full_range: false,
                               codec: "h264".into(), duration_s: 10.0, has_audio: true, field_order: String::new(), ..Default::default() };
        let (a, _) = encode_args(Path::new("in.mp4"), Path::new("out.ts"), &info, &EncodeOpts::default());
        let line = a.join(" ");
        assert!(line.contains("fps=60000/1001,scale=1280:720:in_range=tv:in_color_matrix=bt709:out_range=tv:out_color_matrix=bt709,format=yuv420p"));
        assert!(line.contains("bluray-compat=1:keyint=30:open-gop=0:aud=1:nal-hrd=vbr:vbv-maxrate=30000:vbv-bufsize=30000:level=4.1:bframes=3"));
        assert!(line.ends_with("-c:a ac3 -b:a 192k -ar 48000 -ac 2 -f mpegts -pes_payload_size 0 out.ts"));
        // sans audio : piste muette ; entrelace : options de codage entrelace
        let muet = VideoInfo { has_audio: false, ..info.clone() };
        let l2 = encode_args(Path::new("in.mp4"), Path::new("out.ts"), &muet, &EncodeOpts::default()).0.join(" ");
        assert!(l2.contains("anullsrc") && l2.contains("-map 1:a:0") && l2.contains("-shortest"));
        let i50 = VideoInfo { width: 1920, height: 1080, fps_num: 25, fps_den: 1, field_order: "tt".into(), ..info.clone() };
        let l3 = encode_args(Path::new("in.mp4"), Path::new("out.ts"), &i50, &EncodeOpts::default()).0.join(" ");
        assert!(l3.contains(":tff=1") && l3.contains("+ildct+ilme") && l3.contains(":interl=1"), "{l3}");
    }
}

// ---------------------------------------------------------------- sous-titres integres

/// Piste de sous-titres d'un fichier video.
#[derive(Debug, Clone, PartialEq)]
pub struct SubStreamInfo {
    /// Numero parmi les pistes de sous-titres (0 = la premiere), celui de `-map 0:s:N`.
    pub index: u32,
    pub codec: String,
    pub lang: String,
    pub title: String,
}

impl SubStreamInfo {
    /// Sous-titres texte que bdauthor sait convertir (les formats en images, comme PGS ou DVD, ne le sont pas).
    pub fn is_text(&self) -> bool {
        matches!(self.codec.as_str(), "subrip" | "srt" | "ass" | "ssa" | "mov_text" | "webvtt" | "text" | "microdvd" | "subviewer" | "sami")
    }
}

pub fn parse_sub_streams_flat(text: &str) -> Vec<SubStreamInfo> {
    let mut streams: BTreeMap<u32, BTreeMap<String, String>> = BTreeMap::new();
    for line in text.lines() {
        let Some((k, v)) = line.split_once('=') else { continue };
        if let Some(rest) = k.strip_prefix("streams.stream.") {
            if let Some((idx, field)) = rest.split_once('.') {
                if let Ok(i) = idx.parse::<u32>() {
                    streams.entry(i).or_default().insert(field.to_string(), v.trim().trim_matches('"').to_string());
                }
            }
        }
    }
    let get = |m: &BTreeMap<String, String>, k: &str| m.get(k).cloned().unwrap_or_default();
    streams
        .values()
        .filter(|s| s.get("codec_type").map(String::as_str) == Some("subtitle"))
        .enumerate()
        .map(|(n, s)| SubStreamInfo { index: n as u32, codec: get(s, "codec_name"), lang: get(s, "tags.language"), title: get(s, "tags.title") })
        .collect()
}

pub fn list_subtitle_streams(tools: &Tools, src: &Path) -> Result<Vec<SubStreamInfo>> {
    let mut c = Command::new(&tools.ffprobe);
    c.args(["-v", "error", "-of", "flat", "-show_entries", "stream=index,codec_type,codec_name:stream_tags=language,title"]).arg(src);
    no_window(&mut c);
    let out = c.output()?;
    if !out.status.success() {
        return Err(Error::Probe(String::from_utf8_lossy(&out.stderr).trim().to_string()));
    }
    Ok(parse_sub_streams_flat(&String::from_utf8_lossy(&out.stdout)))
}

/// Extrait la piste de sous-titres texte `index` (voir `SubStreamInfo::index`) en SRT.
pub fn extract_subtitle_srt(tools: &Tools, src: &Path, index: u32, dst: &Path) -> Result<()> {
    let mut c = Command::new(&tools.ffmpeg);
    c.args(["-y", "-v", "error", "-i"]).arg(src).args(["-map", &format!("0:s:{index}"), "-f", "srt"]).arg(dst);
    no_window(&mut c);
    let out = c.output()?;
    if !out.status.success() {
        return Err(Error::Format(trf!("cannot extract subtitle track {} (only text subtitles are supported): {}", index + 1, String::from_utf8_lossy(&out.stderr).trim())));
    }
    Ok(())
}

/// Convertit un fichier de sous-titres texte que ffmpeg sait lire (TTML, LRC, STL, JACOsub...) en SRT.
pub fn convert_subtitle_file_to_srt(tools: &Tools, src: &Path, dst: &Path) -> Result<()> {
    let mut c = Command::new(&tools.ffmpeg);
    c.args(["-y", "-v", "error", "-i"]).arg(src).args(["-f", "srt"]).arg(dst);
    no_window(&mut c);
    let out = c.output()?;
    if !out.status.success() {
        return Err(Error::Format(trf!("subtitle conversion failed ({}): {}", src.display(), String::from_utf8_lossy(&out.stderr).trim())));
    }
    Ok(())
}

// ---------------------------------------------------------------- pistes audio integrees

/// Piste audio d'un fichier video.
#[derive(Debug, Clone, PartialEq)]
pub struct AudioStreamInfo {
    /// Numero parmi les pistes audio (0 = la premiere), celui de `-map 0:a:N`.
    pub index: u32,
    pub codec: String,
    pub lang: String,
    pub title: String,
    pub channels: u32,
}

pub fn parse_audio_streams_flat(text: &str) -> Vec<AudioStreamInfo> {
    let mut streams: BTreeMap<u32, BTreeMap<String, String>> = BTreeMap::new();
    for line in text.lines() {
        let Some((k, v)) = line.split_once('=') else { continue };
        if let Some(rest) = k.strip_prefix("streams.stream.") {
            if let Some((idx, field)) = rest.split_once('.') {
                if let Ok(i) = idx.parse::<u32>() {
                    streams.entry(i).or_default().insert(field.to_string(), v.trim().trim_matches('"').to_string());
                }
            }
        }
    }
    let get = |m: &BTreeMap<String, String>, k: &str| m.get(k).cloned().unwrap_or_default();
    streams
        .values()
        .filter(|s| s.get("codec_type").map(String::as_str) == Some("audio"))
        .enumerate()
        .map(|(n, s)| AudioStreamInfo { index: n as u32, codec: get(s, "codec_name"), lang: get(s, "tags.language"), title: get(s, "tags.title"), channels: get(s, "channels").parse().unwrap_or(0) })
        .collect()
}

/// Pistes audio d'un fichier (video ou audio).
pub fn list_audio_streams(tools: &Tools, src: &Path) -> Result<Vec<AudioStreamInfo>> {
    let mut c = Command::new(&tools.ffprobe);
    c.args(["-v", "error", "-of", "flat", "-show_entries", "stream=index,codec_type,codec_name,channels:stream_tags=language,title"]).arg(src);
    no_window(&mut c);
    let out = c.output()?;
    if !out.status.success() {
        return Err(Error::Probe(String::from_utf8_lossy(&out.stderr).trim().to_string()));
    }
    Ok(parse_audio_streams_flat(&String::from_utf8_lossy(&out.stdout)))
}

// ---------------------------------------------------------------- ecoute d'une piste audio

/// Enveloppe d'une piste audio : `bins` valeurs entre 0 et 1 (plus forte amplitude de chaque tranche egale de `duration`).
/// `track` : numero de la piste audio du fichier (0 = la premiere) ; `None` : la meilleure piste (fichier audio).
pub fn audio_peaks(tools: &Tools, src: &Path, track: Option<u32>, duration: f64, bins: usize) -> Result<Vec<f32>> {
    const RATE: u32 = 4000;
    let bins = bins.max(1);
    let mut c = Command::new(&tools.ffmpeg);
    c.args(["-v", "error", "-nostdin", "-i"]).arg(src).arg("-vn");
    if let Some(n) = track { c.args(["-map", &format!("0:a:{n}")]); }
    c.args(["-ac", "1", "-ar", &RATE.to_string(), "-f", "s16le", "-"]).stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::null());
    no_window(&mut c);
    let mut child = c.spawn()?;
    let mut out = child.stdout.take().ok_or_else(|| Error::Format("stdout".into()))?;
    let total = (duration * RATE as f64).max(1.0);
    let mut peaks = vec![0.0f32; bins];
    let (mut buf, mut idx, mut odd) = (vec![0u8; 65536], 0u64, None::<u8>);
    loop {
        let n = out.read(&mut buf)?;
        if n == 0 { break; }
        for &b in &buf[..n] {
            match odd.take() {
                None => odd = Some(b),
                Some(lo) => {
                    let v = (i16::from_le_bytes([lo, b]) as f32 / 32768.0).abs();
                    let k = ((idx as f64 / total * bins as f64) as usize).min(bins - 1);
                    if v > peaks[k] { peaks[k] = v; }
                    idx += 1;
                }
            }
        }
    }
    let status = child.wait()?;
    if idx == 0 || !status.success() && idx == 0 {
        return Err(Error::Format(tr("no audio in this track").into()));
    }
    Ok(peaks)
}

impl Tools {
    /// ffplay (fourni avec ffmpeg) : a cote de ffmpeg, sinon dans le PATH.
    pub fn ffplay(&self) -> Option<PathBuf> {
        let next = self.ffmpeg.parent().filter(|p| !p.as_os_str().is_empty()).map(|d| d.join(exe_name("ffplay")));
        next.filter(|p| works(p)).or_else(|| Some(PathBuf::from(exe_name("ffplay"))).filter(|p| works(p)))
    }
}

/// Lecture du son d'une piste, sans fenetre, par ffplay. Arretee a la destruction.
pub struct AudioPlayback {
    child: Child,
}

impl AudioPlayback {
    /// Joue `src` a partir de `from` secondes. `track` : numero de la piste audio du fichier (0 = la premiere) ; `None` : la
    /// meilleure piste.
    pub fn start(tools: &Tools, src: &Path, track: Option<u32>, from: f64) -> Result<AudioPlayback> {
        let play = tools.ffplay().ok_or_else(|| Error::NotFound(tr("ffplay not found: put ffplay.exe next to ffmpeg.exe (it comes with ffmpeg) to listen to the tracks.").into()))?;
        let mut c = Command::new(play);
        c.args(["-nodisp", "-autoexit", "-loglevel", "quiet", "-vn", "-ss", &format!("{:.3}", from.max(0.0))]);
        if let Some(n) = track { c.args(["-ast", &format!("a:{n}")]); }
        c.arg("-i").arg(src).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null());
        no_window(&mut c);
        Ok(AudioPlayback { child: c.spawn()? })
    }

    /// Le son est termine (fin de la piste ou processus arrete).
    pub fn finished(&mut self) -> bool { !matches!(self.child.try_wait(), Ok(None)) }

    pub fn stop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

impl Drop for AudioPlayback {
    fn drop(&mut self) { self.stop(); }
}

// ---------------------------------------------------------------- copie sans reencodage (source deja conforme)

/// Raisons qui empechent de recopier tel quel le flux video de `info` dans un disque Blu-ray (liste vide : rien ne s'y oppose dans ce
/// que ffprobe indique du flux). Ni le debit ni la taille du tampon ne sont examines ici : voir `scan_video_rate` et `rate_blockers`.
pub fn copy_blockers(info: &VideoInfo) -> Vec<String> {
    let mut p = Vec::new();
    if info.codec != "h264" {
        p.push(trf!("video codec “{}” (H.264 expected)", info.codec));
        return p;
    }
    if !matches!(info.profile.as_str(), "High" | "Main") {
        p.push(trf!("H.264 profile “{}” (High or Main expected)", if info.profile.is_empty() { "?" } else { info.profile.as_str() }));
    }
    if info.level == 0 || info.level > 41 {
        p.push(trf!("H.264 level {} (4.1 at most)", if info.level == 0 { "?".to_string() } else { format!("{}.{}", info.level / 10, info.level % 10) }));
    }
    if info.pix_fmt != "yuv420p" || info.full_range {
        p.push(trf!("pixel format “{}” (8-bit 4:2:0, limited range expected)", info.pix_fmt));
    }
    if !info.cfr {
        p.push(tr("variable frame rate").to_string());
    }
    let tg = target_of(info);
    if tg.deinterlace || tg.scan == Scan::PsF {
        p.push(trf!("{} fps at {}x{}: this format has to be converted to be allowed on a Blu-ray", format!("{:.3}", info.fps()), info.width, info.height));
    } else {
        if (info.width, info.height) != (tg.width, tg.height) {
            p.push(trf!("size {}x{} (a Blu-ray needs {}x{})", info.width, info.height, tg.width, tg.height));
        }
        let fps = if info.interlaced() && info.fps() >= 45.0 { info.fps() / 2.0 } else { info.fps() };
        let want = frac(tg.fps);
        if (fps - want).abs() > 0.01 {
            p.push(trf!("frame rate {} (a Blu-ray needs {})", format!("{fps:.3}"), format!("{want:.3}")));
        }
    }
    // niveau 4.1 : 4 images de reference en 1080, 9 en 720
    let max_refs = if info.height > 720 { 4 } else { 9 };
    if info.refs > max_refs {
        p.push(trf!("{} reference frames (at most {} at this size)", info.refs, max_refs));
    }
    p
}

/// Debits d'un flux video, mesures sur ses paquets.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct StreamRate {
    /// taille totale du flux video (octets)
    pub bytes: u64,
    /// debit moyen (kb/s)
    pub avg_kbps: f64,
    /// debit maximal sur une seconde glissante (kb/s)
    pub peak_kbps: f64,
    /// plus grand ecart entre deux images cles (s)
    pub max_key_gap_s: f64,
}

/// Debits calcules depuis les paquets `(instant en s, taille en octets, image cle ?)` ; `duration_s` : duree du fichier (0 : deduite).
pub fn rate_from_packets(pk: &[(f64, u32, bool)], duration_s: f64) -> StreamRate {
    if pk.is_empty() { return StreamRate::default(); }
    let mut v: Vec<(f64, u32, bool)> = pk.to_vec();
    v.sort_by(|a, b| a.0.total_cmp(&b.0));
    let bytes: u64 = v.iter().map(|p| p.1 as u64).sum();
    let span = (v[v.len() - 1].0 - v[0].0).max(0.0);
    let dur = if duration_s > 0.0 { duration_s } else { span.max(0.001) };
    // fenetre glissante d'une seconde : (t - 1, t]
    let (mut lo, mut acc, mut peak) = (0usize, 0u64, 0u64);
    for i in 0..v.len() {
        acc += v[i].1 as u64;
        while v[i].0 - v[lo].0 >= 1.0 { acc -= v[lo].1 as u64; lo += 1; }
        peak = peak.max(acc);
    }
    let keys: Vec<f64> = v.iter().filter(|p| p.2).map(|p| p.0).collect();
    let gap = keys.windows(2).map(|w| w[1] - w[0]).fold(0.0, f64::max);
    StreamRate { bytes, avg_kbps: bytes as f64 * 8.0 / dur / 1000.0, peak_kbps: peak as f64 * 8.0 / 1000.0, max_key_gap_s: gap }
}

/// Lit tous les paquets video du fichier avec ffprobe (sans decoder) et en tire les debits.
pub fn scan_video_rate(tools: &Tools, src: &Path, duration_s: f64, cancel: &AtomicBool) -> Result<StreamRate> {
    let mut c = Command::new(&tools.ffprobe);
    c.args(["-v", "error", "-select_streams", "v:0", "-show_entries", "packet=pts_time,dts_time,size,flags", "-of", "csv=p=0"]).arg(src);
    c.stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::null());
    no_window(&mut c);
    let mut child = c.spawn()?;
    let stdout = child.stdout.take().expect("stdout");
    let mut pk: Vec<(f64, u32, bool)> = Vec::new();
    for (n, line) in BufReader::new(stdout).lines().enumerate() {
        if n % 4096 == 0 && cancel.load(Ordering::Relaxed) {
            let _ = child.kill();
            let _ = child.wait();
            return Err(Error::Cancelled);
        }
        let Ok(line) = line else { break };
        let f: Vec<&str> = line.trim().split(',').collect();
        if f.len() < 4 { continue; }
        let t = f[1].parse::<f64>().or_else(|_| f[0].parse::<f64>());
        let (Ok(t), Ok(size)) = (t, f[2].parse::<u32>()) else { continue };
        pk.push((t, size, f[3].starts_with('K')));
    }
    let _ = child.wait();
    if pk.is_empty() { return Err(Error::Probe(tr("no video packet").into())); }
    Ok(rate_from_packets(&pk, duration_s))
}

/// Raisons tirees des debits qui empechent la copie (`max_kbps` : debit maximal accepte sur une seconde).
pub fn rate_blockers(r: &StreamRate, max_kbps: u32) -> Vec<String> {
    let mut p = Vec::new();
    if r.peak_kbps > max_kbps as f64 {
        p.push(trf!("peak bitrate {} kb/s over one second (limit {} kb/s)", format!("{:.0}", r.peak_kbps), max_kbps));
    }
    if r.max_key_gap_s > 2.5 {
        p.push(trf!("key frames {} s apart (2.5 s at most)", format!("{:.1}", r.max_key_gap_s)));
    }
    p
}

/// Verifie si la video `src` peut etre copiee sans reencodage. `Ok(Ok(debits))` : oui ; `Ok(Err(raisons))` : non.
pub fn check_copy(tools: &Tools, src: &Path, info: &VideoInfo, max_kbps: u32, cancel: &AtomicBool) -> Result<std::result::Result<StreamRate, Vec<String>>> {
    let b = copy_blockers(info);
    if !b.is_empty() { return Ok(Err(b)); }
    let r = scan_video_rate(tools, src, info.duration_s, cancel)?;
    let b = rate_blockers(&r, max_kbps);
    Ok(if b.is_empty() { Ok(r) } else { Err(b) })
}

#[cfg(test)]
mod copie_tests {
    use super::*;

    fn conforme() -> VideoInfo {
        VideoInfo { width: 1920, height: 1080, fps_num: 24000, fps_den: 1001, pix_fmt: "yuv420p".into(), codec: "h264".into(), duration_s: 100.0, has_audio: true,
                    profile: "High".into(), level: 41, refs: 4, cfr: true, ..Default::default() }
    }

    #[test]
    fn source_conforme_et_ses_obstacles() {
        assert!(copy_blockers(&conforme()).is_empty());
        let mut i = conforme(); i.codec = "hevc".into();
        assert_eq!(copy_blockers(&i).len(), 1);
        let mut i = conforme(); i.level = 51; i.profile = "High 4:4:4 Predictive".into();
        assert_eq!(copy_blockers(&i).len(), 2);
        let mut i = conforme(); i.width = 1920; i.height = 800; // 2,40:1 non anamorphose : taille non admise
        assert!(copy_blockers(&i).iter().any(|m| m.contains("1920x800")), "{:?}", copy_blockers(&i));
        let mut i = conforme(); i.fps_num = 25; i.fps_den = 1; // 1080p25 : doit passer en PsF
        assert!(!copy_blockers(&i).is_empty());
        let mut i = conforme(); i.cfr = false; i.full_range = true; i.refs = 6;
        assert_eq!(copy_blockers(&i).len(), 3);
        // 720p59.94 et 1080i50 (trames du haut) sont admis
        let i = VideoInfo { width: 1280, height: 720, fps_num: 60000, fps_den: 1001, refs: 3, ..conforme() };
        assert!(copy_blockers(&i).is_empty(), "{:?}", copy_blockers(&i));
        let i = VideoInfo { fps_num: 25, fps_den: 1, field_order: "tt".into(), ..conforme() };
        assert!(copy_blockers(&i).is_empty(), "{:?}", copy_blockers(&i));
    }

    #[test]
    fn sondage_profil_niveau_cadence() {
        let t = "streams.stream.0.codec_type=\"video\"\nstreams.stream.0.codec_name=\"h264\"\nstreams.stream.0.profile=\"High\"\nstreams.stream.0.level=41\nstreams.stream.0.refs=4\n\
                 streams.stream.0.width=1920\nstreams.stream.0.height=1080\nstreams.stream.0.pix_fmt=\"yuv420p\"\nstreams.stream.0.r_frame_rate=\"24000/1001\"\nstreams.stream.0.avg_frame_rate=\"24000/1001\"\nformat.duration=\"10\"\n";
        let i = parse_probe_flat(t).unwrap();
        assert_eq!((i.profile.as_str(), i.level, i.refs, i.cfr), ("High", 41, 4, true));
        assert!(copy_blockers(&i).is_empty());
        let vfr = t.replace("avg_frame_rate=\"24000/1001\"", "avg_frame_rate=\"2300/100\"");
        assert!(!parse_probe_flat(&vfr).unwrap().cfr);
    }

    #[test]
    fn debits_des_paquets() {
        // 1 image par 0,5 s : 1000 octets, une image cle sur 4 ; un paquet de 50 000 octets cree un pic
        let mut pk: Vec<(f64, u32, bool)> = (0..20).map(|k| (k as f64 * 0.5, 1000, k % 4 == 0)).collect();
        let r = rate_from_packets(&pk, 0.0);
        assert!((r.max_key_gap_s - 2.0).abs() < 1e-9);
        assert!((r.peak_kbps - 16.0).abs() < 1e-9, "{}", r.peak_kbps); // deux paquets par seconde glissante = 2000 octets
        assert!(rate_blockers(&r, 30000).is_empty());
        pk[5].1 = 50_000;
        let r = rate_from_packets(&pk, 0.0);
        assert!(r.peak_kbps > 400.0 && !rate_blockers(&r, 300).is_empty());
        assert_eq!(r.bytes, 19 * 1000 + 50_000 - 0);
        let slow = rate_from_packets(&[(0.0, 10, true), (3.0, 10, true)], 0.0);
        assert!(rate_blockers(&slow, 30000).iter().any(|m| m.contains("3.0")));
    }

    #[test]
    fn arguments_copie_et_debit_cible() {
        let info = conforme();
        let copie = EncodeOpts { copy_video: true, ..Default::default() };
        let l = encode_args(Path::new("in.mkv"), Path::new("out.ts"), &info, &copie).0.join(" ");
        assert!(l.contains("-c:v copy") && !l.contains("libx264") && !l.contains("-vf") && !l.contains("-crf"), "{l}");
        assert!(l.contains("-c:a ac3") && l.ends_with("out.ts"));
        let cible = EncodeOpts { video_kbps: Some(8000), ..Default::default() };
        let (p1, _) = first_pass_args(Path::new("in.mkv"), &info, &cible, Path::new("log"));
        let l1 = p1.join(" ");
        assert!(l1.contains("-b:v 8000k") && !l1.contains("-crf") && l1.contains("-an") && l1.contains("-pass 1 -passlogfile log") && l1.contains("-f null"), "{l1}");
        let (p2, _) = encode_args_full(Path::new("in.mkv"), Path::new("out.ts"), &info, &cible, &[], Some((2, Path::new("log"))));
        let l2 = p2.join(" ");
        assert!(l2.contains("-b:v 8000k") && l2.contains("-pass 2 -passlogfile log") && l2.contains("vbv-maxrate=30000"), "{l2}");
        // sans debit cible : le CRF comme avant
        assert!(encode_args(Path::new("in.mkv"), Path::new("out.ts"), &info, &EncodeOpts::default()).0.join(" ").contains("-crf 18"));
    }
}
