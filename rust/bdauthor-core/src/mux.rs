//! Multiplexeur M2TS d'un clip de menu IG (sous-TS) et primitives communes (PSI, PCR, PES) — equivalent de `mux.py`.
//! Regles retrouvees sur le disque de reference (80002.m2ts) : voir le module Python.
use std::collections::BTreeSet;

pub const TS_SIZE: u64 = 188;
pub const PAYLOAD: usize = 184;
pub const STREAM_ID_IG: u8 = 0xBD;
pub const ALIGN_PACKETS: usize = 32;
/// 'HDMV' + copy control
pub const PMT_DESCRIPTORS: [u8; 12] = [0x05, 0x04, 0x48, 0x44, 0x4d, 0x56, 0x88, 0x04, 0x0f, 0xff, 0xfc, 0xfc];

pub fn crc32_mpeg(data: &[u8]) -> u32 {
    let mut c: u32 = 0xFFFF_FFFF;
    for &x in data {
        c ^= (x as u32) << 24;
        for _ in 0..8 {
            c = if c & 0x8000_0000 != 0 { (c << 1) ^ 0x04C1_1DB7 } else { c << 1 };
        }
    }
    c
}

fn section(mut body: Vec<u8>) -> Vec<u8> {
    let c = crc32_mpeg(&body);
    body.extend_from_slice(&c.to_be_bytes());
    body
}

fn p16(v: &mut Vec<u8>, x: u16) {
    v.extend_from_slice(&x.to_be_bytes());
}

pub fn make_pat(pmt_pid: u16) -> Vec<u8> {
    let (program, tsid, network_pid) = (1u16, 1u16, 0x001Fu16);
    let mut prog = Vec::new();
    for x in [0, 0xE000 | network_pid, program, 0xE000 | pmt_pid] {
        p16(&mut prog, x);
    }
    let mut body = vec![0x00];
    p16(&mut body, 0xB000 | (5 + prog.len() as u16 + 4));
    p16(&mut body, tsid);
    body.extend_from_slice(&[0xC1, 0, 0]);
    body.extend(prog);
    section(body)
}

/// Flux de la PMT : (stream_type, pid, descripteurs ES).
#[derive(Debug, Clone, PartialEq)]
pub struct PmtStream {
    pub stream_type: u8,
    pub pid: u16,
    pub desc: Vec<u8>,
}

pub fn make_pmt(streams: &[PmtStream], pcr_pid: u16) -> Vec<u8> {
    let program = 1u16;
    let mut lp = Vec::new();
    for s in streams {
        lp.push(s.stream_type);
        p16(&mut lp, 0xE000 | s.pid);
        p16(&mut lp, 0xF000 | s.desc.len() as u16);
        lp.extend_from_slice(&s.desc);
    }
    let info = PMT_DESCRIPTORS;
    let n = 9 + info.len() + lp.len() + 4;
    let mut body = vec![0x02];
    p16(&mut body, 0xB000 | n as u16);
    p16(&mut body, program);
    body.extend_from_slice(&[0xC1, 0, 0]);
    p16(&mut body, 0xE000 | pcr_pid);
    p16(&mut body, 0xF000 | info.len() as u16);
    body.extend_from_slice(&info);
    body.extend(lp);
    section(body)
}

/// `peak_rate` en unites de 50 octets/s.
pub fn make_sit(peak_rate: u32) -> Vec<u8> {
    let program = 1u16;
    let mut desc = vec![0x63, 0x08];
    desc.extend_from_slice(&(0xC0_0000 | peak_rate).to_be_bytes()[1..]);
    desc.extend_from_slice(&[0xff; 5]);
    let mut body = vec![0x7F];
    p16(&mut body, 0xF000 | (2 + 3 + 2 + desc.len() as u16 + 4 + 4));
    p16(&mut body, 0xFFFF);
    body.extend_from_slice(&[0xC1, 0, 0]);
    p16(&mut body, 0xF000 | desc.len() as u16);
    body.extend(desc);
    p16(&mut body, program);
    p16(&mut body, 0x8000);
    section(body)
}

/// Un PES de menu : horodatages (90 kHz) et segments IG concatenes.
#[derive(Debug, Clone, PartialEq)]
pub struct PesSpec {
    pub pts: u64,
    pub dts: Option<u64>,
    pub data: Vec<u8>,
}

pub fn ts5(prefix: u8, v: u64) -> [u8; 5] {
    [(prefix << 4) | ((((v >> 30) & 7) as u8) << 1) | 1, ((v >> 22) & 0xFF) as u8, ((((v >> 15) & 0x7F) as u8) << 1) | 1,
     ((v >> 7) & 0xFF) as u8, (((v & 0x7F) as u8) << 1) | 1]
}

pub fn build_ig_pes(spec: &PesSpec) -> Result<Vec<u8>, String> {
    let (flags, ts): (u8, Vec<u8>) = match spec.dts {
        None => (0x80, ts5(0b0010, spec.pts).to_vec()),
        Some(d) => (0xC0, [ts5(0b0011, spec.pts), ts5(0b0001, d)].concat()),
    };
    let length = 3 + ts.len() + spec.data.len();
    if length > 0xFFFF {
        return Err("PES trop long pour un champ de longueur 16 bits".into());
    }
    let mut o = vec![0, 0, 1, STREAM_ID_IG];
    p16(&mut o, length as u16);
    o.extend_from_slice(&[0x85, flags, ts.len() as u8]);
    o.extend(ts);
    o.extend_from_slice(&spec.data);
    Ok(o)
}

/// Duree de decodage d'un objet ou du plan : ceil(w * h * 90000 / 8e6).
pub fn plane_ticks(w: u64, h: u64) -> u64 {
    (w * h * 90000).div_ceil(8_000_000)
}

/// PTS/DTS des PES d'un menu. `ics`, `pds` : segments complets ; `ods` : (segment complet, largeur, hauteur).
pub fn ig_pes_specs(ics: &[u8], pds: &[u8], ods: &[(Vec<u8>, u64, u64)], ics_pts: u64, video_size: (u64, u64)) -> Vec<PesSpec> {
    let mut t = ics_pts - plane_ticks(video_size.0, video_size.1);
    let mut specs = vec![PesSpec { pts: ics_pts, dts: Some(t), data: ics.to_vec() }, PesSpec { pts: t, dts: None, data: pds.to_vec() }];
    for (seg, w, h) in ods {
        specs.push(PesSpec { pts: t + plane_ticks(*w, *h), dts: Some(t), data: seg.clone() });
        t += plane_ticks(*w, *h);
    }
    specs.push(PesSpec { pts: t, dts: None, data: vec![0x80, 0, 0] });
    specs
}

pub fn ats_of(n: u64, peak_rate: u64) -> u64 {
    (n * TS_SIZE * 27_000_000 / (peak_rate * 50)) & 0x3FFF_FFFF
}

pub fn pcr_packet(pcr: u64) -> Vec<u8> {
    let v: u64 = ((pcr / 300) << 15) | (0x3F << 9) | (pcr % 300);
    let mut o = vec![0x47, 0x10, 0x01, 0x20, 183, 0x10];
    o.extend_from_slice(&v.to_be_bytes()[2..]);
    o.resize(188, 0xff);
    o
}

pub fn psi_packet(pid: u16, section: &[u8], cc: u8) -> Vec<u8> {
    let mut pl = vec![0u8];
    pl.extend_from_slice(section);
    pl.resize(PAYLOAD, 0xFF);
    let mut o = vec![0x47, 0x60 | (pid >> 8) as u8, (pid & 0xFF) as u8, 0x10 | (cc & 15)];
    o.extend(pl);
    o
}

pub fn null_packet() -> Vec<u8> {
    let mut o = vec![0x47, 0x1F, 0xFF, 0x10];
    o.resize(188, 0);
    o
}

/// Un paquet de PES avec bourrage en champ d'adaptation si la charge est incomplete.
pub fn pes_packet(pid: u16, first: bool, cc: u8, chunk: &[u8]) -> Vec<u8> {
    let b1 = (if first { 0x40 } else { 0 }) | (pid >> 8) as u8;
    if chunk.len() == PAYLOAD {
        let mut o = vec![0x47, b1, (pid & 0xFF) as u8, 0x10 | cc];
        o.extend_from_slice(chunk);
        o
    } else {
        let alen = PAYLOAD - chunk.len() - 1;
        let mut o = vec![0x47, b1, (pid & 0xFF) as u8, 0x30 | cc, alen as u8];
        if alen > 0 {
            o.push(0);
            o.resize(o.len() + alen - 1, 0xff);
        }
        o.extend_from_slice(chunk);
        o
    }
}

/// Contenu complet d'un .m2ts de menu. `pcr_extra_at` : indices de paquets ou inserer un PCR (rejeu exact d'un disque de reference) ;
/// sinon un PCR est insere des que `pcr_interval` ticks (27 MHz) se sont ecoules (90 ms par defaut).
pub fn mux_ig_clip(pes_specs: &[PesSpec], pcr_extra_at: Option<&[usize]>) -> Result<Vec<u8>, String> {
    let (ig_pid, pmt_pid, pcr_pid, peak_rate, ig_stream_type, pcr_interval) = (0x1400u16, 0x0100u16, 0x1001u16, 107_679u64, 0x91u8, 2_430_000i64);
    let mut pkts: Vec<Option<Vec<u8>>> = vec![
        Some(psi_packet(0, &make_pat(pmt_pid), 0)),
        Some(psi_packet(pmt_pid, &make_pmt(&[PmtStream { stream_type: ig_stream_type, pid: ig_pid, desc: vec![] }], pcr_pid), 0)),
        Some(psi_packet(0x1F, &make_sit(peak_rate as u32), 0)),
        None,
    ];
    let (mut cc, mut last_pcr_ats) = (0u8, 0i64);
    let extra: BTreeSet<usize> = pcr_extra_at.unwrap_or(&[]).iter().copied().collect();
    for spec in pes_specs {
        let pes = build_ig_pes(spec)?;
        let (mut pos, mut first) = (0usize, true);
        while pos < pes.len() {
            let idx = pkts.len();
            if extra.contains(&idx) || (pcr_extra_at.is_none() && ats_of(idx as u64, peak_rate) as i64 - last_pcr_ats >= pcr_interval) {
                last_pcr_ats = ats_of(idx as u64, peak_rate) as i64;
                pkts.push(None);
                continue;
            }
            let chunk = &pes[pos..(pos + PAYLOAD).min(pes.len())];
            pos += chunk.len();
            pkts.push(Some(pes_packet(ig_pid, first, cc, chunk)));
            first = false;
            cc = (cc + 1) & 15;
        }
    }
    while pkts.len() % ALIGN_PACKETS != 0 {
        pkts.push(Some(null_packet()));
    }
    let mut out = Vec::with_capacity(pkts.len() * 192);
    for (n, p) in pkts.into_iter().enumerate() {
        let ats = ats_of(n as u64, peak_rate);
        let p = p.unwrap_or_else(|| pcr_packet(ats));
        out.extend_from_slice(&(ats as u32).to_be_bytes());
        out.extend(p);
    }
    Ok(out)
}
