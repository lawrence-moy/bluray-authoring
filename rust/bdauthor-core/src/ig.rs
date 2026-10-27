//! Segments du flux IG (Interactive Graphics) : ICS, PDS, ODS, et compression RLE (equivalent de `ig.py`).
use crate::mobj::Command;
use crate::reader::{bits, err, PResult, Put, Reader};
use std::collections::BTreeMap;

pub const SEG_PDS: u8 = 0x14;
pub const SEG_ODS: u8 = 0x15;
pub const SEG_ICS: u8 = 0x18;
pub const SEG_END: u8 = 0x80;

// ------------------------------------------------------------------ PDS
/// Entree de palette : (Y, Cr, Cb, alpha).
pub type PalEntry = (u8, u8, u8, u8);

#[derive(Debug, Clone, PartialEq, Default)]
pub struct Pds {
    pub palette_id: u8,
    pub version: u8,
    pub entries: BTreeMap<u8, PalEntry>,
    /// Ordre d'apparition des indices (conserve pour un aller-retour exact).
    pub order: Vec<u8>,
}

impl Pds {
    pub fn new(palette_id: u8, version: u8) -> Self {
        Pds { palette_id, version, ..Default::default() }
    }
    pub fn set(&mut self, index: u8, e: PalEntry) {
        if self.entries.insert(index, e).is_none() {
            self.order.push(index);
        }
    }
    pub fn parse(b: &[u8]) -> PResult<Self> {
        if b.len() < 2 || (b.len() - 2) % 5 != 0 {
            return err("PDS : longueur invalide");
        }
        let mut p = Pds::new(b[0], b[1]);
        for c in b[2..].chunks(5) {
            p.set(c[0], (c[1], c[2], c[3], c[4]));
        }
        Ok(p)
    }
    pub fn pack(&self) -> Vec<u8> {
        let mut o = vec![self.palette_id, self.version];
        for i in &self.order {
            let (y, cr, cb, a) = self.entries[i];
            o.extend_from_slice(&[*i, y, cr, cb, a]);
        }
        o
    }
    /// Conversion YCrCb (BT.709, plage limitee) -> RGBA.
    pub fn rgba(&self, index: u8) -> [u8; 4] {
        let (y, cr, cb, a) = self.entries.get(&index).copied().unwrap_or((16, 128, 128, 0));
        let c = |v: f64| py_round(v).clamp(0.0, 255.0) as u8;
        let yy = 1.164 * (y as f64 - 16.0);
        let (cr, cb) = (cr as f64 - 128.0, cb as f64 - 128.0);
        [c(yy + 1.793 * cr), c(yy - 0.213 * cb - 0.533 * cr), c(yy + 2.112 * cb), a]
    }
}

/// `round()` de Python 3 : arrondi au pair le plus proche sur les .5.
pub fn py_round(v: f64) -> f64 {
    let r = v.round();
    if (v - v.trunc()).abs() == 0.5 && r % 2.0 != 0.0 { r - v.signum() } else { r }
}

// ------------------------------------------------------------------ RLE
pub fn rle_decode(data: &[u8], width: usize, height: usize) -> PResult<Vec<Vec<u8>>> {
    let (mut rows, mut row, mut pos) = (Vec::new(), Vec::new(), 0usize);
    let next = |pos: &mut usize| -> PResult<u8> {
        let b = *data.get(*pos).ok_or_else(|| crate::reader::ParseError("RLE tronque".into()))?;
        *pos += 1;
        Ok(b)
    };
    while pos < data.len() {
        let b = next(&mut pos)?;
        if b != 0 {
            row.push(b);
            continue;
        }
        let b = next(&mut pos)?;
        if b == 0 {
            rows.push(std::mem::take(&mut row));
            continue;
        }
        let (kind, mut n) = (b & 0xC0, (b & 0x3F) as usize);
        if kind == 0x40 || kind == 0xC0 {
            n = (n << 8) | next(&mut pos)? as usize;
        }
        let color = if kind == 0x80 || kind == 0xC0 { next(&mut pos)? } else { 0 };
        row.extend(std::iter::repeat(color).take(n));
    }
    if !row.is_empty() {
        rows.push(row);
    }
    if rows.len() != height || rows.iter().any(|r| r.len() != width) {
        return err("RLE incoherent avec la taille declaree");
    }
    Ok(rows)
}

pub fn rle_encode<R: AsRef<[u8]>>(rows: &[R]) -> Vec<u8> {
    let mut out = Vec::new();
    for row in rows {
        let row = row.as_ref();
        let mut i = 0;
        while i < row.len() {
            let (c, mut n) = (row[i], 1usize);
            while i + n < row.len() && row[i + n] == c && n < 0x3FFF {
                n += 1;
            }
            i += n;
            if c != 0 && n <= 2 {
                out.extend(std::iter::repeat(c).take(n));
            } else if c == 0 && n < 64 {
                out.extend_from_slice(&[0, n as u8]);
            } else if c == 0 {
                out.extend_from_slice(&[0, 0x40 | (n >> 8) as u8, (n & 0xFF) as u8]);
            } else if n < 64 {
                out.extend_from_slice(&[0, 0x80 | n as u8, c]);
            } else {
                out.extend_from_slice(&[0, 0xC0 | (n >> 8) as u8, (n & 0xFF) as u8, c]);
            }
        }
        out.extend_from_slice(&[0, 0]);
    }
    out
}

// ------------------------------------------------------------------ ODS
#[derive(Debug, Clone, PartialEq)]
pub struct Ods {
    pub object_id: u16,
    pub version: u8,
    pub sequence_flag: u8, // 0x80 premier, 0x40 dernier
    pub width: u16,
    pub height: u16,
    pub rle: Vec<u8>,
}

impl Default for Ods {
    fn default() -> Self {
        Ods { object_id: 0, version: 0, sequence_flag: 0xC0, width: 0, height: 0, rle: Vec::new() }
    }
}

impl Ods {
    pub fn parse(b: &[u8]) -> PResult<Self> {
        let mut r = Reader::new(b);
        let (object_id, version, sequence_flag) = (r.u16()?, r.u8()?, r.u8()?);
        if sequence_flag & 0x80 == 0 {
            return err("ODS fragmente : non gere");
        }
        let d = r.raw(3)?;
        let dlen = ((d[0] as usize) << 16) | ((d[1] as usize) << 8) | d[2] as usize;
        let (width, height) = (r.u16()?, r.u16()?);
        let rle = b[r.pos..].to_vec();
        if dlen != rle.len() + 4 {
            return err("data_length incoherent");
        }
        Ok(Ods { object_id, version, sequence_flag, width, height, rle })
    }
    pub fn pack(&self) -> Vec<u8> {
        let mut o = Vec::with_capacity(self.rle.len() + 11);
        o.p16(self.object_id);
        o.p8(self.version);
        o.p8(self.sequence_flag);
        let l = self.rle.len() + 4;
        o.extend_from_slice(&[(l >> 16) as u8, (l >> 8) as u8, l as u8]);
        o.p16(self.width);
        o.p16(self.height);
        o.extend_from_slice(&self.rle);
        o
    }
    pub fn pixels(&self) -> PResult<Vec<Vec<u8>>> {
        rle_decode(&self.rle, self.width as usize, self.height as usize)
    }
}

// ------------------------------------------------------------------ ICS
#[derive(Debug, Clone, PartialEq)]
pub struct Window { pub window_id: u8, pub x: u16, pub y: u16, pub w: u16, pub h: u16 }

#[derive(Debug, Clone, PartialEq)]
pub struct CompObject {
    pub object_id_ref: u16,
    pub window_id_ref: u8,
    pub flags: u8,
    pub x: u16,
    pub y: u16,
    pub crop: Option<[u16; 4]>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Effect { pub duration: u32, pub palette_id_ref: u8, pub objects: Vec<CompObject> }

#[derive(Debug, Clone, PartialEq, Default)]
pub struct EffectSequence { pub windows: Vec<Window>, pub effects: Vec<Effect> }

fn u24(r: &mut Reader) -> PResult<u32> {
    let d = r.raw(3)?;
    Ok(((d[0] as u32) << 16) | ((d[1] as u32) << 8) | d[2] as u32)
}
fn p24(o: &mut Vec<u8>, v: u32) {
    o.extend_from_slice(&[(v >> 16) as u8, (v >> 8) as u8, v as u8]);
}
fn u40(r: &mut Reader) -> PResult<u64> {
    Ok(r.raw(5)?.iter().fold(0u64, |a, &b| (a << 8) | b as u64))
}
fn p40(o: &mut Vec<u8>, v: u64) {
    o.extend_from_slice(&v.to_be_bytes()[3..]);
}

impl EffectSequence {
    pub fn parse(r: &mut Reader) -> PResult<Self> {
        let mut s = EffectSequence::default();
        for _ in 0..r.u8()? {
            s.windows.push(Window { window_id: r.u8()?, x: r.u16()?, y: r.u16()?, w: r.u16()?, h: r.u16()? });
        }
        for _ in 0..r.u8()? {
            let duration = u24(r)?;
            let (palette_id_ref, n) = (r.u8()?, r.u8()?);
            let mut objects = Vec::new();
            for _ in 0..n {
                let mut o = CompObject {
                    object_id_ref: r.u16()?, window_id_ref: r.u8()?, flags: r.u8()?,
                    x: r.u16()?, y: r.u16()?, crop: None,
                };
                if o.flags & 0x80 != 0 {
                    o.crop = Some([r.u16()?, r.u16()?, r.u16()?, r.u16()?]);
                }
                objects.push(o);
            }
            s.effects.push(Effect { duration, palette_id_ref, objects });
        }
        Ok(s)
    }
    pub fn pack(&self) -> Vec<u8> {
        let mut o = vec![self.windows.len() as u8];
        for w in &self.windows {
            o.p8(w.window_id);
            for v in [w.x, w.y, w.w, w.h] { o.p16(v); }
        }
        o.p8(self.effects.len() as u8);
        for e in &self.effects {
            p24(&mut o, e.duration);
            o.p8(e.palette_id_ref);
            o.p8(e.objects.len() as u8);
            for c in &e.objects {
                o.p16(c.object_id_ref);
                o.p8(c.window_id_ref);
                o.p8(c.flags);
                o.p16(c.x);
                o.p16(c.y);
                if let Some(cr) = c.crop {
                    for v in cr { o.p16(v); }
                }
            }
        }
        o
    }
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct Button {
    pub button_id: u16,
    pub numeric_select_value: u16,
    pub auto_action_flags: u8,
    pub x: u16,
    pub y: u16,
    pub upper: u16,
    pub lower: u16,
    pub left: u16,
    pub right: u16,
    pub normal: (u16, u16, u8),        // objet debut, objet fin, flags
    pub selected: (u8, u16, u16, u8),  // son, debut, fin, flags
    pub activated: (u8, u16, u16),     // son, debut, fin
    pub commands: Vec<Command>,
}

impl Button {
    pub fn parse(r: &mut Reader) -> PResult<Self> {
        let mut b = Button {
            button_id: r.u16()?, numeric_select_value: r.u16()?, auto_action_flags: r.u8()?,
            x: r.u16()?, y: r.u16()?, upper: r.u16()?, lower: r.u16()?, left: r.u16()?, right: r.u16()?,
            ..Default::default()
        };
        b.normal = (r.u16()?, r.u16()?, r.u8()?);
        b.selected = (r.u8()?, r.u16()?, r.u16()?, r.u8()?);
        b.activated = (r.u8()?, r.u16()?, r.u16()?);
        for _ in 0..r.u16()? {
            b.commands.push(Command::parse(r)?);
        }
        Ok(b)
    }
    pub fn pack(&self) -> Vec<u8> {
        let mut o = Vec::new();
        o.p16(self.button_id);
        o.p16(self.numeric_select_value);
        o.p8(self.auto_action_flags);
        for v in [self.x, self.y, self.upper, self.lower, self.left, self.right] { o.p16(v); }
        o.p16(self.normal.0); o.p16(self.normal.1); o.p8(self.normal.2);
        o.p8(self.selected.0); o.p16(self.selected.1); o.p16(self.selected.2); o.p8(self.selected.3);
        o.p8(self.activated.0); o.p16(self.activated.1); o.p16(self.activated.2);
        o.p16(self.commands.len() as u16);
        for c in &self.commands { o.extend_from_slice(&c.pack()); }
        o
    }
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct Bog { pub default_valid_button: u16, pub buttons: Vec<Button> }

#[derive(Debug, Clone, PartialEq)]
pub struct Page {
    pub page_id: u8,
    pub version: u8,
    pub uo_mask: [u8; 8],
    pub in_effects: EffectSequence,
    pub out_effects: EffectSequence,
    pub frame_rate_code: u8,
    pub default_selected: u16,
    pub default_activated: u16,
    pub palette_id_ref: u8,
    pub bogs: Vec<Bog>,
}

impl Default for Page {
    fn default() -> Self {
        Page {
            page_id: 0, version: 0, uo_mask: [0; 8], in_effects: Default::default(), out_effects: Default::default(),
            frame_rate_code: 0, default_selected: 0, default_activated: 0, palette_id_ref: 0, bogs: Vec::new(),
        }
    }
}

impl Page {
    pub fn parse(r: &mut Reader) -> PResult<Self> {
        let mut p = Page { page_id: r.u8()?, version: r.u8()?, uo_mask: r.raw(8)?.try_into().unwrap(), ..Default::default() };
        p.in_effects = EffectSequence::parse(r)?;
        p.out_effects = EffectSequence::parse(r)?;
        p.frame_rate_code = r.u8()?;
        p.default_selected = r.u16()?;
        p.default_activated = r.u16()?;
        p.palette_id_ref = r.u8()?;
        for _ in 0..r.u8()? {
            let mut bog = Bog { default_valid_button: r.u16()?, buttons: Vec::new() };
            for _ in 0..r.u8()? {
                bog.buttons.push(Button::parse(r)?);
            }
            p.bogs.push(bog);
        }
        Ok(p)
    }
    pub fn pack(&self) -> Vec<u8> {
        let mut o = vec![self.page_id, self.version];
        o.extend_from_slice(&self.uo_mask);
        o.extend(self.in_effects.pack());
        o.extend(self.out_effects.pack());
        o.p8(self.frame_rate_code);
        o.p16(self.default_selected);
        o.p16(self.default_activated);
        o.p8(self.palette_id_ref);
        o.p8(self.bogs.len() as u8);
        for g in &self.bogs {
            o.p16(g.default_valid_button);
            o.p8(g.buttons.len() as u8);
            for b in &g.buttons { o.extend(b.pack()); }
        }
        o
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Ics {
    pub video_width: u16,
    pub video_height: u16,
    pub frame_rate_byte: u8,
    pub composition_number: u16,
    pub composition_state: u8, // 0x80 = debut d'epoch
    pub sequence_descriptor: u8,
    pub stream_model: u8, // 0 = multiplexe, 1 = hors multiplex
    pub ui_model: u8,     // 0 = normal, 1 = pop-up
    pub composition_timeout_pts: u64,
    pub selection_timeout_pts: u64,
    pub user_timeout_duration: u32,
    pub pages: Vec<Page>,
}

impl Default for Ics {
    fn default() -> Self {
        Ics {
            video_width: 1920, video_height: 1080, frame_rate_byte: 0x60, composition_number: 0,
            composition_state: 0x80, sequence_descriptor: 0xC0, stream_model: 0, ui_model: 0,
            composition_timeout_pts: 0, selection_timeout_pts: 0, user_timeout_duration: 0, pages: Vec::new(),
        }
    }
}

impl Ics {
    pub fn parse(b: &[u8]) -> PResult<Self> {
        let mut r = Reader::new(b);
        let mut i = Ics {
            video_width: r.u16()?, video_height: r.u16()?, frame_rate_byte: r.u8()?,
            composition_number: r.u16()?, composition_state: r.u8()?, sequence_descriptor: r.u8()?,
            ..Default::default()
        };
        let length = u24(&mut r)? as usize;
        if b.len() < 12 || length != b.len() - 12 {
            return err("longueur de composition incoherente");
        }
        let flags = r.u8()? as u32;
        i.stream_model = bits(flags, 1, 7) as u8;
        i.ui_model = bits(flags, 1, 6) as u8;
        if i.stream_model == 0 {
            let m = (1u64 << 33) - 1;
            i.composition_timeout_pts = u40(&mut r)? & m;
            i.selection_timeout_pts = u40(&mut r)? & m;
        }
        i.user_timeout_duration = u24(&mut r)?;
        for _ in 0..r.u8()? {
            i.pages.push(Page::parse(&mut r)?);
        }
        if r.pos != b.len() {
            return err(format!("{} octets non consommes", b.len() - r.pos));
        }
        Ok(i)
    }
    pub fn pack(&self) -> Vec<u8> {
        let mut body = vec![(self.stream_model << 7) | (self.ui_model << 6)];
        if self.stream_model == 0 {
            p40(&mut body, self.composition_timeout_pts);
            p40(&mut body, self.selection_timeout_pts);
        }
        p24(&mut body, self.user_timeout_duration);
        body.p8(self.pages.len() as u8);
        for p in &self.pages { body.extend(p.pack()); }
        let mut o = Vec::new();
        o.p16(self.video_width);
        o.p16(self.video_height);
        o.p8(self.frame_rate_byte);
        o.p16(self.composition_number);
        o.p8(self.composition_state);
        o.p8(self.sequence_descriptor);
        p24(&mut o, body.len() as u32);
        o.extend(body);
        o
    }
}
