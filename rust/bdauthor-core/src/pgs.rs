//! Sous-titres Blu-ray en images (PGS, « Presentation Graphics ») : segments PCS/WDS/PDS/ODS/END, ensembles d'affichage horodates,
//! et PES prets a etre multiplexes dans le clip du film (PID 0x1200 + n, stream_type 0x90).
use crate::ig::{rle_encode, Ods, PalEntry, Pds};
use crate::m2ts::Segment;
use crate::mux::{plane_ticks, ts5, PesSpec};
use crate::palette::{quantize_images, RgbaImage};
use crate::project::Renderer;
use crate::subtitle::{Cue, HAlign, SubStyle, VPos};
use std::collections::BTreeMap;

pub const PG_PID0: u16 = 0x1200;
pub const PG_STREAM_TYPE: u8 = 0x90;
pub const STREAM_ID_PG: u8 = 0xBD;
pub const SEG_PCS: u8 = 0x16;
pub const SEG_WDS: u8 = 0x17;
pub const SEG_PDS: u8 = 0x14;
pub const SEG_ODS: u8 = 0x15;
pub const SEG_END: u8 = 0x80;

/// Duree minimale d'affichage d'un sous-titre (90 kHz) : 0,3 s.
pub const MIN_DURATION: u64 = 27_000;
/// Taille maximale des donnees RLE d'un fragment d'ODS (le champ de longueur d'un segment est sur 16 bits).
const ODS_CHUNK: usize = 64_000;

fn seg(t: u8, payload: Vec<u8>) -> Vec<u8> {
    Segment { seg_type: t, payload }.pack()
}

/// Un sous-titre pret a etre emis : image indexee, position, entrees de palette.
#[derive(Debug, Clone, PartialEq)]
pub struct SubImage {
    pub x: u16,
    pub y: u16,
    pub w: u16,
    pub h: u16,
    pub entries: BTreeMap<u8, PalEntry>,
    pub rows: Vec<Vec<u8>>,
}

/// Segments ODS (un seul, ou plusieurs fragments si les donnees sont longues) de l'objet `id`.
pub fn ods_segments(id: u16, w: u16, h: u16, rle: &[u8]) -> Vec<Vec<u8>> {
    if rle.len() <= ODS_CHUNK {
        return vec![seg(SEG_ODS, Ods { object_id: id, version: 0, sequence_flag: 0xC0, width: w, height: h, rle: rle.to_vec() }.pack())];
    }
    let chunks: Vec<&[u8]> = rle.chunks(ODS_CHUNK).collect();
    let mut out = Vec::new();
    for (k, c) in chunks.iter().enumerate() {
        let flag = match (k == 0, k + 1 == chunks.len()) {
            (true, _) => 0x80,
            (_, true) => 0x40,
            _ => 0x00,
        };
        let mut p = Vec::with_capacity(c.len() + 11);
        p.extend_from_slice(&id.to_be_bytes());
        p.extend_from_slice(&[0, flag]);
        if k == 0 {
            let l = rle.len() + 4;
            p.extend_from_slice(&[(l >> 16) as u8, (l >> 8) as u8, l as u8]);
            p.extend_from_slice(&w.to_be_bytes());
            p.extend_from_slice(&h.to_be_bytes());
        }
        p.extend_from_slice(c);
        out.push(seg(SEG_ODS, p));
    }
    out
}

fn pcs(vw: u16, vh: u16, rate_code: u8, comp_no: u16, state: u8, obj: Option<(u16, u16)>) -> Vec<u8> {
    let mut p = Vec::new();
    p.extend_from_slice(&vw.to_be_bytes());
    p.extend_from_slice(&vh.to_be_bytes());
    p.push(rate_code << 4);
    p.extend_from_slice(&comp_no.to_be_bytes());
    p.extend_from_slice(&[state, 0, 0, obj.is_some() as u8]);
    if let Some((x, y)) = obj {
        p.extend_from_slice(&[0, 0, 0, 0]); // objet 0, fenetre 0, pas de recadrage
        p.extend_from_slice(&x.to_be_bytes());
        p.extend_from_slice(&y.to_be_bytes());
    }
    seg(SEG_PCS, p)
}

fn wds(x: u16, y: u16, w: u16, h: u16) -> Vec<u8> {
    let mut p = vec![1, 0];
    for v in [x, y, w, h] {
        p.extend_from_slice(&v.to_be_bytes());
    }
    seg(SEG_WDS, p)
}

/// PES d'un segment PG (stream_id 0xBD).
pub fn build_pg_pes(spec: &PesSpec) -> Result<Vec<u8>, String> {
    let (flags, ts): (u8, Vec<u8>) = match spec.dts {
        None => (0x80, ts5(0b0010, spec.pts).to_vec()),
        Some(d) => (0xC0, [ts5(0b0011, spec.pts), ts5(0b0001, d)].concat()),
    };
    let length = 3 + ts.len() + spec.data.len();
    if length > 0xFFFF {
        return Err("PES de sous-titre trop long".into());
    }
    let mut o = vec![0, 0, 1, STREAM_ID_PG];
    o.extend_from_slice(&(length as u16).to_be_bytes());
    o.extend_from_slice(&[0x81, flags, ts.len() as u8]);
    o.extend(ts);
    o.extend_from_slice(&spec.data);
    Ok(o)
}

/// Un sous-titre a afficher de `start` a `end` (90 kHz absolus).
pub struct Timed {
    pub start: u64,
    pub end: u64,
    pub img: SubImage,
}

/// Ensembles d'affichage de toute une piste. `first_ok` : plus petit DTS admis (les sous-titres trop proches du debut sont retardes).
/// Chaque sous-titre ouvre sa propre « epoque » ; un ensemble d'effacement le suit, sauf si le suivant arrive trop tot pour le permettre
/// (le suivant remplace alors directement le precedent).
pub fn display_sets(items: &[Timed], video: (u32, u32), rate_code: u8, first_ok: u64) -> Result<Vec<PesSpec>, String> {
    let (vw, vh) = (video.0 as u16, video.1 as u16);
    let clear_t = plane_ticks(video.0 as u64, video.1 as u64);
    let mut specs = Vec::new();
    let mut comp_no = 0u16;
    let mut next_ok = first_ok; // aucun DTS ne doit etre inferieur a la fin de l'ensemble precedent
    for (i, it) in items.iter().enumerate() {
        let im = &it.img;
        let start = it.start.max(next_ok + clear_t);
        let mut end = it.end.max(start + MIN_DURATION);
        if let Some(n) = items.get(i + 1) {
            end = end.min(n.start.max(start + MIN_DURATION));
        }
        let rle = rle_encode(&im.rows);
        let obj_t = plane_ticks(im.w as u64, im.h as u64);
        let t = start - clear_t; // heure de decodage de l'ensemble
        let mut pds = Pds::new(0, 0);
        for (k, e) in &im.entries {
            pds.set(*k, *e);
        }
        specs.push(PesSpec { pts: start, dts: Some(t), data: pcs(vw, vh, rate_code, comp_no, 0x80, Some((im.x, im.y))) });
        specs.push(PesSpec { pts: t, dts: None, data: wds(im.x, im.y, im.w, im.h) });
        specs.push(PesSpec { pts: t, dts: None, data: seg(SEG_PDS, pds.pack()) });
        for s in ods_segments(0, im.w, im.h, &rle) {
            specs.push(PesSpec { pts: t + obj_t.min(clear_t), dts: Some(t), data: s });
        }
        specs.push(PesSpec { pts: t + obj_t.min(clear_t), dts: None, data: seg(SEG_END, vec![]) });
        comp_no = comp_no.wrapping_add(1);
        next_ok = t + obj_t.min(clear_t);
        // effacement
        let next_start = items.get(i + 1).map(|n| n.start);
        let clear_dts = end.saturating_sub(plane_ticks(im.w as u64, im.h as u64)).max(next_ok);
        let room = next_start.map_or(true, |n| n.saturating_sub(clear_t) > end);
        if room {
            specs.push(PesSpec { pts: end, dts: Some(clear_dts), data: pcs(vw, vh, rate_code, comp_no, 0x00, None) });
            specs.push(PesSpec { pts: clear_dts, dts: None, data: wds(im.x, im.y, im.w, im.h) });
            specs.push(PesSpec { pts: clear_dts, dts: None, data: seg(SEG_END, vec![]) });
            comp_no = comp_no.wrapping_add(1);
            next_ok = clear_dts;
        }
    }
    Ok(specs)
}

// ------------------------------------------------------------------ du texte aux images
/// Rogne les bords transparents ; `None` si l'image est entierement transparente. Retourne (image, decalage x, decalage y).
pub fn crop_visible(im: &RgbaImage, alpha_min: u8) -> Option<(RgbaImage, u32, u32)> {
    let (mut x0, mut y0, mut x1, mut y1) = (u32::MAX, u32::MAX, 0u32, 0u32);
    for y in 0..im.height {
        for x in 0..im.width {
            if im.pixel(x, y)[3] >= alpha_min {
                x0 = x0.min(x);
                y0 = y0.min(y);
                x1 = x1.max(x);
                y1 = y1.max(y);
            }
        }
    }
    if x1 < x0 {
        return None;
    }
    let (w, h) = (x1 - x0 + 1, y1 - y0 + 1);
    let mut out = RgbaImage::new(w, h);
    for y in 0..h {
        for x in 0..w {
            out.set(x, y, im.pixel(x0 + x, y0 + y));
        }
    }
    Some((out, x0, y0))
}

/// Style mis a l'echelle d'une video de `vh` lignes (reference : 1080).
pub fn scaled_style(s: &SubStyle, vh: u32) -> SubStyle {
    let k = vh as f64 / 1080.0;
    let sc = |v: i32| (v as f64 * k).round() as i32;
    SubStyle { size: sc(s.size).max(8), outline: sc(s.outline), margin: sc(s.margin), margin_x: sc(s.margin_x), shadow: sc(s.shadow), bg_pad: sc(s.bg_pad), bg_radius: sc(s.bg_radius), ..s.clone() }
}

/// Coin haut-gauche d'une image `w` x `h` (style deja mis a l'echelle) dans une video `video`.
pub fn place(st: &SubStyle, video: (u32, u32), w: u32, h: u32) -> (u32, u32) {
    let (w, h) = (w.min(video.0), h.min(video.1));
    let mx = st.margin_x.max(0) as u32;
    let x = match st.halign {
        HAlign::Center => (video.0 - w) / 2,
        HAlign::Left => mx.min(video.0 - w),
        HAlign::Right => video.0.saturating_sub(w + mx),
    };
    let y = match st.pos {
        VPos::Bottom => video.1.saturating_sub(st.margin.max(0) as u32 + h),
        VPos::Top => (st.margin.max(0) as u32).min(video.1 - h),
        VPos::Middle => (video.1 - h) / 2,
    };
    (x, y)
}

/// Image quantifiee et placee d'un texte ; `None` si le texte ne produit aucun pixel visible.
pub fn render_cue(text: &str, style: &SubStyle, renderer: &dyn Renderer, video: (u32, u32)) -> Result<Option<SubImage>, String> {
    let st = scaled_style(style, video.1);
    let Some(img) = renderer.subtitle(text, &st, video.0) else { return Ok(None) };
    let Some((img, _, _)) = crop_visible(&img, 12) else { return Ok(None) };
    let (w, h) = (img.width.min(video.0), img.height.min(video.1));
    let (x, y) = place(&st, video, w, h);
    // paliers d'opacite : les 4 habituels (bords lisses) plus ceux de la boite et de l'ombre, pour qu'ils soient rendus exactement
    let mut levels = vec![255u8, 190, 120, 60];
    let mut colors = 6;
    if st.bg_opacity > 0 {
        levels.push((st.bg_opacity.clamp(1, 100) as u32 * 255 / 100) as u8);
        colors += 2;
    }
    if st.shadow > 0 {
        levels.push((st.shadow_opacity.clamp(1, 100) as u32 * 255 / 100) as u8);
        colors += 1;
    }
    levels.sort_unstable_by(|a, b| b.cmp(a));
    levels.dedup();
    let (entries, mut rows) = quantize_images(&[&img], colors, &levels, 12, false)?;
    Ok(Some(SubImage { x: x as u16, y: y as u16, w: w as u16, h: h as u16, entries, rows: rows.remove(0) }))
}

/// Piste qui n'affiche jamais rien : un seul ensemble d'affichage (debut d'epoque sans objet). Sert aux langues du projet qu'une
/// video n'a pas, pour que le numero de piste de chaque langue reste le meme dans toutes les videos.
pub fn empty_track(video: (u32, u32), rate_code: u8, first_ok: u64) -> Vec<PesSpec> {
    let (vw, vh) = (video.0 as u16, video.1 as u16);
    let clear_t = plane_ticks(video.0 as u64, video.1 as u64);
    let t = first_ok + clear_t;
    vec![
        PesSpec { pts: t + clear_t, dts: Some(t), data: pcs(vw, vh, rate_code, 0, 0x80, None) },
        PesSpec { pts: t, dts: None, data: wds(0, 0, 1, 1) },
        PesSpec { pts: t, dts: None, data: seg(SEG_END, vec![]) },
    ]
}

/// Piste complete : repliques deja fusionnees (temps en ms depuis le debut de la video), `base90` = PTS du temps 0.
pub fn encode_track(
    cues: &[Cue], style: &SubStyle, renderer: &dyn Renderer, video: (u32, u32), rate_code: u8, base90: u64, first_ok: u64,
) -> Result<Vec<PesSpec>, String> {
    let mut items = Vec::new();
    for c in cues {
        if let Some(img) = render_cue(&c.text, style, renderer, video)? {
            items.push(Timed { start: base90 + c.start_ms as u64 * 90, end: base90 + c.end_ms as u64 * 90, img });
        }
    }
    display_sets(&items, video, rate_code, first_ok)
}

// ------------------------------------------------------------------ lecture (controles et tests)
/// Decoupe des donnees PG en segments (type, charge utile).
pub fn parse_segments(data: &[u8]) -> Result<Vec<(u8, Vec<u8>)>, String> {
    let (mut out, mut p) = (Vec::new(), 0usize);
    while p < data.len() {
        if p + 3 > data.len() {
            return Err("segment PG tronque".into());
        }
        let (t, n) = (data[p], ((data[p + 1] as usize) << 8) | data[p + 2] as usize);
        if p + 3 + n > data.len() {
            return Err("segment PG tronque".into());
        }
        out.push((t, data[p + 3..p + 3 + n].to_vec()));
        p += 3 + n;
    }
    Ok(out)
}

/// Objet decode d'un ensemble d'affichage.
#[derive(Debug, Clone, PartialEq)]
pub struct DecodedObject {
    pub x: u16,
    pub y: u16,
    pub w: u16,
    pub h: u16,
    pub rgba: RgbaImage,
}

#[derive(Debug, Clone, PartialEq)]
pub struct DecodedSet {
    pub pts: u64,
    pub state: u8,
    pub comp_no: u16,
    pub objects: Vec<DecodedObject>,
}

/// Relit les ensembles d'affichage d'une piste (PES dans l'ordre du flux) : reassemble les ODS fragmentes, applique la palette.
pub fn decode_sets(specs: &[PesSpec]) -> Result<Vec<DecodedSet>, String> {
    use crate::ig::rle_decode;
    let mut sets = Vec::new();
    let mut cur: Option<(u64, u8, u16, Vec<(u16, u16)>)> = None; // pts, etat, no, positions
    let mut pal = Pds::new(0, 0);
    let mut ods: Option<(u16, u16, Vec<u8>, usize)> = None;
    let mut pending: Vec<(u16, u16, u16, u16, Vec<u8>)> = Vec::new();
    for sp in specs {
        for (t, p) in parse_segments(&sp.data)? {
            match t {
                SEG_PCS => {
                    let nobj = p[10] as usize;
                    let mut pos = Vec::new();
                    for k in 0..nobj {
                        let o = 11 + 8 * k;
                        pos.push((u16::from_be_bytes([p[o + 4], p[o + 5]]), u16::from_be_bytes([p[o + 6], p[o + 7]])));
                    }
                    cur = Some((sp.pts, p[7], u16::from_be_bytes([p[5], p[6]]), pos));
                    pending.clear();
                }
                SEG_PDS => pal = Pds::parse(&p).map_err(|e| e.0)?,
                SEG_ODS => {
                    let flag = p[3];
                    if flag & 0x80 != 0 {
                        let len = ((p[4] as usize) << 16) | ((p[5] as usize) << 8) | p[6] as usize;
                        ods = Some((u16::from_be_bytes([p[7], p[8]]), u16::from_be_bytes([p[9], p[10]]), p[11..].to_vec(), len - 4));
                    } else if let Some(o) = ods.as_mut() {
                        o.2.extend_from_slice(&p[4..]);
                    }
                    if flag & 0x40 != 0 {
                        let (w, h, d, len) = ods.take().ok_or("ODS sans debut")?;
                        if d.len() != len {
                            return Err("longueur RLE incoherente".into());
                        }
                        pending.push((w, h, w, h, d));
                    }
                }
                SEG_END => {
                    if let Some((pts, state, comp_no, pos)) = cur.take() {
                        let mut objects = Vec::new();
                        for (k, (x, y)) in pos.iter().enumerate() {
                            let (w, h, _, _, d) = pending.get(k).ok_or("objet manquant")?;
                            let rows = rle_decode(d, *w as usize, *h as usize).map_err(|e| e.0)?;
                            let mut im = RgbaImage::new(*w as u32, *h as u32);
                            for (yy, r) in rows.iter().enumerate() {
                                for (xx, &i) in r.iter().enumerate() {
                                    im.set(xx as u32, yy as u32, pal.rgba(i));
                                }
                            }
                            objects.push(DecodedObject { x: *x, y: *y, w: *w, h: *h, rgba: im });
                        }
                        sets.push(DecodedSet { pts, state, comp_no, objects });
                    }
                }
                _ => {}
            }
        }
    }
    Ok(sets)
}
