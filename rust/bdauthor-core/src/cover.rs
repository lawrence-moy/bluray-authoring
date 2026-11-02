//! Jaquette (feuille a deplier d'un boitier) : dos, tranche et face, avec fond perdu. Modele, enregistrement dans le projet,
//! mise en page en pixels (pour le rendu) et export PDF. Le dessin des textes et des images est fait par `bdauthor-render`.
use crate::design::Align;
use crate::json::Value;
use crate::palette::RgbaImage;
use crate::pdf;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// Alignement vertical du texte dans son cadre.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VAlign { Top, Middle, Bottom }

impl VAlign {
    pub fn name(self) -> &'static str { match self { VAlign::Top => "haut", VAlign::Middle => "milieu", VAlign::Bottom => "bas" } }
    pub fn from_name(s: &str) -> Option<VAlign> { match s { "haut" => Some(VAlign::Top), "milieu" => Some(VAlign::Middle), "bas" => Some(VAlign::Bottom), _ => None } }
}

/// Effets et mise en forme d'un texte (valeurs par defaut = texte simple centre verticalement).
#[derive(Debug, Clone, PartialEq)]
pub struct TextFx {
    /// epaisseur du contour (mm), 0 : aucun
    pub outline_mm: f64,
    pub outline_color: [u8; 3],
    /// decalage de l'ombre (mm, vers le bas et la droite), 0 : aucune
    pub shadow_mm: f64,
    pub shadow_color: [u8; 3],
    /// interligne (1 = normal)
    pub spacing: f64,
    pub valign: VAlign,
    /// reduit la taille du texte pour qu'il tienne dans le cadre (la taille indiquee est un maximum)
    pub autofit: bool,
}

impl Default for TextFx {
    fn default() -> TextFx {
        TextFx { outline_mm: 0.0, outline_color: [0, 0, 0], shadow_mm: 0.0, shadow_color: [0, 0, 0], spacing: 1.0, valign: VAlign::Middle, autofit: false }
    }
}

/// Forme geometrique.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShapeKind { Rect, Ellipse, Line }

impl ShapeKind {
    pub fn name(self) -> &'static str { match self { ShapeKind::Rect => "rectangle", ShapeKind::Ellipse => "ellipse", ShapeKind::Line => "ligne" } }
    pub fn from_name(s: &str) -> Option<ShapeKind> { match s { "rectangle" => Some(ShapeKind::Rect), "ellipse" => Some(ShapeKind::Ellipse), "ligne" => Some(ShapeKind::Line), _ => None } }
    pub fn label(self) -> &'static str { match self { ShapeKind::Rect => "Rectangle", ShapeKind::Ellipse => "Ellipse", ShapeKind::Line => "Ligne" } }
}

/// Contenu d'un element de jaquette.
#[derive(Debug, Clone, PartialEq)]
pub enum CoverKind {
    Text { text: String, size_pt: f64, color: [u8; 3], align: Align, font: String, fx: TextFx },
    Image { path: String },
    /// rectangle (coins arrondis de rayon `radius_mm`), ellipse inscrite dans le cadre, ou trait horizontal au milieu du cadre
    /// (tourne par `rot`) ; `fill` et `stroke` : couleurs, `None` = absent ; la bordure est a l'interieur du cadre
    Shape { shape: ShapeKind, fill: Option<[u8; 3]>, stroke: Option<[u8; 3]>, stroke_mm: f64, radius_mm: f64 },
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
    /// numero du calque (0 = le plus en arriere)
    pub layer: usize,
}

impl CoverItem {
    pub fn text(text: &str, x: f64, y: f64, w: f64, h: f64, size_pt: f64) -> CoverItem {
        CoverItem { kind: CoverKind::Text { text: text.into(), size_pt, color: [255, 255, 255], align: Align::Center, font: String::new(), fx: TextFx::default() }, x, y, w, h, rot: 0, name: String::new(), layer: 0 }
    }
    /// Rectangle plein blanc, ellipse pleine blanche, ou trait blanc de 0,5 mm.
    pub fn shape(shape: ShapeKind, x: f64, y: f64, w: f64, h: f64) -> CoverItem {
        let (fill, stroke) = if shape == ShapeKind::Line { (None, Some([255, 255, 255])) } else { (Some([255, 255, 255]), None) };
        CoverItem { kind: CoverKind::Shape { shape, fill, stroke, stroke_mm: 0.5, radius_mm: 0.0 }, x, y, w, h, rot: 0, name: String::new(), layer: 0 }
    }
    pub fn image(path: &str, x: f64, y: f64, w: f64, h: f64) -> CoverItem {
        CoverItem { kind: CoverKind::Image { path: path.into() }, x, y, w, h, rot: 0, name: String::new(), layer: 0 }
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
    /// calques, du plus en arriere au plus en avant ; vide : un seul calque implicite
    pub layers: Vec<CoverLayer>,
}

impl Default for Cover {
    fn default() -> Cover {
        Cover { face_mm: 129.0, height_mm: 148.0, spine_mm: 14.0, bleed_mm: 3.0, colors: [[24, 28, 40], [40, 60, 110], [24, 28, 40]], bg_image: String::new(), marks: false, dpi: 300, items: vec![], layers: vec![] }
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
    pub items: Vec<(CoverDecor, u16)>,
}

/// Element pose, en pixels (cadre et contenu).
#[derive(Debug, Clone, PartialEq)]
pub enum CoverDecor {
    Text { text: String, x: i32, y: i32, w: i32, h: i32, size: i32, color: [u8; 3], align: Align, font: String, valign: VAlign, spacing: f32, outline: i32, outline_color: [u8; 3], shadow: i32, shadow_color: [u8; 3], autofit: bool },
    Image { path: PathBuf, x: i32, y: i32, w: i32, h: i32 },
    Shape { shape: ShapeKind, x: i32, y: i32, w: i32, h: i32, fill: Option<[u8; 3]>, stroke: Option<[u8; 3]>, stroke_px: i32, radius_px: i32 },
}

impl CoverDecor {
    /// Cadre (x, y, largeur, hauteur).
    pub fn rect(&self) -> (i32, i32, i32, i32) {
        match self { CoverDecor::Text { x, y, w, h, .. } | CoverDecor::Image { x, y, w, h, .. } | CoverDecor::Shape { x, y, w, h, .. } => (*x, *y, *w, *h) }
    }
    /// Le meme element dans un autre cadre.
    pub fn with_rect(mut self, nx: i32, ny: i32, nw: i32, nh: i32) -> CoverDecor {
        match &mut self { CoverDecor::Text { x, y, w, h, .. } | CoverDecor::Image { x, y, w, h, .. } | CoverDecor::Shape { x, y, w, h, .. } => { *x = nx; *y = ny; *w = nw; *h = nh; } }
        self
    }
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
    pub fn layout(&self, dpi: u32, base: &Path) -> CoverLayout { self.layout_with(dpi, base, &BTreeMap::new()) }

    /// Comme `layout`, en remplacant dans les textes les champs `{nom}` par leur valeur dans `vars` (voir `FIELDS`).
    pub fn layout_with(&self, dpi: u32, base: &Path, vars: &BTreeMap<String, String>) -> CoverLayout {
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
        let items = draw_order(&self.items, &self.layers).into_iter().map(|i| &self.items[i]).filter_map(|it| {
            let (x, y) = (b + mm_px(it.x, dpi), b + mm_px(it.y, dpi));
            let (iw, ih) = (mm_px(it.w, dpi).max(1), mm_px(it.h, dpi).max(1));
            let d = match &it.kind {
                CoverKind::Text { text, size_pt, color, align, font, fx } => {
                    let text = apply_vars(text, vars);
                    if text.is_empty() { return None; }
                    let font = if font.is_empty() || Path::new(font).is_absolute() { font.clone() } else if base.join(font).is_file() { base.join(font).to_string_lossy().into_owned() } else { font.clone() };
                    CoverDecor::Text { text, x, y, w: iw, h: ih, size: ((size_pt / 72.0 * dpi as f64).round() as i32).max(1), color: *color, align: *align, font, valign: fx.valign,
                        spacing: fx.spacing as f32, outline: if fx.outline_mm > 0.0 { mm_px(fx.outline_mm, dpi).max(1) } else { 0 }, outline_color: fx.outline_color,
                        shadow: if fx.shadow_mm > 0.0 { mm_px(fx.shadow_mm, dpi).max(1) } else { 0 }, shadow_color: fx.shadow_color, autofit: fx.autofit }
                }
                CoverKind::Image { path } => {
                    if path.is_empty() { return None; }
                    CoverDecor::Image { path: abs(path), x, y, w: iw, h: ih }
                }
                CoverKind::Shape { shape, fill, stroke, stroke_mm, radius_mm } => {
                    CoverDecor::Shape { shape: *shape, x, y, w: iw, h: ih, fill: *fill, stroke: *stroke, stroke_px: if stroke.is_some() { mm_px(*stroke_mm, dpi).max(1) } else { 0 }, radius_px: mm_px(*radius_mm, dpi).max(0) }
                }
            };
            Some((d, it.rot))
        }).collect();
        CoverLayout { size, fills, bg_image: if self.bg_image.is_empty() { None } else { Some(abs(&self.bg_image)) }, items }
    }

    /// PDF de la jaquette a partir de son image `sheet` (feuille entiere, fond perdu compris). Avec les traits de coupe, la page
    /// est agrandie de 10 mm de chaque cote : traits de coupe aux quatre coins de la jaquette rognee, traits de pliage de la tranche.
    /// Traits de coupe aux quatre coins de la jaquette rognee dont le coin haut gauche est en (x0, y0) sur la page, et traits de
    /// pliage de la tranche en haut et en bas (mm depuis le coin haut gauche de la page).
    fn cut_lines(&self, x0: f64, y0: f64) -> Vec<(f64, f64, f64, f64)> {
        let (tw, th) = self.trim_mm();
        let (x1, y1) = (x0 + tw, y0 + th);
        let t = 6.0; // longueur des traits
        let gap = 1.5; // ecart entre le trait et le coin (pour ne pas couvrir le fond perdu)
        let mut lines = vec![];
        for &(x, y, dx, dy) in &[(x0, y0, -1.0, -1.0), (x1, y0, 1.0, -1.0), (x0, y1, -1.0, 1.0), (x1, y1, 1.0, 1.0)] {
            lines.push((x + dx * gap, y, x + dx * (gap + t), y));
            lines.push((x, y + dy * gap, x, y + dy * (gap + t)));
        }
        for fx in [x0 + self.face_mm, x0 + self.face_mm + self.spine_mm] {
            lines.push((fx, y0 - gap, fx, y0 - gap - t));
            lines.push((fx, y1 + gap, fx, y1 + gap + t));
        }
        lines
    }

    /// PDF sur une page A4 (portrait ou paysage selon la place), jaquette a sa taille reelle, centree, avec traits de coupe et de
    /// pliage : a imprimer a 100 % (sans « ajuster a la page ») puis a decouper.
    pub fn to_pdf_a4(&self, sheet: &RgbaImage, title: &str) -> Result<Vec<u8>, String> {
        let (sw, sh) = self.sheet_mm();
        let (tw, th) = self.trim_mm();
        let page = a4_page(tw, th, "La jaquette")?;
        let (x0, y0) = ((page.0 - tw) / 2.0, (page.1 - th) / 2.0);
        let b = self.bleed_mm;
        Ok(pdf::image_pdf(page, sheet, (x0 - b, y0 - b, sw, sh), &self.cut_lines(x0, y0), title))
    }

    pub fn to_pdf(&self, sheet: &RgbaImage, title: &str) -> Vec<u8> {
        let (sw, sh) = self.sheet_mm();
        if !self.marks {
            return pdf::image_pdf((sw, sh), sheet, (0.0, 0.0, sw, sh), &[], title);
        }
        let m = 10.0;
        let b = self.bleed_mm;
        let lines = self.cut_lines(m + b, m + b);
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
        let items: Vec<Value> = self.items.iter().map(item_to_value).collect();
        if !self.layers.is_empty() { o.push(("calques".into(), layers_to_value(&self.layers))); }
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
            for (k, e) in a.iter().enumerate() { c.items.push(item_from_value(e, k, "jaquette")?); }
        }
        c.layers = layers_from_value(v.get("calques"));
        clamp_layers(&mut c.items, c.layers.len());
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

// ------------------------------------------------------------------ champs automatiques
/// Champs remplis depuis le projet : (nom, description).
pub const FIELDS: [(&str, &str); 6] = [
    ("titre", "titre du disque"),
    ("liste_videos", "une ligne par video : numero, titre et duree"),
    ("nb_videos", "nombre de videos"),
    ("duree_totale", "duree totale des videos"),
    ("nb_chapitres", "nombre total de chapitres manuels"),
    ("annee", "annee en cours"),
];

/// Remplace `{nom}` par sa valeur ; un champ inconnu reste tel quel.
pub fn apply_vars(text: &str, vars: &BTreeMap<String, String>) -> String {
    let mut out = text.to_string();
    for (k, v) in vars { out = out.replace(&format!("{{{k}}}"), v); }
    out
}

/// Duree en « h:mm:ss » (ou « m:ss » sous une heure).
pub fn clock(seconds: f64) -> String {
    let s = seconds.round().max(0.0) as u64;
    if s >= 3600 { format!("{}:{:02}:{:02}", s / 3600, s / 60 % 60, s % 60) } else { format!("{}:{:02}", s / 60, s % 60) }
}

// ------------------------------------------------------------------ document a feuille unique
/// Ce que l'editeur de feuille (jaquette, etiquette) sait faire d'un document.
pub trait SheetDoc {
    fn sheet_mm(&self) -> (f64, f64);
    fn bleed_mm(&self) -> f64;
    fn dpi(&self) -> u32;
    fn items(&self) -> &Vec<CoverItem>;
    fn items_mut(&mut self) -> &mut Vec<CoverItem>;
    fn layers(&self) -> &Vec<CoverLayer>;
    /// Lignes d'aimantation : abscisses et ordonnees (mm) des bords, pliages, centres et marges de securite.
    fn snap_lines(&self) -> (Vec<f64>, Vec<f64>);
    /// Zone de securite (3 mm a l'interieur) : rectangles (x, y, w, h) et cercles (cx, cy, r), en mm.
    fn safe_guides(&self) -> (Vec<(f64, f64, f64, f64)>, Vec<(f64, f64, f64)>);
    fn layers_mut(&mut self) -> &mut Vec<CoverLayer>;
    /// Zones ou « Placer sur » met un element : (nom, rectangle en mm depuis le coin de la feuille rognee)
    fn zones(&self) -> Vec<(&'static str, (f64, f64, f64, f64))>;
    fn layout_with(&self, dpi: u32, base: &Path, vars: &BTreeMap<String, String>) -> CoverLayout;
    fn to_pdf(&self, sheet: &RgbaImage, title: &str) -> Vec<u8>;
    /// PDF A4 a taille reelle, centre, avec reperes de decoupe ; erreur si le format fini ne tient pas sur un A4.
    fn to_pdf_a4(&self, sheet: &RgbaImage, title: &str) -> Result<Vec<u8>, String>;
    /// Reperes dessines sur l'apercu : rectangles (x, y, w, h), traits (x1, y1, x2, y2) et cercles (cx, cy, r), en mm depuis le coin rogne
    fn guides(&self) -> (Vec<(f64, f64, f64, f64)>, Vec<(f64, f64, f64, f64)>, Vec<(f64, f64, f64)>);
}

impl SheetDoc for Cover {
    fn sheet_mm(&self) -> (f64, f64) { Cover::sheet_mm(self) }
    fn bleed_mm(&self) -> f64 { self.bleed_mm }
    fn dpi(&self) -> u32 { self.dpi }
    fn items(&self) -> &Vec<CoverItem> { &self.items }
    fn items_mut(&mut self) -> &mut Vec<CoverItem> { &mut self.items }
    fn layers(&self) -> &Vec<CoverLayer> { &self.layers }
    fn layers_mut(&mut self) -> &mut Vec<CoverLayer> { &mut self.layers }
    fn snap_lines(&self) -> (Vec<f64>, Vec<f64>) {
        let (tw, th) = self.trim_mm();
        let (f, sp, b, m) = (self.face_mm, self.spine_mm, self.bleed_mm, SAFE_MM);
        let mut xs = vec![-b, 0.0, f / 2.0, f, f + sp / 2.0, f + sp, f + sp + f / 2.0, tw, tw + b, m, tw - m];
        if f > 2.0 * m { xs.extend([f - m, f + sp + m]); }
        (xs, vec![-b, 0.0, th / 2.0, th, th + b, m, th - m])
    }
    fn safe_guides(&self) -> (Vec<(f64, f64, f64, f64)>, Vec<(f64, f64, f64)>) {
        let (_, th) = self.trim_mm();
        let (f, sp, m) = (self.face_mm, self.spine_mm, SAFE_MM);
        let mut r = vec![(m, m, f - 2.0 * m, th - 2.0 * m), (f + sp + m, m, f - 2.0 * m, th - 2.0 * m)];
        if sp > 2.0 * m { r.push((f + m, m, sp - 2.0 * m, th - 2.0 * m)); }
        (r, vec![])
    }
    fn zones(&self) -> Vec<(&'static str, (f64, f64, f64, f64))> {
        vec![("le dos", self.zone_mm(Zone::Back)), ("la tranche", self.zone_mm(Zone::Spine)), ("la face", self.zone_mm(Zone::Front))]
    }
    fn layout_with(&self, dpi: u32, base: &Path, vars: &BTreeMap<String, String>) -> CoverLayout { Cover::layout_with(self, dpi, base, vars) }
    fn to_pdf(&self, sheet: &RgbaImage, title: &str) -> Vec<u8> { Cover::to_pdf(self, sheet, title) }
    fn to_pdf_a4(&self, sheet: &RgbaImage, title: &str) -> Result<Vec<u8>, String> { Cover::to_pdf_a4(self, sheet, title) }
    fn guides(&self) -> (Vec<(f64, f64, f64, f64)>, Vec<(f64, f64, f64, f64)>, Vec<(f64, f64, f64)>) {
        let (tw, th) = self.trim_mm();
        let f = [self.face_mm, self.face_mm + self.spine_mm];
        (vec![(0.0, 0.0, tw, th)], f.iter().map(|&x| (x, 0.0, x, th)).collect(), vec![])
    }
}

/// Etiquette de disque : cercle de `diameter_mm` perce d'un trou central, dans une feuille carree avec fond perdu.
#[derive(Debug, Clone, PartialEq)]
pub struct DiscLabel {
    pub diameter_mm: f64,
    /// diametre de la zone non imprimee au centre
    pub hole_mm: f64,
    pub bleed_mm: f64,
    pub color: [u8; 3],
    pub bg_image: String,
    /// cercles de repere (contour et trou) traces sur le PDF
    pub marks: bool,
    pub dpi: u32,
    pub items: Vec<CoverItem>,
    pub layers: Vec<CoverLayer>,
}

impl Default for DiscLabel {
    fn default() -> DiscLabel {
        DiscLabel { diameter_mm: 118.0, hole_mm: 25.0, bleed_mm: 2.0, color: [24, 28, 40], bg_image: String::new(), marks: false, dpi: 300, items: vec![], layers: vec![] }
    }
}

impl DiscLabel {
    pub fn is_default(&self) -> bool { *self == DiscLabel::default() }

    pub fn to_value(&self) -> Option<Value> {
        if self.is_default() { return None; }
        let mut o: Vec<(String, Value)> = vec![
            ("diametre_mm".into(), Value::Num(self.diameter_mm)), ("trou_mm".into(), Value::Num(self.hole_mm)), ("fond_perdu_mm".into(), Value::Num(self.bleed_mm)),
            ("couleur".into(), color_hex(self.color)), ("reperes".into(), Value::Bool(self.marks)), ("dpi".into(), Value::Num(self.dpi as f64)),
        ];
        if !self.bg_image.is_empty() { o.push(("image_fond".into(), Value::Str(self.bg_image.clone()))); }
        if !self.layers.is_empty() { o.push(("calques".into(), layers_to_value(&self.layers))); }
        o.push(("elements".into(), Value::Arr(self.items.iter().map(item_to_value).collect())));
        Some(Value::Obj(o))
    }

    pub fn from_value(v: Option<&Value>) -> Result<DiscLabel, String> {
        let mut c = DiscLabel::default();
        let Some(v) = v else { return Ok(c) };
        let num = |k: &str, d: f64| v.get(k).and_then(Value::as_f64).filter(|x| x.is_finite()).unwrap_or(d);
        c.diameter_mm = num("diametre_mm", c.diameter_mm).clamp(30.0, 200.0);
        c.hole_mm = num("trou_mm", c.hole_mm).clamp(0.0, c.diameter_mm - 5.0);
        c.bleed_mm = num("fond_perdu_mm", c.bleed_mm).clamp(0.0, 20.0);
        c.dpi = num("dpi", c.dpi as f64).round().clamp(72.0, 600.0) as u32;
        c.marks = v.get("reperes").and_then(Value::as_bool).unwrap_or(false);
        c.color = color_from(v.get("couleur"), c.color);
        c.bg_image = v.get("image_fond").and_then(Value::as_str).unwrap_or("").to_string();
        if let Some(a) = v.get("elements").and_then(Value::as_array) {
            for (k, e) in a.iter().enumerate() { c.items.push(item_from_value(e, k, "etiquette")?); }
        }
        c.layers = layers_from_value(v.get("calques"));
        clamp_layers(&mut c.items, c.layers.len());
        Ok(c)
    }
}

impl SheetDoc for DiscLabel {
    fn sheet_mm(&self) -> (f64, f64) { let s = self.diameter_mm + 2.0 * self.bleed_mm; (s, s) }
    fn bleed_mm(&self) -> f64 { self.bleed_mm }
    fn dpi(&self) -> u32 { self.dpi }
    fn items(&self) -> &Vec<CoverItem> { &self.items }
    fn items_mut(&mut self) -> &mut Vec<CoverItem> { &mut self.items }
    fn layers(&self) -> &Vec<CoverLayer> { &self.layers }
    fn layers_mut(&mut self) -> &mut Vec<CoverLayer> { &mut self.layers }
    fn snap_lines(&self) -> (Vec<f64>, Vec<f64>) {
        let (d, b, h) = (self.diameter_mm, self.bleed_mm, self.hole_mm);
        let l = vec![-b, 0.0, d / 2.0 - h / 2.0, d / 2.0, d / 2.0 + h / 2.0, d, d + b];
        (l.clone(), l)
    }
    fn safe_guides(&self) -> (Vec<(f64, f64, f64, f64)>, Vec<(f64, f64, f64)>) {
        let r = self.diameter_mm / 2.0;
        (vec![], vec![(r, r, (r - SAFE_MM).max(1.0)), (r, r, self.hole_mm / 2.0 + SAFE_MM)])
    }
    fn zones(&self) -> Vec<(&'static str, (f64, f64, f64, f64))> {
        let (d, h) = (self.diameter_mm, self.hole_mm);
        // bande au-dessus du trou central et bande en dessous : la ou le texte reste lisible sur un disque
        vec![("le disque", (0.0, 0.0, d, d)), ("le haut", (d * 0.15, d * 0.04, d * 0.70, (d - h) / 2.0 * 0.6)), ("le bas", (d * 0.15, d / 2.0 + h / 2.0 + (d - h) / 2.0 * 0.15, d * 0.70, (d - h) / 2.0 * 0.6))]
    }
    fn layout_with(&self, dpi: u32, base: &Path, vars: &BTreeMap<String, String>) -> CoverLayout {
        // meme mise en forme que la jaquette : une seule zone couvrant la feuille
        let as_cover = Cover { face_mm: 0.0, spine_mm: self.diameter_mm, height_mm: self.diameter_mm, bleed_mm: self.bleed_mm,
            colors: [self.color; 3], bg_image: self.bg_image.clone(), marks: false, dpi: self.dpi, items: self.items.clone(), layers: self.layers.clone() };
        as_cover.layout_with(dpi, base, vars)
    }
    fn to_pdf(&self, sheet: &RgbaImage, title: &str) -> Vec<u8> {
        let (sw, sh) = self.sheet_mm();
        let (cx, cy) = (sw / 2.0, sh / 2.0);
        // l'image est decoupee en couronne (fond perdu compris) ; les reperes tombent sur le contour et le trou
        let clip = [(cx, cy, self.diameter_mm / 2.0 + self.bleed_mm), (cx, cy, (self.hole_mm / 2.0 - self.bleed_mm).max(0.5))];
        let circles = if self.marks { vec![(cx, cy, self.diameter_mm / 2.0), (cx, cy, self.hole_mm / 2.0)] } else { vec![] };
        pdf::image_pdf_shaped((sw, sh), sheet, (0.0, 0.0, sw, sh), &[], &clip, &circles, title)
    }
    fn to_pdf_a4(&self, sheet: &RgbaImage, title: &str) -> Result<Vec<u8>, String> {
        let (sw, sh) = self.sheet_mm();
        let d = self.diameter_mm;
        let page = a4_page(d, d, "L'étiquette")?;
        let (cx, cy) = (page.0 / 2.0, page.1 / 2.0);
        let r = d / 2.0;
        let clip = [(cx, cy, r + self.bleed_mm), (cx, cy, (self.hole_mm / 2.0 - self.bleed_mm).max(0.5))];
        // contour et trou traces, plus quatre reperes en croix hors du fond perdu
        let circles = [(cx, cy, r), (cx, cy, self.hole_mm / 2.0)];
        let (gap, t) = (self.bleed_mm + 1.5, 5.0);
        let lines = [(cx - r - gap, cy, cx - r - gap - t, cy), (cx + r + gap, cy, cx + r + gap + t, cy), (cx, cy - r - gap, cx, cy - r - gap - t), (cx, cy + r + gap, cx, cy + r + gap + t)];
        Ok(pdf::image_pdf_shaped(page, sheet, (cx - sw / 2.0, cy - sh / 2.0, sw, sh), &lines, &clip, &circles, title))
    }
    fn guides(&self) -> (Vec<(f64, f64, f64, f64)>, Vec<(f64, f64, f64, f64)>, Vec<(f64, f64, f64)>) {
        let r = self.diameter_mm / 2.0;
        (vec![], vec![], vec![(r, r, r), (r, r, self.hole_mm / 2.0)])
    }
}

// ------------------------------------------------------------------ historique annuler / refaire
/// Historique d'un document : chaque modification empile l'etat precedent (les modifications rapprochees, comme un glissement,
/// ne font qu'une etape).
#[derive(Debug, Clone)]
pub struct EditHistory<T: Clone + PartialEq> {
    undo: Vec<T>,
    redo: Vec<T>,
    last: T,
    last_ms: u64,
}

impl<T: Clone + PartialEq> EditHistory<T> {
    pub const MERGE_MS: u64 = 600;
    pub const MAX: usize = 200;

    pub fn new(cur: &T) -> Self { EditHistory { undo: vec![], redo: vec![], last: cur.clone(), last_ms: 0 } }

    /// A appeler a chaque image avec l'etat courant et l'heure (ms) : enregistre ce qui a change.
    pub fn observe(&mut self, cur: &T, ms: u64) {
        if *cur == self.last { return; }
        if self.undo.is_empty() || ms.saturating_sub(self.last_ms) > Self::MERGE_MS {
            self.undo.push(self.last.clone());
            if self.undo.len() > Self::MAX { self.undo.remove(0); }
        }
        self.last = cur.clone();
        self.last_ms = ms;
        self.redo.clear();
    }

    pub fn undo(&mut self, cur: &mut T) -> bool {
        let Some(p) = self.undo.pop() else { return false };
        self.redo.push(cur.clone());
        *cur = p.clone();
        self.last = p;
        self.last_ms = 0;
        true
    }

    pub fn redo(&mut self, cur: &mut T) -> bool {
        let Some(n) = self.redo.pop() else { return false };
        self.undo.push(cur.clone());
        *cur = n.clone();
        self.last = n;
        self.last_ms = 0;
        true
    }

    pub fn can_undo(&self) -> bool { !self.undo.is_empty() }
    pub fn can_redo(&self) -> bool { !self.redo.is_empty() }
}

fn item_to_value(it: &CoverItem) -> Value {
    let n = |x: f64| Value::Num(x);
    let mut e: Vec<(String, Value)> = Vec::new();
    match &it.kind {
        CoverKind::Text { text, size_pt, color, align, font, fx } => {
            e.push(("type".into(), Value::Str("texte".into())));
            e.push(("texte".into(), Value::Str(text.clone())));
            e.push(("taille_pt".into(), n(*size_pt)));
            e.push(("couleur".into(), color_hex(*color)));
            e.push(("alignement".into(), Value::Str(align.name().into())));
            if !font.is_empty() { e.push(("police".into(), Value::Str(font.clone()))); }
            let d = TextFx::default();
            if fx.outline_mm != d.outline_mm { e.push(("contour_mm".into(), n(fx.outline_mm))); }
            if fx.outline_color != d.outline_color { e.push(("contour_couleur".into(), color_hex(fx.outline_color))); }
            if fx.shadow_mm != d.shadow_mm { e.push(("ombre_mm".into(), n(fx.shadow_mm))); }
            if fx.shadow_color != d.shadow_color { e.push(("ombre_couleur".into(), color_hex(fx.shadow_color))); }
            if fx.spacing != d.spacing { e.push(("interligne".into(), n(fx.spacing))); }
            if fx.valign != d.valign { e.push(("alignement_v".into(), Value::Str(fx.valign.name().into()))); }
            if fx.autofit { e.push(("ajuster".into(), Value::Bool(true))); }
        }
        CoverKind::Shape { shape, fill, stroke, stroke_mm, radius_mm } => {
            e.push(("type".into(), Value::Str("forme".into())));
            e.push(("forme".into(), Value::Str(shape.name().into())));
            if let Some(c) = fill { e.push(("remplissage".into(), color_hex(*c))); }
            if let Some(c) = stroke { e.push(("bordure".into(), color_hex(*c))); }
            e.push(("epaisseur_mm".into(), n(*stroke_mm)));
            if *radius_mm != 0.0 { e.push(("rayon_mm".into(), n(*radius_mm))); }
        }
        CoverKind::Image { path } => {
            e.push(("type".into(), Value::Str("image".into())));
            e.push(("fichier".into(), Value::Str(path.clone())));
        }
    }
    for (k, v) in [("x", it.x), ("y", it.y), ("w", it.w), ("h", it.h)] { e.push((k.into(), n((v * 100.0).round() / 100.0))); }
    if it.rot != 0 { e.push(("rotation".into(), n(it.rot as f64))); }
    if !it.name.is_empty() { e.push(("nom".into(), Value::Str(it.name.clone()))); }
    if it.layer != 0 { e.push(("calque".into(), n(it.layer as f64))); }
    Value::Obj(e)
}

fn item_from_value(e: &Value, k: usize, what: &str) -> Result<CoverItem, String> {
    let f = |key: &str| e.get(key).and_then(Value::as_f64).unwrap_or(0.0);
    let kind = match e.get("type").and_then(Value::as_str) {
        Some("texte") => CoverKind::Text {
            text: e.get("texte").and_then(Value::as_str).unwrap_or("").to_string(),
            size_pt: e.get("taille_pt").and_then(Value::as_f64).unwrap_or(24.0).clamp(1.0, 600.0),
            color: color_from(e.get("couleur"), [255, 255, 255]),
            align: e.get("alignement").and_then(Value::as_str).and_then(Align::from_name).unwrap_or(Align::Center),
            font: e.get("police").and_then(Value::as_str).unwrap_or("").to_string(),
            fx: {
                let d = TextFx::default();
                let g = |k: &str, dv: f64| e.get(k).and_then(Value::as_f64).filter(|v| v.is_finite()).unwrap_or(dv);
                TextFx {
                    outline_mm: g("contour_mm", d.outline_mm).clamp(0.0, 10.0),
                    outline_color: color_from(e.get("contour_couleur"), d.outline_color),
                    shadow_mm: g("ombre_mm", d.shadow_mm).clamp(0.0, 10.0),
                    shadow_color: color_from(e.get("ombre_couleur"), d.shadow_color),
                    spacing: g("interligne", d.spacing).clamp(0.3, 5.0),
                    valign: e.get("alignement_v").and_then(Value::as_str).and_then(VAlign::from_name).unwrap_or(d.valign),
                    autofit: e.get("ajuster").and_then(Value::as_bool).unwrap_or(false),
                }
            },
        },
        Some("forme") => CoverKind::Shape {
            shape: e.get("forme").and_then(Value::as_str).and_then(ShapeKind::from_name).unwrap_or(ShapeKind::Rect),
            fill: e.get("remplissage").map(|v| color_from(Some(v), [255, 255, 255])),
            stroke: e.get("bordure").map(|v| color_from(Some(v), [255, 255, 255])),
            stroke_mm: e.get("epaisseur_mm").and_then(Value::as_f64).filter(|v| v.is_finite()).unwrap_or(0.5).clamp(0.0, 50.0),
            radius_mm: e.get("rayon_mm").and_then(Value::as_f64).filter(|v| v.is_finite()).unwrap_or(0.0).clamp(0.0, 200.0),
        },
        Some("image") => CoverKind::Image { path: e.get("fichier").and_then(Value::as_str).ok_or_else(|| format!("{what}, élément {} : « fichier » manquant", k + 1))?.to_string() },
        t => return Err(format!("{what}, élément {} : type {:?} inconnu", k + 1, t)),
    };
    let rot = match f("rotation").round() as i64 { 90 => 90, 270 | -90 => 270, _ => 0 };
    Ok(CoverItem { kind, x: f("x"), y: f("y"), w: f("w").max(1.0), h: f("h").max(1.0), rot, name: e.get("nom").and_then(Value::as_str).unwrap_or("").to_string(), layer: f("calque").max(0.0) as usize })
}

// ------------------------------------------------------------------ calques
/// Calque : regroupe des elements, peut etre masque (ni dessine, ni exporte) ou verrouille (non selectionnable a la souris).
#[derive(Debug, Clone, PartialEq)]
pub struct CoverLayer {
    pub name: String,
    pub visible: bool,
    pub locked: bool,
}

impl CoverLayer {
    pub fn new(name: &str) -> CoverLayer { CoverLayer { name: name.into(), visible: true, locked: false } }
}

/// Calques tels que l'interface les presente : au moins un (le calque implicite quand la liste est vide).
pub fn layers_or_default(layers: &[CoverLayer]) -> Vec<CoverLayer> {
    if layers.is_empty() { vec![CoverLayer::new("Calque 1")] } else { layers.to_vec() }
}

/// Forme enregistree : un unique calque d'origine devient la liste vide.
pub fn normalize_layers(layers: Vec<CoverLayer>) -> Vec<CoverLayer> {
    if layers.len() == 1 && layers[0] == CoverLayer::new("Calque 1") { vec![] } else { layers }
}

/// (visible, verrouille) du calque `i` ; un numero hors liste vaut le calque implicite.
pub fn layer_state(layers: &[CoverLayer], i: usize) -> (bool, bool) {
    layers.get(i).map(|l| (l.visible, l.locked)).unwrap_or((true, false))
}

/// Indices des elements a dessiner, de l'arriere vers l'avant : par calque, puis dans l'ordre de la liste ; calques masques exclus.
pub fn draw_order(items: &[CoverItem], layers: &[CoverLayer]) -> Vec<usize> {
    let mut v: Vec<usize> = (0..items.len()).filter(|&i| layer_state(layers, items[i].layer).0).collect();
    v.sort_by_key(|&i| items[i].layer);
    v
}

fn clamp_layers(items: &mut [CoverItem], n: usize) {
    let max = n.max(1) - 1;
    for it in items { it.layer = it.layer.min(max); }
}

fn layers_to_value(layers: &[CoverLayer]) -> Value {
    Value::Arr(layers.iter().map(|l| Value::Obj(vec![
        ("nom".into(), Value::Str(l.name.clone())), ("visible".into(), Value::Bool(l.visible)), ("verrouille".into(), Value::Bool(l.locked)),
    ])).collect())
}

fn layers_from_value(v: Option<&Value>) -> Vec<CoverLayer> {
    let Some(a) = v.and_then(Value::as_array) else { return vec![] };
    let l: Vec<CoverLayer> = a.iter().enumerate().map(|(k, e)| CoverLayer {
        name: e.get("nom").and_then(Value::as_str).map(str::to_string).unwrap_or_else(|| format!("Calque {}", k + 1)),
        visible: e.get("visible").and_then(Value::as_bool).unwrap_or(true),
        locked: e.get("verrouille").and_then(Value::as_bool).unwrap_or(false),
    }).collect();
    normalize_layers(l)
}

/// Page A4 (largeur, hauteur en mm) la mieux adaptee a un format fini `tw` x `th` avec 8 mm de marge pour les reperes de coupe :
/// portrait si possible, sinon paysage.
fn a4_page(tw: f64, th: f64, what: &str) -> Result<(f64, f64), String> {
    let (nw, nh) = (tw + 16.0, th + 16.0);
    if nw <= 210.0 && nh <= 297.0 { Ok((210.0, 297.0)) }
    else if nw <= 297.0 && nh <= 210.0 { Ok((297.0, 210.0)) }
    else { Err(format!("{what} ({tw:.0} × {th:.0} mm, plus 8 mm de marge pour les repères) ne tient pas sur une page A4.")) }
}

/// Marge de securite : le texte important reste a au moins cette distance (mm) des bords.
pub const SAFE_MM: f64 = 3.0;

/// Aimante un rectangle (x, y, w, h) en mm sur des lignes verticales `xs` et horizontales `ys` a moins de `thr` mm. `handle` : 0 =
/// deplacement (bords et centre), 1..4 = coin tire (haut gauche, haut droit, bas gauche, bas droit : seuls les bords qui bougent
/// s'aimantent). Renvoie le rectangle corrige et les lignes (abscisse, ordonnee) sur lesquelles il s'est aimante.
pub fn snap_rect(handle: u8, r: (f64, f64, f64, f64), xs: &[f64], ys: &[f64], thr: f64) -> ((f64, f64, f64, f64), Option<f64>, Option<f64>) {
    let (mut x, mut y, mut w, mut h) = r;
    let best = |edges: &[f64], lines: &[f64]| -> Option<(f64, f64)> {
        let mut b: Option<(f64, f64)> = None;
        for &e in edges { for &l in lines { let d = l - e; if d.abs() <= thr && b.map_or(true, |(bd, _)| d.abs() < (bd).abs()) { b = Some((d, l)); } } }
        b
    };
    let xe: Vec<f64> = match handle { 0 => vec![x, x + w / 2.0, x + w], 1 | 3 => vec![x], _ => vec![x + w] };
    let ye: Vec<f64> = match handle { 0 => vec![y, y + h / 2.0, y + h], 1 | 2 => vec![y], _ => vec![y + h] };
    let (mut gx, mut gy) = (None, None);
    if let Some((d, l)) = best(&xe, xs) {
        match handle { 0 => x += d, 1 | 3 => { x += d; w -= d } _ => w += d }
        gx = Some(l);
    }
    if let Some((d, l)) = best(&ye, ys) {
        match handle { 0 => y += d, 1 | 2 => { y += d; h -= d } _ => h += d }
        gy = Some(l);
    }
    if w < 1.0 || h < 1.0 { return (r, None, None); }
    ((x, y, w, h), gx, gy)
}

/// Aligne les elements `sel` (indices) : avec au moins deux elements, par rapport a leur cadre englobant ; avec un seul, par rapport
/// a la feuille rognee `page` (largeur, hauteur en mm). Repartition : au moins trois elements. Renvoie vrai si quelque chose a bouge.
pub fn align_cover(items: &mut [CoverItem], sel: &[usize], op: crate::editor::ItemAlign, page: (f64, f64)) -> bool {
    use crate::editor::ItemAlign as A;
    let mut idx: Vec<usize> = sel.iter().copied().filter(|&i| i < items.len()).collect();
    idx.sort_unstable();
    idx.dedup();
    if idx.is_empty() { return false; }
    let (mut x0, mut y0, mut x1, mut y1) = (f64::MAX, f64::MAX, f64::MIN, f64::MIN);
    for &i in &idx { let it = &items[i]; x0 = x0.min(it.x); y0 = y0.min(it.y); x1 = x1.max(it.x + it.w); y1 = y1.max(it.y + it.h); }
    let (rx0, ry0, rx1, ry1) = if idx.len() >= 2 { (x0, y0, x1, y1) } else { (0.0, 0.0, page.0, page.1) };
    let before: Vec<(f64, f64)> = idx.iter().map(|&i| (items[i].x, items[i].y)).collect();
    match op {
        A::Left => idx.iter().for_each(|&i| items[i].x = rx0),
        A::Right => idx.iter().for_each(|&i| items[i].x = rx1 - items[i].w),
        A::CenterH => idx.iter().for_each(|&i| items[i].x = (rx0 + rx1 - items[i].w) / 2.0),
        A::Top => idx.iter().for_each(|&i| items[i].y = ry0),
        A::Bottom => idx.iter().for_each(|&i| items[i].y = ry1 - items[i].h),
        A::CenterV => idx.iter().for_each(|&i| items[i].y = (ry0 + ry1 - items[i].h) / 2.0),
        A::DistributeH | A::DistributeV => {
            if idx.len() >= 3 {
                let horiz = op == A::DistributeH;
                let centre = |it: &CoverItem| if horiz { it.x + it.w / 2.0 } else { it.y + it.h / 2.0 };
                idx.sort_by(|&a, &b| centre(&items[a]).partial_cmp(&centre(&items[b])).unwrap_or(std::cmp::Ordering::Equal));
                let (first, last) = (centre(&items[idx[0]]), centre(&items[*idx.last().unwrap()]));
                let n = (idx.len() - 1) as f64;
                for (k, &i) in idx.iter().enumerate() {
                    let c = first + (last - first) * k as f64 / n;
                    if horiz { items[i].x = c - items[i].w / 2.0; } else { items[i].y = c - items[i].h / 2.0; }
                }
            }
        }
        A::PageH => { let dx = (page.0 - (x1 - x0)) / 2.0 - x0; idx.iter().for_each(|&i| items[i].x += dx); }
        A::PageV => { let dy = (page.1 - (y1 - y0)) / 2.0 - y0; idx.iter().for_each(|&i| items[i].y += dy); }
    }
    // arrondi au centieme de mm : l'enregistrement fait de meme
    for &i in &idx { items[i].x = (items[i].x * 100.0).round() / 100.0; items[i].y = (items[i].y * 100.0).round() / 100.0; }
    let mut moved = false;
    for (k, &i) in idx.iter().enumerate() { if (items[i].x, items[i].y) != before[k] { moved = true; } }
    moved
}
