//! Quantification d'images RGBA vers une palette Blu-ray commune (Y, Cr, Cb, alpha) — equivalent de `png_palette.py`,
//! sans dependance : coupe mediane et tramage de Floyd-Steinberg implementes ici (Pillow n'est plus necessaire).
//!
//! Les pixels (presque) transparents prennent l'indice 0 ; les autres sont quantifies en `rgb_colors` couleurs
//! (sur l'ensemble des images) puis associes au palier d'alpha le plus proche. Une entree = (couleur, alpha).
use crate::ig::{py_round, PalEntry};
use std::collections::{BTreeMap, HashMap};

pub const ALPHA_LEVELS: [u8; 4] = [255, 192, 128, 64];
pub const ALPHA_MIN: u8 = 16; // en dessous : transparent
pub const MAX_ENTRIES: usize = 254;

/// Image RGBA 8 bits, lignes contigues (R, G, B, A).
#[derive(Debug, Clone, PartialEq)]
pub struct RgbaImage {
    pub width: u32,
    pub height: u32,
    pub data: Vec<u8>,
}

impl RgbaImage {
    pub fn new(width: u32, height: u32) -> Self {
        RgbaImage { width, height, data: vec![0; (width * height * 4) as usize] }
    }
    pub fn from_raw(width: u32, height: u32, data: Vec<u8>) -> Result<Self, String> {
        if data.len() != (width as usize) * (height as usize) * 4 {
            return Err("taille de tampon RGBA incoherente".into());
        }
        Ok(RgbaImage { width, height, data })
    }
    pub fn pixel(&self, x: u32, y: u32) -> [u8; 4] {
        let i = ((y * self.width + x) * 4) as usize;
        [self.data[i], self.data[i + 1], self.data[i + 2], self.data[i + 3]]
    }
    pub fn set(&mut self, x: u32, y: u32, p: [u8; 4]) {
        let i = ((y * self.width + x) * 4) as usize;
        self.data[i..i + 4].copy_from_slice(&p);
    }
}

/// BT.709, plage limitee (inverse de `Pds::rgba`). Retourne (Y, Cr, Cb).
pub fn rgb_to_ycrcb(r: u8, g: u8, b: u8) -> (u8, u8, u8) {
    let (r, g, b) = (r as f64, g as f64, b as f64);
    let y = 16.0 + 0.1826 * r + 0.6142 * g + 0.0620 * b;
    let cb = 128.0 - 0.1006 * r - 0.3386 * g + 0.4392 * b;
    let cr = 128.0 + 0.4392 * r - 0.3989 * g - 0.0403 * b;
    let c = |v: f64| py_round(v).clamp(0.0, 255.0) as u8;
    (c(y), c(cr), c(cb))
}

// ------------------------------------------------------------------ coupe mediane
struct Bucket {
    colors: Vec<([u8; 3], u32)>, // couleurs distinctes et effectifs
}

impl Bucket {
    fn total(&self) -> u64 {
        self.colors.iter().map(|c| c.1 as u64).sum()
    }
    fn ranges(&self) -> [u32; 3] {
        let mut lo = [255u8; 3];
        let mut hi = [0u8; 3];
        for (c, _) in &self.colors {
            for k in 0..3 {
                lo[k] = lo[k].min(c[k]);
                hi[k] = hi[k].max(c[k]);
            }
        }
        [(hi[0] - lo[0]) as u32, (hi[1] - lo[1]) as u32, (hi[2] - lo[2]) as u32]
    }
    fn mean(&self) -> [u8; 3] {
        let t = self.total().max(1);
        let mut s = [0u64; 3];
        for (c, n) in &self.colors {
            for k in 0..3 {
                s[k] += c[k] as u64 * *n as u64;
            }
        }
        [((s[0] + t / 2) / t) as u8, ((s[1] + t / 2) / t) as u8, ((s[2] + t / 2) / t) as u8]
    }
}

/// Coupe mediane ponderee : au plus `k` couleurs. Resultat deterministe.
pub fn median_cut(counts: &HashMap<[u8; 3], u32>, k: usize) -> Vec<[u8; 3]> {
    let mut colors: Vec<([u8; 3], u32)> = counts.iter().map(|(c, n)| (*c, *n)).collect();
    colors.sort(); // ordre deterministe malgre HashMap
    if colors.is_empty() {
        return vec![];
    }
    let mut buckets = vec![Bucket { colors }];
    while buckets.len() < k {
        // on coupe le seau le plus peuple parmi ceux qui ont plus d'une couleur distincte
        let pick = buckets
            .iter()
            .enumerate()
            .filter(|(_, b)| b.colors.len() > 1)
            .max_by_key(|(i, b)| (b.total(), std::cmp::Reverse(*i)))
            .map(|(i, _)| i);
        let Some(i) = pick else { break };
        let mut b = buckets.swap_remove(i);
        let r = b.ranges();
        let ch = (0..3).max_by_key(|&k| (r[k], std::cmp::Reverse(k))).unwrap();
        b.colors.sort_by_key(|(c, _)| (c[ch], c[(ch + 1) % 3], c[(ch + 2) % 3]));
        let half = b.total() / 2;
        let (mut acc, mut cut) = (0u64, 0usize);
        for (j, (_, n)) in b.colors.iter().enumerate() {
            acc += *n as u64;
            cut = j + 1;
            if acc >= half {
                break;
            }
        }
        cut = cut.clamp(1, b.colors.len() - 1);
        let right = b.colors.split_off(cut);
        buckets.push(Bucket { colors: b.colors });
        buckets.push(Bucket { colors: right });
    }
    let mut pal: Vec<[u8; 3]> = buckets.iter().map(|b| b.mean()).collect();
    pal.sort();
    pal.dedup();
    pal
}

fn dist2(a: [u8; 3], b: [u8; 3]) -> i32 {
    (0..3).map(|k| { let d = a[k] as i32 - b[k] as i32; d * d }).sum()
}

fn nearest(pal: &[[u8; 3]], c: [u8; 3]) -> usize {
    let mut best = (i32::MAX, 0);
    for (i, p) in pal.iter().enumerate() {
        let d = dist2(*p, c);
        if d < best.0 {
            best = (d, i);
        }
    }
    best.1
}

fn visible_counts(images: &[&RgbaImage], alpha_min: u8) -> HashMap<[u8; 3], u32> {
    let mut m = HashMap::new();
    for im in images {
        for p in im.data.chunks_exact(4) {
            if p[3] >= alpha_min {
                *m.entry([p[0], p[1], p[2]]).or_insert(0) += 1;
            }
        }
    }
    m
}

pub type Quantized = (BTreeMap<u8, PalEntry>, Vec<Vec<Vec<u8>>>);

/// Retourne (entrees {indice: (Y, Cr, Cb, alpha)}, lignes d'indices de chaque image).
pub fn quantize_images(images: &[&RgbaImage], rgb_colors: usize, alpha_levels: &[u8], alpha_min: u8, opaque_ok: bool) -> Result<Quantized, String> {
    if rgb_colors * alpha_levels.len() + 1 > MAX_ENTRIES {
        return Err("trop de couleurs x paliers d'alpha pour une palette Blu-ray".into());
    }
    if opaque_ok && images.iter().all(|im| im.data.chunks_exact(4).all(|p| p[3] < alpha_min || p[3] >= 250)) {
        return Ok(quantize_opaque(images, alpha_min));
    }
    let pal = median_cut(&visible_counts(images, alpha_min), rgb_colors);
    let mut cache: HashMap<[u8; 3], usize> = HashMap::new();
    let mut used: HashMap<(usize, usize), u8> = HashMap::new();
    let mut order: Vec<(usize, usize)> = Vec::new();
    let mut out = Vec::new();
    for im in images {
        let mut rows = Vec::with_capacity(im.height as usize);
        for y in 0..im.height {
            let mut row = Vec::with_capacity(im.width as usize);
            for x in 0..im.width {
                let [r, g, b, a] = im.pixel(x, y);
                if a < alpha_min {
                    row.push(0);
                    continue;
                }
                let ci = *cache.entry([r, g, b]).or_insert_with(|| nearest(&pal, [r, g, b]));
                // palier d'alpha le plus proche (a egalite : le premier, comme min() en Python)
                let ai = (0..alpha_levels.len()).min_by_key(|&k| ((alpha_levels[k] as i32 - a as i32).abs(), k)).unwrap();
                let next = (used.len() + 1) as u8;
                let idx = *used.entry((ci, ai)).or_insert_with(|| { order.push((ci, ai)); next });
                row.push(idx);
            }
            rows.push(row);
        }
        out.push(rows);
    }
    let mut entries = BTreeMap::new();
    entries.insert(0u8, (16, 128, 128, 0));
    for (ci, ai) in order {
        let (y, cr, cb) = rgb_to_ycrcb(pal[ci][0], pal[ci][1], pal[ci][2]);
        entries.insert(used[&(ci, ai)], (y, cr, cb, alpha_levels[ai]));
    }
    Ok((entries, out))
}

/// Images sans transparence partielle (vignettes) : jusqu'a 253 couleurs communes et tramage de Floyd-Steinberg.
/// Indice 0 = transparent, indices 1..n = couleurs opaques.
fn quantize_opaque(images: &[&RgbaImage], alpha_min: u8) -> Quantized {
    let mut entries = BTreeMap::new();
    entries.insert(0u8, (16, 128, 128, 0));
    let pal = median_cut(&visible_counts(images, alpha_min), MAX_ENTRIES - 1);
    for (k, c) in pal.iter().enumerate() {
        let (y, cr, cb) = rgb_to_ycrcb(c[0], c[1], c[2]);
        entries.insert((k + 1) as u8, (y, cr, cb, 255));
    }
    let mut out = Vec::new();
    for im in images {
        let (w, h) = (im.width as usize, im.height as usize);
        // erreurs de diffusion, deux lignes (courante et suivante), 3 canaux, x16
        let mut err_cur = vec![[0i32; 3]; w + 2];
        let mut err_next = vec![[0i32; 3]; w + 2];
        let mut rows = Vec::with_capacity(h);
        for y in 0..h {
            let mut row = vec![0u8; w];
            for x in 0..w {
                let p = im.pixel(x as u32, y as u32);
                if p[3] < alpha_min || pal.is_empty() {
                    continue; // transparent : indice 0, pas de diffusion
                }
                let mut c = [0u8; 3];
                for k in 0..3 {
                    c[k] = (p[k] as i32 + err_cur[x + 1][k] / 16).clamp(0, 255) as u8;
                }
                let i = nearest(&pal, c);
                row[x] = (i + 1) as u8;
                for k in 0..3 {
                    let e = c[k] as i32 - pal[i][k] as i32;
                    err_cur[x + 2][k] += e * 7;
                    err_next[x][k] += e * 3;
                    err_next[x + 1][k] += e * 5;
                    err_next[x + 2][k] += e;
                }
            }
            rows.push(row);
            err_cur = std::mem::replace(&mut err_next, vec![[0i32; 3]; w + 2]);
        }
        out.push(rows);
    }
    (entries, out)
}
