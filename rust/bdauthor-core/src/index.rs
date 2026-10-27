//! index.bdmv (structure INDX) — equivalent de `index.py`.
use crate::reader::*;

pub const OBJ_HDMV: u8 = 1;
pub const OBJ_BDJ: u8 = 2;

#[derive(Debug, Clone, PartialEq, Default)]
pub struct IndexObject {
    pub object_type: u8,     // 1 = HDMV, 2 = BD-J
    pub access_type: u8,     // titres uniquement (2 bits)
    pub playback_type: u8,   // 2 bits
    pub id_ref: u16,         // numero d'objet dans MovieObject.bdmv
    pub bdj_tail: Vec<u8>,   // BD-J : 8 octets bruts
}

impl IndexObject {
    pub fn hdmv(access_type: u8, playback_type: u8, id_ref: u16) -> Self {
        IndexObject { object_type: OBJ_HDMV, access_type, playback_type, id_ref, bdj_tail: vec![] }
    }
    pub fn parse(r: &mut Reader) -> PResult<Self> {
        let head = r.u32()?;
        let mut o = IndexObject { object_type: bits(head, 2, 30) as u8, access_type: bits(head, 2, 28) as u8, ..Default::default() };
        if o.object_type == OBJ_HDMV {
            o.playback_type = bits(r.u16()? as u32, 2, 14) as u8;
            o.id_ref = r.u16()?;
            r.raw(4)?;
        } else {
            o.bdj_tail = r.raw(8)?.to_vec();
        }
        Ok(o)
    }
    pub fn pack(&self) -> Vec<u8> {
        let mut out = Vec::new();
        out.p32(((self.object_type as u32) << 30) | ((self.access_type as u32) << 28));
        if self.object_type == OBJ_HDMV {
            out.p16((self.playback_type as u16) << 14);
            out.p16(self.id_ref);
            out.zeros(4);
        } else {
            out.extend_from_slice(&self.bdj_tail);
        }
        out
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct IndexBdmv {
    pub version: String,
    pub app_flags: u8,
    pub video_format: u8, // 4 bits
    pub frame_rate: u8,   // 4 bits
    pub user_data: Vec<u8>,
    pub first_playback: IndexObject,
    pub top_menu: IndexObject,
    pub titles: Vec<IndexObject>,
}

impl Default for IndexBdmv {
    fn default() -> Self {
        IndexBdmv {
            version: "0200".into(), app_flags: 0, video_format: 0, frame_rate: 0, user_data: vec![0; 32],
            first_playback: IndexObject::hdmv(0, 0, 0), top_menu: IndexObject::hdmv(0, 0, 0), titles: vec![],
        }
    }
}

impl IndexBdmv {
    pub fn parse(data: &[u8]) -> PResult<Self> {
        let mut r = Reader::new(data);
        if r.raw(4)? != b"INDX" {
            return err("signature INDX absente");
        }
        let version = r.ascii(4)?;
        let index_start = r.u32()? as usize;
        r.u32()?; // adresse des extensions (ignorees)
        r.seek(40);
        r.u32()?; // longueur AppInfo
        let app_flags = r.u8()?;
        let fmt = r.u8()? as u32;
        let user_data = r.raw(32)?.to_vec();
        r.seek(index_start);
        r.u32()?; // longueur de la table
        let first_playback = IndexObject::parse(&mut r)?;
        let top_menu = IndexObject::parse(&mut r)?;
        let n = r.u16()?;
        let titles = (0..n).map(|_| IndexObject::parse(&mut r)).collect::<PResult<Vec<_>>>()?;
        Ok(IndexBdmv { version, app_flags, video_format: bits(fmt, 4, 4) as u8, frame_rate: bits(fmt, 4, 0) as u8, user_data,
                       first_playback, top_menu, titles })
    }

    pub fn pack(&self) -> Vec<u8> {
        let mut app = vec![self.app_flags, (self.video_format << 4) | self.frame_rate];
        app.extend_from_slice(&self.user_data);
        let mut table = self.first_playback.pack();
        table.extend(self.top_menu.pack());
        table.p16(self.titles.len() as u16);
        for t in &self.titles {
            table.extend(t.pack());
        }
        let index_start = 40 + 4 + app.len();
        let mut out = Vec::new();
        out.text("INDX");
        out.text(&self.version);
        out.p32(index_start as u32);
        out.p32(0);
        out.zeros(24);
        out.p32(app.len() as u32);
        out.extend(app);
        out.p32(table.len() as u32);
        out.extend(table);
        out
    }

    pub fn dump(&self) -> String {
        let d = |o: &IndexObject| {
            if o.object_type == OBJ_HDMV {
                format!("HDMV playback_type={} object={}", o.playback_type, o.id_ref)
            } else {
                format!("BD-J {:?}", &o.bdj_tail[..o.bdj_tail.len().min(5)])
            }
        };
        let ud: Vec<u8> = {
            let mut v = self.user_data.clone();
            while v.last() == Some(&0) { v.pop(); }
            v
        };
        let mut lines = vec![
            format!("version        : {}", self.version),
            format!("video_format   : {}  frame_rate: {}", self.video_format, self.frame_rate),
            format!("user_data      : {}", String::from_utf8_lossy(&ud)),
            format!("First Playback : {}", d(&self.first_playback)),
            format!("Top Menu       : {}", d(&self.top_menu)),
            format!("titres         : {}", self.titles.len()),
        ];
        for (i, t) in self.titles.iter().enumerate() {
            lines.push(format!("  titre {}: {} access_type={}", i + 1, d(t), t.access_type));
        }
        lines.join("\n")
    }
}
