//! Verification de conformite Blu-ray d'un flux H.264 deja encode (unites d'acces en Annex B).
//!
//! Sert a juger la sortie d'un encodeur materiel (Quick Sync, NVENC, AMF) : un pilote peut ignorer un reglage sans le dire.
//! Points verifies : profil (Main ou High, 8 bits 4:2:0), niveau 4.1 au plus, nombre d'images de reference permis par le
//! niveau, parametres HRD dans le SPS avec un tampon de 30 Mbit au plus et un debit de 40 Mb/s au plus, messages SEI
//! « buffering period » a chaque image cle et « picture timing » a chaque image, delimiteurs d'unite d'acces, ecart entre
//! images cles, suites d'images B et debit de pointe sur une seconde.
use crate::i18n::tr;
use crate::trf;

/// Plafond du debit video d'un Blu-ray (b/s) et du tampon de decodage en 1080 (bits).
pub const MAX_VIDEO_BPS: u64 = 40_000_000;
pub const MAX_CPB_BITS: u64 = 30_000_000;
/// Images B consecutives au plus.
pub const MAX_B_RUN: usize = 3;

/// Ce que dit le SPS.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct SpsInfo {
    pub profile: u8,
    pub level: u8,
    pub chroma_format: u8,
    pub bit_depth: u8,
    pub width_mbs: u32,
    /// hauteur de l'image en macroblocs (les deux trames pour un flux entrelace)
    pub height_mbs: u32,
    pub frame_mbs_only: bool,
    pub max_refs: u32,
    /// parametres HRD (NAL) : debit (b/s) et tampon (bits) du premier ordonnancement
    pub hrd: Option<(u64, u64)>,
    /// les messages « picture timing » sont attendus (HRD present ou pic_struct present)
    pub pic_timing_expected: bool,
}

/// Resultat : defauts trouves (vide : conforme) et mesures.
#[derive(Debug, Clone, Default)]
pub struct Report {
    pub issues: Vec<String>,
    pub sps: Option<SpsInfo>,
    /// plus grand ecart entre images cles (images), plus longue suite d'images B, debit de pointe sur 1 s (kb/s)
    pub gop_max: usize,
    pub b_run_max: usize,
    pub peak_kbps: u64,
    pub frames: usize,
}

impl Report {
    pub fn ok(&self) -> bool { self.issues.is_empty() }
}

struct Bits<'a> {
    b: &'a [u8],
    p: usize,
}

impl Bits<'_> {
    fn u(&mut self, k: u32) -> Option<u64> {
        let mut v = 0u64;
        for _ in 0..k {
            let byte = *self.b.get(self.p / 8)?;
            v = (v << 1) | ((byte >> (7 - self.p % 8)) & 1) as u64;
            self.p += 1;
        }
        Some(v)
    }
    fn ue(&mut self) -> Option<u64> {
        let mut z = 0u32;
        while self.u(1)? == 0 {
            z += 1;
            if z > 32 { return None; }
        }
        Some((1u64 << z) - 1 + if z > 0 { self.u(z)? } else { 0 })
    }
    fn se(&mut self) -> Option<i64> {
        let k = self.ue()? as i64;
        Some(if k & 1 == 1 { (k + 1) / 2 } else { -(k / 2) })
    }
}

/// Charge utile d'un NAL sans ses octets anti-emulation (en-tete d'un octet retire).
fn rbsp(nal: &[u8]) -> Vec<u8> {
    let mut raw = Vec::with_capacity(nal.len());
    for (k, &x) in nal.iter().enumerate().skip(1) {
        if k >= 3 && x == 3 && nal[k - 1] == 0 && nal[k - 2] == 0 { continue; }
        raw.push(x);
    }
    raw
}

fn hrd(b: &mut Bits) -> Option<(u64, u64)> {
    let cnt = b.ue()? + 1;
    let (br_scale, cpb_scale) = (b.u(4)?, b.u(4)?);
    let mut first = None;
    for _ in 0..cnt {
        let br = (b.ue()? + 1) << (6 + br_scale);
        let cpb = (b.ue()? + 1) << (4 + cpb_scale);
        b.u(1)?;
        first.get_or_insert((br, cpb));
    }
    b.u(20)?;
    first
}

/// Lit un SPS (`nal` : NAL complet, en-tete compris).
pub fn parse_sps(nal: &[u8]) -> Option<SpsInfo> {
    let raw = rbsp(nal);
    let mut b = Bits { b: &raw, p: 0 };
    let mut s = SpsInfo { profile: b.u(8)? as u8, ..Default::default() };
    b.u(8)?;
    s.level = b.u(8)? as u8;
    b.ue()?;
    s.chroma_format = 1;
    s.bit_depth = 8;
    if [100, 110, 122, 244, 44, 83, 86, 118, 128, 138, 139, 134, 135].contains(&s.profile) {
        s.chroma_format = b.ue()? as u8;
        if s.chroma_format == 3 { b.u(1)?; }
        s.bit_depth = (b.ue()? + 8) as u8;
        b.ue()?;
        b.u(1)?;
        if b.u(1)? == 1 {
            for i in 0..(if s.chroma_format != 3 { 8 } else { 12 }) {
                if b.u(1)? == 1 {
                    let (mut last, mut next, size) = (8i64, 8i64, if i < 6 { 16 } else { 64 });
                    for _ in 0..size {
                        if next != 0 { next = (last + b.se()?).rem_euclid(256); }
                        if next != 0 { last = next; }
                    }
                }
            }
        }
    }
    b.ue()?;
    match b.ue()? {
        0 => { b.ue()?; }
        1 => {
            b.u(1)?;
            b.se()?;
            b.se()?;
            for _ in 0..b.ue()? { b.se()?; }
        }
        _ => {}
    }
    s.max_refs = b.ue()? as u32;
    b.u(1)?;
    s.width_mbs = b.ue()? as u32 + 1;
    let map_units = b.ue()? as u32 + 1;
    s.frame_mbs_only = b.u(1)? == 1;
    s.height_mbs = map_units * if s.frame_mbs_only { 1 } else { 2 };
    if !s.frame_mbs_only { b.u(1)?; }
    b.u(1)?;
    if b.u(1)? == 1 { for _ in 0..4 { b.ue()?; } }
    if b.u(1)? == 1 {
        // VUI
        if b.u(1)? == 1 && b.u(8)? == 255 { b.u(32)?; }
        if b.u(1)? == 1 { b.u(1)?; }
        if b.u(1)? == 1 {
            b.u(4)?;
            if b.u(1)? == 1 { b.u(24)?; }
        }
        if b.u(1)? == 1 { b.ue()?; b.ue()?; }
        if b.u(1)? == 1 { b.u(65)?; }
        let nal_hrd = b.u(1)? == 1;
        if nal_hrd { s.hrd = Some(hrd(&mut b)?); }
        let vcl_hrd = b.u(1)? == 1;
        if vcl_hrd {
            let v = hrd(&mut b)?;
            s.hrd.get_or_insert(v);
        }
        if nal_hrd || vcl_hrd { b.u(1)?; }
        s.pic_timing_expected = nal_hrd || vcl_hrd || b.u(1)? == 1;
    }
    Some(s)
}

/// Types des messages SEI d'un NAL SEI.
fn sei_types(nal: &[u8]) -> Vec<u64> {
    let raw = rbsp(nal);
    let (mut out, mut i) = (Vec::new(), 0usize);
    while i < raw.len() && raw[i] != 0x80 {
        let (mut t, mut n) = (0u64, 0usize);
        while i < raw.len() && raw[i] == 0xFF { t += 255; i += 1; }
        if i >= raw.len() { break; }
        t += raw[i] as u64;
        i += 1;
        while i < raw.len() && raw[i] == 0xFF { n += 255; i += 1; }
        if i >= raw.len() { break; }
        n += raw[i] as usize;
        i += 1 + n;
        out.push(t);
    }
    out
}

/// Type de tranche (0 P, 1 B, 2 I, 3 SP, 4 SI) de la premiere tranche d'un NAL de type 1 ou 5.
fn slice_type(nal: &[u8]) -> Option<u8> {
    let raw = rbsp(&nal[..nal.len().min(32)]);
    let mut b = Bits { b: &raw, p: 0 };
    b.ue()?;
    Some((b.ue()? % 5) as u8)
}

/// Images de reference permises au niveau `level` pour une image de `mbs` macroblocs (DPB du niveau 4.1 et en dessous).
fn dpb_frames(level: u8, mbs: u32) -> u32 {
    let max_dpb_mbs: u32 = match level {
        0..=10 => 396, 11 => 900, 12 | 13 | 20 => 2376, 21 => 4752, 22 | 30 => 8100, 31 => 18000, 32 => 20480, _ => 32768,
    };
    (max_dpb_mbs / mbs.max(1)).min(16)
}

/// Verifie un flux : `aus` = unites d'acces dans l'ordre de decodage, avec leur PTS/DTS (90 kHz) ;
/// `keyint` : ecart maximal voulu entre images cles (images).
pub fn check(aus: &[crate::vmux::VideoAu], keyint: u32) -> Report {
    let mut r = Report { frames: aus.len(), ..Default::default() };
    if aus.is_empty() {
        r.issues.push(tr("no video picture").to_string());
        return r;
    }
    let (mut no_aud, mut no_bp, mut no_pt) = (0usize, 0usize, 0usize);
    let (mut since_key, mut b_run) = (None::<usize>, 0usize);
    let mut first_key = true;
    for (k, au) in aus.iter().enumerate() {
        let nals = crate::tsclip::nal_units(&au.data);
        if nals.first().map(|n| n.0) != Some(9) { no_aud += 1; }
        let (mut key, mut is_b, mut bp, mut pt) = (false, false, false, false);
        for &(t, off, len) in &nals {
            let nal = &au.data[off..off + len];
            match t {
                7 if r.sps.is_none() => r.sps = parse_sps(nal),
                6 => {
                    for st in sei_types(nal) {
                        if st == 0 { bp = true; }
                        if st == 1 { pt = true; }
                    }
                }
                5 => key = true,
                1 => match slice_type(nal) {
                    Some(2) | Some(4) => key = true,
                    Some(1) => is_b = true,
                    _ => {}
                },
                _ => {}
            }
        }
        if k == 0 && !key { first_key = false; }
        if key {
            if let Some(n) = since_key { r.gop_max = r.gop_max.max(n); }
            since_key = Some(1);
            if !bp { no_bp += 1; }
        } else if let Some(n) = since_key.as_mut() {
            *n += 1;
        }
        b_run = if is_b { b_run + 1 } else { 0 };
        r.b_run_max = r.b_run_max.max(b_run);
        if !pt { no_pt += 1; }
    }
    if let Some(n) = since_key { r.gop_max = r.gop_max.max(n); }
    // debit de pointe : octets decodes sur une seconde glissante (DTS, a defaut PTS)
    let times: Vec<(u64, u64)> = aus.iter().map(|a| (a.dts.unwrap_or(a.pts), a.data.len() as u64 * 8)).collect();
    let (mut j, mut sum, mut peak) = (0usize, 0u64, 0u64);
    for i in 0..times.len() {
        sum += times[i].1;
        while times[i].0.saturating_sub(times[j].0) >= 90_000 { sum -= times[j].1; j += 1; }
        peak = peak.max(sum);
    }
    r.peak_kbps = peak / 1000;

    let is = &mut r.issues;
    match &r.sps {
        None => is.push(tr("no readable sequence parameter set (SPS)").to_string()),
        Some(s) => {
            if !matches!(s.profile, 77 | 100) { is.push(trf!("H.264 profile {} (Main or High expected)", s.profile)); }
            if s.chroma_format != 1 || s.bit_depth != 8 { is.push(tr("not 8-bit 4:2:0").to_string()); }
            if s.level > 41 { is.push(trf!("H.264 level {}.{} (4.1 at most)", s.level / 10, s.level % 10)); }
            let allowed = dpb_frames(s.level.min(41), s.width_mbs * s.height_mbs);
            if s.max_refs > allowed { is.push(trf!("{} reference pictures ({} at most at this size)", s.max_refs, allowed)); }
            match s.hrd {
                None => is.push(tr("no HRD parameters in the SPS (needed by Blu-ray players)").to_string()),
                Some((br, cpb)) => {
                    if cpb > MAX_CPB_BITS { is.push(trf!("decoder buffer of {} kbit (30000 at most)", cpb / 1000)); }
                    if br > MAX_VIDEO_BPS { is.push(trf!("declared peak rate of {} kb/s (40000 at most)", br / 1000)); }
                }
            }
            if no_pt > 0 && s.pic_timing_expected { is.push(trf!("{} picture(s) without picture timing message", no_pt)); }
        }
    }
    if !first_key { is.push(tr("the stream does not start with a key picture").to_string()); }
    if no_bp > 0 { is.push(trf!("{} key picture(s) without buffering period message", no_bp)); }
    if no_aud > 0 { is.push(trf!("{} picture(s) without access unit delimiter", no_aud)); }
    if r.gop_max > keyint as usize { is.push(trf!("{} pictures between key pictures ({} at most)", r.gop_max, keyint)); }
    if r.b_run_max > MAX_B_RUN { is.push(trf!("{} B pictures in a row ({} at most)", r.b_run_max, MAX_B_RUN)); }
    if peak > MAX_VIDEO_BPS { is.push(trf!("peak video rate of {} kb/s over one second (40000 at most)", r.peak_kbps)); }
    r
}
