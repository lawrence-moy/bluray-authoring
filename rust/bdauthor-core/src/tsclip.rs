//! Analyse d'un clip .m2ts de film (H.264 + AC3) et generation de son .clpi — equivalent de `tsclip.py`.
use crate::clpi::*;
use crate::m2ts::*;
use crate::reader::*;

pub const VIDEO_TYPES: [u8; 3] = [0x1B, 0x02, 0xEA];
pub const AUDIO_TYPES: [u8; 1] = [0x81];
/// (cadence, duree d'image en ticks 90 kHz, code de cadence du .clpi), dans l'ordre du dictionnaire Python
pub const FRAME_RATES: [(f64, f64, u8); 6] =
    [(23.976, 3753.75, 1), (24.0, 3750.0, 2), (25.0, 3600.0, 3), (29.97, 3003.0, 4), (50.0, 1800.0, 6), (59.94, 1501.5, 7)];

/// (pcr_pid, pmt_pid, [(stream_type, pid)]) d'apres la PAT et la PMT.
pub fn parse_psi(data: &[u8]) -> PResult<(u16, u16, Vec<(u8, u16)>)> {
    let section = |pid: u16| -> PResult<Vec<u8>> {
        for p in iter_packets(data)? {
            let p = p?;
            if p.pid == pid && p.pusi {
                let pl = p.payload;
                let off = 1 + *pl.first().ok_or_else(|| ParseError("section PSI vide".into()))? as usize;
                return Ok(pl.get(off..).unwrap_or(&[]).to_vec());
            }
        }
        err(format!("section PSI absente (PID {pid:#x})"))
    };
    let g = |s: &[u8], i: usize| -> PResult<usize> { s.get(i).map(|&b| b as usize).ok_or_else(|| ParseError("section PSI tronquee".into())) };
    let pat = section(0)?;
    let n = ((g(&pat, 1)? & 0xF) << 8) | g(&pat, 2)?;
    let mut pmt_pid = None;
    let mut i = 8;
    while i + 4 < 3 + n {
        if (g(&pat, i)? << 8 | g(&pat, i + 1)?) != 0 {
            pmt_pid = Some((((g(&pat, i + 2)? & 0x1F) << 8) | g(&pat, i + 3)?) as u16);
            break;
        }
        i += 4;
    }
    let pmt_pid = pmt_pid.ok_or_else(|| ParseError("PMT introuvable dans la PAT".into()))?;
    let pmt = section(pmt_pid)?;
    let n = ((g(&pmt, 1)? & 0xF) << 8) | g(&pmt, 2)?;
    let pcr_pid = (((g(&pmt, 8)? & 0x1F) << 8) | g(&pmt, 9)?) as u16;
    let mut pos = 12 + (((g(&pmt, 10)? & 0xF) << 8) | g(&pmt, 11)?);
    let mut streams = Vec::new();
    while pos + 4 < 3 + n {
        streams.push((g(&pmt, pos)? as u8, (((g(&pmt, pos + 1)? & 0x1F) << 8) | g(&pmt, pos + 2)?) as u16));
        pos += 5 + (((g(&pmt, pos + 3)? & 0xF) << 8) | g(&pmt, pos + 4)?);
    }
    Ok((pcr_pid, pmt_pid, streams))
}

// ---------------------------------------------------------------- H.264
struct Bits<'a> {
    b: &'a [u8],
    p: usize,
}
impl<'a> Bits<'a> {
    fn u(&mut self, k: u32) -> PResult<u64> {
        let mut v = 0u64;
        for _ in 0..k {
            let byte = *self.b.get(self.p / 8).ok_or_else(|| ParseError("SPS tronque".into()))?;
            v = (v << 1) | ((byte >> (7 - self.p % 8)) & 1) as u64;
            self.p += 1;
        }
        Ok(v)
    }
    fn ue(&mut self) -> PResult<u64> {
        let mut z = 0u32;
        while self.u(1)? == 0 {
            z += 1;
            if z > 32 {
                return err("Exp-Golomb invalide");
            }
        }
        Ok((1u64 << z) - 1 + if z > 0 { self.u(z)? } else { 0 })
    }
    fn se(&mut self) -> PResult<i64> {
        let k = self.ue()? as i64;
        Ok(if k & 1 == 1 { (k + 1) / 2 } else { -(k / 2) })
    }
}

/// [(type, offset_debut_charge, longueur_charge)] pour un flux Annex B.
pub fn nal_units(b: &[u8]) -> Vec<(u8, usize, usize)> {
    let mut idx = Vec::new();
    let mut i = 0usize;
    while i + 3 <= b.len() {
        match b[i..].windows(3).position(|w| w == [0, 0, 1]) {
            Some(j) => {
                idx.push(i + j);
                i += j + 3;
            }
            None => break,
        }
    }
    let ends: Vec<usize> = idx.iter().skip(1).copied().chain(std::iter::once(b.len())).collect();
    idx.iter().zip(ends).filter(|(&s, _)| s + 3 < b.len()).map(|(&s, e)| (b[s + 3] & 0x1F, s + 3, e - s - 3)).collect()
}

/// `nal` = octets du NAL SPS (en-tete inclus). Retourne (largeur, hauteur, frame_mbs_only).
pub fn parse_sps(nal: &[u8]) -> PResult<(u32, u32, bool)> {
    let mut raw = Vec::with_capacity(nal.len());
    for (k, &x) in nal.iter().enumerate() {
        if k >= 2 && x == 3 && nal[k - 1] == 0 && nal[k - 2] == 0 {
            continue;
        }
        raw.push(x);
    }
    let mut b = Bits { b: raw.get(1..).unwrap_or(&[]), p: 0 };
    let profile = b.u(8)?;
    b.u(8)?;
    b.u(8)?;
    b.ue()?;
    let mut chroma = 1u64;
    if [100, 110, 122, 244, 44, 83, 86, 118, 128, 138, 139, 134, 135].contains(&profile) {
        chroma = b.ue()?;
        if chroma == 3 {
            b.u(1)?;
        }
        b.ue()?;
        b.ue()?;
        b.u(1)?;
        if b.u(1)? == 1 {
            for i in 0..(if chroma != 3 { 8 } else { 12 }) {
                if b.u(1)? == 1 {
                    let (mut last, size) = (8i64, if i < 6 { 16 } else { 64 });
                    let mut nxt = 8i64;
                    for _ in 0..size {
                        if nxt != 0 {
                            nxt = (last + b.se()?).rem_euclid(256);
                        }
                        if nxt != 0 {
                            last = nxt;
                        }
                    }
                }
            }
        }
    }
    b.ue()?;
    let poc = b.ue()?;
    if poc == 0 {
        b.ue()?;
    } else if poc == 1 {
        b.u(1)?;
        b.se()?;
        b.se()?;
        for _ in 0..b.ue()? {
            b.se()?;
        }
    }
    b.ue()?;
    b.u(1)?;
    let (w_mbs, h_map) = (b.ue()? + 1, b.ue()? + 1);
    let frame_mbs_only = b.u(1)?;
    if frame_mbs_only == 0 {
        b.u(1)?;
    }
    b.u(1)?;
    let mut crop = (0u64, 0u64, 0u64, 0u64);
    if b.u(1)? == 1 {
        crop = (b.ue()?, b.ue()?, b.ue()?, b.ue()?);
    }
    let (cx, cy) = if chroma == 1 { (2, 2 * (2 - frame_mbs_only)) } else { (1, 2 - frame_mbs_only) };
    Ok(((w_mbs * 16 - cx * (crop.0 + crop.1)) as u32, ((2 - frame_mbs_only) * h_map * 16 - cy * (crop.2 + crop.3)) as u32, frame_mbs_only == 1))
}

pub fn video_format_code(height: u32, progressive: bool) -> PResult<u8> {
    Ok(match (height, progressive) {
        (480, true) => 3, (480, false) => 1, (576, true) => 7, (576, false) => 2, (720, _) => 5, (1080, true) => 6, (1080, false) => 4,
        _ => return err(format!("hauteur {height} non geree")),
    })
}

fn median(v: &mut Vec<u64>) -> f64 {
    v.sort_unstable();
    let n = v.len();
    if n % 2 == 1 { v[n / 2] as f64 } else { (v[n / 2 - 1] + v[n / 2]) as f64 / 2.0 }
}

pub fn frame_rate(pts_list: &[u64]) -> PResult<f64> {
    let mut s: Vec<u64> = pts_list.to_vec();
    s.sort_unstable();
    s.dedup();
    let mut d: Vec<u64> = s.windows(2).map(|w| w[1] - w[0]).collect();
    if d.is_empty() {
        return err("cadence indeterminee (moins de 2 images)");
    }
    let fps = 90000.0 / median(&mut d);
    let mut best = FRAME_RATES[0].0;
    for &(r, _, _) in &FRAME_RATES {
        if (r - fps).abs() < (best - fps).abs() {
            best = r;
        }
    }
    if (best - fps).abs() > 0.2 {
        return err(format!("cadence non reconnue : {fps:.3} i/s"));
    }
    Ok(best)
}

/// (code de format, code de frequence) depuis le premier en-tete AC3.
pub fn ac3_attrs(payload: &[u8]) -> PResult<(u8, u8)> {
    let i = payload.windows(2).position(|w| w == [0x0b, 0x77]).ok_or_else(|| ParseError("synchro AC3 absente".into()))?;
    let (b4, b6) = (*payload.get(i + 4).ok_or_else(|| ParseError("AC3 tronque".into()))?, *payload.get(i + 6).ok_or_else(|| ParseError("AC3 tronque".into()))?);
    if b4 >> 6 != 0 {
        return err("seul 48 kHz est gere");
    }
    let acmod = b6 >> 5;
    Ok((if acmod == 1 { 1 } else if acmod == 2 { 3 } else { 6 }, 1))
}

#[derive(Debug, Clone)]
pub struct ClipScan {
    pub num_packets: usize,
    pub pcr_pid: u16,
    pub pmt_pid: u16,
    pub video: (u16, u8), // (pid, stream_type)
    pub audio: Vec<(u16, u8)>,
    pub width: u32,
    pub height: u32,
    pub progressive: bool,
    pub fps: f64,
    pub start90: u64,
    pub end90: u64,
    pub eps: Vec<(u64, usize, usize)>, // (pts, spn, taille de l'unite d'acces)
    pub video_code: u8,
    pub audio_codes: Vec<(u16, u8, u8)>, // (pid, stream_type, octet format/frequence)
}

pub fn scan_clip(data: &[u8]) -> PResult<ClipScan> {
    let (pcr_pid, pmt_pid, streams) = parse_psi(data)?;
    let vid = *streams.iter().find(|s| VIDEO_TYPES.contains(&s.0)).ok_or_else(|| ParseError("aucun flux video".into()))?;
    let aud: Vec<(u8, u16)> = streams.iter().copied().filter(|s| AUDIO_TYPES.contains(&s.0)).collect();
    let vpes = reassemble(data, vid.1)?;
    let mut sps = None;
    let mut eps = Vec::new();
    for e in &vpes {
        for (t, off, ln) in nal_units(&e.data) {
            if t == 7 {
                if sps.is_none() {
                    sps = Some(parse_sps(&e.data[off..off + ln])?);
                }
                eps.push((e.pts.ok_or_else(|| ParseError("PES video sans PTS".into()))?, e.first_packet, e.data.len()));
                break;
            }
        }
    }
    let (w, h, prog) = sps.ok_or_else(|| ParseError("aucun SPS dans le flux video".into()))?;
    let pts: Vec<u64> = vpes.iter().filter_map(|e| e.pts).collect();
    let fps = frame_rate(&pts)?;
    let mut firsts = vec![*pts.iter().min().unwrap()];
    let mut audio_codes = Vec::new();
    for &(t, pid) in &aud {
        let a = reassemble(data, pid)?;
        if let Some(m) = a.iter().filter_map(|e| e.pts).min() {
            firsts.push(m);
        }
        let first = a.first().ok_or_else(|| ParseError("flux audio vide".into()))?;
        let (fmt, rate) = ac3_attrs(&first.data)?;
        audio_codes.push((pid, t, (fmt << 4) | rate));
    }
    let rate_code = FRAME_RATES.iter().find(|r| r.0 == fps).unwrap().2;
    let video_code = (video_format_code(h, prog)? << 4) | rate_code;
    Ok(ClipScan {
        num_packets: data.len() / 192, pcr_pid, pmt_pid, video: (vid.1, vid.0), audio: aud.iter().map(|&(t, p)| (p, t)).collect(),
        width: w, height: h, progressive: prog, fps, start90: *firsts.iter().min().unwrap(), end90: *pts.iter().max().unwrap(), eps, video_code, audio_codes,
    })
}

/// Hypothese ajustee sur 19 images (valeurs 2 et 3 seulement observees) : ceil(log2(taille / 64 Kio)), bornee a 0..=7.
pub fn i_end_offset(au_bytes: usize) -> u32 {
    if au_bytes == 0 {
        return 0;
    }
    let v = (au_bytes as f64 / 65536.0).log2().ceil();
    v.clamp(0.0, 7.0) as u32
}

pub fn build_ep_map(pid: u16, eps: &[(u64, usize, usize)]) -> Cpi {
    let mut st = EpStream { pid, ep_stream_type: 1, coarse: vec![], fine: vec![] };
    let mut last: Option<(u64, usize)> = None;
    for (i, &(pts, spn, size)) in eps.iter().enumerate() {
        let (c, s) = ((pts >> 19) & 0x3FFF, spn >> 17);
        if last != Some((c, s)) {
            st.coarse.push(EpCoarse { ref_fine: i as u32, pts: c as u32, spn: spn as u32 });
            last = Some((c, s));
        }
        st.fine.push(EpFine { angle_change: 0, end_offset: i_end_offset(size), pts: ((pts >> 9) & 0x7FF) as u32, spn: (spn & 0x1FFFF) as u32 });
    }
    Cpi { cpi_type: 1, streams: vec![st] }
}

/// Division entiere flottante a la maniere de Python (`a // b`).
pub fn py_floordiv(a: f64, b: f64) -> f64 {
    let m = a % b;
    let mut div = (a - m) / b;
    if m != 0.0 && (b < 0.0) != (m < 0.0) {
        div -= 1.0;
    }
    let mut fl = div.floor();
    if div - fl > 0.5 {
        fl += 1.0;
    }
    fl
}

/// .clpi d'un clip de film. `pad` : octet de remplissage des attributs de flux (multiAVCHD : b'0', disques usuels : zeros).
pub fn make_clip_clpi(data: &[u8], lang: [u8; 3], pad: u8, aspect_code: Option<u8>, version: &str) -> PResult<Vec<u8>> {
    let sc = scan_clip(data)?;
    let (start45, end45) = ((sc.start90 / 2) as u32, (sc.end90 / 2) as u32);
    let rate = py_floordiv(data.len() as f64, (end45 - start45) as f64 / 45000.0) as u32;
    let aspect = aspect_code.unwrap_or(if (sc.width as f64 / sc.height as f64 - 16.0 / 9.0).abs() < 0.05 { 3 } else { 2 });
    let fill = |head: &[u8]| -> Vec<u8> {
        let mut v = head.to_vec();
        v.extend(std::iter::repeat(pad).take(12));
        v.resize(20, 0);
        v
    };
    let mut streams = vec![Stream { pid: sc.video.0, coding_type: sc.video.1, attrs: fill(&[sc.video_code, aspect << 4, 0, 0]) }];
    for &(pid, typ, code) in &sc.audio_codes {
        streams.push(Stream { pid, coding_type: typ, attrs: fill(&[code, lang[0], lang[1], lang[2]]) });
    }
    Ok(Clpi {
        version: version.into(),
        clip_info: ClipInfo { clip_stream_type: 1, application_type: 1, is_cc5: 0, ts_recording_rate: rate, num_source_packets: sc.num_packets as u32, ..Default::default() },
        sequence: SequenceInfo { atc: vec![AtcSeq { spn_atc_start: 0, offset_stc_id: 0, stc: vec![StcSeq { pcr_pid: sc.pcr_pid, spn_stc_start: 0, start_time: start45, end_time: end45 }] }] },
        program: ProgramInfo { programs: vec![Program { spn_start: 0, pmt_pid: sc.pmt_pid, num_groups: 0, streams }] },
        cpi: Some(build_ep_map(sc.video.0, &sc.eps)),
        clip_mark: vec![0; 4],
        extension: vec![],
    }
    .pack())
}
