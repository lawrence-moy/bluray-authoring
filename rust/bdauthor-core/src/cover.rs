//! Jaquette (feuille a deplier d'un boitier) : dos, tranche et face, avec fond perdu. Modele, enregistrement dans le projet,
//! mise en page en pixels (pour le rendu) et export PDF. Le dessin des textes et des images est fait par `bdauthor-render`.
use crate::i18n::tr;
use crate::trf;
use crate::design::{Align, VideoFrame};
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
    /// gras et italique (synthetiques, comme pour les sous-titres)
    pub bold: bool,
    pub italic: bool,
}

impl Default for TextFx {
    fn default() -> TextFx {
        TextFx { outline_mm: 0.0, outline_color: [0, 0, 0], shadow_mm: 0.0, shadow_color: [0, 0, 0], spacing: 1.0, valign: VAlign::Middle, autofit: false, bold: false, italic: false }
    }
}

/// Type de trait d'une ligne.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LineStyle { Solid, Dashed, Dotted, DashDot, Double }

impl LineStyle {
    pub const ALL: [LineStyle; 5] = [LineStyle::Solid, LineStyle::Dashed, LineStyle::Dotted, LineStyle::DashDot, LineStyle::Double];
    pub fn name(self) -> &'static str { match self { LineStyle::Solid => "plein", LineStyle::Dashed => "tirets", LineStyle::Dotted => "points", LineStyle::DashDot => "tirets_points", LineStyle::Double => "double" } }
    pub fn from_name(s: &str) -> Option<LineStyle> { LineStyle::ALL.into_iter().find(|l| l.name() == s) }
    pub fn label(self) -> &'static str { match self { LineStyle::Solid => tr("Solid line"), LineStyle::Dashed => tr("Dashes"), LineStyle::Dotted => tr("Dots"), LineStyle::DashDot => tr("Dashes and dots"), LineStyle::Double => tr("Double line") } }
}

/// Pointes de fleche d'une ligne : `Start` au bout de gauche (ou du haut si la ligne est verticale), `End` a l'autre.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LineArrow { None, Start, End, Both }

impl LineArrow {
    pub const ALL: [LineArrow; 4] = [LineArrow::None, LineArrow::Start, LineArrow::End, LineArrow::Both];
    pub fn name(self) -> &'static str { match self { LineArrow::None => "aucune", LineArrow::Start => "debut", LineArrow::End => "fin", LineArrow::Both => "deux" } }
    pub fn from_name(s: &str) -> Option<LineArrow> { LineArrow::ALL.into_iter().find(|l| l.name() == s) }
    pub fn label(self) -> &'static str { match self { LineArrow::None => tr("No arrowhead"), LineArrow::Start => tr("At the start"), LineArrow::End => tr("At the end"), LineArrow::Both => tr("At both ends") } }
    pub fn flags(self) -> (bool, bool) { match self { LineArrow::None => (false, false), LineArrow::Start => (true, false), LineArrow::End => (false, true), LineArrow::Both => (true, true) } }
    pub fn from_flags(start: bool, end: bool) -> LineArrow { match (start, end) { (false, false) => LineArrow::None, (true, false) => LineArrow::Start, (false, true) => LineArrow::End, _ => LineArrow::Both } }
}

/// Forme geometrique.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShapeKind { Rect, Ellipse, Line }

impl ShapeKind {
    pub fn name(self) -> &'static str { match self { ShapeKind::Rect => "rectangle", ShapeKind::Ellipse => "ellipse", ShapeKind::Line => "ligne" } }
    pub fn from_name(s: &str) -> Option<ShapeKind> { match s { "rectangle" => Some(ShapeKind::Rect), "ellipse" => Some(ShapeKind::Ellipse), "ligne" => Some(ShapeKind::Line), _ => None } }
    pub fn label(self) -> &'static str { match self { ShapeKind::Rect => tr("Rectangle"), ShapeKind::Ellipse => tr("Ellipse"), ShapeKind::Line => tr("Line") } }
}

/// Contenu d'un element de jaquette.
#[derive(Debug, Clone, PartialEq)]
pub enum CoverKind {
    Text { text: String, size_pt: f64, color: [u8; 3], align: Align, font: String, fx: TextFx },
    Image { path: String },
    /// image d'une video du projet (comme le widget « Image de la video » du menu) : video, instant et recadrage ; l'image est
    /// extraite a part et passee a la mise en page par `vars` sous la cle `frame_key`
    VideoImage { frame: VideoFrame },
    /// rectangle (coins arrondis de rayon `radius_mm`), ellipse inscrite dans le cadre, ou trait horizontal au milieu du cadre
    /// (tourne par `rot`) ; `fill` et `stroke` : couleurs, `None` = absent ; la bordure est a l'interieur du cadre
    /// `flip` (lignes) : la ligne va du coin bas gauche au coin haut droit du cadre au lieu de haut gauche -> bas droit
    Shape { shape: ShapeKind, fill: Option<[u8; 3]>, stroke: Option<[u8; 3]>, stroke_mm: f64, radius_mm: f64, flip: bool, style: LineStyle, arrow: LineArrow },
    /// tableau des informations d'une ou plusieurs videos (duree, langues, sous-titres, format, son) ; les valeurs viennent de
    /// `vars` (cles `info_key`, voir `ProjectModel::cover_info_vars`)
    InfoTable { table: InfoTable },
}

/// Information d'une video affichee par un tableau de jaquette.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InfoField { Title, Duration, Languages, Subtitles, Format, Sound }

impl InfoField {
    pub const ALL: [InfoField; 6] = [InfoField::Title, InfoField::Duration, InfoField::Languages, InfoField::Subtitles, InfoField::Format, InfoField::Sound];
    /// Nom dans le fichier de projet et dans les cles de `vars`.
    pub fn name(self) -> &'static str {
        match self { InfoField::Title => "title", InfoField::Duration => "duration", InfoField::Languages => "languages", InfoField::Subtitles => "subtitles", InfoField::Format => "format", InfoField::Sound => "sound" }
    }
    pub fn from_name(s: &str) -> Option<InfoField> { InfoField::ALL.into_iter().find(|f| f.name() == s) }
}

/// Cle de `vars` de la valeur de `field` pour la video de rang `video`.
pub fn info_key(video: usize, field: InfoField) -> String { format!("@info:{video}:{}", field.name()) }
/// Cle de `vars` de l'intitule de `field` (en-tete du tableau, dans la langue de l'interface).
pub fn info_label_key(field: InfoField) -> String { format!("@info:label:{}", field.name()) }
/// Cle de `vars` du nombre de videos du projet.
pub const INFO_COUNT_KEY: &str = "@info:count";

/// Tableau d'informations : une ligne par video et une colonne par information (ou l'inverse avec `transpose`).
#[derive(Debug, Clone, PartialEq)]
pub struct InfoTable {
    /// videos montrees (rangs) ; vide : toutes
    pub videos: Vec<usize>,
    /// informations montrees, dans l'ordre des colonnes
    pub fields: Vec<InfoField>,
    /// ligne (ou colonne) des intitules
    pub header: bool,
    /// une ligne par information et une colonne par video (pratique pour une seule video : intitule | valeur)
    pub transpose: bool,
    pub size_pt: f64,
    pub color: [u8; 3],
    pub header_color: [u8; 3],
    pub header_bold: bool,
    /// fond des intitules ; `None` : transparent
    pub header_fill: Option<[u8; 3]>,
    /// fond des autres cases (hors intitules) ; `None` : transparent
    pub fill: Option<[u8; 3]>,
    /// traits de la grille ; `None` : sans grille
    pub grid: Option<[u8; 3]>,
    pub grid_mm: f64,
    pub font: String,
    pub align: Align,
}

impl Default for InfoTable {
    fn default() -> Self {
        InfoTable { videos: vec![], fields: InfoField::ALL[1..].to_vec(), header: true, transpose: false, size_pt: 7.0, color: [255, 255, 255],
            header_color: [255, 255, 255], header_bold: true, header_fill: None, fill: None, grid: Some([255, 255, 255]), grid_mm: 0.15, font: String::new(), align: Align::Left }
    }
}

impl InfoTable {
    /// Videos montrees (rangs valides parmi `count`), dans l'ordre.
    pub fn shown_videos(&self, count: usize) -> Vec<usize> {
        if self.videos.is_empty() { return (0..count).collect(); }
        // dans l'ordre de la liste, sans doublon
        let mut out: Vec<usize> = Vec::new();
        for &v in &self.videos { if v < count && !out.contains(&v) { out.push(v); } }
        out
    }

    /// Cellules du tableau, ligne par ligne, avec pour chacune son type (intitule ?). Les valeurs inconnues sont vides.
    pub fn cells(&self, vars: &BTreeMap<String, String>) -> Vec<Vec<(String, bool)>> {
        let count = vars.get(INFO_COUNT_KEY).and_then(|c| c.parse().ok()).unwrap_or(0);
        let vids = self.shown_videos(count);
        let label = |f: InfoField| vars.get(&info_label_key(f)).cloned().unwrap_or_else(|| f.name().to_string());
        let val = |v: usize, f: InfoField| vars.get(&info_key(v, f)).cloned().unwrap_or_default();
        if self.fields.is_empty() || vids.is_empty() { return vec![]; }
        if self.transpose {
            self.fields.iter().map(|&f| {
                let mut row = Vec::new();
                if self.header { row.push((label(f), true)); }
                row.extend(vids.iter().map(|&v| (val(v, f), false)));
                row
            }).collect()
        } else {
            let mut rows = Vec::new();
            if self.header { rows.push(self.fields.iter().map(|&f| (label(f), true)).collect()); }
            rows.extend(vids.iter().map(|&v| self.fields.iter().map(|&f| (val(v, f), false)).collect()));
            rows
        }
    }

    /// Dessin du tableau dans le cadre (x, y, w, h) en pixels : fonds des intitules, textes, puis grille. Les colonnes sont
    /// proportionnelles a leur texte le plus long ; un texte trop long est reduit pour tenir dans sa case.
    fn decors(&self, (x, y, w, h): (i32, i32, i32, i32), dpi: u32, vars: &BTreeMap<String, String>, base: &Path) -> Vec<CoverDecor> {
        let cells = self.cells(vars);
        let (nr, nc) = (cells.len(), cells.first().map_or(0, |r| r.len()));
        if nr == 0 || nc == 0 || w < 2 || h < 2 { return vec![]; }
        let weight: Vec<f64> = (0..nc).map(|c| cells.iter().map(|r| r[c].0.chars().count()).max().unwrap_or(0).max(3) as f64 + 2.0).collect();
        let total: f64 = weight.iter().sum();
        // bords des colonnes et des lignes, au pixel pres et sans trou
        let mut xs = vec![x];
        let mut acc = 0.0;
        for wt in &weight { acc += wt; xs.push(x + (w as f64 * acc / total).round() as i32); }
        let ys: Vec<i32> = (0..=nr).map(|r| y + (h as f64 * r as f64 / nr as f64).round() as i32).collect();
        let size = ((self.size_pt / 72.0 * dpi as f64).round() as i32).max(1);
        let pad = (size / 3).max(1);
        let font = if self.font.is_empty() || Path::new(&self.font).is_absolute() { self.font.clone() } else if base.join(&self.font).is_file() { base.join(&self.font).to_string_lossy().into_owned() } else { self.font.clone() };
        let mut out = Vec::new();
        let stroke_px = mm_px(self.grid_mm, dpi).max(1);
        // fonds : intitules et autres cases ont chacun le leur
        for (r, row) in cells.iter().enumerate() {
            for (c, (_, head)) in row.iter().enumerate() {
                let Some(fill) = (if *head { self.header_fill } else { self.fill }) else { continue };
                out.push(CoverDecor::Shape { shape: ShapeKind::Rect, x: xs[c], y: ys[r], w: xs[c + 1] - xs[c], h: ys[r + 1] - ys[r], fill: Some(fill), stroke: None, stroke_px: 0, radius_px: 0, flip: false, style: LineStyle::Solid, arrow: LineArrow::None });
            }
        }
        for (r, row) in cells.iter().enumerate() {
            for (c, (text, head)) in row.iter().enumerate() {
                if text.is_empty() { continue; }
                let (cw, ch) = (xs[c + 1] - xs[c] - 2 * pad, ys[r + 1] - ys[r]);
                if cw < 1 || ch < 1 { continue; }
                out.push(CoverDecor::Text { text: text.clone(), x: xs[c] + pad, y: ys[r], w: cw, h: ch, size, color: if *head { self.header_color } else { self.color }, align: self.align, font: font.clone(),
                    valign: VAlign::Middle, spacing: 1.0, outline: 0, outline_color: [0, 0, 0], shadow: 0, shadow_color: [0, 0, 0], autofit: true, bold: *head && self.header_bold, italic: false });
            }
        }
        if let Some(g) = self.grid {
            let line = |x: i32, y: i32, w: i32, h: i32| CoverDecor::Shape { shape: ShapeKind::Line, x, y, w, h, fill: None, stroke: Some(g), stroke_px, radius_px: 0, flip: false, style: LineStyle::Solid, arrow: LineArrow::None };
            // traits a l'interieur du cadre : les bords exterieurs sont decales d'un demi-trait
            let half = stroke_px / 2;
            for (k, &yy) in ys.iter().enumerate() {
                let yy = if k == 0 { yy + half } else if k == nr { yy - half - stroke_px % 2 } else { yy };
                out.push(line(x, yy, w, 0));
            }
            for (k, &xx) in xs.iter().enumerate() {
                let xx = if k == 0 { xx + half } else if k == nc { xx - half - stroke_px % 2 } else { xx };
                out.push(line(xx, y, 0, h));
            }
        }
        out
    }
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
    /// Extremites d'une ligne (debut = a gauche) ; `None` pour tout autre element.
    pub fn line_ends(&self) -> Option<((f64, f64), (f64, f64))> {
        match &self.kind {
            CoverKind::Shape { shape: ShapeKind::Line, flip, .. } => Some(if *flip { ((self.x, self.y + self.h), (self.x + self.w, self.y)) } else { ((self.x, self.y), (self.x + self.w, self.y + self.h)) }),
            _ => None,
        }
    }

    /// Place une ligne entre deux extremites (le cadre est le rectangle englobant). Sans effet sur un autre element.
    pub fn set_line_ends(&mut self, a: (f64, f64), b: (f64, f64)) {
        let r = |v: f64| (v * 100.0).round() / 100.0;
        if let CoverKind::Shape { shape: ShapeKind::Line, flip, .. } = &mut self.kind {
            *flip = (a.0 - b.0) * (a.1 - b.1) < 0.0;
            self.x = r(a.0.min(b.0));
            self.y = r(a.1.min(b.1));
            self.w = r((a.0 - b.0).abs());
            self.h = r((a.1 - b.1).abs());
        }
    }

    /// Deplace une extremite d'une ligne : `moving_start` indique si c'est le bout « debut » (celui qui porte `LineArrow::Start`) ;
    /// `fixed` est l'autre bout, `to` la nouvelle position. Les pointes de fleche suivent leur extremite meme si les bouts se croisent.
    pub fn move_line_end(&mut self, moving_start: bool, fixed: (f64, f64), to: (f64, f64)) {
        let (st, en) = match &self.kind { CoverKind::Shape { shape: ShapeKind::Line, arrow, .. } => arrow.flags(), _ => return };
        let (mv, fx) = if moving_start { (st, en) } else { (en, st) };
        self.set_line_ends(fixed, to);
        // le bout « debut » est le plus a gauche (a x egal, le plus haut)
        let to_is_start = to.0 < fixed.0 || (to.0 == fixed.0 && to.1 <= fixed.1);
        let (a, b) = if to_is_start { (mv, fx) } else { (fx, mv) };
        if let CoverKind::Shape { arrow, .. } = &mut self.kind { *arrow = LineArrow::from_flags(a, b); }
    }

    pub fn shape(shape: ShapeKind, x: f64, y: f64, w: f64, h: f64) -> CoverItem {
        let (fill, stroke) = if shape == ShapeKind::Line { (None, Some([255, 255, 255])) } else { (Some([255, 255, 255]), None) };
        CoverItem { kind: CoverKind::Shape { shape, fill, stroke, stroke_mm: 0.5, radius_mm: 0.0, flip: false, style: LineStyle::Solid, arrow: LineArrow::None }, x, y, w, h, rot: 0, name: String::new(), layer: 0 }
    }
    /// Image d'une video (a choisir) : 16/9 par defaut.
    pub fn video_image(x: f64, y: f64, w: f64, h: f64) -> CoverItem {
        CoverItem { kind: CoverKind::VideoImage { frame: VideoFrame { video: None, at: 0.0, crop: None } }, x, y, w, h, rot: 0, name: String::new(), layer: 0 }
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
        Cover { face_mm: 129.0, height_mm: 148.0, spine_mm: 11.0, bleed_mm: 3.0, colors: [[24, 28, 40], [40, 60, 110], [24, 28, 40]], bg_image: String::new(), marks: false, dpi: 300, items: vec![], layers: vec![] }
    }
}

/// Formats usuels : (nom, largeur du dos et de la face, hauteur, tranche), en mm.
pub const FORMATS: [(&str, f64, f64, f64); 5] = [
    ("Blu-ray standard (11 mm spine)", 129.0, 148.0, 11.0),
    ("Thick Blu-ray (14 mm spine)", 129.0, 148.0, 14.0),
    ("Standard DVD (14 mm spine)", 129.0, 183.0, 14.0),
    ("Slim DVD (7 mm spine)", 129.0, 183.0, 7.0),
    ("Slim Blu-ray (7 mm spine)", 129.0, 148.0, 7.0),
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
    Text { text: String, x: i32, y: i32, w: i32, h: i32, size: i32, color: [u8; 3], align: Align, font: String, valign: VAlign, spacing: f32, outline: i32, outline_color: [u8; 3], shadow: i32, shadow_color: [u8; 3], autofit: bool, bold: bool, italic: bool },
    Image { path: PathBuf, x: i32, y: i32, w: i32, h: i32 },
    Shape { shape: ShapeKind, x: i32, y: i32, w: i32, h: i32, fill: Option<[u8; 3]>, stroke: Option<[u8; 3]>, stroke_px: i32, radius_px: i32, flip: bool, style: LineStyle, arrow: LineArrow },
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
        let one = |it: &CoverItem| -> Option<(CoverDecor, u16)> {
            let (x, y) = (b + mm_px(it.x, dpi), b + mm_px(it.y, dpi));
            let is_line = matches!(&it.kind, CoverKind::Shape { shape: ShapeKind::Line, .. });
            // une ligne peut etre exactement horizontale ou verticale (cadre de largeur ou hauteur nulle)
            let (iw, ih) = if is_line { (mm_px(it.w, dpi).max(0), mm_px(it.h, dpi).max(0)) } else { (mm_px(it.w, dpi).max(1), mm_px(it.h, dpi).max(1)) };
            let d = match &it.kind {
                CoverKind::Text { text, size_pt, color, align, font, fx } => {
                    let text = apply_vars(text, vars);
                    if text.is_empty() { return None; }
                    let font = if font.is_empty() || Path::new(font).is_absolute() { font.clone() } else if base.join(font).is_file() { base.join(font).to_string_lossy().into_owned() } else { font.clone() };
                    CoverDecor::Text { text, x, y, w: iw, h: ih, size: ((size_pt / 72.0 * dpi as f64).round() as i32).max(1), color: *color, align: *align, font, valign: fx.valign,
                        spacing: fx.spacing as f32, outline: if fx.outline_mm > 0.0 { mm_px(fx.outline_mm, dpi).max(1) } else { 0 }, outline_color: fx.outline_color,
                        shadow: if fx.shadow_mm > 0.0 { mm_px(fx.shadow_mm, dpi).max(1) } else { 0 }, shadow_color: fx.shadow_color, autofit: fx.autofit, bold: fx.bold, italic: fx.italic }
                }
                CoverKind::Image { path } => {
                    if path.is_empty() { return None; }
                    CoverDecor::Image { path: abs(path), x, y, w: iw, h: ih }
                }
                CoverKind::VideoImage { frame } => {
                    // l'image extraite est donnee par `vars` ; sans elle (pas encore prete, video a choisir) l'element est ignore
                    let p = vars.get(&frame_key(frame)?)?;
                    CoverDecor::Image { path: PathBuf::from(p), x, y, w: iw, h: ih }
                }
                CoverKind::InfoTable { .. } => return None,
                CoverKind::Shape { shape, fill, stroke, stroke_mm, radius_mm, flip, style, arrow } => {
                    CoverDecor::Shape { flip: *flip, style: *style, arrow: *arrow, shape: *shape, x, y, w: iw, h: ih, fill: *fill, stroke: *stroke, stroke_px: if stroke.is_some() { mm_px(*stroke_mm, dpi).max(1) } else { 0 }, radius_px: mm_px(*radius_mm, dpi).max(0) }
                }
            };
            Some((d, if is_line { 0 } else { it.rot }))
        };
        let mut items = Vec::new();
        for i in draw_order(&self.items, &self.layers) {
            let it = &self.items[i];
            match &it.kind {
                // un tableau : plusieurs dessins, toujours a l'horizontale
                CoverKind::InfoTable { table } => {
                    let r = (b + mm_px(it.x, dpi), b + mm_px(it.y, dpi), mm_px(it.w, dpi).max(1), mm_px(it.h, dpi).max(1));
                    items.extend(table.decors(r, dpi, vars, base).into_iter().map(|d| (d, 0)));
                }
                _ => items.extend(one(it)),
            }
        }
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
        let page = a4_page(tw, th, tr("The cover"))?;
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
            ("face_width_mm".into(), n(self.face_mm)), ("height_mm".into(), n(self.height_mm)), ("spine_mm".into(), n(self.spine_mm)), ("bleed_mm".into(), n(self.bleed_mm)),
            ("colors".into(), Value::Obj(vec![("back".into(), color_hex(self.colors[0])), ("spine".into(), color_hex(self.colors[1])), ("front".into(), color_hex(self.colors[2]))])),
            ("crop_marks".into(), Value::Bool(self.marks)), ("dpi".into(), n(self.dpi as f64)),
        ];
        if !self.bg_image.is_empty() { o.push(("background_image".into(), Value::Str(self.bg_image.clone()))); }
        let items: Vec<Value> = self.items.iter().map(item_to_value).collect();
        if !self.layers.is_empty() { o.push(("layers".into(), layers_to_value(&self.layers))); }
        o.push(("items".into(), Value::Arr(items)));
        Some(Value::Obj(o))
    }

    pub fn from_value(v: Option<&Value>) -> Result<Cover, String> {
        let mut c = Cover::default();
        let Some(v) = v else { return Ok(c) };
        let num = |k: &str, d: f64| v.get(k).and_then(Value::as_f64).filter(|x| x.is_finite()).unwrap_or(d);
        c.face_mm = num("face_width_mm", c.face_mm).clamp(20.0, 400.0);
        c.height_mm = num("height_mm", c.height_mm).clamp(20.0, 400.0);
        c.spine_mm = num("spine_mm", c.spine_mm).clamp(0.0, 100.0);
        c.bleed_mm = num("bleed_mm", c.bleed_mm).clamp(0.0, 20.0);
        c.dpi = num("dpi", c.dpi as f64).round().clamp(72.0, 600.0) as u32;
        c.marks = v.get("crop_marks").and_then(Value::as_bool).unwrap_or(false);
        if let Some(cs) = v.get("colors") {
            for (i, k) in ["back", "spine", "front"].iter().enumerate() { c.colors[i] = color_from(cs.get(k), c.colors[i]); }
        }
        c.bg_image = v.get("background_image").and_then(Value::as_str).unwrap_or("").to_string();
        if let Some(a) = v.get("items").and_then(Value::as_array) {
            for (k, e) in a.iter().enumerate() { c.items.push(item_from_value(e, k, "jaquette")?); }
        }
        c.layers = layers_from_value(v.get("layers"));
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
    ("titre", "disc title"),
    ("liste_videos", "one line per video: number, title and duration"),
    ("nb_videos", "number of videos"),
    ("duree_totale", "total duration of the videos"),
    ("nb_chapitres", "total number of manual chapters"),
    ("annee", "current year"),
];

/// Remplace `{nom}` par sa valeur ; un champ inconnu reste tel quel.
/// Cle sous laquelle `Cover::layout_with` cherche, dans `vars`, le fichier PNG de l'image d'une video (`None` : video a choisir).
/// Le fichier est celui de `framesrc::write_video_frame_png`.
pub fn frame_key(f: &VideoFrame) -> Option<String> {
    let c = f.crop.map_or([-1; 4], |c| c.map(|v| (v * 1e4).round() as i64));
    Some(format!("@image_video:{}:{}:{}:{}:{}:{}", f.video?, (f.at * 1000.0).round() as i64, c[0], c[1], c[2], c[3]))
}

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
            ("diameter_mm".into(), Value::Num(self.diameter_mm)), ("hole_mm".into(), Value::Num(self.hole_mm)), ("bleed_mm".into(), Value::Num(self.bleed_mm)),
            ("color".into(), color_hex(self.color)), ("marks".into(), Value::Bool(self.marks)), ("dpi".into(), Value::Num(self.dpi as f64)),
        ];
        if !self.bg_image.is_empty() { o.push(("background_image".into(), Value::Str(self.bg_image.clone()))); }
        if !self.layers.is_empty() { o.push(("layers".into(), layers_to_value(&self.layers))); }
        o.push(("items".into(), Value::Arr(self.items.iter().map(item_to_value).collect())));
        Some(Value::Obj(o))
    }

    pub fn from_value(v: Option<&Value>) -> Result<DiscLabel, String> {
        let mut c = DiscLabel::default();
        let Some(v) = v else { return Ok(c) };
        let num = |k: &str, d: f64| v.get(k).and_then(Value::as_f64).filter(|x| x.is_finite()).unwrap_or(d);
        c.diameter_mm = num("diameter_mm", c.diameter_mm).clamp(30.0, 200.0);
        c.hole_mm = num("hole_mm", c.hole_mm).clamp(0.0, c.diameter_mm - 5.0);
        c.bleed_mm = num("bleed_mm", c.bleed_mm).clamp(0.0, 20.0);
        c.dpi = num("dpi", c.dpi as f64).round().clamp(72.0, 600.0) as u32;
        c.marks = v.get("marks").and_then(Value::as_bool).unwrap_or(false);
        c.color = color_from(v.get("color"), c.color);
        c.bg_image = v.get("background_image").and_then(Value::as_str).unwrap_or("").to_string();
        if let Some(a) = v.get("items").and_then(Value::as_array) {
            for (k, e) in a.iter().enumerate() { c.items.push(item_from_value(e, k, "etiquette")?); }
        }
        c.layers = layers_from_value(v.get("layers"));
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
        let page = a4_page(d, d, tr("The label"))?;
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
            e.push(("text".into(), Value::Str(text.clone())));
            e.push(("size_pt".into(), n(*size_pt)));
            e.push(("color".into(), color_hex(*color)));
            e.push(("align".into(), Value::Str(align.name().into())));
            if !font.is_empty() { e.push(("font".into(), Value::Str(font.clone()))); }
            let d = TextFx::default();
            if fx.outline_mm != d.outline_mm { e.push(("outline_mm".into(), n(fx.outline_mm))); }
            if fx.outline_color != d.outline_color { e.push(("outline_color".into(), color_hex(fx.outline_color))); }
            if fx.shadow_mm != d.shadow_mm { e.push(("shadow_mm".into(), n(fx.shadow_mm))); }
            if fx.shadow_color != d.shadow_color { e.push(("shadow_color".into(), color_hex(fx.shadow_color))); }
            if fx.spacing != d.spacing { e.push(("line_spacing".into(), n(fx.spacing))); }
            if fx.valign != d.valign { e.push(("valign".into(), Value::Str(fx.valign.name().into()))); }
            if fx.autofit { e.push(("autofit".into(), Value::Bool(true))); }
            if fx.bold { e.push(("bold".into(), Value::Bool(true))); }
            if fx.italic { e.push(("italic".into(), Value::Bool(true))); }
        }
        CoverKind::Shape { shape, fill, stroke, stroke_mm, radius_mm, flip, style, arrow } => {
            if *style != LineStyle::Solid { e.push(("line_style".into(), Value::Str(style.name().into()))); }
            if *arrow != LineArrow::None { e.push(("arrow".into(), Value::Str(arrow.name().into()))); }
            if *flip { e.push(("flipped".into(), Value::Bool(true))); }
            e.push(("type".into(), Value::Str("forme".into())));
            e.push(("shape".into(), Value::Str(shape.name().into())));
            if let Some(c) = fill { e.push(("fill".into(), color_hex(*c))); }
            if let Some(c) = stroke { e.push(("border".into(), color_hex(*c))); }
            e.push(("thickness_mm".into(), n(*stroke_mm)));
            if *radius_mm != 0.0 { e.push(("radius_mm".into(), n(*radius_mm))); }
        }
        CoverKind::Image { path } => {
            e.push(("type".into(), Value::Str("image".into())));
            e.push(("file".into(), Value::Str(path.clone())));
        }
        CoverKind::InfoTable { table: t } => {
            e.push(("type".into(), Value::Str("tableau_infos".into())));
            if !t.videos.is_empty() { e.push(("videos".into(), Value::Arr(t.videos.iter().map(|v| n(*v as f64 + 1.0)).collect()))); }
            e.push(("fields".into(), Value::Arr(t.fields.iter().map(|f| Value::Str(f.name().into())).collect())));
            e.push(("header".into(), Value::Bool(t.header)));
            if t.transpose { e.push(("transpose".into(), Value::Bool(true))); }
            e.push(("size_pt".into(), n(t.size_pt)));
            e.push(("color".into(), color_hex(t.color)));
            e.push(("header_color".into(), color_hex(t.header_color)));
            e.push(("header_bold".into(), Value::Bool(t.header_bold)));
            if let Some(c) = t.header_fill { e.push(("header_fill".into(), color_hex(c))); }
            if let Some(c) = t.fill { e.push(("fill".into(), color_hex(c))); }
            if let Some(c) = t.grid { e.push(("grid".into(), color_hex(c))); }
            e.push(("grid_mm".into(), n(t.grid_mm)));
            if !t.font.is_empty() { e.push(("font".into(), Value::Str(t.font.clone()))); }
            e.push(("align".into(), Value::Str(t.align.name().into())));
        }
        CoverKind::VideoImage { frame } => {
            e.push(("type".into(), Value::Str("image_video".into())));
            if let Some(v) = frame.video { e.push(("video".into(), n(v as f64 + 1.0))); }
            e.push(("time_s".into(), n((frame.at * 1e4).round() / 1e4)));
            if let Some(c) = frame.crop { e.push(("crop".into(), Value::Arr(c.iter().map(|v| n((v * 1e4).round() / 1e4)).collect()))); }
        }
    }
    for (k, v) in [("x", it.x), ("y", it.y), ("w", it.w), ("h", it.h)] { e.push((k.into(), n((v * 100.0).round() / 100.0))); }
    if it.rot != 0 { e.push(("rotation".into(), n(it.rot as f64))); }
    if !it.name.is_empty() { e.push(("name".into(), Value::Str(it.name.clone()))); }
    if it.layer != 0 { e.push(("layer".into(), n(it.layer as f64))); }
    Value::Obj(e)
}

fn item_from_value(e: &Value, k: usize, what: &str) -> Result<CoverItem, String> {
    let f = |key: &str| e.get(key).and_then(Value::as_f64).unwrap_or(0.0);
    let kind = match e.get("type").and_then(Value::as_str) {
        Some("texte") => CoverKind::Text {
            text: e.get("text").and_then(Value::as_str).unwrap_or("").to_string(),
            size_pt: e.get("size_pt").and_then(Value::as_f64).unwrap_or(24.0).clamp(1.0, 600.0),
            color: color_from(e.get("color"), [255, 255, 255]),
            align: e.get("align").and_then(Value::as_str).and_then(Align::from_name).unwrap_or(Align::Center),
            font: e.get("font").and_then(Value::as_str).unwrap_or("").to_string(),
            fx: {
                let d = TextFx::default();
                let g = |k: &str, dv: f64| e.get(k).and_then(Value::as_f64).filter(|v| v.is_finite()).unwrap_or(dv);
                TextFx {
                    outline_mm: g("outline_mm", d.outline_mm).clamp(0.0, 10.0),
                    outline_color: color_from(e.get("outline_color"), d.outline_color),
                    shadow_mm: g("shadow_mm", d.shadow_mm).clamp(0.0, 10.0),
                    shadow_color: color_from(e.get("shadow_color"), d.shadow_color),
                    spacing: g("line_spacing", d.spacing).clamp(0.3, 5.0),
                    valign: e.get("valign").and_then(Value::as_str).and_then(VAlign::from_name).unwrap_or(d.valign),
                    autofit: e.get("autofit").and_then(Value::as_bool).unwrap_or(false),
                    bold: e.get("bold").and_then(Value::as_bool).unwrap_or(false),
                    italic: e.get("italic").and_then(Value::as_bool).unwrap_or(false),
                }
            },
        },
        Some("forme") => CoverKind::Shape {
            shape: e.get("shape").and_then(Value::as_str).and_then(ShapeKind::from_name).unwrap_or(ShapeKind::Rect),
            fill: e.get("fill").map(|v| color_from(Some(v), [255, 255, 255])),
            stroke: e.get("border").map(|v| color_from(Some(v), [255, 255, 255])),
            stroke_mm: e.get("thickness_mm").and_then(Value::as_f64).filter(|v| v.is_finite()).unwrap_or(0.5).clamp(0.0, 50.0),
            radius_mm: e.get("radius_mm").and_then(Value::as_f64).filter(|v| v.is_finite()).unwrap_or(0.0).clamp(0.0, 200.0),
            flip: matches!(e.get("flipped"), Some(Value::Bool(true))),
            style: e.get("line_style").and_then(Value::as_str).and_then(LineStyle::from_name).unwrap_or(LineStyle::Solid),
            arrow: e.get("arrow").and_then(Value::as_str).and_then(LineArrow::from_name).unwrap_or(LineArrow::None),
        },
        Some("image") => CoverKind::Image { path: e.get("file").and_then(Value::as_str).ok_or_else(|| trf!("{}, item {}: “file” missing", what, k + 1))?.to_string() },
        Some("image_video") => CoverKind::VideoImage { frame: VideoFrame {
            video: e.get("video").and_then(Value::as_f64).filter(|v| *v >= 1.0).map(|v| v as usize - 1),
            at: e.get("time_s").and_then(Value::as_f64).filter(|v| v.is_finite()).unwrap_or(0.0).max(0.0),
            crop: match e.get("crop") {
                Some(Value::Arr(a)) if a.len() == 4 => {
                    let g = |i: usize| a[i].as_f64().filter(|v| v.is_finite()).unwrap_or(0.0).clamp(0.0, 1.0);
                    Some([g(0), g(1), g(2).max(0.01), g(3).max(0.01)])
                }
                _ => None,
            },
        } },
        Some("tableau_infos") => CoverKind::InfoTable { table: {
            let d = InfoTable::default();
            let g = |k: &str, dv: f64| e.get(k).and_then(Value::as_f64).filter(|v| v.is_finite()).unwrap_or(dv);
            let b = |k: &str, dv: bool| e.get(k).and_then(Value::as_bool).unwrap_or(dv);
            InfoTable {
                videos: match e.get("videos") { Some(Value::Arr(a)) => a.iter().filter_map(Value::as_f64).filter(|v| *v >= 1.0).map(|v| v as usize - 1).collect(), _ => vec![] },
                fields: match e.get("fields") { Some(Value::Arr(a)) => a.iter().filter_map(Value::as_str).filter_map(InfoField::from_name).collect(), _ => d.fields.clone() },
                header: b("header", d.header),
                transpose: b("transpose", false),
                size_pt: g("size_pt", d.size_pt).clamp(1.0, 200.0),
                color: color_from(e.get("color"), d.color),
                header_color: color_from(e.get("header_color"), d.header_color),
                header_bold: b("header_bold", d.header_bold),
                header_fill: e.get("header_fill").map(|v| color_from(Some(v), [0, 0, 0])),
                fill: e.get("fill").map(|v| color_from(Some(v), [0, 0, 0])),
                grid: e.get("grid").map(|v| color_from(Some(v), [255, 255, 255])),
                grid_mm: g("grid_mm", d.grid_mm).clamp(0.05, 5.0),
                font: e.get("font").and_then(Value::as_str).unwrap_or("").to_string(),
                align: e.get("align").and_then(Value::as_str).and_then(Align::from_name).unwrap_or(d.align),
            }
        } },
        t => return Err(trf!("{}, item {}: unknown type {}", what, k + 1, format!("{:?}", t))),
    };
    let min_side = if matches!(&kind, CoverKind::Shape { shape: ShapeKind::Line, .. }) { 0.0 } else { 1.0 };
    let rot = match f("rotation").round() as i64 { 90 => 90, 270 | -90 => 270, _ => 0 };
    Ok(CoverItem { kind, x: f("x"), y: f("y"), w: f("w").max(min_side), h: f("h").max(min_side), rot, name: e.get("name").and_then(Value::as_str).unwrap_or("").to_string(), layer: f("layer").max(0.0) as usize })
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
        ("name".into(), Value::Str(l.name.clone())), ("visible".into(), Value::Bool(l.visible)), ("locked".into(), Value::Bool(l.locked)),
    ])).collect())
}

fn layers_from_value(v: Option<&Value>) -> Vec<CoverLayer> {
    let Some(a) = v.and_then(Value::as_array) else { return vec![] };
    let l: Vec<CoverLayer> = a.iter().enumerate().map(|(k, e)| CoverLayer {
        name: e.get("name").and_then(Value::as_str).map(str::to_string).unwrap_or_else(|| format!("Calque {}", k + 1)),
        visible: e.get("visible").and_then(Value::as_bool).unwrap_or(true),
        locked: e.get("locked").and_then(Value::as_bool).unwrap_or(false),
    }).collect();
    normalize_layers(l)
}

/// Page A4 (largeur, hauteur en mm) la mieux adaptee a un format fini `tw` x `th` avec 8 mm de marge pour les reperes de coupe :
/// portrait si possible, sinon paysage.
fn a4_page(tw: f64, th: f64, what: &str) -> Result<(f64, f64), String> {
    let (nw, nh) = (tw + 16.0, th + 16.0);
    if nw <= 210.0 && nh <= 297.0 { Ok((210.0, 297.0)) }
    else if nw <= 297.0 && nh <= 210.0 { Ok((297.0, 210.0)) }
    else { Err(trf!("{} ({} × {} mm, plus 8 mm of margin for the marks) does not fit on an A4 page.", what, format!("{:.0}", tw), format!("{:.0}", th))) }
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
    if handle != 0 && (w < 1.0 || h < 1.0) { return (r, None, None); }
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
