//! Fichiers .clpi (informations de clip, signature HDMV) — equivalent de `clpi.py`.
//! Sections : ClipInfo, SequenceInfo, ProgramInfo, CPI (EP_map), ClipMark. Attributs de flux et ClipMark restent bruts.
use crate::reader::*;

#[derive(Debug, Clone, PartialEq)]
pub struct ClipInfo {
    pub clip_stream_type: u8,
    pub application_type: u8,
    pub is_cc5: u8,
    pub ts_recording_rate: u32,  // octets / seconde
    pub num_source_packets: u32, // paquets de 192 octets
    pub ts_validity: u8,
    pub ts_format_id: String,
}

impl Default for ClipInfo {
    fn default() -> Self {
        ClipInfo { clip_stream_type: 1, application_type: 1, is_cc5: 0, ts_recording_rate: 0, num_source_packets: 0, ts_validity: 0x80, ts_format_id: "HDMV".into() }
    }
}

impl ClipInfo {
    pub fn parse(r: &mut Reader) -> PResult<Self> {
        r.u32()?;
        r.u16()?;
        let (clip_stream_type, application_type) = (r.u8()?, r.u8()?);
        let is_cc5 = bits(r.u32()?, 1, 0) as u8;
        let (ts_recording_rate, num_source_packets) = (r.u32()?, r.u32()?);
        r.raw(128)?;
        r.u16()?;
        let ts_validity = r.u8()?;
        let ts_format_id = r.ascii(4)?;
        r.raw(25)?;
        Ok(ClipInfo { clip_stream_type, application_type, is_cc5, ts_recording_rate, num_source_packets, ts_validity, ts_format_id })
    }
    pub fn pack(&self) -> Vec<u8> {
        let mut ts = vec![self.ts_validity];
        ts.text(&self.ts_format_id);
        ts.zeros(25);
        let mut b = Vec::new();
        b.p16(0);
        b.p8(self.clip_stream_type);
        b.p8(self.application_type);
        b.p32(self.is_cc5 as u32);
        b.p32(self.ts_recording_rate);
        b.p32(self.num_source_packets);
        b.zeros(128);
        b.p16(ts.len() as u16);
        b.extend(ts);
        let mut out = Vec::new();
        out.p32(b.len() as u32);
        out.extend(b);
        out
    }
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct StcSeq {
    pub pcr_pid: u16,
    pub spn_stc_start: u32,
    pub start_time: u32, // horloge 45 kHz
    pub end_time: u32,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct AtcSeq {
    pub spn_atc_start: u32,
    pub offset_stc_id: u8,
    pub stc: Vec<StcSeq>,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct SequenceInfo {
    pub atc: Vec<AtcSeq>,
}

impl SequenceInfo {
    pub fn parse(r: &mut Reader) -> PResult<Self> {
        r.u32()?;
        r.u8()?;
        let n_atc = r.u8()?;
        let mut atc = Vec::new();
        for _ in 0..n_atc {
            let spn_atc_start = r.u32()?;
            let n = r.u8()?;
            let offset_stc_id = r.u8()?;
            let stc = (0..n)
                .map(|_| Ok(StcSeq { pcr_pid: r.u16()?, spn_stc_start: r.u32()?, start_time: r.u32()?, end_time: r.u32()? }))
                .collect::<PResult<Vec<_>>>()?;
            atc.push(AtcSeq { spn_atc_start, offset_stc_id, stc });
        }
        Ok(SequenceInfo { atc })
    }
    pub fn pack(&self) -> Vec<u8> {
        let mut b = vec![0, self.atc.len() as u8];
        for a in &self.atc {
            b.p32(a.spn_atc_start);
            b.p8(a.stc.len() as u8);
            b.p8(a.offset_stc_id);
            for t in &a.stc {
                b.p16(t.pcr_pid);
                b.p32(t.spn_stc_start);
                b.p32(t.start_time);
                b.p32(t.end_time);
            }
        }
        let mut out = Vec::new();
        out.p32(b.len() as u32);
        out.extend(b);
        out
    }
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct Stream {
    pub pid: u16,
    pub coding_type: u8,
    pub attrs: Vec<u8>, // attributs bruts (apres coding_type)
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct Program {
    pub spn_start: u32,
    pub pmt_pid: u16,
    pub num_groups: u8,
    pub streams: Vec<Stream>,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct ProgramInfo {
    pub programs: Vec<Program>,
}

impl ProgramInfo {
    pub fn parse(r: &mut Reader) -> PResult<Self> {
        r.u32()?;
        r.u8()?;
        let np = r.u8()?;
        let mut programs = Vec::new();
        for _ in 0..np {
            let (spn_start, pmt_pid) = (r.u32()?, r.u16()?);
            let n = r.u8()?;
            let num_groups = r.u8()?;
            let mut streams = Vec::new();
            for _ in 0..n {
                let (pid, ln) = (r.u16()?, r.u8()? as usize);
                let coding_type = r.u8()?;
                let attrs = r.raw(ln.checked_sub(1).ok_or_else(|| ParseError("longueur de flux nulle".into()))?)?.to_vec();
                streams.push(Stream { pid, coding_type, attrs });
            }
            programs.push(Program { spn_start, pmt_pid, num_groups, streams });
        }
        Ok(ProgramInfo { programs })
    }
    pub fn pack(&self) -> Vec<u8> {
        let mut b = vec![0, self.programs.len() as u8];
        for p in &self.programs {
            b.p32(p.spn_start);
            b.p16(p.pmt_pid);
            b.p8(p.streams.len() as u8);
            b.p8(p.num_groups);
            for s in &p.streams {
                b.p16(s.pid);
                b.p8(s.attrs.len() as u8 + 1);
                b.p8(s.coding_type);
                b.extend_from_slice(&s.attrs);
            }
        }
        let mut out = Vec::new();
        out.p32(b.len() as u32);
        out.extend(b);
        out
    }
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct EpCoarse {
    pub ref_fine: u32, // 18 bits : index de l'entree fine associee
    pub pts: u32,      // 14 bits : PTS[32:19]
    pub spn: u32,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct EpFine {
    pub angle_change: u32, // 1 bit
    pub end_offset: u32,   // 3 bits
    pub pts: u32,          // 11 bits : PTS[19:9]
    pub spn: u32,          // 17 bits : poids faibles du SPN
}

#[derive(Debug, Clone, PartialEq)]
pub struct EpStream {
    pub pid: u16,
    pub ep_stream_type: u8,
    pub coarse: Vec<EpCoarse>,
    pub fine: Vec<EpFine>,
}

impl EpStream {
    /// (pts 90 kHz, spn complet) de chaque point d'acces.
    pub fn entries(&self) -> Vec<(u64, u32)> {
        let mut out = Vec::new();
        let mut ci = 0usize;
        for (i, f) in self.fine.iter().enumerate() {
            while ci + 1 < self.coarse.len() && self.coarse[ci + 1].ref_fine as usize <= i {
                ci += 1;
            }
            let c = &self.coarse[ci];
            out.push((((c.pts as u64) << 19) | ((f.pts as u64) << 9), (c.spn & !0x1FFFF) | f.spn));
        }
        out
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Cpi {
    pub cpi_type: u8,
    pub streams: Vec<EpStream>,
}

impl Cpi {
    /// `None` si la section est vide (clip sans EP_map, ex. menu IG).
    pub fn parse(r: &mut Reader) -> PResult<Option<Self>> {
        if r.u32()? == 0 {
            return Ok(None);
        }
        let cpi_type = bits(r.u16()? as u32, 4, 0) as u8;
        let base = r.pos; // debut de l'EP_map (apres le type)
        r.u8()?;
        let n = r.u8()?;
        let mut streams = Vec::new();
        r.seek(base + 2);
        for _ in 0..n {
            let pid = r.u16()?;
            let mut v: u128 = 0;
            for &b in r.raw(10)? {
                v = (v << 8) | b as u128;
            }
            let addr = (v & 0xFFFF_FFFF) as usize;
            let nfine = ((v >> 32) & 0x3FFFF) as usize;
            let ncoarse = ((v >> 50) & 0xFFFF) as usize;
            let ep_stream_type = ((v >> 66) & 0xF) as u8;
            let save = r.pos;
            r.seek(base + addr);
            let start = r.pos;
            let fine_off = r.u32()? as usize;
            let mut coarse = Vec::new();
            for _ in 0..ncoarse {
                let w = r.u32()?;
                coarse.push(EpCoarse { ref_fine: bits(w, 18, 14), pts: bits(w, 14, 0), spn: r.u32()? });
            }
            r.seek(start + fine_off);
            let mut fine = Vec::new();
            for _ in 0..nfine {
                let w = r.u32()?;
                fine.push(EpFine { angle_change: bits(w, 1, 31), end_offset: bits(w, 3, 28), pts: bits(w, 11, 17), spn: bits(w, 17, 0) });
            }
            streams.push(EpStream { pid, ep_stream_type, coarse, fine });
            r.seek(save);
        }
        Ok(Some(Cpi { cpi_type, streams }))
    }

    pub fn pack(&self) -> Vec<u8> {
        let n = self.streams.len();
        let mut heads = Vec::new();
        let mut blocks: Vec<Vec<u8>> = Vec::new();
        let mut addr = 2 + 12 * n;
        for s in &self.streams {
            let mut blk = Vec::new();
            blk.p32(4 + 8 * s.coarse.len() as u32);
            for c in &s.coarse {
                blk.p32((c.ref_fine << 14) | c.pts);
                blk.p32(c.spn);
            }
            for f in &s.fine {
                blk.p32((f.angle_change << 31) | (f.end_offset << 28) | (f.pts << 17) | f.spn);
            }
            let v: u128 = ((s.ep_stream_type as u128) << 66) | ((s.coarse.len() as u128) << 50) | ((s.fine.len() as u128) << 32) | addr as u128;
            heads.p16(s.pid);
            heads.extend_from_slice(&v.to_be_bytes()[6..]);
            addr += blk.len();
            blocks.push(blk);
        }
        let mut body = Vec::new();
        body.p16(self.cpi_type as u16);
        body.extend_from_slice(&[0, n as u8]);
        body.extend(heads);
        for b in blocks {
            body.extend(b);
        }
        let mut out = Vec::new();
        out.p32(body.len() as u32);
        out.extend(body);
        out
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Clpi {
    pub version: String,
    pub clip_info: ClipInfo,
    pub sequence: SequenceInfo,
    pub program: ProgramInfo,
    pub cpi: Option<Cpi>,
    pub clip_mark: Vec<u8>, // section brute, longueur incluse
    pub extension: Vec<u8>,
}

impl Clpi {
    pub fn parse(data: &[u8]) -> PResult<Self> {
        let mut r = Reader::new(data);
        if r.raw(4)? != b"HDMV" {
            return err("signature HDMV absente");
        }
        let version = r.ascii(4)?;
        let (seq, prog, cpi, mark, ext) = (r.u32()? as usize, r.u32()? as usize, r.u32()? as usize, r.u32()? as usize, r.u32()? as usize);
        r.seek(40);
        let clip_info = ClipInfo::parse(&mut r)?;
        r.seek(seq);
        let sequence = SequenceInfo::parse(&mut r)?;
        r.seek(prog);
        let program = ProgramInfo::parse(&mut r)?;
        r.seek(cpi);
        let cpi = Cpi::parse(&mut r)?;
        r.seek(mark);
        let n = r.u32()? as usize;
        let clip_mark = data.get(mark..mark + 4 + n).ok_or_else(|| ParseError("ClipMark hors du fichier".into()))?.to_vec();
        let extension = if ext != 0 { data.get(ext..).unwrap_or(&[]).to_vec() } else { vec![] };
        Ok(Clpi { version, clip_info, sequence, program, cpi, clip_mark, extension })
    }

    pub fn pack(&self) -> Vec<u8> {
        let cpi = self.cpi.as_ref().map(|c| c.pack()).unwrap_or_else(|| vec![0; 4]);
        let secs = [self.clip_info.pack(), self.sequence.pack(), self.program.pack(), cpi, self.clip_mark.clone()];
        let mut pos = vec![40usize];
        for s in &secs {
            pos.push(pos.last().unwrap() + s.len());
        }
        let ext = if self.extension.is_empty() { 0 } else { *pos.last().unwrap() };
        let mut out = Vec::new();
        out.text("HDMV");
        out.text(&self.version);
        for k in 1..=4 {
            out.p32(pos[k] as u32);
        }
        out.p32(ext as u32);
        out.zeros(12);
        for s in secs {
            out.extend(s);
        }
        out.extend_from_slice(&self.extension);
        out
    }

    pub fn dump(&self) -> String {
        let ci = &self.clip_info;
        let app = match ci.application_type {
            1 => "film (main TS)", 2 => "diaporama", 3 => "diaporama navigable", 4 => "sous-TS diaporama",
            5 => "sous-TS menu interactif", 6 => "sous-TS texte", 7 => "sous-chemin (ES)", _ => "?",
        };
        let mut out = vec![
            format!("version {}", self.version),
            format!("ClipInfo : type d'application {} = {app}, {} paquets ({} octets), debit {} o/s", ci.application_type,
                    ci.num_source_packets, ci.num_source_packets as u64 * 192, ci.ts_recording_rate),
        ];
        for a in &self.sequence.atc {
            for t in &a.stc {
                out.push(format!("Sequence : PCR PID 0x{:04x}, {:.3}s -> {:.3}s", t.pcr_pid, t.start_time as f64 / 45000.0, t.end_time as f64 / 45000.0));
            }
        }
        for p in &self.program.programs {
            out.push(format!("Programme : PMT PID 0x{:04x}", p.pmt_pid));
            for s in &p.streams {
                let mut extra = String::new();
                if matches!(s.coding_type, 0x01 | 0x02 | 0x1B | 0x24 | 0xEA) && !s.attrs.is_empty() {
                    let f = match s.attrs[0] >> 4 { 1 => "480i", 2 => "576i", 3 => "480p", 4 => "1080i", 5 => "720p", 6 => "1080p", 7 => "576p", _ => "?" };
                    let r = match s.attrs[0] & 15 { 1 => "23.976", 2 => "24", 3 => "25", 4 => "29.97", 6 => "50", 7 => "59.94", _ => "?" };
                    extra = format!(" {f} {r} i/s");
                } else if matches!(s.coding_type, 0x80..=0x86) && s.attrs.len() >= 4 {
                    extra = format!(" langue {}", String::from_utf8_lossy(&s.attrs[1..4]));
                } else if matches!(s.coding_type, 0x90 | 0x91) && s.attrs.len() >= 3 {
                    extra = format!(" langue {}", String::from_utf8_lossy(&s.attrs[0..3]));
                }
                let name = match s.coding_type {
                    0x01 => "MPEG-1 video".to_string(), 0x02 => "MPEG-2 video".into(), 0x1B => "H.264".into(), 0x24 => "HEVC".into(),
                    0xEA => "VC-1".into(), 0x03 => "MPEG-1 audio".into(), 0x04 => "MPEG-2 audio".into(), 0x80 => "LPCM".into(),
                    0x81 => "AC3".into(), 0x82 => "DTS".into(), 0x83 => "TrueHD".into(), 0x84 => "E-AC3".into(), 0x85 => "DTS-HD".into(),
                    0x86 => "DTS-HD MA".into(), 0x90 => "PG (sous-titres)".into(), 0x91 => "IG (menu)".into(), 0x92 => "Text sous-titres".into(),
                    t => format!("0x{t:x}"),
                };
                out.push(format!("  flux PID 0x{:04x} : {name}{extra}", s.pid));
            }
        }
        match &self.cpi {
            None => out.push("EP_map : aucune".into()),
            Some(c) => {
                for s in &c.streams {
                    let e = s.entries();
                    out.push(format!("EP_map PID 0x{:04x} : {} entrees coarse, {} fine", s.pid, s.coarse.len(), e.len()));
                    for (pts, spn) in e.iter().take(4) {
                        out.push(format!("  t={:.3}s  SPN={spn}  (octet {})", *pts as f64 / 90000.0, *spn as u64 * 192));
                    }
                    if e.len() > 4 {
                        let l = e.last().unwrap();
                        out.push(format!("  ... derniere : t={:.3}s  SPN={}", l.0 as f64 / 90000.0, l.1));
                    }
                }
            }
        }
        out.join("\n")
    }
}
