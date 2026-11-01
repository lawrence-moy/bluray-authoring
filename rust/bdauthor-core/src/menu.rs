//! Construction d'un menu IG : boutons -> ICS + PDS + ODS -> clip .m2ts + .clpi (equivalent de `menu.py`).
//!
//! Ce module ne depend d'aucune bibliotheque d'images : chaque bouton porte deja ses images RGBA
//! (normal, selectionne, active). Le rendu du texte et le chargement des PNG sont dans `render` (feature `render`).
use crate::clpi::{AtcSeq, ClipInfo, Clpi, Program, ProgramInfo, SequenceInfo, Stream, StcSeq};
use crate::ig::*;
use crate::m2ts::Segment;
use crate::mobj::Command;
use crate::mux::{ig_pes_specs, mux_ig_clip, PesSpec};
use crate::palette::{quantize_rgba, RgbaImage, ALPHA_MIN};
use std::collections::HashMap;
use std::sync::Arc;

/// Mot d'instruction JUMP_TITLE (operande immediat dans `dst`).
pub const JUMP_TITLE_INSN: u32 = 0x2181_0000;

#[derive(Debug, Clone)]
pub struct MenuButton {
    pub label: String,
    pub title: u32, // JUMP_TITLE vers ce numero (0 = menu principal, 1.. = titres)
    pub x: i32,
    pub y: i32,
    /// Images (normal, selectionne, active) ; 1 ou 2 elements acceptes, les images identiques (meme `Arc`) sont partagees.
    pub images: Vec<Arc<RgbaImage>>,
    pub neighbors: Option<(u16, u16, u16, u16)>, // (haut, bas, gauche, droite)
    /// voisins choisis a la main, direction par direction (haut, bas, gauche, droite) ; `None` = calcul automatique
    pub nav_set: [Option<u16>; 4],
    pub commands: Vec<Command>,                  // vide = JUMP_TITLE `title`
    /// Etat selectionne anime : images suivantes de l'animation, apres `images[1]` (vide = image fixe).
    pub sel_frames: Vec<Arc<RgbaImage>>,
    /// Animation jouee une seule fois (sinon en boucle).
    pub sel_once: bool,
    /// Etat normal anime : images suivantes de l'animation, apres `images[0]` (vide = image fixe).
    pub normal_frames: Vec<Arc<RgbaImage>>,
    /// Animation de l'etat normal jouee une seule fois (sinon en boucle).
    pub normal_once: bool,
    /// Sons (identifiants dans sound.bdmv, 0xFF = aucun) : arrivee de la selection, appui sur OK.
    pub sound_select: u8,
    pub sound_activate: u8,
}

impl MenuButton {
    pub fn new(label: &str, title: u32, x: i32, y: i32, images: Vec<Arc<RgbaImage>>) -> Self {
        MenuButton { label: label.into(), title, x, y, images, neighbors: None, nav_set: [None; 4], commands: vec![], sel_frames: vec![], sel_once: false, normal_frames: vec![], normal_once: false, sound_select: 0xFF, sound_activate: 0xFF }
    }
    pub fn width(&self) -> i32 { self.images.first().map_or(0, |i| i.width as i32) }
    pub fn height(&self) -> i32 { self.images.first().map_or(0, |i| i.height as i32) }
}

/// (haut, bas, gauche, droite) de chaque bouton, d'apres la position des centres ; soi-meme si aucun voisin.
pub fn auto_neighbors(rects: &[(i32, i32, i32, i32)]) -> Vec<(u16, u16, u16, u16)> {
    let cx: Vec<f64> = rects.iter().map(|r| r.0 as f64 + r.2 as f64 / 2.0).collect();
    let cy: Vec<f64> = rects.iter().map(|r| r.1 as f64 + r.3 as f64 / 2.0).collect();
    let n = rects.len();
    let pick = |i: usize, ok: &dyn Fn(usize) -> bool, score: &dyn Fn(usize) -> f64| -> u16 {
        (0..n)
            .filter(|&j| j != i && ok(j))
            .min_by(|&a, &b| score(a).partial_cmp(&score(b)).unwrap().then(a.cmp(&b)))
            .unwrap_or(i) as u16
    };
    (0..n)
        .map(|i| {
            (
                pick(i, &|j| cy[j] < cy[i], &|j| (cy[i] - cy[j]) + 2.0 * (cx[j] - cx[i]).abs()),
                pick(i, &|j| cy[j] > cy[i], &|j| (cy[j] - cy[i]) + 2.0 * (cx[j] - cx[i]).abs()),
                pick(i, &|j| cx[j] < cx[i], &|j| (cx[i] - cx[j]) + 2.0 * (cy[j] - cy[i]).abs()),
                pick(i, &|j| cx[j] > cx[i], &|j| (cx[j] - cx[i]) + 2.0 * (cy[j] - cy[i]).abs()),
            )
        })
        .collect()
}

#[derive(Debug, Clone)]
pub struct MenuOptions {
    pub ics_pts: u64,
    pub video_size: (u32, u32),
    pub ui_model: u8,
    pub rgb_colors: usize,
    /// Code de cadence de la video sous le menu (1 = 23,976, 2 = 24, 3 = 25, 4 = 29,97, 6 = 50, 7 = 59,94).
    pub frame_rate_code: u8,
    /// Cadence voulue pour les animations de boutons (images par seconde).
    pub anim_fps: f64,
}
impl Default for MenuOptions {
    fn default() -> Self {
        MenuOptions { ics_pts: 42956, video_size: (1920, 1080), ui_model: 1, rgb_colors: 63, frame_rate_code: 1, anim_fps: 8.0 }
    }
}

pub struct BuiltMenu {
    pub specs: Vec<PesSpec>,
    pub ics: Ics,
    pub pds: Pds,
    pub ods: Vec<Ods>,
}

/// Retourne les PES, l'ICS, la PDS et les ODS d'un menu a une page.
pub fn build_menu(buttons: &[MenuButton], o: &MenuOptions) -> Result<BuiltMenu, String> {
    // images uniques ; pour chaque bouton, indices de ses 3 etats dans `flat`
    let mut flat: Vec<Arc<RgbaImage>> = Vec::new();
    let mut owner: Vec<[usize; 3]> = Vec::new();
    for (n, b) in buttons.iter().enumerate() {
        if b.images.is_empty() {
            return Err(format!("le bouton {n} n'a aucune image"));
        }
        let mut ids = [0usize; 3];
        for k in 0..3 {
            let im = &b.images[k.min(b.images.len() - 1)];
            ids[k] = match flat.iter().position(|f| Arc::ptr_eq(f, im)) {
                Some(p) => p,
                None => { flat.push(im.clone()); flat.len() - 1 }
            };
        }
        owner.push(ids);
    }
    // etats selectionnes animes : images propres (objets consecutifs exiges par la norme), sans partage avec d'autres boutons
    let mut anim: Vec<Vec<usize>> = Vec::new();
    let mut nanim: Vec<Vec<usize>> = Vec::new();
    for b in buttons {
        let mut keys = Vec::new();
        if !b.sel_frames.is_empty() {
            for im in std::iter::once(&b.images[1.min(b.images.len() - 1)]).chain(b.sel_frames.iter()) {
                flat.push(im.clone());
                keys.push(flat.len() - 1);
            }
        }
        anim.push(keys);
        let mut keys = Vec::new();
        if !b.normal_frames.is_empty() {
            for im in std::iter::once(&b.images[0]).chain(b.normal_frames.iter()) {
                flat.push(im.clone());
                keys.push(flat.len() - 1);
            }
        }
        nanim.push(keys);
    }
    let refs: Vec<&RgbaImage> = flat.iter().map(|a| a.as_ref()).collect();
    // palette choisie d'apres le contenu : couleurs et niveaux d'opacite (halo, ombres, bords adoucis)
    let (entries, rows) = quantize_rgba(&refs, ALPHA_MIN);
    let mut pds = Pds::new(0, 0);
    for (i, e) in entries {
        pds.set(i, e);
    }
    let rects: Vec<(i32, i32, i32, i32)> = buttons.iter().map(|b| (b.x, b.y, b.width(), b.height())).collect();
    for (n, r) in rects.iter().enumerate() {
        if r.0 < 0 || r.1 < 0 || r.0 + r.2 > o.video_size.0 as i32 || r.1 + r.3 > o.video_size.1 as i32 {
            return Err(format!("le bouton {n} ({},{},{}x{}) sort de l'ecran {:?}", r.0, r.1, r.2, r.3, o.video_size));
        }
    }
    let nbs = auto_neighbors(&rects);
    let (mut ods, mut bogs, mut made): (Vec<Ods>, Vec<Bog>, HashMap<usize, u16>) = (vec![], vec![], HashMap::new());
    for (n, b) in buttons.iter().enumerate() {
        let mut oid = [0u16; 3];
        for k in 0..3 {
            let key = owner[n][k];
            oid[k] = *made.entry(key).or_insert_with(|| {
                let im = &flat[key];
                ods.push(Ods { object_id: ods.len() as u16, version: 0, sequence_flag: 0xC0, width: im.width as u16, height: im.height as u16, rle: rle_encode(&rows[key]) });
                (ods.len() - 1) as u16
            });
        }
        let (mut base, mut sel, act) = (oid[0], oid[1], oid[2]);
        // etat normal anime : objets consecutifs, en boucle sauf « une fois »
        let (mut base_end, mut base_flags) = (base, 0u8);
        if !nanim[n].is_empty() {
            for (j, &key) in nanim[n].iter().enumerate() {
                let im = &flat[key];
                ods.push(Ods { object_id: ods.len() as u16, version: 0, sequence_flag: 0xC0, width: im.width as u16, height: im.height as u16, rle: rle_encode(&rows[key]) });
                if j == 0 { base = (ods.len() - 1) as u16; }
            }
            base_end = (ods.len() - 1) as u16;
            base_flags = if b.normal_once { 0 } else { 0x80 };
        }
        // animation : objets consecutifs [debut, fin], lecture en boucle (bit « repeat »)
        let (mut sel_end, mut sel_flags) = (sel, 0u8);
        if !anim[n].is_empty() {
            for (j, &key) in anim[n].iter().enumerate() {
                let im = &flat[key];
                ods.push(Ods { object_id: ods.len() as u16, version: 0, sequence_flag: 0xC0, width: im.width as u16, height: im.height as u16, rle: rle_encode(&rows[key]) });
                if j == 0 { sel = (ods.len() - 1) as u16; }
            }
            sel_end = (ods.len() - 1) as u16;
            sel_flags = if b.sel_once { 0 } else { 0x80 };
        }
        let (mut up, mut down, mut left, mut right) = b.neighbors.unwrap_or(nbs[n]);
        for (slot, set) in [&mut up, &mut down, &mut left, &mut right].into_iter().zip(b.nav_set) {
            if let Some(v) = set.filter(|v| (*v as usize) < buttons.len()) { *slot = v; }
        }
        let btn = Button {
            button_id: n as u16, numeric_select_value: 0xFFFF, auto_action_flags: 0, x: b.x as u16, y: b.y as u16,
            upper: up, lower: down, left, right,
            normal: (base, base_end, base_flags), selected: (b.sound_select, sel, sel_end, sel_flags), activated: (b.sound_activate, act, act),
            commands: if b.commands.is_empty() { vec![Command::new(JUMP_TITLE_INSN, b.title, 0)] } else { b.commands.clone() },
        };
        bogs.push(Bog { default_valid_button: n as u16, buttons: vec![btn] });
    }
    // cadence des animations : une image toutes les (code + 1) images video, soit environ 8 images par seconde
    let animated = anim.iter().chain(nanim.iter()).any(|a| !a.is_empty());
    let page = Page { default_activated: 0xFFFF, bogs, frame_rate_code: if animated { anim_rate_code(o.frame_rate_code, o.anim_fps) } else { 0 }, ..Default::default() };
    let ics = Ics {
        video_width: o.video_size.0 as u16, video_height: o.video_size.1 as u16, frame_rate_byte: o.frame_rate_code << 4,
        composition_number: 0, composition_state: 0x80, sequence_descriptor: 0xC0, stream_model: 1, ui_model: o.ui_model,
        pages: vec![page], ..Default::default()
    };
    let seg = |t: u8, p: Vec<u8>| Segment { seg_type: t, payload: p }.pack();
    let ods_segs: Vec<(Vec<u8>, u64, u64)> = ods.iter().map(|d| (seg(SEG_ODS, d.pack()), d.width as u64, d.height as u64)).collect();
    let specs = ig_pes_specs(&seg(SEG_ICS, ics.pack()), &seg(SEG_PDS, pds.pack()), &ods_segs, o.ics_pts, (o.video_size.0 as u64, o.video_size.1 as u64));
    Ok(BuiltMenu { specs, ics, pds, ods })
}

/// Code de cadence des animations de boutons pour une video de code `video_code` et une cadence voulue `anim_fps` (images par
/// seconde) : le lecteur avance d'une image toutes les `code + 1` images video.
pub fn anim_rate_code(video_code: u8, anim_fps: f64) -> u8 {
    let fps = match video_code { 1 => 23.976, 2 => 24.0, 3 => 25.0, 4 => 29.97, 6 => 50.0, 7 => 59.94, _ => 25.0 };
    ((fps / anim_fps.max(0.5)).round() as i64 - 1).clamp(0, 255) as u8
}

/// Fichier .clpi d'un clip de menu IG (sans EP_map). Horloge 45 kHz.
pub fn make_clpi(num_source_packets: u32, ics_pts: u64, rate: u32, lang: [u8; 3], end_extra: u32) -> Vec<u8> {
    let (ig_pid, pcr_pid, pmt_pid) = (0x1400u16, 0x1001u16, 0x0100u16);
    let start = (ics_pts / 2) as u32;
    let mut attrs = lang.to_vec();
    attrs.extend_from_slice(&[0; 17]);
    Clpi {
        version: "0100".into(),
        clip_info: ClipInfo { clip_stream_type: 1, application_type: 5, is_cc5: 0, ts_recording_rate: rate, num_source_packets, ..Default::default() },
        sequence: SequenceInfo { atc: vec![AtcSeq { spn_atc_start: 0, offset_stc_id: 0, stc: vec![StcSeq { pcr_pid, spn_stc_start: 0, start_time: start, end_time: start + end_extra }] }] },
        program: ProgramInfo { programs: vec![Program { spn_start: 0, pmt_pid, num_groups: 0, streams: vec![Stream { pid: ig_pid, coding_type: 0x91, attrs }] }] },
        cpi: None,
        clip_mark: vec![0; 4],
        extension: vec![],
    }
    .pack()
}

pub struct MenuClip {
    pub m2ts: Vec<u8>,
    pub clpi: Vec<u8>,
    pub menu: BuiltMenu,
}

pub fn build_menu_clip(buttons: &[MenuButton], o: &MenuOptions) -> Result<MenuClip, String> {
    let menu = build_menu(buttons, o)?;
    let m2ts = mux_ig_clip(&menu.specs, None)?;
    let clpi = make_clpi((m2ts.len() / 192) as u32, o.ics_pts, 5_388_536, *b"eng", 1501);
    Ok(MenuClip { m2ts, clpi, menu })
}

/// Plus petit PTS d'ICS (pair, 90 kHz) laissant le temps de decoder la liste d'objets (nombre de pixels de chacun).
pub fn required_ics_pts(object_pixels: &[u64], video_size: (u64, u64), minimum: u64, margin: u64) -> u64 {
    let ceil_div = |a: u64, b: u64| (a + b - 1) / b;
    let plane = ceil_div(video_size.0 * video_size.1 * 90000, 8_000_000);
    let need = plane + object_pixels.iter().map(|&px| ceil_div(px * 90000, 8_000_000)).sum::<u64>() + margin;
    minimum.max(need + (need % 2))
}
