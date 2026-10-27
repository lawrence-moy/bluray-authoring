//! Pilotage de ffmpeg / ffprobe : meme logique et memes commandes que `bdauthor/encode.py`.
//!
//! * `Tools::locate` trouve ffmpeg (variable `BDAUTHOR_FFMPEG_DIR`, dossier de l'executable, puis PATH).
//! * `probe` lit les caracteristiques d'une source.
//! * `encode_to_ts` produit le flux MPEG-TS H.264 (High@4.1, GOP ferme, AC3 48 kHz) que le muxeur transformera en .m2ts.
//! * `make_still_ts` : image fixe -> clip video seul. `extract_thumbnail` : une image de la video.
//! La progression est rapportee par `-progress pipe:1` ; l'annulation tue le processus ffmpeg.

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
            Error::Io(e) => write!(f, "erreur d'entree/sortie : {e}"),
            Error::NotFound(m) => write!(f, "{m}"),
            Error::Probe(m) => write!(f, "source illisible : {m}"),
            Error::Ffmpeg { code, stderr } => write!(f, "ffmpeg a echoue (code {code:?}) : {}", stderr.trim()),
            Error::Cancelled => write!(f, "operation annulee"),
            Error::Format(m) => write!(f, "donnees invalides : {m}"),
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

impl Tools {
    /// Cherche ffmpeg et ffprobe : `BDAUTHOR_FFMPEG_DIR`, dossier de l'executable (et son sous-dossier `ffmpeg`/`bin`), puis PATH.
    pub fn locate() -> Result<Tools> {
        let mut dirs: Vec<PathBuf> = Vec::new();
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
            "ffmpeg/ffprobe introuvables : placez ffmpeg.exe et ffprobe.exe a cote du programme, \
             ou indiquez leur dossier dans la variable BDAUTHOR_FFMPEG_DIR, ou ajoutez-les au PATH."
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

#[derive(Debug, Clone, PartialEq)]
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
}

impl VideoInfo {
    pub fn fps(&self) -> f64 {
        self.fps_num as f64 / self.fps_den.max(1) as f64
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
        .ok_or_else(|| Error::Probe("aucun flux video".into()))?;
    let get = |m: &BTreeMap<String, String>, k: &str| m.get(k).cloned().unwrap_or_default();
    let (n, d) = get(video, "r_frame_rate")
        .split_once('/')
        .map(|(a, b)| (a.parse::<u32>().unwrap_or(0), b.parse::<u32>().unwrap_or(1)))
        .unwrap_or((0, 1));
    if n == 0 {
        return Err(Error::Probe("cadence d'images illisible".into()));
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
    })
}

pub fn probe(tools: &Tools, src: &Path) -> Result<VideoInfo> {
    let mut c = Command::new(&tools.ffprobe);
    c.args(["-v", "error", "-of", "flat", "-show_entries"])
        .arg("stream=index,codec_type,codec_name,width,height,r_frame_rate,pix_fmt,color_range:format=duration")
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
}

fn film_fps(fps: f64) -> &'static str {
    if (fps - 23.976).abs() < (fps - 24.0).abs() { "24000/1001" } else { "24" }
}

/// Cible Blu-ray la plus proche de la source (cadence changee par doublement/suppression d'images, jamais par interpolation).
pub fn pick_target(width: u32, height: u32, fps: f64) -> Target {
    let _ = width;
    let t = |w, h, f, l| Target { width: w, height: h, fps: f, label: l };
    if height > 800 && fps < 26.0 {
        return t(1920, 1080, film_fps(fps), "1080p");
    }
    if fps >= 55.0 {
        return t(1280, 720, "60000/1001", "720p59.94");
    }
    if fps >= 45.0 {
        return t(1280, 720, "50", "720p50");
    }
    if fps >= 26.0 {
        return t(1280, 720, "60000/1001", "720p59.94 (cadence doublee)");
    }
    t(1280, 720, film_fps(fps), "720p24")
}

#[derive(Debug, Clone)]
pub struct EncodeOpts {
    pub crf: u32,
    pub maxrate: u32,
    pub keyint: u32,
    pub audio_bitrate: String,
    pub preset: String,
}

impl Default for EncodeOpts {
    fn default() -> Self {
        EncodeOpts { crf: 18, maxrate: 30000, keyint: 30, audio_bitrate: "192k".into(), preset: "medium".into() }
    }
}

fn x264_args(crf: u32, maxrate: u32, keyint: u32, preset: &str) -> Vec<String> {
    let x264 = format!(
        "bluray-compat=1:keyint={keyint}:open-gop=0:aud=1:nal-hrd=vbr:vbv-maxrate={maxrate}:vbv-bufsize=30000:level=4.1:bframes=3"
    );
    let v = |a: &[&str]| a.iter().map(|s| s.to_string()).collect::<Vec<_>>();
    let mut out = v(&["-c:v", "libx264", "-profile:v", "high", "-preset", preset, "-crf"]);
    out.push(crf.to_string());
    out.extend(v(&["-x264-params"]));
    out.push(x264);
    out.extend(v(&["-color_primaries", "bt709", "-color_trc", "bt709", "-colorspace", "bt709", "-color_range", "tv"]));
    out
}

// ---------------------------------------------------------------- execution

/// Lance ffmpeg avec `-progress pipe:1`, rapporte la progression (0.0..=1.0 d'apres `duration_s`) et gere l'annulation.
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
    for line in BufReader::new(stdout).lines() {
        if cancel.load(Ordering::Relaxed) {
            let _ = child.kill();
            let _ = child.wait();
            let _ = err_thread.join();
            return Err(Error::Cancelled);
        }
        let Ok(line) = line else { break };
        if let Some(us) = line.strip_prefix("out_time_us=") {
            if let (Ok(us), true) = (us.trim().parse::<i64>(), duration_s > 0.0) {
                progress((us.max(0) as f64 / 1e6 / duration_s).min(1.0));
            }
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
pub fn encode_args(src: &Path, dst: &Path, info: &VideoInfo, o: &EncodeOpts) -> (Vec<String>, Target) {
    let tg = pick_target(info.width, info.height, info.fps());
    let vf = format!(
        "fps={},scale={}:{}:in_range={}:in_color_matrix=bt709:out_range=tv:out_color_matrix=bt709,format=yuv420p",
        tg.fps, tg.width, tg.height, if info.full_range { "pc" } else { "tv" }
    );
    let mut a: Vec<String> = ["-i"].iter().map(|x| x.to_string()).collect();
    a.push(s(src));
    a.extend(["-map", "0:v:0", "-map", "0:a:0", "-vf"].iter().map(|x| x.to_string()));
    a.push(vf);
    a.extend(x264_args(o.crf, o.maxrate, o.keyint, &o.preset));
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
    let (args, tg) = encode_args(src, dst, info, opts);
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
    a.extend(x264_args(opts.crf, 30000, 30, &opts.preset));
    a.extend(["-an", "-f", "mpegts"].iter().map(|x| x.to_string()));
    a.push(s(dst));
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
    crate::palette::RgbaImage::from_raw(size.0, size.1, data?).map_err(|_| Error::Format("miniature : taille inattendue (video trop courte ?)".into()))
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
        assert_eq!(pick_target(1920, 1080, 30.0).label, "720p59.94 (cadence doublee)");
        assert_eq!(pick_target(640, 480, 15.0).label, "720p24");
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
                               codec: "h264".into(), duration_s: 10.0, has_audio: true };
        let (a, _) = encode_args(Path::new("in.mp4"), Path::new("out.ts"), &info, &EncodeOpts::default());
        let line = a.join(" ");
        assert!(line.contains("fps=60000/1001,scale=1280:720:in_range=tv:in_color_matrix=bt709:out_range=tv:out_color_matrix=bt709,format=yuv420p"));
        assert!(line.contains("bluray-compat=1:keyint=30:open-gop=0:aud=1:nal-hrd=vbr:vbv-maxrate=30000:vbv-bufsize=30000:level=4.1:bframes=3"));
        assert!(line.ends_with("-c:a ac3 -b:a 192k -ar 48000 -ac 2 -f mpegts -pes_payload_size 0 out.ts"));
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
        return Err(Error::Format(format!(
            "extraction de la piste de sous-titres {} impossible (seuls les sous-titres texte sont geres) : {}",
            index + 1,
            String::from_utf8_lossy(&out.stderr).trim()
        )));
    }
    Ok(())
}
