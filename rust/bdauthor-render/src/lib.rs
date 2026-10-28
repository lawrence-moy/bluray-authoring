//! Rendu des images de boutons (equivalent des parties PIL de `menu.py`) : chargement PNG/JPEG, texte avec police
//! systeme, rectangles arrondis. Ce crate depend de `image` et `ab_glyph` (non compilables dans l'environnement hors ligne
//! du developpement : valides par `cargo test` sous Windows).
use ab_glyph::{point, Font, FontVec, PxScale, ScaleFont};
use bdauthor_core::design::{nav_applicable, Align, Kind, MenuType, PageDesign};
use bdauthor_core::palette::RgbaImage;
use bdauthor_core::project::Decor;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::sync::Arc;

pub type Result<T> = std::result::Result<T, String>;

/// Couleurs du mode texte (normal, selectionne, active) et du libelle.
pub const FILL_COLORS: [[u8; 3]; 3] = [[30, 60, 140], [90, 150, 230], [240, 150, 40]];
pub const TEXT_COLOR: [u8; 3] = [255, 255, 255];

/// Plus grand cote (px) d'un SVG converti en image ; l'image est ensuite reduite a la taille de son cadre.
pub const SVG_RASTER_MAX: u32 = 2048;

/// Fichier SVG (ou SVG compresse) d'apres son extension ?
pub fn is_svg(path: &Path) -> bool {
    path.extension().and_then(|e| e.to_str()).map_or(false, |e| e.eq_ignore_ascii_case("svg") || e.eq_ignore_ascii_case("svgz"))
}

/// Dessine un SVG (bibliotheque resvg) a la taille `max_side` pour son plus grand cote, fond transparent.
pub fn load_svg(path: &Path, max_side: u32) -> Result<RgbaImage> {
    use resvg::{tiny_skia, usvg};
    static FONTS: std::sync::OnceLock<Arc<usvg::fontdb::Database>> = std::sync::OnceLock::new();
    let data = std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let mut opt = usvg::Options::default();
    opt.resources_dir = path.parent().map(Path::to_path_buf);
    // polices du systeme pour les textes du SVG (chargees une seule fois)
    opt.fontdb = FONTS.get_or_init(|| {
        let mut db = usvg::fontdb::Database::new();
        db.load_system_fonts();
        Arc::new(db)
    }).clone();
    let tree = usvg::Tree::from_data(&data, &opt).map_err(|e| format!("{} : SVG illisible ({e})", path.display()))?;
    let size = tree.size();
    let (sw, sh) = (size.width().max(1.0), size.height().max(1.0));
    let k = max_side as f32 / sw.max(sh);
    let (w, h) = (((sw * k).round() as u32).max(1), ((sh * k).round() as u32).max(1));
    let mut pm = tiny_skia::Pixmap::new(w, h).ok_or_else(|| format!("{} : taille de SVG invalide", path.display()))?;
    resvg::render(&tree, tiny_skia::Transform::from_scale(w as f32 / sw, h as f32 / sh), &mut pm.as_mut());
    // tiny-skia travaille en alpha premultiplie : retour aux couleurs directes
    let mut px = pm.take();
    for p in px.chunks_exact_mut(4) {
        let a = p[3] as u32;
        if a > 0 && a < 255 {
            for c in &mut p[..3] {
                *c = ((*c as u32 * 255 + a / 2) / a).min(255) as u8;
            }
        }
    }
    RgbaImage::from_raw(w, h, px)
}

/// Charge une image PNG, JPEG, BMP ou SVG.
pub fn load_image(path: &Path) -> Result<RgbaImage> {
    if is_svg(path) {
        return load_svg(path, SVG_RASTER_MAX);
    }
    let im = image::open(path).map_err(|e| format!("{}: {e}", path.display()))?.to_rgba8();
    let (w, h) = im.dimensions();
    RgbaImage::from_raw(w, h, im.into_raw())
}

/// Reduit/agrandit `src` pour tenir dans (w, h) en conservant le rapport (filtre Lanczos), centre sur fond transparent.
pub fn fit_image(src: &RgbaImage, w: u32, h: u32) -> RgbaImage {
    let buf = image::RgbaImage::from_raw(src.width, src.height, src.data.clone()).expect("tampon coherent");
    let scale = (w as f64 / src.width as f64).min(h as f64 / src.height as f64);
    let (nw, nh) = (((src.width as f64 * scale).round() as u32).max(1), ((src.height as f64 * scale).round() as u32).max(1));
    let r = image::imageops::resize(&buf, nw, nh, image::imageops::FilterType::Lanczos3);
    let mut out = RgbaImage::new(w, h);
    let (ox, oy) = ((w - nw.min(w)) / 2, (h - nh.min(h)) / 2);
    for y in 0..nh.min(h) {
        for x in 0..nw.min(w) {
            out.set(ox + x, oy + y, r.get_pixel(x, y).0);
        }
    }
    out
}

/// Cherche une police : `BDAUTHOR_FONT`, puis polices usuelles de Windows, Linux, macOS.
pub fn find_font() -> Option<PathBuf> {
    if let Ok(p) = std::env::var("BDAUTHOR_FONT") {
        if Path::new(&p).is_file() {
            return Some(p.into());
        }
    }
    let dirs = ["C:\\Windows\\Fonts", "/usr/share/fonts/truetype/dejavu", "/usr/share/fonts/dejavu", "/usr/share/fonts/TTF", "/Library/Fonts", "/System/Library/Fonts/Supplemental"];
    let names = ["arialbd.ttf", "segoeuib.ttf", "calibrib.ttf", "DejaVuSans-Bold.ttf", "Arial Bold.ttf", "arial.ttf", "DejaVuSans.ttf"];
    for n in names {
        for d in dirs {
            let p = Path::new(d).join(n);
            if p.is_file() {
                return Some(p);
            }
        }
    }
    None
}

pub fn load_font(path: &Path) -> Result<FontVec> {
    let data = std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
    FontVec::try_from_vec(data).map_err(|e| format!("police invalide {}: {e}", path.display()))
}

/// Dimensions (largeur, hauteur, decalage x) du texte au corps `size` px.
fn text_extent(font: &FontVec, text: &str, size: f32) -> (f32, f32) {
    let sf = font.as_scaled(PxScale::from(size));
    let mut w = 0.0;
    let mut prev = None;
    for c in text.chars() {
        let id = font.glyph_id(c);
        if let Some(p) = prev {
            w += sf.kern(p, id);
        }
        w += sf.h_advance(id);
        prev = Some(id);
    }
    (w, sf.ascent() - sf.descent())
}

/// Dessine `text` (couleur `rgb`) au centre de `img`.
pub fn draw_text_centered(img: &mut RgbaImage, font: &FontVec, text: &str, size: f32, rgb: [u8; 3]) {
    let (w, h) = (img.width, img.height);
    draw_text_in(img, font, text, size, rgb, (0, 0, w, h));
}

/// Dessine `text` centre dans le rectangle (x, y, largeur, hauteur) de `img`.
pub fn draw_text_in(img: &mut RgbaImage, font: &FontVec, text: &str, size: f32, rgb: [u8; 3], rect: (u32, u32, u32, u32)) {
    draw_line(img, font, text, size, rgb, rect, Align::Center);
}

/// Texte sur une ou plusieurs lignes (separees par « \n »), centre verticalement dans `rect` (coordonnees pouvant deborder de l'image).
pub fn draw_text_block(img: &mut RgbaImage, font: &FontVec, text: &str, size: f32, rgb: [u8; 3], rect: (i32, i32, i32, i32), align: Align) {
    let lines: Vec<&str> = text.split('\n').collect();
    let lh = text_extent(font, "Ag", size).1 * 1.1;
    let top = rect.1 as f32 + (rect.3 as f32 - lh * lines.len() as f32) / 2.0;
    for (k, l) in lines.iter().enumerate() {
        let y = top + lh * k as f32;
        draw_line_f(img, font, l, size, rgb, (rect.0 as f32, y, rect.2 as f32, lh), align);
    }
}

fn draw_line(img: &mut RgbaImage, font: &FontVec, text: &str, size: f32, rgb: [u8; 3], rect: (u32, u32, u32, u32), align: Align) {
    draw_line_f(img, font, text, size, rgb, (rect.0 as f32, rect.1 as f32, rect.2 as f32, rect.3 as f32), align);
}

fn draw_line_f(img: &mut RgbaImage, font: &FontVec, text: &str, size: f32, rgb: [u8; 3], rect: (f32, f32, f32, f32), align: Align) {
    if text.is_empty() || size < 1.0 {
        return;
    }
    let sf = font.as_scaled(PxScale::from(size));
    let (tw, th) = text_extent(font, text, size);
    let mut x = rect.0 + match align { Align::Left => 0.0, Align::Center => (rect.2 - tw) / 2.0, Align::Right => rect.2 - tw };
    let base = rect.1 + (rect.3 - th) / 2.0 + sf.ascent();
    let mut prev = None;
    for c in text.chars() {
        let id = font.glyph_id(c);
        if let Some(p) = prev {
            x += sf.kern(p, id);
        }
        let g = id.with_scale_and_position(PxScale::from(size), point(x, base));
        x += sf.h_advance(id);
        prev = Some(id);
        if let Some(og) = font.outline_glyph(g) {
            let b = og.px_bounds();
            og.draw(|gx, gy, cov| {
                let (px, py) = (b.min.x as i64 + gx as i64, b.min.y as i64 + gy as i64);
                if px < 0 || py < 0 || px >= img.width as i64 || py >= img.height as i64 {
                    return;
                }
                let [r, g2, b2, a] = img.pixel(px as u32, py as u32);
                let cv = cov.clamp(0.0, 1.0);
                let mix = |d: u8, s: u8| (d as f32 * (1.0 - cv) + s as f32 * cv).round() as u8;
                let na = (a as f32 + (255.0 - a as f32) * cv).round() as u8;
                if a == 0 {
                    img.set(px as u32, py as u32, [rgb[0], rgb[1], rgb[2], na]);
                } else {
                    img.set(px as u32, py as u32, [mix(r, rgb[0]), mix(g2, rgb[1]), mix(b2, rgb[2]), na]);
                }
            });
        }
    }
}

/// Rectangle arrondi plein (rayon `radius`), bords lisses (alpha partiel sur 1 px).
pub fn rounded_rect(w: u32, h: u32, radius: f32, rgb: [u8; 3]) -> RgbaImage {
    let mut im = RgbaImage::new(w, h);
    let (hw, hh) = (w as f32 / 2.0, h as f32 / 2.0);
    for y in 0..h {
        for x in 0..w {
            let (px, py) = (x as f32 + 0.5 - hw, y as f32 + 0.5 - hh);
            let (qx, qy) = (px.abs() - (hw - radius), py.abs() - (hh - radius));
            let d = (qx.max(0.0).powi(2) + qy.max(0.0).powi(2)).sqrt() + qx.max(qy).min(0.0) - radius;
            let a = (0.5 - d).clamp(0.0, 1.0);
            if a > 0.0 {
                im.set(x, y, [rgb[0], rgb[1], rgb[2], (a * 255.0).round() as u8]);
            }
        }
    }
    im
}

/// Trois images (normal, selectionne, active) d'un bouton texte.
pub fn text_button_states(font: &FontVec, label: &str, w: u32, h: u32) -> Vec<Arc<RgbaImage>> {
    FILL_COLORS
        .iter()
        .map(|c| {
            let mut im = rounded_rect(w, h, 16.0, *c);
            draw_text_centered(&mut im, font, label, 40.0, TEXT_COLOR);
            Arc::new(im)
        })
        .collect()
}

// ------------------------------------------------------------------ elements de menu du projet
/// Reduit le corps jusqu'a 12 px puis tronque avec « ... » pour que `text` tienne dans `width` (marge de 12 px).
pub fn fit_text(font: &FontVec, text: &str, width: u32, mut size: f32) -> (String, f32) {
    let fits = |t: &str, sz: f32| text_extent(font, t, sz).0 <= width as f32 - 12.0;
    while !fits(text, size) && size > 12.0 {
        size -= 2.0;
    }
    let mut t = text.to_string();
    if !fits(&t, size) {
        while t.chars().count() > 1 && !fits(&format!("{t}..."), size) {
            t.pop();
        }
        t.push_str("...");
    }
    (t, size)
}

fn fill_rect(w: u32, h: u32, rgb: [u8; 3]) -> RgbaImage {
    let mut im = RgbaImage::new(w, h);
    for y in 0..h {
        for x in 0..w {
            im.set(x, y, [rgb[0], rgb[1], rgb[2], 255]);
        }
    }
    im
}

fn border(im: &mut RgbaImage, selected: bool) {
    let w = im.width;
    let bw = if selected { (w / 60).max(3) } else { 1 };
    let col = if selected { [255, 255, 255, 255] } else { [70, 85, 120, 255] };
    for k in 0..bw.min(im.width / 2).min(im.height / 2) {
        for x in k..im.width - k {
            im.set(x, k, col);
            im.set(x, im.height - 1 - k, col);
        }
        for y in k..im.height - k {
            im.set(k, y, col);
            im.set(im.width - 1 - k, y, col);
        }
    }
}

const NORMAL_BG: [u8; 3] = [24, 32, 58];
const SELECTED_BG: [u8; 3] = [60, 100, 190];

/// Moteur de dessin complet pour `bdauthor_core::project` (police systeme, voir `find_font`).
pub struct FontRenderer {
    font: FontVec,
    /// Images deja chargees (cle : chemin et date de modification), pour l'apercu interactif.
    images: std::sync::Mutex<std::collections::HashMap<(PathBuf, u64), Arc<RgbaImage>>>,
    /// Elements deja dessines (boutons, textes, images redimensionnees, degrade) pour l'apercu : la cle resume tout ce qui influe sur le dessin.
    sprites: std::sync::Mutex<std::collections::HashMap<u64, Arc<RgbaImage>>>,
    /// Polices personnalisees deja chargees (cle : nom ou chemin tel qu'ecrit dans le projet) ; `None` = introuvable.
    fonts: std::sync::Mutex<std::collections::HashMap<String, Option<Arc<FontVec>>>>,
}

impl FontRenderer {
    pub fn new() -> Result<Self> {
        let p = find_font().ok_or("aucune police trouvee : indiquez un fichier .ttf dans la variable BDAUTHOR_FONT")?;
        Ok(FontRenderer::with_font(load_font(&p)?))
    }
    pub fn with_font(font: FontVec) -> Self {
        FontRenderer { font, images: Default::default(), sprites: Default::default(), fonts: Default::default() }
    }

    /// Charge une image (avec cache).
    pub fn cached_image(&self, path: &Path) -> Result<Arc<RgbaImage>> {
        let mtime = std::fs::metadata(path).ok().and_then(|m| m.modified().ok()).and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map_or(0, |d| d.as_secs());
        let key = (path.to_path_buf(), mtime);
        if let Some(i) = self.images.lock().unwrap().get(&key) {
            return Ok(i.clone());
        }
        let im = Arc::new(load_image(path)?);
        self.images.lock().unwrap().insert(key, im.clone());
        Ok(im)
    }

    /// Pose textes et images (`decor`) sur `im`.
    pub fn compose_on(&self, mut im: RgbaImage, decor: &[Decor]) -> Result<RgbaImage> {
        for d in decor {
            match d {
                Decor::Text { text, x, y, w, h, size: sz, color, align, font } => {
                    let custom = self.custom_font(font, Path::new(""));
                    draw_text_block(&mut im, custom.as_deref().unwrap_or(&self.font), text, *sz as f32, *color, (*x, *y, *w, *h), *align)
                }
                Decor::Image { path, x, y, w, h } => {
                    let src = self.cached_image(path)?;
                    let sc = (*w as f64 / src.width as f64).min(*h as f64 / src.height as f64);
                    let (nw, nh) = (((src.width as f64 * sc).round() as u32).max(1), ((src.height as f64 * sc).round() as u32).max(1));
                    blit(&mut im, &resize(&src, nw, nh), *x + (*w - nw as i32) / 2, *y + (*h - nh as i32) / 2, 1.0);
                }
            }
        }
        Ok(im)
    }
}

impl bdauthor_core::project::Renderer for FontRenderer {
    fn thumb_button(&self, thumb: &RgbaImage, label: &str, w: u32, h_total: u32, h_thumb: u32) -> [Arc<RgbaImage>; 2] {
        let mk = |selected: bool| {
            let mut im = fill_rect(w, h_total, if selected { SELECTED_BG } else { NORMAL_BG });
            for y in 0..h_thumb.min(thumb.height).min(h_total) {
                for x in 0..w.min(thumb.width) {
                    im.set(x, y, thumb.pixel(x, y));
                }
            }
            let (txt, sz) = fit_text(&self.font, label, w, ((h_total - h_thumb) as f32 * 0.55).max(14.0));
            draw_text_in(&mut im, &self.font, &txt, sz, [255, 255, 255], (0, h_thumb, w, h_total - h_thumb));
            border(&mut im, selected);
            Arc::new(im)
        };
        [mk(false), mk(true)]
    }

    fn text_button(&self, label: &str, w: u32, h: u32) -> [Arc<RgbaImage>; 2] {
        let mk = |selected: bool| {
            let mut im = fill_rect(w, h, if selected { SELECTED_BG } else { NORMAL_BG });
            let (txt, sz) = fit_text(&self.font, label, w, (h as f32 * 0.5).max(14.0));
            draw_text_centered(&mut im, &self.font, &txt, sz, [255, 255, 255]);
            border(&mut im, selected);
            Arc::new(im)
        };
        [mk(false), mk(true)]
    }

    fn decor_button(&self, decor: &Decor, w: u32, h: u32) -> [Arc<RgbaImage>; 2] {
        let one = std::slice::from_ref(decor);
        // normal : l'element seul sur fond transparent ; selectionne : sur un cadre bleu translucide borde de blanc
        let normal = self.compose_on(RgbaImage::new(w, h), one).unwrap_or_else(|_| RgbaImage::new(w, h));
        let mut back = rounded_rect(w, h, (w.min(h) as f32 * 0.15).min(16.0), SELECTED_BG);
        for p in back.data.chunks_exact_mut(4) {
            p[3] = (p[3] as u32 * 150 / 255) as u8;
        }
        let mut sel = self.compose_on(back.clone(), one).unwrap_or(back);
        border(&mut sel, true);
        [Arc::new(normal), Arc::new(sel)]
    }

    fn overlay(&self, states: [Arc<RgbaImage>; 2], decor: &[Decor]) -> [Arc<RgbaImage>; 2] {
        let one = |a: &Arc<RgbaImage>| Arc::new(self.compose_on(a.as_ref().clone(), decor).unwrap_or_else(|_| a.as_ref().clone()));
        [one(&states[0]), one(&states[1])]
    }

    fn subtitle(&self, text: &str, style: &bdauthor_core::subtitle::SubStyle, max_w: u32) -> Option<RgbaImage> {
        if text.trim().is_empty() {
            return None;
        }
        let custom = self.custom_font(&style.font, Path::new(""));
        let font: &FontVec = custom.as_deref().unwrap_or(&self.font);
        let lines: Vec<&str> = text.split('\n').collect();
        let outline = style.outline.max(0);
        let pad = outline + 4;
        // la plus longue ligne doit tenir dans la largeur de la video : la taille est reduite sinon
        let avail = (max_w as f32 * 0.92 - 2.0 * pad as f32).max(40.0);
        let mut size = style.size.max(8) as f32;
        let widest = |sz: f32| lines.iter().map(|l| text_extent(font, l, sz).0).fold(0.0f32, f32::max);
        let wide = widest(size);
        if wide > avail {
            size = (size * avail / wide).max(8.0);
        }
        let lh = text_extent(font, "Ag", size).1 * 1.1;
        let (tw, th) = (widest(size).ceil() as i32 + 2, (lh * lines.len() as f32).ceil() as i32);
        let (w, h) = (tw + 2 * pad, th + 2 * pad);
        let mut im = RgbaImage::new(w as u32, h as u32);
        let rect = |dx: i32, dy: i32| (pad + dx, pad + dy, tw, th);
        if outline > 0 {
            // contour : le texte redessine a tous les decalages entiers du disque de rayon `outline`
            for dy in -outline..=outline {
                for dx in -outline..=outline {
                    if dx * dx + dy * dy <= outline * outline + outline / 2 {
                        draw_text_block(&mut im, font, text, size, style.outline_color, rect(dx, dy), Align::Center);
                    }
                }
            }
        }
        draw_text_block(&mut im, font, text, size, style.color, rect(0, 0), Align::Center);
        Some(im)
    }

    fn compose_background(&self, base: Option<&Path>, size: (u32, u32), decor: &[Decor]) -> std::result::Result<RgbaImage, String> {
        let im = match base {
            Some(f) => cover_resize(self.cached_image(f)?.as_ref(), size.0, size.1),
            None => gradient(size),
        };
        self.compose_on(im, decor)
    }

    fn background(&self, title: &str, size: (u32, u32)) -> RgbaImage {
        let w = size.0;
        let mut im = gradient(size);
        let sz = 84.0 * w as f32 / 1920.0;
        let (txt, sz) = fit_text(&self.font, title, w.saturating_sub(200).max(50), sz);
        let top = (110.0 * w as f32 / 1920.0).round() as u32;
        draw_text_in(&mut im, &self.font, &txt, sz, [255, 255, 255], (0, top, w, (sz * 1.3) as u32));
        im
    }
}

// ------------------------------------------------------------------ images, fond, apercu
/// Degrade bleu par defaut du fond de menu.
pub fn gradient(size: (u32, u32)) -> RgbaImage {
    let (w, h) = size;
    let mut im = RgbaImage::new(w, h);
    for y in 0..h {
        let t = y as f64 / (h.max(2) - 1) as f64;
        let c = [(18.0 + 20.0 * t).round() as u8, (26.0 + 30.0 * t).round() as u8, (60.0 + 50.0 * t).round() as u8, 255];
        for x in 0..w {
            im.set(x, y, c);
        }
    }
    im
}

fn to_buf(src: &RgbaImage) -> image::RgbaImage {
    image::RgbaImage::from_raw(src.width, src.height, src.data.clone()).expect("tampon coherent")
}

fn from_buf(b: image::RgbaImage) -> RgbaImage {
    let (w, h) = b.dimensions();
    RgbaImage { width: w, height: h, data: b.into_raw() }
}

/// Redimensionne sans conserver le rapport (Lanczos).
pub fn resize(src: &RgbaImage, w: u32, h: u32) -> RgbaImage {
    from_buf(image::imageops::resize(&to_buf(src), w.max(1), h.max(1), image::imageops::FilterType::Lanczos3))
}

/// Remplit (w, h) en conservant le rapport : redimensionne puis rogne au centre (comme le fond `-vf scale+crop`).
pub fn cover_resize(src: &RgbaImage, w: u32, h: u32) -> RgbaImage {
    let sc = (w as f64 / src.width as f64).max(h as f64 / src.height as f64);
    let (nw, nh) = (((src.width as f64 * sc).round() as u32).max(w), ((src.height as f64 * sc).round() as u32).max(h));
    let r = to_buf(&resize(src, nw, nh));
    from_buf(image::imageops::crop_imm(&r, (nw - w) / 2, (nh - h) / 2, w, h).to_image())
}

/// Superpose `src` sur `dst` en (x, y) (peut deborder), opacite globale `opacity`.
pub fn blit(dst: &mut RgbaImage, src: &RgbaImage, x: i32, y: i32, opacity: f32) {
    let (x0, x1) = (x.max(0), (x + src.width as i32).min(dst.width as i32));
    let (y0, y1) = (y.max(0), (y + src.height as i32).min(dst.height as i32));
    if x0 >= x1 || y0 >= y1 {
        return;
    }
    let (dw, sw) = (dst.width as usize, src.width as usize);
    for dy in y0..y1 {
        let srow = ((dy - y) as usize * sw + (x0 - x) as usize) * 4;
        let drow = (dy as usize * dw + x0 as usize) * 4;
        for k in 0..(x1 - x0) as usize {
            let (si, di) = (srow + k * 4, drow + k * 4);
            let s = [src.data[si], src.data[si + 1], src.data[si + 2], src.data[si + 3]];
            if s[3] == 255 && opacity >= 1.0 {
                dst.data[di..di + 4].copy_from_slice(&s);
                continue;
            }
            let a = s[3] as f32 / 255.0 * opacity;
            if a <= 0.0 { continue; }
            let d = [dst.data[di], dst.data[di + 1], dst.data[di + 2], dst.data[di + 3]];
            let mix = |dc: u8, sc: u8| (dc as f32 * (1.0 - a) + sc as f32 * a).round() as u8;
            let na = (d[3] as f32 + (255.0 - d[3] as f32) * a).round() as u8;
            dst.data[di..di + 4].copy_from_slice(&[mix(d[0], s[0]), mix(d[1], s[1]), mix(d[2], s[2]), na]);
        }
    }
}

/// Un element a placer sur une page : son libelle et (si disponible) la miniature reelle de la video ou du chapitre.
#[derive(Clone)]
pub struct PreviewSlot {
    pub label: String,
    pub thumb: Option<Arc<RgbaImage>>,
    /// Identifie le contenu de la miniature (change quand l'image change) : sert de cle de cache.
    pub key: u64,
}

fn placeholder_thumb(w: u32, h: u32, n: usize, font: &FontVec) -> RgbaImage {
    let mut im = RgbaImage::new(w, h);
    for y in 0..h {
        let t = y as f32 / h.max(2) as f32;
        let c = [(70.0 - 30.0 * t) as u8, (80.0 - 30.0 * t) as u8, (100.0 - 30.0 * t) as u8, 255];
        for x in 0..w { im.set(x, y, c); }
    }
    draw_text_centered(&mut im, font, &n.to_string(), h as f32 * 0.5, [200, 210, 230]);
    im
}

/// Dossiers ou chercher les polices installees.
fn system_font_dirs() -> Vec<PathBuf> {
    let mut v = vec![PathBuf::from("C:\\Windows\\Fonts"), "/usr/share/fonts/truetype/dejavu".into(), "/usr/share/fonts/TTF".into(), "/Library/Fonts".into(), "/System/Library/Fonts/Supplemental".into()];
    if let Ok(w) = std::env::var("WINDIR") {
        v.insert(0, Path::new(&w).join("Fonts"));
    }
    if let Ok(l) = std::env::var("LOCALAPPDATA") {
        v.push(Path::new(&l).join("Microsoft\\Windows\\Fonts"));
    }
    v
}

/// Polices usuelles installees : (libelle, nom de fichier).
pub fn common_fonts() -> Vec<(&'static str, &'static str)> {
    const ALL: [(&str, &str); 22] = [
        ("Arial", "arial.ttf"), ("Arial gras", "arialbd.ttf"), ("Segoe UI", "segoeui.ttf"), ("Segoe UI gras", "segoeuib.ttf"),
        ("Calibri", "calibri.ttf"), ("Calibri gras", "calibrib.ttf"), ("Verdana", "verdana.ttf"), ("Tahoma", "tahoma.ttf"),
        ("Trebuchet MS", "trebuc.ttf"), ("Times New Roman", "times.ttf"), ("Georgia", "georgia.ttf"), ("Georgia gras", "georgiab.ttf"),
        ("Palatino Linotype", "pala.ttf"), ("Garamond", "GARA.TTF"), ("Impact", "impact.ttf"), ("Comic Sans MS", "comic.ttf"),
        ("Courier New", "cour.ttf"), ("Consolas", "consola.ttf"), ("Cambria", "cambria.ttc"), ("DejaVu Sans", "DejaVuSans.ttf"),
        ("DejaVu Sans gras", "DejaVuSans-Bold.ttf"), ("DejaVu Serif", "DejaVuSerif.ttf"),
    ];
    let dirs = system_font_dirs();
    ALL.into_iter().filter(|(_, f)| !f.ends_with(".ttc") && dirs.iter().any(|d| d.join(f).is_file())).collect()
}

fn hkey(f: impl FnOnce(&mut std::collections::hash_map::DefaultHasher)) -> u64 {
    let mut h = std::collections::hash_map::DefaultHasher::new();
    f(&mut h);
    h.finish()
}

impl FontRenderer {
    /// Police personnalisee `name` : chemin (absolu, ou relatif a `project_dir`) ou nom d'un fichier de police installe.
    /// `None` si `name` est vide ou si la police est introuvable (le dessin retombe alors sur la police par defaut).
    pub fn custom_font(&self, name: &str, project_dir: &Path) -> Option<Arc<FontVec>> {
        if name.is_empty() {
            return None;
        }
        let key = format!("{}|{}", project_dir.display(), name);
        if let Some(f) = self.fonts.lock().unwrap().get(&key) {
            return f.clone();
        }
        let direct = if Path::new(name).is_absolute() { PathBuf::from(name) } else { project_dir.join(name) };
        let path = if direct.is_file() { Some(direct) } else { system_font_dirs().into_iter().map(|d| d.join(name)).find(|p| p.is_file()) };
        let f = path.and_then(|p| load_font(&p).ok()).map(Arc::new);
        self.fonts.lock().unwrap().insert(key, f.clone());
        f
    }

    /// Retourne l'element en cache pour `key`, ou le dessine avec `make`. Le cache est vide quand il depasse 160 elements.
    fn cached_sprite(&self, key: u64, make: impl FnOnce() -> RgbaImage) -> Arc<RgbaImage> {
        if let Some(i) = self.sprites.lock().unwrap().get(&key) {
            return i.clone();
        }
        let im = Arc::new(make());
        let mut m = self.sprites.lock().unwrap();
        if m.len() >= 160 {
            m.clear();
        }
        m.insert(key, im.clone());
        im
    }

    /// Texte dessine sur fond transparent, dans une image un peu plus grande que son cadre (le texte peut deborder) ;
    /// retourne l'image et la marge (px, py) a retrancher a la position du cadre.
    #[allow(clippy::too_many_arguments)]
    fn text_sprite(&self, text: &str, size: i32, color: [u8; 3], align: Align, font_name: &str, project_dir: &Path, w: i32, h: i32) -> (Arc<RgbaImage>, i32, i32) {
        let custom = self.custom_font(font_name, project_dir);
        let font: &FontVec = custom.as_deref().unwrap_or(&self.font);
        let lines: Vec<&str> = text.split('\n').collect();
        let maxw = lines.iter().map(|l| text_extent(font, l, size as f32).0).fold(0.0f32, f32::max);
        let lh = text_extent(font, "Ag", size as f32).1 * 1.1;
        let px = ((maxw - w as f32).max(0.0) / 2.0).ceil() as i32 + 4 + size / 4;
        let py = ((lh * lines.len() as f32 - h as f32).max(0.0) / 2.0).ceil() as i32 + 4 + size / 2;
        let (cw, ch) = ((w + 2 * px).clamp(1, 8192), (h + 2 * py).clamp(1, 8192));
        let key = hkey(|hh| ("texte", text, size, color, align.name(), font_name, w, h).hash(hh));
        let spr = self.cached_sprite(key, || {
            let mut im = RgbaImage::new(cw as u32, ch as u32);
            draw_text_block(&mut im, font, text, size as f32, color, (px, py, w, h), align);
            im
        });
        (spr, px, py)
    }

    /// `base` : fond deja redimensionne a `size` (ou `None` pour le degrade par defaut).
    /// Apercu fidele d'une page (meme code de dessin que la construction du disque), a la taille `size`.
    /// `project_dir` sert a resoudre les chemins relatifs des images. Les boutons de navigation sans objet sur cette page sont attenues.
    #[allow(clippy::too_many_arguments)]
    pub fn render_page_preview(&self, design: &PageDesign, menu: MenuType, page: usize, pages: usize, slots: &[PreviewSlot], base: Option<&RgbaImage>, size: (u32, u32), project_dir: &Path) -> RgbaImage {
        use bdauthor_core::project::Renderer;
        let s = size.0 as f64 / 1920.0;
        let r = |v: i32| (v as f64 * s).round() as i32;
        let abs = |f: &str| { let q = Path::new(f); if q.is_absolute() { q.to_path_buf() } else { project_dir.join(q) } };
        let mut im = match base {
            Some(b) => b.clone(),
            None => self.cached_sprite(hkey(|h| ("degrade", size).hash(h)), || gradient(size)).as_ref().clone(),
        };
        // dans l'ordre de superposition de la mise en page (le dernier est au premier plan)
        let assignment = design.slot_assignment(slots.len());
        let mut k = 0;
        for it in &design.items {
            let (x, y, w, h) = (r(it.x), r(it.y), r(it.w).max(1), r(it.h).max(1));
            match &it.kind {
                Kind::Text { text, size: sz, color, align, font, .. } => {
                    let (spr, px, py) = self.text_sprite(text, r(*sz), *color, *align, font, project_dir, w, h);
                    blit(&mut im, &spr, x - px, y - py, 1.0);
                }
                Kind::Image { path, .. } => {
                    let Ok(src) = self.cached_image(&abs(path)) else { continue };
                    let sc = (w as f64 / src.width as f64).min(h as f64 / src.height as f64);
                    let (nw, nh) = (((src.width as f64 * sc).round() as u32).max(1), ((src.height as f64 * sc).round() as u32).max(1));
                    let rs = self.cached_sprite(hkey(|hh| ("image", Arc::as_ptr(&src) as usize, nw, nh).hash(hh)), || resize(&src, nw, nh));
                    blit(&mut im, &rs, x + (w - nw as i32) / 2, y + (h - nh as i32) / 2, 1.0);
                }
                Kind::Slot { thumb_h, .. } => {
                    let th = r(*thumb_h).clamp(1, h);
                    let element = assignment.get(k).copied().unwrap_or(k);
                    let slot = slots.get(element);
                    let key = hkey(|hh| ("vignette", slot.map(|s| (&s.label, s.key)), element, w, h, th).hash(hh));
                    let n = self.cached_sprite(key, || {
                        let (label, thumb) = match slot {
                            Some(sl) => (sl.label.clone(), sl.thumb.as_ref().map(|t| cover_resize(t, w as u32, th as u32))),
                            None => (if element == usize::MAX { "(vide)".to_string() } else { format!("Élément {}", element + 1) }, None),
                        };
                        let thumb = thumb.unwrap_or_else(|| placeholder_thumb(w as u32, th as u32, element + 1, &self.font));
                        let [n, _] = self.thumb_button(&thumb, &label, w as u32, h as u32, th as u32);
                        n.as_ref().clone()
                    });
                    blit(&mut im, &n, x, y, 1.0);
                    k += 1;
                }
                Kind::Nav { action, text, .. } => {
                    let n = self.cached_sprite(hkey(|hh| ("bouton", text, w, h).hash(hh)), || {
                        let [n, _] = self.text_button(text, w as u32, h as u32);
                        n.as_ref().clone()
                    });
                    blit(&mut im, &n, x, y, if nav_applicable(*action, menu, page, pages) { 1.0 } else { 0.35 });
                }
            }
        }
        im
    }
}
