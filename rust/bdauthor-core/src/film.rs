//! Du fichier video au clip .m2ts Blu-ray : ffmpeg (transcodage) puis lecture du TS produit et muxage — equivalent de la fin de `encode.py`.
use crate::ffmpeg::{self, EncodeOpts, Error, Result, Tools};
use crate::m2ts::reassemble;
use crate::mux::PmtStream;
use crate::reader::*;
use crate::tsclip::{nal_units, parse_psi};
use crate::vmux::*;
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;

pub const START_PTS: i64 = 54_000_000; // 600 s, comme le film de reference

/// Dossier temporaire supprime a la destruction.
struct TempDir(PathBuf);
impl TempDir {
    fn new(parent: Option<&Path>) -> std::io::Result<Self> {
        let base = parent.map(Path::to_path_buf).unwrap_or_else(std::env::temp_dir);
        let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0);
        let p = base.join(format!("bdauthor_{}_{nanos}", std::process::id()));
        std::fs::create_dir_all(&p)?;
        Ok(TempDir(p))
    }
}
impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// Decoupe un bloc de donnees AC3 en trames (sync 0B77, taille d'apres frmsizecod, 48 kHz).
pub fn ac3_frames(payload: &[u8]) -> PResult<Vec<&[u8]>> {
    let (mut out, mut pos) = (Vec::new(), 0usize);
    while pos + 7 <= payload.len() {
        if payload[pos..pos + 2] != [0x0b, 0x77] {
            return err("synchro AC3 absente");
        }
        if payload[pos + 4] >> 6 != 0 {
            return err("seul 48 kHz est gere");
        }
        let n = *AC3_WORDS_48K.get((payload[pos + 4] & 0x3F) as usize).ok_or_else(|| ParseError("frmsizecod AC3 invalide".into()))? as usize * 2;
        out.push(&payload[pos..(pos + n).min(payload.len())]);
        pos += n;
    }
    Ok(out)
}

/// Descripteur AC-3 de la PMT (tag 0x81) d'apres l'en-tete de la premiere trame.
pub fn ac3_descriptor(frame: &[u8]) -> PResult<Vec<u8>> {
    if frame.len() < 10 {
        return err("trame AC3 trop courte");
    }
    let (fscod, frmsizecod, bsid) = (frame[4] >> 6, frame[4] & 0x3F, frame[5] >> 3);
    let (bsmod, acmod) = (frame[5] & 7, frame[6] >> 5);
    let word = u32::from_be_bytes(frame[6..10].try_into().unwrap());
    let bit = |p: usize| ((word >> (31 - p)) & 1) as u8;
    let mut pos = 3usize;
    let mut dsurmod = 0u8;
    if acmod & 1 != 0 && acmod != 1 {
        pos += 2;
    }
    if acmod & 4 != 0 {
        pos += 2;
    }
    if acmod == 2 {
        dsurmod = (bit(pos) << 1) | bit(pos + 1);
        pos += 2;
    }
    let lfeon = bit(pos);
    let channels = if acmod != 0 { acmod + if lfeon != 0 { 8 } else { 0 } } else { 0 };
    Ok(vec![0x81, 4, (fscod << 5) | bsid, ((frmsizecod >> 1) << 2) | dsurmod, (bsmod << 5) | (channels << 1), 0])
}

pub fn avc_descriptor(sps_nal: &[u8]) -> Vec<u8> {
    vec![0x28, 4, sps_nal[1], sps_nal[2], sps_nal[3], 0xBF]
}

#[derive(Debug, Clone)]
pub struct Encoded {
    pub video: Vec<VideoAu>,
    pub audio: Vec<AudioFrame>,
    pub sps: Vec<u8>,
}

/// Lit un MPEG-TS (paquets de 188 octets) produit par ffmpeg : unites d'acces video, trames AC3, SPS ; horodatages decales pour
/// commencer a `START_PTS`.
pub fn load_encoded(ts: &[u8]) -> PResult<Encoded> {
    let mut data = Vec::with_capacity(ts.len() / 188 * 192);
    for c in ts.chunks_exact(188) {
        data.extend_from_slice(&[0, 0, 0, 0]);
        data.extend_from_slice(c);
    }
    let (_, _, streams) = parse_psi(&data)?;
    let vpid = streams.iter().find(|s| s.0 == 0x1B).ok_or_else(|| ParseError("aucun flux H.264".into()))?.1;
    let apid = streams.iter().find(|s| s.0 == 0x81).map(|s| s.1);
    let mut video = Vec::new();
    let mut sps: Option<Vec<u8>> = None;
    for e in reassemble(&data, vpid)? {
        if sps.is_none() {
            if let Some(&(_, off, ln)) = nal_units(&e.data).iter().find(|n| n.0 == 7) {
                sps = Some(e.data[off..off + ln].to_vec());
            }
        }
        video.push((e.pts.ok_or_else(|| ParseError("PES video sans PTS".into()))?, e.dts, e.data));
    }
    let mut audio = Vec::new();
    if let Some(a) = apid {
        for e in reassemble(&data, a)? {
            let pts = e.pts.ok_or_else(|| ParseError("PES audio sans PTS".into()))?;
            for (k, fr) in ac3_frames(&e.data)?.into_iter().enumerate() {
                audio.push((pts + k as u64 * AC3_FRAME_TICKS, fr.to_vec()));
            }
        }
    }
    let first = video.iter().map(|v| v.0).chain(audio.iter().map(|a| a.0)).min().ok_or_else(|| ParseError("flux vide".into()))? as i64;
    let shift = START_PTS - first;
    let sh = |p: u64| (p as i64 + shift) as u64;
    let mut video: Vec<VideoAu> = video.into_iter().map(|(p, d, x)| VideoAu { pts: sh(p), dts: d.map(sh), data: x }).collect();
    video.sort_by_key(|a| a.dts.unwrap_or(a.pts));
    let audio = audio.into_iter().map(|(p, x)| AudioFrame { pts: sh(p), data: x }).collect();
    Ok(Encoded { video, audio, sps: sps.ok_or_else(|| ParseError("aucun SPS".into()))? })
}

/// Muxage M2TS d'un flux encode (`lang` : code de langue de la piste audio, 3 lettres).
pub fn mux_encoded(enc: &Encoded, lang: [u8; 3]) -> Result<(Vec<u8>, MuxStats)> {
    let mut streams = vec![PmtStream { stream_type: 0x1B, pid: 0x1011, desc: avc_descriptor(&enc.sps) }];
    if let Some(a0) = enc.audio.first() {
        let mut d = vec![0x05, 0x04, 0x41, 0x43, 0x2d, 0x33];
        d.extend(ac3_descriptor(&a0.data)?);
        d.extend_from_slice(&[0x0A, 4]);
        d.extend_from_slice(&lang);
        d.push(0);
        streams.push(PmtStream { stream_type: 0x81, pid: 0x1100, desc: d });
    }
    mux_film(&enc.video, &enc.audio, &streams, &MuxParams::default()).map_err(Error::Format)
}

#[derive(Debug, Clone)]
pub struct FilmStats {
    pub cible: String,
    pub images: usize,
    pub trames_audio: usize,
    pub mux: MuxStats,
}

/// Video quelconque -> contenu d'un .m2ts Blu-ray.
pub fn make_film_m2ts(
    tools: &Tools, src: &Path, lang: [u8; 3], opts: &EncodeOpts, workdir: Option<&Path>, progress: &mut dyn FnMut(f64), cancel: &AtomicBool,
) -> Result<(Vec<u8>, FilmStats)> {
    let info = ffmpeg::probe(tools, src)?;
    let tmp = TempDir::new(workdir)?;
    let ts_path = tmp.0.join("enc.ts");
    let target = ffmpeg::encode_to_ts(tools, src, &ts_path, &info, opts, progress, cancel)?;
    let enc = load_encoded(&std::fs::read(&ts_path)?)?;
    drop(tmp);
    let (m2ts, mux) = mux_encoded(&enc, lang)?;
    Ok((m2ts, FilmStats { cible: target.label.to_string(), images: enc.video.len(), trames_audio: enc.audio.len(), mux }))
}

/// Image fixe -> clip video seul (pas d'audio) de `seconds` secondes au format `size`.
pub fn make_still_m2ts(
    tools: &Tools, image: &Path, seconds: f64, size: (u32, u32), fps: &str, opts: &EncodeOpts, workdir: Option<&Path>, cancel: &AtomicBool,
) -> Result<(Vec<u8>, FilmStats)> {
    let tmp = TempDir::new(workdir)?;
    let ts_path = tmp.0.join("still.ts");
    ffmpeg::make_still_ts(tools, image, &ts_path, seconds, size, fps, opts, cancel)?;
    let enc = load_encoded(&std::fs::read(&ts_path)?)?;
    drop(tmp);
    let (m2ts, mux) = mux_encoded(&enc, *b"und")?;
    Ok((m2ts, FilmStats { cible: format!("{}x{} fixe", size.0, size.1), images: enc.video.len(), trames_audio: 0, mux }))
}
