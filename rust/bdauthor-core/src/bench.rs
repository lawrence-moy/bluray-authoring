//! Mesure de vitesse de l'encodage video : un extrait d'une vraie video passe par la chaine de la generation (meme filtre,
//! meme encodeur, sortie jetee), avec x264, avec l'encodeur materiel, et avec l'encodeur materiel plus le decodage par la carte
//! graphique. ffmpeg (`-benchmark`) donne le temps processeur consomme : on sait ainsi si c'est le processeur qui freine.
use crate::ffmpeg::{no_window, probe, EncodeOpts, Error, Result, Tools};
use crate::hwenc::VideoEncoder;
use crate::i18n::tr;
use crate::trf;
use std::io::{BufRead, BufReader, Read};
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Instant;

/// Une configuration mesuree.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Case {
    /// x264, decodage par le processeur
    X264,
    /// encodeur materiel, decodage par le processeur (la chaine actuelle)
    Hw,
    /// encodeur materiel, decodage par la carte graphique
    HwGpuDecode,
}

impl Case {
    pub fn label(self) -> &'static str {
        match self {
            Case::X264 => "x264 (software)",
            Case::Hw => "hardware encoder, decoding by the processor",
            Case::HwGpuDecode => "hardware encoder, decoding by the graphics chip",
        }
    }
}

/// Mesure d'une configuration.
#[derive(Debug, Clone, PartialEq)]
pub struct Measure {
    /// images encodees, duree reelle (s), images par seconde, vitesse par rapport au temps reel de la video
    pub frames: u64,
    pub seconds: f64,
    pub fps: f64,
    pub speed: f64,
    /// coeurs de processeur occupes en moyenne (temps processeur / duree) et part de la machine (0 a 1)
    pub cpu_cores: f64,
    pub cpu_share: f64,
}

/// Resultat d'une configuration : mesure ou message d'echec.
#[derive(Debug, Clone, PartialEq)]
pub struct CaseResult {
    pub case: Case,
    pub result: std::result::Result<Measure, String>,
}

/// Lit la ligne « bench: utime=…s stime=…s rtime=…s » de ffmpeg : (temps utilisateur + systeme, temps reel) en secondes.
pub fn parse_bench(stderr: &str) -> Option<(f64, f64)> {
    let l = stderr.lines().rev().find(|l| l.contains("bench:") && l.contains("rtime="))?;
    let get = |k: &str| -> Option<f64> {
        let i = l.find(k)? + k.len();
        l[i..].split(|c: char| c == 's' || c.is_whitespace()).next()?.parse().ok()
    };
    Some((get("utime=")? + get("stime=").unwrap_or(0.0), get("rtime=")?))
}

/// Lance une mesure ; `progress` recoit l'avancement (0 a 1) d'apres la duree de l'extrait.
fn run_case(tools: &Tools, args: &[String], dur: f64, fps_src: f64, progress: &mut dyn FnMut(f64), cancel: &AtomicBool) -> Result<Measure> {
    let mut c = Command::new(&tools.ffmpeg);
    c.args(["-hide_banner", "-nostdin", "-y", "-benchmark", "-nostats", "-progress", "pipe:1"]).args(args);
    c.stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped());
    no_window(&mut c);
    let t0 = Instant::now();
    let mut child = c.spawn()?;
    let mut stderr = child.stderr.take().ok_or_else(|| Error::Format("stderr".into()))?;
    let err_thread = std::thread::spawn(move || { let mut s = String::new(); let _ = stderr.read_to_string(&mut s); s });
    let stdout = child.stdout.take().ok_or_else(|| Error::Format("stdout".into()))?;
    let mut frames = 0u64;
    for line in BufReader::new(stdout).lines() {
        if cancel.load(Ordering::Relaxed) {
            let _ = child.kill();
            let _ = child.wait();
            let _ = err_thread.join();
            return Err(Error::Cancelled);
        }
        let Ok(line) = line else { break };
        if let Some(v) = line.strip_prefix("frame=") { frames = v.trim().parse().unwrap_or(frames); }
        if let Some(v) = line.strip_prefix("out_time_us=") {
            if let Ok(us) = v.trim().parse::<i64>() { progress((us.max(0) as f64 / 1e6 / dur.max(0.01)).min(1.0)); }
        }
    }
    let status = child.wait()?;
    let elapsed = t0.elapsed().as_secs_f64();
    let err = err_thread.join().unwrap_or_default();
    if !status.success() {
        return Err(Error::Ffmpeg { code: status.code(), stderr: match crate::hwenc::classify_failure(&err) {
            crate::hwenc::Status::Unavailable(m) | crate::hwenc::Status::Refused(m) => m,
            _ => err.lines().rev().take(3).collect::<Vec<_>>().join(" / "),
        } });
    }
    if frames == 0 { return Err(Error::Format(tr("no picture encoded").into())); }
    let (cpu, real) = parse_bench(&err).unwrap_or((0.0, elapsed));
    let real = real.max(0.001);
    let cores = std::thread::available_parallelism().map_or(1, |n| n.get()) as f64;
    progress(1.0);
    Ok(Measure {
        frames, seconds: real, fps: frames as f64 / real, speed: frames as f64 / fps_src.max(1.0) / real,
        cpu_cores: cpu / real, cpu_share: (cpu / real / cores).min(1.0),
    })
}

/// Mesure `dur` secondes de `src` a partir du milieu de la video (ou du debut si elle est courte), avec le preset et la qualite
/// `crf` : x264, puis l'encodeur materiel `hw` (s'il y en a un et qu'il s'applique a cette video), avec et sans decodage par
/// la carte graphique. `step` recoit l'avancement global (0 a 1) et la configuration en cours.
#[allow(clippy::too_many_arguments)]
pub fn measure(
    tools: &Tools, src: &Path, dur: f64, preset: &str, crf: u32, hw: Option<(VideoEncoder, bool)>, step: &mut dyn FnMut(f64, &str), cancel: &AtomicBool,
) -> Result<(Vec<CaseResult>, Option<String>)> {
    let info = probe(tools, src)?;
    let dur = dur.min(info.duration_s.max(1.0));
    let start = ((info.duration_s - dur) / 2.0).max(0.0);
    let base = EncodeOpts { preset: preset.to_string(), crf, ..Default::default() };
    let mut cases: Vec<(Case, EncodeOpts, bool)> = vec![(Case::X264, base.clone(), false)];
    let mut note = None;
    if let Some((e, interlaced)) = hw.filter(|h| h.0 != VideoEncoder::X264) {
        let o = EncodeOpts { encoder: e, encoder_interlaced: interlaced, ..base.clone() };
        let (_, tg, applies) = crate::ffmpeg::bench_args(src, &info, &o, start, dur, false);
        if applies {
            cases.push((Case::Hw, o.clone(), false));
            cases.push((Case::HwGpuDecode, o, true));
        } else {
            note = Some(trf!("target {}: the hardware encoder does not code interlaced video, so this video stays on x264", tg.label));
        }
    } else {
        note = Some(tr("no hardware encoder chosen or detected: only x264 is measured").to_string());
    }
    let n = cases.len() as f64;
    let mut out = Vec::new();
    for (k, (case, o, gpu)) in cases.into_iter().enumerate() {
        let (args, _, _) = crate::ffmpeg::bench_args(src, &info, &o, start, dur, gpu);
        let label = match case { Case::X264 => trf!("{} ({})", tr(case.label()), preset), _ => trf!("{}: {}", tr(o.encoder.label()), tr(case.label())) };
        step(k as f64 / n, &label);
        let r = run_case(tools, &args, dur, info.fps(), &mut |f| step((k as f64 + f) / n, &label), cancel);
        match r {
            Err(Error::Cancelled) => return Err(Error::Cancelled),
            r => out.push(CaseResult { case, result: r.map_err(|e| match e { Error::Ffmpeg { stderr, .. } => stderr, e => e.to_string() }) }),
        }
    }
    step(1.0, "");
    Ok((out, note))
}
