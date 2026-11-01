//! Jaquette (feuille a deplier d'un boitier) : dos, tranche et face, avec fond perdu. Modele, enregistrement dans le projet,
//! mise en page en pixels (pour le rendu) et export PDF. Le dessin des textes et des images est fait par `bdauthor-render`.
use crate::design::Align;
use crate::json::Value;
use crate::palette::RgbaImage;
use crate::pdf;
use crate::project::Decor;
use std::path::{Path, PathBuf};

/// Contenu d'un element de jaquette.
#[derive(Debug, Clone, PartialEq)]
pub enum CoverKind {
    Text { text: String, size_pt: f64, color: [u8; 3], align: Align, font: String },
    Image { path: String },
}

/// Element de jaquette. Position et taille en millimetres depuis le coin haut gauche de la jaquette *rognee* (le fond perdu a
/// donc des coordonnees negatives) ; `rot` : rotation du cadre (0, 90 ou 270 degres, sens horaire).
#[derive(Debug, Clone, PartialEq)]
pub struct CoverItem {
    pub kind: CoverKind,
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
    pub rot: u16,
    pub name: String,
}

impl CoverItem {
    pub fn text(text: &str, x: f64, y: f64, w: f64, h: f64, size_pt: f64) -> CoverItem {
        CoverItem { kind: CoverKind::Text { text: text.into(), size_pt, color: [255, 255, 255], align: Align::Center, font: String::new() }, x, y, w, h, rot: 0, name: String::new() }
    }
    pub fn image(path: &str, x: f64, y: f64, w: f64, h: f64) -> CoverItem {
        CoverItem { kind: CoverKind::Image { path: path.into() }, x, y, w, h, rot: 0, name: String::new() }
    }
}

/// Zone de la jaquette.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Zone { Back, Spine, Front }

#[derive(Debug, Clone, PartialEq)]
pub struct Cover {
    /// largeur du dos et de la face (mm)
    pub face_mm: f64,
    pub height_mm: f64,
    pub spine_mm: f64,
    pub bleed_mm: f64,
    /// couleurs de fond du dos, de la tranche et de la face
    pub colors: [[u8; 3]; 3],
    /// image couvrant toute la feuille (fond perdu compris), facultative
    pub bg_image: String,
    /// traits de coupe et de pliage autour de la feuille
    pub marks: bool,
    pub dpi: u32,
    pub items: Vec<CoverItem>,
}

impl Default for Cover {
    fn default() -> Cover {
        Cover { face_mm: 129.0, height_mm: 148.0, spine_mm: 14.0, bleed_mm: 3.0, colors: [[24, 28, 40], [40, 60, 110], [24, 28, 40]], bg_image: String::new(), marks: false, dpi: 300, items: vec![] }
    }
}

/// Formats usuels : (nom, largeur du dos et de la face, hauteur, tranche), en mm.
pub const FORMATS: [(&str, f64, f64, f64); 4] = [
    ("Blu-ray standard (tranche 14 mm)", 129.0, 148.0, 14.0),
    ("Blu-ray fin (tranche 11 mm)", 129.0, 148.0, 11.0),
    ("DVD standard (tranche 14 mm)", 129.0, 183.0, 14.0),
    ("DVD fin (tranche 7 mm)", 129.0, 183.0, 7.0),
];

/// Resultat de la mise en page en pixels.
#[derive(Debug, Clone)]
pub struct CoverLayout {
    /// taille de l'image de la feuille entiere (fond perdu compris)
    pub size: (u32, u32),
    /// zones colorees (x, y, w, h) en pixels, etendues jusqu'au bord du fond perdu
    pub fills: Vec<((i32, i32, i32, i32), [u8; 3])>,
    pub bg_image: Option<PathBuf>,
    /// elements : cadre et contenu en pixels, rotation
    pub items: Vec<(Decor, u16)>,
}

fn mm_px(mm: f64, dpi: u32) -> i32 { (mm / 25.4 * dpi as f64).round() as i32 }

fn color_hex(c: [u8; 3]) -> Value { Value::Str(format!("#{:02x}{:02x}{:02x}", c[0], c[1], c[2])) }
fn color_from(v: Option<&Value>, d: [u8; 3]) -> [u8; 3] {
    let Some(s) = v.and_then(Value::as_str) else { return d };
    let s = s.trim_start_matches('#');
    match u32::from_str_radix(s, 16) { Ok(n) if s.len() == 6 => [(n >> 16) as u8, (n >> 8) as u8, n as u8], _ => d }
}

impl Cover {
    /// Taille de la feuille rognee (mm).
    pub fn trim_mm(&self) -> (f64, f64) { (2.0 * self.face_mm + self.spine_mm, self.height_mm) }
    /// Taille de la feuille avec fond perdu (mm).
    pub fn sheet_mm(&self) -> (f64, f64) { let t = self.trim_mm(); (t.0 + 2.0 * self.bleed_mm, t.1 + 2.0 * self.bleed_mm) }
    /// Rectangle (x, y, largeur, hauteur) de chaque zone, en mm depuis le coin de la feuille rognee.
    pub fn zone_mm(&self, z: Zone) -> (f64, f64, f64, f64) {
        match z {
            Zone::Back => (0.0, 0.0, self.face_mm, self.height_mm),
            Zone::Spine => (self.face_mm, 0.0, self.spine_mm, self.height_mm),
            Zone::Front => (self.face_mm + self.spine_mm, 0.0, self.face_mm, self.height_mm),
        }
    }
    pub fn is_default(&self) -> bool { *self == Cover::default() }

    /// Ajuste les dimensions a un format usuel (rang dans `FORMATS`).
    pub fn set_format(&mut self, k: usize) {
        if let Some(f) = FORMATS.get(k) { self.face_mm = f.1; self.height_mm = f.2; self.spine_mm = f.3; }
    }

    /// Mise en page en pixels a `dpi` points par pouce ; `base` : dossier du projet (chemins relatifs).
    pub fn layout(&self, dpi: u32, base: &Path) -> CoverLayout {
        let dpi = dpi.max(30);
        let (sw, sh) = self.sheet_mm();
        let size = (mm_px(sw, dpi).max(1) as u32, mm_px(sh, dpi).max(1) as u32);
        let b = mm_px(self.bleed_mm, dpi);
        let (fw, sp) = (mm_px(self.face_mm, dpi), mm_px(self.spine_mm, dpi));
        let (w, h) = (size.0 as i32, size.1 as i32);
        // les zones d'extremite debordent sur le fond perdu ; les separations tombent au pixel pres sans trou
        let fills = vec![
            ((0, 0, b + fw, h), self.colors[0]),
            ((b + fw, 0, sp, h), self.colors[1]),
            ((b + fw + sp, 0, w - (b + fw + sp), h), self.colors[2]),
        ];
        let abs = |p: &str| -> PathBuf { let q = Path::new(p); if q.is_absolute() { q.to_path_buf() } else { base.join(q) } };
        let items = self.items.iter().filter_map(|it| {
            let (x, y) = (b + mm_px(it.x, dpi), b + mm_px(it.y, dpi));
            let (iw, ih) = (mm_px(it.w, dpi).max(1), mm_px(it.h, dpi).max(1));
            let d = match &it.kind {
                CoverKind::Text { text, size_pt, color, align, font } => {
                    if text.is_empty() { return None; }
                    let font = if font.is_empty() || Path::new(font).is_absolute() { font.clone() } else if base.join(font).is_file() { base.join(font).to_string_lossy().into_owned() } else { font.clone() };
                    Decor::Text { text: text.clone(), x, y, w: iw, h: ih, size: ((size_pt / 72.0 * dpi as f64).round() as i32).max(1), color: *color, align: *align, font }
                }
                CoverKind::Image { path } => {
                    if path.is_empty() { return None; }
                    Decor::Image { path: abs(path), x, y, w: iw, h: ih }
                }
            };
            Some((d, it.rot))
        }).collect();
        CoverLayout { size, fills, bg_image: if self.bg_image.is_empty() { None } else { Some(abs(&self.bg_image)) }, items }
    }

    /// PDF de la jaquette a partir de son image `sheet` (feuille entiere, fond perdu compris). Avec les traits de coupe, la page
    /// est agrandie de 10 mm de chaque cote : traits de coupe aux quatre coins de la jaquette rognee, traits de pliage de la tranche.
    pub fn to_pdf(&self, sheet: &RgbaImage, title: &str) -> Vec<u8> {
        let (sw, sh) = self.sheet_mm();
        if !self.marks {
            return pdf::image_pdf((sw, sh), sheet, (0.0, 0.0, sw, sh), &[], title);
        }
        let m = 10.0;
        let (tw, th) = self.trim_mm();
        let b = self.bleed_mm;
        let (x0, y0, x1, y1) = (m + b, m + b, m + b + tw, m + b + th); // jaquette rognee dans la page
        let t = 6.0; // longueur des traits
        let gap = 1.5; // ecart entre le trait et le coin (pour ne pas couvrir le fond perdu)
        let mut lines = vec![];
        for &(x, y, dx, dy) in &[(x0, y0, -1.0, -1.0), (x1, y0, 1.0, -1.0), (x0, y1, -1.0, 1.0), (x1, y1, 1.0, 1.0)] {
            lines.push((x + dx * gap, y, x + dx * (gap + t), y));
            lines.push((x, y + dy * gap, x, y + dy * (gap + t)));
        }
        // pliages : bords de la tranche, en haut et en bas
        for fx in [x0 + self.face_mm, x0 + self.face_mm + self.spine_mm] {
            lines.push((fx, y0 - gap, fx, y0 - gap - t));
            lines.push((fx, y1 + gap, fx, y1 + gap + t));
        }
        pdf::image_pdf((sw + 2.0 * m, sh + 2.0 * m), sheet, (m, m, sw, sh), &lines, title)
    }

    // ------------------------------------------------------------------ JSON
    pub fn to_value(&self) -> Option<Value> {
        if self.is_default() { return None; }
        let n = |x: f64| Value::Num(x);
        let mut o: Vec<(String, Value)> = vec![
            ("face_mm".into(), n(self.face_mm)), ("hauteur_mm".into(), n(self.height_mm)), ("tranche_mm".into(), n(self.spine_mm)), ("fond_perdu_mm".into(), n(self.bleed_mm)),
            ("couleurs".into(), Value::Obj(vec![("dos".into(), color_hex(self.colors[0])), ("tranche".into(), color_hex(self.colors[1])), ("face".into(), color_hex(self.colors[2]))])),
            ("traits_coupe".into(), Value::Bool(self.marks)), ("dpi".into(), n(self.dpi as f64)),
        ];
        if !self.bg_image.is_empty() { o.push(("image_fond".into(), Value::Str(self.bg_image.clone()))); }
        let items = self.items.iter().map(|it| {
            let mut e: Vec<(String, Value)> = Vec::new();
            match &it.kind {
                CoverKind::Text { text, size_pt, color, align, font } => {
                    e.push(("type".into(), Value::Str("texte".into())));
                    e.push(("texte".into(), Value::Str(text.clone())));
                    e.push(("taille_pt".into(), n(*size_pt)));
                    e.push(("couleur".into(), color_hex(*color)));
                    e.push(("alignement".into(), Value::Str(align.name().into())));
                    if !font.is_empty() { e.push(("police".into(), Value::Str(font.clone()))); }
                }
                CoverKind::Image { path } => {
                    e.push(("type".into(), Value::Str("image".into())));
                    e.push(("fichier".into(), Value::Str(path.clone())));
                }
            }
            for (k, v) in [("x", it.x), ("y", it.y), ("w", it.w), ("h", it.h)] { e.push((k.into(), n((v * 100.0).round() / 100.0))); }
            if it.rot != 0 { e.push(("rotation".into(), n(it.rot as f64))); }
            if !it.name.is_empty() { e.push(("nom".into(), Value::Str(it.name.clone()))); }
            Value::Obj(e)
        }).collect();
        o.push(("elements".into(), Value::Arr(items)));
        Some(Value::Obj(o))
    }

    pub fn from_value(v: Option<&Value>) -> Result<Cover, String> {
        let mut c = Cover::default();
        let Some(v) = v else { return Ok(c) };
        let num = |k: &str, d: f64| v.get(k).and_then(Value::as_f64).filter(|x| x.is_finite()).unwrap_or(d);
        c.face_mm = num("face_mm", c.face_mm).clamp(20.0, 400.0);
        c.height_mm = num("hauteur_mm", c.height_mm).clamp(20.0, 400.0);
        c.spine_mm = num("tranche_mm", c.spine_mm).clamp(0.0, 100.0);
        c.bleed_mm = num("fond_perdu_mm", c.bleed_mm).clamp(0.0, 20.0);
        c.dpi = num("dpi", c.dpi as f64).round().clamp(72.0, 600.0) as u32;
        c.marks = v.get("traits_coupe").and_then(Value::as_bool).unwrap_or(false);
        if let Some(cs) = v.get("couleurs") {
            for (i, k) in ["dos", "tranche", "face"].iter().enumerate() { c.colors[i] = color_from(cs.get(k), c.colors[i]); }
        }
        c.bg_image = v.get("image_fond").and_then(Value::as_str).unwrap_or("").to_string();
        if let Some(a) = v.get("elements").and_then(Value::as_array) {
            for (k, e) in a.iter().enumerate() {
                let f = |key: &str| e.get(key).and_then(Value::as_f64).unwrap_or(0.0);
                let kind = match e.get("type").and_then(Value::as_str) {
                    Some("texte") => CoverKind::Text {
                        text: e.get("texte").and_then(Value::as_str).unwrap_or("").to_string(),
                        size_pt: e.get("taille_pt").and_then(Value::as_f64).unwrap_or(24.0).clamp(1.0, 600.0),
                        color: color_from(e.get("couleur"), [255, 255, 255]),
                        align: e.get("alignement").and_then(Value::as_str).and_then(Align::from_name).unwrap_or(Align::Center),
                        font: e.get("police").and_then(Value::as_str).unwrap_or("").to_string(),
                    },
                    Some("image") => CoverKind::Image { path: e.get("fichier").and_then(Value::as_str).ok_or_else(|| format!("jaquette, élément {} : « fichier » manquant", k + 1))?.to_string() },
                    t => return Err(format!("jaquette, élément {} : type {:?} inconnu", k + 1, t)),
                };
                let rot = match f("rotation").round() as i64 { 90 => 90, 270 | -90 => 270, _ => 0 };
                c.items.push(CoverItem { kind, x: f("x"), y: f("y"), w: f("w").max(1.0), h: f("h").max(1.0), rot, name: e.get("nom").and_then(Value::as_str).unwrap_or("").to_string() });
            }
        }
        Ok(c)
    }
}

impl CoverLayout {
    /// Feuille unie : les trois zones colorees (sans image de fond ni elements).
    pub fn background(&self) -> RgbaImage {
        let mut im = RgbaImage::new(self.size.0, self.size.1);
        for ((x, y, w, h), c) in &self.fills {
            for yy in (*y).max(0)..(*y + *h).min(self.size.1 as i32) {
                for xx in (*x).max(0)..(*x + *w).min(self.size.0 as i32) { im.set(xx as u32, yy as u32, [c[0], c[1], c[2], 255]); }
            }
        }
        im
    }
}

/// Morceau (x, y, w, h) d'une image, borne a ses limites.
pub fn crop(img: &RgbaImage, x: i32, y: i32, w: i32, h: i32) -> RgbaImage {
    let (x0, y0) = (x.clamp(0, img.width as i32), y.clamp(0, img.height as i32));
    let (x1, y1) = ((x + w).clamp(0, img.width as i32), (y + h).clamp(0, img.height as i32));
    let mut out = RgbaImage::new((x1 - x0).max(0) as u32, (y1 - y0).max(0) as u32);
    for yy in y0..y1 { for xx in x0..x1 { out.set((xx - x0) as u32, (yy - y0) as u32, img.pixel(xx as u32, yy as u32)); } }
    out
}

/// Tourne l'image de `q` quarts de tour dans le sens horaire (q modulo 4).
pub fn rotate_quarters(img: &RgbaImage, q: u32) -> RgbaImage {
    let (w, h) = (img.width, img.height);
    match q % 4 {
        0 => img.clone(),
        1 => { let mut o = RgbaImage::new(h, w); for y in 0..h { for x in 0..w { o.set(h - 1 - y, x, img.pixel(x, y)); } } o }
        2 => { let mut o = RgbaImage::new(w, h); for y in 0..h { for x in 0..w { o.set(w - 1 - x, h - 1 - y, img.pixel(x, y)); } } o }
        _ => { let mut o = RgbaImage::new(h, w); for y in 0..h { for x in 0..w { o.set(y, w - 1 - x, img.pixel(x, y)); } } o }
    }
}
