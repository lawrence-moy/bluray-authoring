//! Lecture de fichiers .m2ts : paquets source de 192 octets, PES, segments IG — equivalent de `m2ts.py`.
//! Un paquet source = 4 octets (2 bits copy_permission + 30 bits ATS) + 188 octets TS.
use crate::reader::*;

pub const SRC: usize = 192;

#[derive(Debug, Clone)]
pub struct TsPacket<'a> {
    pub index: usize,
    pub copy_permission: u8,
    pub ats: u32, // horodatage d'arrivee (27 MHz, 30 bits)
    pub pid: u16,
    pub pusi: bool, // payload_unit_start_indicator
    pub afc: u8,    // adaptation_field_control (1 charge, 2 adaptation, 3 les deux)
    pub cc: u8,     // continuity counter
    pub pcr: Option<u64>, // ticks 27 MHz
    pub payload: &'a [u8],
}

pub struct Packets<'a> {
    data: &'a [u8],
    n: usize,
}

/// Parcours paresseux des paquets d'un fichier .m2ts (erreur si la taille n'est pas un multiple de 192 ou si la synchro manque).
pub fn iter_packets(data: &[u8]) -> PResult<Packets<'_>> {
    if data.len() % SRC != 0 {
        return err("taille non multiple de 192");
    }
    Ok(Packets { data, n: 0 })
}

impl<'a> Iterator for Packets<'a> {
    type Item = PResult<TsPacket<'a>>;
    fn next(&mut self) -> Option<Self::Item> {
        let n = self.n;
        if n * SRC >= self.data.len() {
            return None;
        }
        self.n += 1;
        let p = &self.data[n * SRC..(n + 1) * SRC];
        let head = u32::from_be_bytes(p[..4].try_into().unwrap());
        let ts = &p[4..];
        if ts[0] != 0x47 {
            return Some(err(format!("octet de synchro absent au paquet {n}")));
        }
        let afc = (ts[3] >> 4) & 3;
        let (mut pos, mut pcr) = (4usize, None);
        if afc == 2 || afc == 3 {
            let alen = ts[4] as usize;
            if alen >= 7 && ts[5] & 0x10 != 0 {
                let v = ts[6..12].iter().fold(0u64, |a, &b| (a << 8) | b as u64);
                pcr = Some((v >> 15) * 300 + (v & 0x1FF));
            }
            pos = 5 + alen;
        }
        let payload = if afc == 1 || afc == 3 { ts.get(pos..).unwrap_or(&[]) } else { &[][..] };
        Some(Ok(TsPacket {
            index: n, copy_permission: (head >> 30) as u8, ats: head & 0x3FFF_FFFF, pid: (((ts[1] & 0x1F) as u16) << 8) | ts[2] as u16,
            pusi: (ts[1] >> 6) & 1 != 0, afc, cc: ts[3] & 15, pcr, payload,
        }))
    }
}

#[derive(Debug, Clone)]
pub struct Pes {
    pub pid: u16,
    pub first_packet: usize,
    pub stream_id: u8,
    pub pts: Option<u64>,
    pub dts: Option<u64>,
    pub data: Vec<u8>, // charge utile apres l'en-tete PES
    pub declared_length: usize,
    pub header: Vec<u8>, // en-tete PES brut
}

fn ts33(b: &[u8]) -> u64 {
    (((b[0] as u64 >> 1) & 7) << 30) | ((b[1] as u64) << 22) | ((b[2] as u64 >> 1) << 15) | ((b[3] as u64) << 7) | (b[4] as u64 >> 1)
}

pub fn parse_pes(pid: u16, first: usize, raw: &[u8]) -> PResult<Pes> {
    if raw.len() < 9 || raw[..3] != [0, 0, 1] {
        return err("start code PES absent");
    }
    let (stream_id, plen) = (raw[3], u16::from_be_bytes([raw[4], raw[5]]) as usize);
    let (flags, hlen) = (raw[7], raw[8] as usize);
    let (mut pts, mut dts) = (None, None);
    if flags & 0x80 != 0 {
        pts = Some(ts33(raw.get(9..14).ok_or_else(|| ParseError("PES tronque".into()))?));
    }
    if flags & 0x40 != 0 {
        dts = Some(ts33(raw.get(14..19).ok_or_else(|| ParseError("PES tronque".into()))?));
    }
    let end = 9 + hlen;
    if end > raw.len() {
        return err("en-tete PES tronque");
    }
    Ok(Pes { pid, first_packet: first, stream_id, pts, dts, data: raw[end..].to_vec(), declared_length: plen, header: raw[..end].to_vec() })
}

/// Liste des PES complets d'un PID.
pub fn reassemble(data: &[u8], pid: u16) -> PResult<Vec<Pes>> {
    let mut out = Vec::new();
    let mut buf: Option<Vec<u8>> = None;
    let mut first = 0usize;
    for p in iter_packets(data)? {
        let p = p?;
        if p.pid != pid || p.payload.is_empty() {
            continue;
        }
        if p.pusi {
            if let Some(b) = buf.take() {
                out.push(parse_pes(pid, first, &b)?);
            }
            buf = Some(p.payload.to_vec());
            first = p.index;
        } else if let Some(b) = buf.as_mut() {
            b.extend_from_slice(p.payload);
        }
    }
    if let Some(b) = buf {
        out.push(parse_pes(pid, first, &b)?);
    }
    Ok(out)
}

#[derive(Debug, Clone, PartialEq)]
pub struct Segment {
    pub seg_type: u8,
    pub payload: Vec<u8>,
}

impl Segment {
    pub fn name(&self) -> String {
        match self.seg_type {
            0x14 => "PDS".into(), 0x15 => "ODS".into(), 0x16 => "PCS".into(), 0x17 => "WDS".into(), 0x18 => "ICS".into(), 0x80 => "END".into(),
            t => format!("0x{t:x}"),
        }
    }
    pub fn pack(&self) -> Vec<u8> {
        let mut o = vec![self.seg_type];
        o.extend_from_slice(&(self.payload.len() as u16).to_be_bytes());
        o.extend_from_slice(&self.payload);
        o
    }
}

pub fn split_segments(data: &[u8]) -> PResult<Vec<Segment>> {
    let (mut segs, mut pos) = (Vec::new(), 0usize);
    while pos < data.len() {
        if pos + 3 > data.len() {
            return err(format!("segment tronque a l'offset {pos}"));
        }
        let (t, n) = (data[pos], u16::from_be_bytes([data[pos + 1], data[pos + 2]]) as usize);
        if pos + 3 + n > data.len() {
            return err(format!("segment tronque a l'offset {pos}"));
        }
        segs.push(Segment { seg_type: t, payload: data[pos + 3..pos + 3 + n].to_vec() });
        pos += 3 + n;
    }
    Ok(segs)
}
