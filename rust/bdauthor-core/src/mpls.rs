//! Playlists .mpls (structure MPLS) — equivalent de `mpls.py`.
//! AppInfo et la queue de chaque PlayItem (masque UO, drapeaux, still, table STN) restent bruts.
use crate::reader::*;

pub const TICKS: u32 = 45000; // horloge des temps de playlist : 45 kHz

#[derive(Debug, Clone, PartialEq)]
pub struct PlayItem {
    pub clip: String,
    pub codec: String,
    pub is_multi_angle: u8,
    pub connection_condition: u8,
    pub stc_id: u8,
    pub in_time: u32,
    pub out_time: u32,
    pub tail: Vec<u8>, // UO mask, flags, still, table STN (brut)
}

impl PlayItem {
    pub fn new(clip: &str, in_time: u32, out_time: u32, tail: Vec<u8>) -> Self {
        PlayItem { clip: clip.into(), codec: "M2TS".into(), is_multi_angle: 0, connection_condition: 1, stc_id: 0, in_time, out_time, tail }
    }
    pub fn parse(r: &mut Reader) -> PResult<Self> {
        let length = r.u16()? as usize;
        let end = r.pos + length;
        let clip = r.ascii(5)?;
        let codec = r.ascii(4)?;
        let flags = r.u16()? as u32;
        let stc_id = r.u8()?;
        let (in_time, out_time) = (r.u32()?, r.u32()?);
        let rest = end.checked_sub(r.pos).ok_or_else(|| ParseError("PlayItem trop court".into()))?;
        let tail = r.raw(rest)?.to_vec();
        Ok(PlayItem { clip, codec, is_multi_angle: bits(flags, 1, 4) as u8, connection_condition: bits(flags, 4, 0) as u8, stc_id, in_time, out_time, tail })
    }
    pub fn pack(&self) -> Vec<u8> {
        let mut b = Vec::new();
        b.text(&self.clip);
        b.text(&self.codec);
        b.p16(((self.is_multi_angle as u16) << 4) | self.connection_condition as u16);
        b.p8(self.stc_id);
        b.p32(self.in_time);
        b.p32(self.out_time);
        b.extend_from_slice(&self.tail);
        let mut out = Vec::new();
        out.p16(b.len() as u16);
        out.extend(b);
        out
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct SubPlayItem {
    pub clip: String,
    pub codec: String,
    pub connection_condition: u8,
    pub is_multi_clip: u8,
    pub stc_id: u8,
    pub in_time: u32,
    pub out_time: u32,
    pub sync_playitem_id: u16,
    pub sync_start_pts: u32,
}

impl SubPlayItem {
    pub fn parse(r: &mut Reader) -> PResult<Self> {
        r.u16()?;
        let clip = r.ascii(5)?;
        let codec = r.ascii(4)?;
        let flags = r.u32()?;
        Ok(SubPlayItem {
            clip, codec, connection_condition: bits(flags, 4, 1) as u8, is_multi_clip: bits(flags, 1, 0) as u8, stc_id: r.u8()?,
            in_time: r.u32()?, out_time: r.u32()?, sync_playitem_id: r.u16()?, sync_start_pts: r.u32()?,
        })
    }
    pub fn pack(&self) -> Vec<u8> {
        let mut b = Vec::new();
        b.text(&self.clip);
        b.text(&self.codec);
        b.p32(((self.connection_condition as u32) << 1) | self.is_multi_clip as u32);
        b.p8(self.stc_id);
        b.p32(self.in_time);
        b.p32(self.out_time);
        b.p16(self.sync_playitem_id);
        b.p32(self.sync_start_pts);
        let mut out = Vec::new();
        out.p16(b.len() as u16);
        out.extend(b);
        out
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct SubPath {
    pub subpath_type: u8, // 3 = menu IG (presentation)
    pub is_repeat: u8,
    pub items: Vec<SubPlayItem>,
}

impl SubPath {
    pub fn parse(r: &mut Reader) -> PResult<Self> {
        r.u32()?;
        r.u8()?;
        let subpath_type = r.u8()?;
        let is_repeat = bits(r.u16()? as u32, 1, 0) as u8;
        r.u8()?;
        let n = r.u8()?;
        let items = (0..n).map(|_| SubPlayItem::parse(r)).collect::<PResult<Vec<_>>>()?;
        Ok(SubPath { subpath_type, is_repeat, items })
    }
    pub fn pack(&self) -> Vec<u8> {
        let mut b = vec![0, self.subpath_type];
        b.p16(self.is_repeat as u16);
        b.p8(0);
        b.p8(self.items.len() as u8);
        for i in &self.items {
            b.extend(i.pack());
        }
        let mut out = Vec::new();
        out.p32(b.len() as u32);
        out.extend(b);
        out
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Mark {
    pub mark_type: u8, // 1 = chapitre (entry mark), 2 = link point
    pub playitem_ref: u16,
    pub time: u32,
    pub es_pid: u16,
    pub duration: u32,
}

impl Mark {
    pub fn chapter(time: u32) -> Self {
        Mark { mark_type: 1, playitem_ref: 0, time, es_pid: 0xFFFF, duration: 0 }
    }
    pub fn parse(r: &mut Reader) -> PResult<Self> {
        r.u8()?;
        Ok(Mark { mark_type: r.u8()?, playitem_ref: r.u16()?, time: r.u32()?, es_pid: r.u16()?, duration: r.u32()? })
    }
    pub fn pack(&self) -> Vec<u8> {
        let mut o = vec![0, self.mark_type];
        o.p16(self.playitem_ref);
        o.p32(self.time);
        o.p16(self.es_pid);
        o.p32(self.duration);
        o
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Mpls {
    pub version: String,
    pub app_info: Vec<u8>,
    pub playitems: Vec<PlayItem>,
    pub subpaths: Vec<SubPath>,
    pub marks: Vec<Mark>,
    pub extension: Vec<u8>,
}

impl Mpls {
    pub fn parse(data: &[u8]) -> PResult<Self> {
        let mut r = Reader::new(data);
        if r.raw(4)? != b"MPLS" {
            return err("signature MPLS absente");
        }
        let version = r.ascii(4)?;
        let (pl_start, mark_start, ext_start) = (r.u32()? as usize, r.u32()? as usize, r.u32()? as usize);
        r.seek(40);
        let n = r.u32()? as usize;
        let app_info = r.raw(n)?.to_vec();
        r.seek(pl_start);
        r.u32()?;
        r.u16()?;
        let (n_items, n_sub) = (r.u16()?, r.u16()?);
        let playitems = (0..n_items).map(|_| PlayItem::parse(&mut r)).collect::<PResult<Vec<_>>>()?;
        let subpaths = (0..n_sub).map(|_| SubPath::parse(&mut r)).collect::<PResult<Vec<_>>>()?;
        r.seek(mark_start);
        r.u32()?;
        let n_marks = r.u16()?;
        let marks = (0..n_marks).map(|_| Mark::parse(&mut r)).collect::<PResult<Vec<_>>>()?;
        let extension = if ext_start != 0 { data.get(ext_start..).unwrap_or(&[]).to_vec() } else { vec![] };
        Ok(Mpls { version, app_info, playitems, subpaths, marks, extension })
    }

    pub fn pack(&self) -> Vec<u8> {
        let mut app = Vec::new();
        app.p32(self.app_info.len() as u32);
        app.extend_from_slice(&self.app_info);
        let mut pl_body = Vec::new();
        pl_body.p16(0);
        pl_body.p16(self.playitems.len() as u16);
        pl_body.p16(self.subpaths.len() as u16);
        for i in &self.playitems { pl_body.extend(i.pack()); }
        for s in &self.subpaths { pl_body.extend(s.pack()); }
        let mut pl = Vec::new();
        pl.p32(pl_body.len() as u32);
        pl.extend(pl_body);
        let mut mk_body = Vec::new();
        mk_body.p16(self.marks.len() as u16);
        for k in &self.marks { mk_body.extend(k.pack()); }
        let mut mk = Vec::new();
        mk.p32(mk_body.len() as u32);
        mk.extend(mk_body);
        let pl_start = 40 + app.len();
        let mark_start = pl_start + pl.len();
        let ext_start = if self.extension.is_empty() { 0 } else { mark_start + mk.len() };
        let mut out = Vec::new();
        out.text("MPLS");
        out.text(&self.version);
        out.p32(pl_start as u32);
        out.p32(mark_start as u32);
        out.p32(ext_start as u32);
        out.zeros(20);
        out.extend(app);
        out.extend(pl);
        out.extend(mk);
        out.extend_from_slice(&self.extension);
        out
    }

    pub fn dump(&self) -> String {
        let s = |t: u32| format!("{:.3}s", t as f64 / TICKS as f64);
        let mut out = vec![format!("version {}", self.version)];
        for (i, p) in self.playitems.iter().enumerate() {
            out.push(format!("PlayItem {i}: clip {}.{}  in={} out={} duree={} connexion={} ({} octets bruts : UO/STN)",
                             p.clip, p.codec, s(p.in_time), s(p.out_time), s(p.out_time.wrapping_sub(p.in_time)), p.connection_condition, p.tail.len()));
        }
        for (i, sp) in self.subpaths.iter().enumerate() {
            let kind = match sp.subpath_type { 2 => "audio", 3 => "menu IG (presentation)", _ => "?" };
            out.push(format!("SubPath {i}: type {} = {kind}, repeat={}", sp.subpath_type, sp.is_repeat));
            for it in &sp.items {
                out.push(format!("  SubPlayItem: clip {}.{} in={} out={} duree={}", it.clip, it.codec, s(it.in_time), s(it.out_time),
                                 s(it.out_time.wrapping_sub(it.in_time))));
            }
        }
        for (i, k) in self.marks.iter().enumerate() {
            let kind = match k.mark_type { 1 => "chapitre (entry mark)".to_string(), 2 => "link point".to_string(), t => t.to_string() };
            out.push(format!("Mark {i}: {kind} playitem={} t={}", k.playitem_ref, s(k.time)));
        }
        out.join("\n")
    }
}
