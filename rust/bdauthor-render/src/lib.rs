//! Rendu des images de boutons (equivalent des parties PIL de `menu.py`) : chargement PNG/JPEG, texte avec police
//! systeme, rectangles arrondis. Ce crate depend de `image` et `ab_glyph` (non compilables dans l'environnement hors ligne
//! du developpement : valides par `cargo test` sous Windows).
use ab_glyph::{point, Font, FontVec, PxScale, ScaleFont};
use bdauthor_core::design::{nav_applicable, Align, Kind, PageDesign, TextDeco};
use bdauthor_core::palette::RgbaImage;
use bdauthor_core::project::Decor;
use bdauthor_core::style::{BtnState, ButtonStyle};
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

/// Polices du systeme pour les textes des SVG : leur chargement lit toutes les polices installees (une a plusieurs secondes sous
/// Windows), une seule fois, et seulement pour un SVG qui contient du texte.
static SVG_FONTS: std::sync::OnceLock<Arc<resvg::usvg::fontdb::Database>> = std::sync::OnceLock::new();

fn svg_fonts() -> Arc<resvg::usvg::fontdb::Database> {
    SVG_FONTS.get_or_init(|| {
        let mut db = resvg::usvg::fontdb::Database::new();
        db.load_system_fonts();
        if let Some(d) = extra_font_dir() { db.load_fonts_dir(d); }
        Arc::new(db)
    }).clone()
}

/// Le SVG (contenu du fichier) a-t-il du texte a dessiner ? Un SVG compresse (svgz) est suppose en avoir.
fn svg_data_has_text(data: &[u8]) -> bool {
    if data.starts_with(&[0x1f, 0x8b]) { return true; }
    data.windows(5).any(|w| w == b"<text") || data.windows(5).any(|w| w == b":text")
}

/// Le fichier SVG `path` a-t-il besoin des polices du systeme (il contient du texte) ?
pub fn svg_needs_fonts(path: &Path) -> bool {
    is_svg(path) && std::fs::read(path).is_ok_and(|d| svg_data_has_text(&d))
}

/// Charge en arriere-plan les polices du systeme si l'un des SVG de `paths` contient du texte : le premier dessin de ce SVG
/// n'attend plus. Sans SVG a texte, rien n'est charge.
pub fn preload_svg_fonts(paths: Vec<PathBuf>) {
    std::thread::spawn(move || {
        if SVG_FONTS.get().is_none() && paths.iter().any(|p| svg_needs_fonts(p)) { svg_fonts(); }
    });
}

/// Dessine un SVG (bibliotheque resvg) a la taille `max_side` pour son plus grand cote, fond transparent.
pub fn load_svg(path: &Path, max_side: u32) -> Result<RgbaImage> {
    use resvg::{tiny_skia, usvg};
    let data = std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let mut opt = usvg::Options::default();
    opt.resources_dir = path.parent().map(Path::to_path_buf);
    // polices du systeme seulement pour un SVG qui a du texte (un logo n'est souvent fait que de traces)
    if svg_data_has_text(&data) { opt.fontdb = svg_fonts(); }
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

// ------------------------------------------------------------------ texte avec mise en forme (sous-titres)
/// Inclinaison de l'italique synthetique (decalage horizontal par pixel de hauteur au-dessus de la ligne de base).
const SLANT: f32 = 0.25;

/// Epaisseur ajoutee au gras synthetique (le texte est dessine deux fois, decale).
fn bold_off(size: f32) -> f32 {
    (size * 0.035).max(0.8)
}

fn run_width(font: &FontVec, r: &bdauthor_core::subtitle::Run, size: f32) -> f32 {
    text_extent(font, &r.text, size).0 + if r.bold { bold_off(size) } else { 0.0 }
}

fn line_width(font: &FontVec, runs: &[bdauthor_core::subtitle::Run], size: f32) -> f32 {
    runs.iter().map(|r| run_width(font, r, size)).sum()
}

/// Pose un pixel de couverture `cv` de couleur `rgb` (composition « par-dessus » sur une image qui peut etre transparente).
fn plot(img: &mut RgbaImage, px: i64, py: i64, rgb: [u8; 3], cv: f32) {
    if px < 0 || py < 0 || px >= img.width as i64 || py >= img.height as i64 {
        return;
    }
    let cv = cv.clamp(0.0, 1.0);
    if cv <= 0.0 {
        return;
    }
    let [r, g2, b2, a] = img.pixel(px as u32, py as u32);
    let mix = |d: u8, s: u8| (d as f32 * (1.0 - cv) + s as f32 * cv).round() as u8;
    let na = (a as f32 + (255.0 - a as f32) * cv).round() as u8;
    if a == 0 {
        img.set(px as u32, py as u32, [rgb[0], rgb[1], rgb[2], na]);
    } else {
        img.set(px as u32, py as u32, [mix(r, rgb[0]), mix(g2, rgb[1]), mix(b2, rgb[2]), na]);
    }
}

/// Une ligne de texte avec gras et/ou italique synthetiques (meme dessin que les sous-titres), placee dans `rect` selon `align`.
fn draw_line_styled(img: &mut RgbaImage, font: &FontVec, text: &str, size: f32, rgb: [u8; 3], rect: (f32, f32, f32, f32), align: Align, bold: bool, italic: bool) {
    if text.is_empty() || size < 1.0 {
        return;
    }
    let run = bdauthor_core::subtitle::Run { text: text.to_string(), bold, italic, underline: false, color: None };
    let sf = font.as_scaled(PxScale::from(size));
    let (tw, th) = (run_width(font, &run, size), text_extent(font, text, size).1);
    let mut x = rect.0 + match align { Align::Left => 0.0, Align::Center => (rect.2 - tw) / 2.0, Align::Right => rect.2 - tw };
    let base = rect.1 + (rect.3 - th) / 2.0 + sf.ascent();
    draw_run(img, font, &run, size, &mut x, base, rgb);
}

/// Dessine un morceau de texte a partir de `*x` (ligne de base `base`), puis avance `*x`.
fn draw_run(img: &mut RgbaImage, font: &FontVec, r: &bdauthor_core::subtitle::Run, size: f32, x: &mut f32, base: f32, rgb: [u8; 3]) {
    if r.text.is_empty() || size < 1.0 {
        return;
    }
    let sf = font.as_scaled(PxScale::from(size));
    let x0 = *x;
    let bo = bold_off(size);
    let passes = if r.bold { 2 } else { 1 };
    let mut end = x0;
    for k in 0..passes {
        let mut xx = x0 + if k == 1 { bo } else { 0.0 };
        let mut prev = None;
        for c in r.text.chars() {
            let id = font.glyph_id(c);
            if let Some(p) = prev {
                xx += sf.kern(p, id);
            }
            let g = id.with_scale_and_position(PxScale::from(size), point(xx, base));
            xx += sf.h_advance(id);
            prev = Some(id);
            if let Some(og) = font.outline_glyph(g) {
                let b = og.px_bounds();
                og.draw(|gx, gy, cov| {
                    let (px, py) = (b.min.x as i64 + gx as i64, b.min.y as i64 + gy as i64);
                    if r.italic {
                        // cisaillement : le haut du glyphe est decale vers la droite, la couverture est repartie sur deux colonnes
                        let xf = px as f32 + (base - (py as f32 + 0.5)) * SLANT;
                        let col = xf.floor();
                        let fr = xf - col;
                        plot(img, col as i64, py, rgb, cov * (1.0 - fr));
                        plot(img, col as i64 + 1, py, rgb, cov * fr);
                    } else {
                        plot(img, px, py, rgb, cov);
                    }
                });
            }
        }
        if k == 0 {
            end = xx;
        }
    }
    *x = end + if r.bold { bo } else { 0.0 };
    if r.underline {
        let y = (base + size * 0.12).round() as i64;
        let t = (size / 18.0).round().max(1.0) as i64;
        for yy in y..y + t {
            for xx in (x0.floor() as i64)..(x.ceil() as i64) {
                plot(img, xx, yy, rgb, 1.0);
            }
        }
    }
}

/// Lignes de morceaux de texte centrees verticalement dans `rect` (comme `draw_text_block`). `force` : tout prend la couleur `rgb`
/// (passes du contour) ; sinon chaque morceau garde sa couleur propre quand il en a une.
fn draw_runs_block(img: &mut RgbaImage, font: &FontVec, lines: &[Vec<bdauthor_core::subtitle::Run>], size: f32, rgb: [u8; 3], force: bool, rect: (i32, i32, i32, i32), align: Align) {
    let (_, th) = text_extent(font, "Ag", size);
    let lh = th * 1.1;
    let ascent = font.as_scaled(PxScale::from(size)).ascent();
    let top = rect.1 as f32 + (rect.3 as f32 - lh * lines.len() as f32) / 2.0;
    for (k, l) in lines.iter().enumerate() {
        let tw = line_width(font, l, size);
        let mut x = rect.0 as f32 + match align { Align::Left => 0.0, Align::Center => (rect.2 as f32 - tw) / 2.0, Align::Right => rect.2 as f32 - tw };
        let base = top + lh * k as f32 + (lh - th) / 2.0 + ascent;
        for r in l {
            draw_run(img, font, r, size, &mut x, base, if force { rgb } else { r.color.unwrap_or(rgb) });
        }
    }
}

/// Mots d'une ligne de morceaux : (morceaux du mot, espace avant). Un mot peut changer de mise en forme en son milieu.
fn words_of(line: &[bdauthor_core::subtitle::Run]) -> Vec<(Vec<bdauthor_core::subtitle::Run>, bool)> {
    let mut out: Vec<(Vec<bdauthor_core::subtitle::Run>, bool)> = Vec::new();
    let (mut parts, mut sp) = (Vec::new(), false);
    for r in line {
        let mut buf = String::new();
        for ch in r.text.chars() {
            if ch.is_whitespace() {
                if !buf.is_empty() {
                    let mut p = r.clone();
                    p.text = std::mem::take(&mut buf);
                    parts.push(p);
                }
                if !parts.is_empty() {
                    out.push((std::mem::take(&mut parts), sp));
                }
                sp = true;
            } else {
                buf.push(ch);
            }
        }
        if !buf.is_empty() {
            let mut p = r.clone();
            p.text = buf;
            parts.push(p);
        }
    }
    if !parts.is_empty() {
        out.push((parts, sp));
    }
    out
}

/// Coupe aux espaces les lignes plus larges que `avail` ; les lignes coupees sont equilibrees (meme nombre de lignes, largeur minimale).
fn wrap_runs(font: &FontVec, lines: &[Vec<bdauthor_core::subtitle::Run>], size: f32, avail: f32) -> Vec<Vec<bdauthor_core::subtitle::Run>> {
    use bdauthor_core::subtitle::Run;
    let space_w = text_extent(font, " ", size).0;
    let greedy = |words: &[(Vec<Run>, bool)], limit: f32| -> Vec<Vec<Run>> {
        let mut out: Vec<Vec<Run>> = Vec::new();
        let (mut cur, mut cw): (Vec<Run>, f32) = (Vec::new(), 0.0);
        for (parts, _) in words {
            let w = line_width(font, parts, size);
            if !cur.is_empty() && cw + space_w + w > limit {
                out.push(std::mem::take(&mut cur));
                cw = 0.0;
            }
            if let Some(last) = cur.last_mut() {
                last.text.push(' ');
                cw += space_w;
            }
            cur.extend(parts.iter().cloned());
            cw += w;
        }
        if !cur.is_empty() {
            out.push(cur);
        }
        out
    };
    let mut res = Vec::new();
    for l in lines {
        if line_width(font, l, size) <= avail {
            res.push(l.clone());
            continue;
        }
        let words = words_of(l);
        let n = greedy(&words, avail).len();
        // plus petite largeur qui tient encore en `n` lignes
        let (mut lo, mut hi) = (avail * 0.3, avail);
        for _ in 0..14 {
            let mid = (lo + hi) / 2.0;
            if greedy(&words, mid).len() <= n { hi = mid; } else { lo = mid; }
        }
        res.extend(greedy(&words, hi));
    }
    res
}

/// Rectangle arrondi plein (rayon `radius`), bords lisses (alpha partiel sur 1 px).
/// Marges (x, y) a laisser autour du cadre d'un texte qui le deborde (texte centre sur le cadre).
fn text_overflow(font: &FontVec, text: &str, size: f32, w: i32, h: i32) -> (i32, i32) {
    let lines: Vec<&str> = text.split('\n').collect();
    let maxw = lines.iter().map(|l| text_extent(font, l, size).0).fold(0.0f32, f32::max);
    let lh = text_extent(font, "Ag", size).1 * 1.1;
    let s = size as i32;
    (((maxw - w as f32).max(0.0) / 2.0).ceil() as i32 + 4 + s / 4, ((lh * lines.len() as f32 - h as f32).max(0.0) / 2.0).ceil() as i32 + 4 + s / 2)
}

/// Texte d'une page de menu avec son habillage : fond (couleur, opacite, coins arrondis) sur tout le cadre `rect`, puis
/// contour des lettres, puis le texte.
#[allow(clippy::too_many_arguments)]
pub fn draw_deco_text(img: &mut RgbaImage, font: &FontVec, text: &str, size: f32, rgb: [u8; 3], rect: (i32, i32, i32, i32), align: Align, deco: &TextDeco) {
    let (x, y, w, h) = rect;
    if deco.bg_opacity > 0 && w > 0 && h > 0 {
        let r = (deco.bg_radius.max(0) as f32).min(w.min(h) as f32 / 2.0);
        let bg = rounded_rect(w as u32, h as u32, r, deco.bg_color);
        blit(img, &bg, x, y, deco.bg_opacity.min(100) as f32 / 100.0);
    }
    if deco.outline <= 0 {
        draw_text_block(img, font, text, size, rgb, rect, align);
        return;
    }
    // contour : le texte dans la couleur du contour, decale tout autour d'un cercle de rayon `outline`, puis le texte
    let o = deco.outline;
    let (mx, my) = text_overflow(font, text, size, w, h);
    let (px, py) = (mx + o, my + o);
    let (lw, lh) = ((w + 2 * px).clamp(1, 8192) as u32, (h + 2 * py).clamp(1, 8192) as u32);
    let mut ring = RgbaImage::new(lw, lh);
    draw_text_block(&mut ring, font, text, size, deco.outline_color, (px, py, w, h), align);
    let mut body = RgbaImage::new(lw, lh);
    let rr = o as f32;
    let n = ((std::f32::consts::TAU * rr).ceil() as usize).clamp(8, 72);
    for i in 0..n {
        let a = i as f32 / n as f32 * std::f32::consts::TAU;
        blit(&mut body, &ring, (a.cos() * rr).round() as i32, (a.sin() * rr).round() as i32, 1.0);
    }
    // anneaux interieurs : contour plein meme epais
    for k in 1..o {
        let rk = k as f32;
        let nk = ((std::f32::consts::TAU * rk).ceil() as usize).clamp(8, 72);
        for i in 0..nk {
            let a = (i as f32 + 0.5) / nk as f32 * std::f32::consts::TAU;
            blit(&mut body, &ring, (a.cos() * rk).round() as i32, (a.sin() * rk).round() as i32, 1.0);
        }
    }
    draw_text_block(&mut body, font, text, size, rgb, (px, py, w, h), align);
    blit(img, &body, x - px, y - py, 1.0);
}

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
                Decor::Text { text, x, y, w, h, size: sz, color, align, font, deco } => {
                    let custom = self.custom_font(font, Path::new(""));
                    draw_deco_text(&mut im, custom.as_deref().unwrap_or(&self.font), text, *sz as f32, *color, (*x, *y, *w, *h), *align, deco)
                }
                Decor::Shape { shape, x, y, w, h, paint } => draw_paint(&mut im, *shape, (*x, *y, *w, *h), paint),
                Decor::Image { path, x, y, w, h } => {
                    let src = self.cached_image(path)?;
                    let sc = (*w as f64 / src.width as f64).min(*h as f64 / src.height as f64);
                    let (nw, nh) = (((src.width as f64 * sc).round() as u32).max(1), ((src.height as f64 * sc).round() as u32).max(1));
                    // redimensionnement (Lanczos, lent) garde en cache : deplacer un element ne le refait pas
                    let rs = self.cached_sprite(hkey(|hh| ("decor_image", Arc::as_ptr(&src) as usize, nw, nh).hash(hh)), || resize(&src, nw, nh));
                    blit(&mut im, &rs, *x + (*w - nw as i32) / 2, *y + (*h - nh as i32) / 2, 1.0);
                }
            }
        }
        Ok(im)
    }

    /// Feuille entiere (jaquette ou etiquette, fond perdu compris) a `dpi` points par pouce : zones colorees, image de fond, puis
    /// elements dans leur ordre ; `vars` remplit les champs automatiques des textes.
    /// Un element tourne est dessine a plat sur le morceau correspondant, tourne dans l'autre sens, puis remis en place.
    pub fn render_sheet(&self, doc: &dyn bdauthor_core::cover::SheetDoc, dpi: u32, project_dir: &Path, vars: &std::collections::BTreeMap<String, String>) -> Result<RgbaImage> {
        use bdauthor_core::cover::{crop, rotate_quarters};
        let lay = doc.layout_with(dpi, project_dir, vars);
        let mut im = lay.background();
        if let Some(p) = &lay.bg_image {
            let src = self.cached_image(p)?;
            let bg = self.cached_sprite(hkey(|hh| ("sheet_bg", Arc::as_ptr(&src) as usize, lay.size).hash(hh)), || cover_resize(&src, lay.size.0, lay.size.1));
            blit(&mut im, &bg, 0, 0, 1.0);
        }
        for (d, rot) in lay.items {
            // image introuvable : l'element est ignore (l'apercu reste affiche)
            if let bdauthor_core::cover::CoverDecor::Image { path, .. } = &d {
                if self.cached_image(path).is_err() { continue; }
            }
            if rot == 0 {
                im = self.draw_cover_decor(im, &d)?;
                continue;
            }
            let q: u32 = if rot == 90 { 1 } else { 3 };
            let (x, y, w, h) = d.rect();
            let region = crop(&im, x, y, w, h);
            if region.width == 0 || region.height == 0 {
                continue;
            }
            let flat = rotate_quarters(&region, 4 - q);
            let (fw, fh) = (flat.width as i32, flat.height as i32);
            let done = self.draw_cover_decor(flat, &d.with_rect(0, 0, fw, fh))?;
            blit(&mut im, &rotate_quarters(&done, q), x.max(0), y.max(0), 1.0);
        }
        Ok(im)
    }

    /// Un element de jaquette (texte avec effets, image ou forme) pose sur `im`.
    fn draw_cover_decor(&self, mut im: RgbaImage, d: &bdauthor_core::cover::CoverDecor) -> Result<RgbaImage> {
        use bdauthor_core::cover::CoverDecor as D;
        match d {
            D::Image { path, x, y, w, h } => self.compose_on(im, &[Decor::Image { path: path.clone(), x: *x, y: *y, w: *w, h: *h }]),
            D::Shape { shape, x, y, w, h, fill, stroke, stroke_px, radius_px, flip, style, arrow } => {
                if *shape == bdauthor_core::cover::ShapeKind::Line {
                    if let Some(c) = stroke.or(*fill) { draw_segment(&mut im, (*x, *y, *w, *h), *flip, c, *stroke_px, *style, *arrow); }
                    return Ok(im);
                }
                draw_shape(&mut im, *shape, (*x, *y, *w, *h), *fill, *stroke, *stroke_px, *radius_px);
                Ok(im)
            }
            D::Text { .. } => {
                self.draw_cover_text(&mut im, d);
                Ok(im)
            }
        }
    }

    /// Texte de jaquette : interligne, alignement vertical, ajustement a la taille du cadre, contour et ombre.
    fn draw_cover_text(&self, im: &mut RgbaImage, d: &bdauthor_core::cover::CoverDecor) {
        use bdauthor_core::cover::{CoverDecor as D, VAlign};
        let D::Text { text, x, y, w, h, size, size_f, w_f, color, align, font, valign, spacing, outline, outline_color, shadow, shadow_color, autofit, bold, italic, justify } = d else { return };
        let custom = self.custom_font(font, Path::new(""));
        let f: &FontVec = custom.as_deref().unwrap_or(&self.font);
        let lines: Vec<&str> = text.split('\n').collect();
        // justifie : taille et largeur exactes, pour que la coupe des lignes ne depende pas de la resolution
        let mut sz = if *justify { *size_f } else { *size as f32 };
        let wf = if *justify { *w_f } else { *w as f32 };
        let line_h = |s: f32| text_extent(f, "Ag", s).1 * 1.1 * *spacing;
        let measure = |s: &str, z: f32| text_extent(f, s, z).0 + if *bold { bold_off(z) } else { 0.0 };
        // justifie : le texte est coupe en lignes de la largeur du cadre (a la taille `z`)
        let wrapped = |z: f32| bdauthor_core::cover::wrap_words(&lines, wf, &|s| measure(s, z), text_extent(f, " ", z).0);
        if *autofit {
            if *justify {
                // la hauteur du texte coupe croit comme le carre de la taille
                for _ in 0..6 {
                    let th = line_h(sz) * wrapped(sz).len() as f32;
                    if th <= *h as f32 + 0.5 || sz <= 4.0 { break; }
                    sz = (sz * (*h as f32 / th).sqrt() * 0.98).max(4.0);
                }
            } else {
                let widest = |s: f32| lines.iter().map(|l| measure(l, s)).fold(0.0f32, f32::max);
                for _ in 0..3 {
                    let (tw, th) = (widest(sz), line_h(sz) * lines.len() as f32);
                    let k = (*w as f32 / tw.max(1.0)).min(*h as f32 / th.max(1.0));
                    if k >= 0.999 { break; }
                    sz = (sz * k * 0.995).max(4.0);
                }
            }
        }
        let lh = line_h(sz);
        // morceaux a dessiner : (ligne, texte, decalage x, largeur, alignement) ; en justifie, chaque mot d'une ligne etiree est
        // pose a part, l'ecart entre les mots remplissant la largeur du cadre
        let mut parts: Vec<(usize, String, f32, f32, Align)> = Vec::new();
        let mut nlines = lines.len();
        if *justify {
            let jl = wrapped(sz);
            nlines = jl.len();
            for (k, l) in jl.iter().enumerate() {
                if !l.last && l.words.len() >= 2 {
                    let ws: Vec<f32> = l.words.iter().map(|s| measure(s, sz)).collect();
                    let gap = ((wf - ws.iter().sum::<f32>()) / (l.words.len() - 1) as f32).max(0.0);
                    let mut cx = 0.0f32;
                    for (s, ww) in l.words.iter().zip(&ws) {
                        parts.push((k, s.clone(), cx, *ww, Align::Left));
                        cx += *ww + gap;
                    }
                } else {
                    parts.push((k, l.words.join(" "), 0.0, *w as f32, *align));
                }
            }
        } else {
            for (k, l) in lines.iter().enumerate() { parts.push((k, l.to_string(), 0.0, *w as f32, *align)); }
        }
        let total = lh * nlines as f32;
        let top = match valign { VAlign::Top => 0.0, VAlign::Middle => (*h as f32 - total) / 2.0, VAlign::Bottom => *h as f32 - total };
        let paint = |img: &mut RgbaImage, ox: f32, oy: f32, col: [u8; 3]| {
            for (k, t, xo, ww, al) in &parts {
                let rect = (ox + *xo, oy + top + lh * *k as f32, *ww, lh);
                if *bold || *italic { draw_line_styled(img, f, t, sz, col, rect, *al, *bold, *italic); } else { draw_line_f(img, f, t, sz, col, rect, *al); }
            }
        };
        if *outline == 0 && *shadow == 0 {
            paint(im, *x as f32, *y as f32, *color);
            return;
        }
        let pad = *outline + *shadow + 2;
        let (lw, lhh) = ((*w + 2 * pad).max(1) as u32, (*h + 2 * pad).max(1) as u32);
        let mut body = RgbaImage::new(lw, lhh);
        if *outline > 0 {
            // contour : le texte decale tout autour d'un cercle de rayon `outline`
            let mut ring = RgbaImage::new(lw, lhh);
            paint(&mut ring, pad as f32, pad as f32, *outline_color);
            let r = *outline as f32;
            let n = ((std::f32::consts::TAU * r).ceil() as usize).clamp(8, 72);
            for i in 0..n {
                let a = i as f32 / n as f32 * std::f32::consts::TAU;
                blit(&mut body, &ring, (a.cos() * r).round() as i32, (a.sin() * r).round() as i32, 1.0);
            }
            blit(&mut body, &ring, 0, 0, 1.0);
        }
        paint(&mut body, pad as f32, pad as f32, *color);
        if *shadow > 0 {
            let mut tinted = body.clone();
            for p in tinted.data.chunks_exact_mut(4) { p[0] = shadow_color[0]; p[1] = shadow_color[1]; p[2] = shadow_color[2]; }
            let mut out = RgbaImage::new(lw, lhh);
            blit(&mut out, &tinted, *shadow, *shadow, 1.0);
            blit(&mut out, &body, 0, 0, 1.0);
            body = out;
        }
        blit(im, &body, *x - pad, *y - pad, 1.0);
    }
}

// ------------------------------------------------------------------ apparence des boutons (styles)
/// Distance signee (negative a l'interieur) au bord d'un rectangle arrondi de demi-cotes `hw`, `hh`, centre en (0, 0).
fn sd_round_rect(qx: f32, qy: f32, hw: f32, hh: f32, r: f32) -> f32 {
    let r = r.min(hw).min(hh).max(0.0);
    let (ax, ay) = (qx.abs() - (hw - r), qy.abs() - (hh - r));
    (ax.max(0.0).powi(2) + ay.max(0.0).powi(2)).sqrt() + ax.max(ay).min(0.0) - r
}

/// Pose la couleur `rgb` avec l'opacite `a` (0..1) sur le pixel (x, y), qui peut etre transparent.
fn over_pixel(im: &mut RgbaImage, x: i32, y: i32, rgb: [u8; 3], a: f32) {
    if x < 0 || y < 0 || x >= im.width as i32 || y >= im.height as i32 || a <= 0.0 { return; }
    let d = im.pixel(x as u32, y as u32);
    let da = d[3] as f32 / 255.0;
    let oa = a + da * (1.0 - a);
    if oa <= 0.0 { return; }
    let mix = |dc: u8, sc: u8| ((sc as f32 * a + dc as f32 * da * (1.0 - a)) / oa).round().clamp(0.0, 255.0) as u8;
    im.set(x as u32, y as u32, [mix(d[0], rgb[0]), mix(d[1], rgb[1]), mix(d[2], rgb[2]), (oa * 255.0).round() as u8]);
}

/// Distance signee (negative a l'interieur) d'un point au triangle (a, b, c), quel que soit son sens.
fn sd_triangle(p: (f32, f32), a: (f32, f32), b: (f32, f32), c: (f32, f32)) -> f32 {
    let (e0, e1, e2) = ((b.0 - a.0, b.1 - a.1), (c.0 - b.0, c.1 - b.1), (a.0 - c.0, a.1 - c.1));
    let (v0, v1, v2) = ((p.0 - a.0, p.1 - a.1), (p.0 - b.0, p.1 - b.1), (p.0 - c.0, p.1 - c.1));
    let proj = |v: (f32, f32), e: (f32, f32)| { let t = ((v.0 * e.0 + v.1 * e.1) / (e.0 * e.0 + e.1 * e.1).max(1e-9)).clamp(0.0, 1.0); (v.0 - e.0 * t, v.1 - e.1 * t) };
    let (q0, q1, q2) = (proj(v0, e0), proj(v1, e1), proj(v2, e2));
    let s = if e0.0 * e2.1 - e0.1 * e2.0 >= 0.0 { 1.0 } else { -1.0 };
    let cand = [
        (q0.0 * q0.0 + q0.1 * q0.1, s * (v0.0 * e0.1 - v0.1 * e0.0)),
        (q1.0 * q1.0 + q1.1 * q1.1, s * (v1.0 * e1.1 - v1.1 * e1.0)),
        (q2.0 * q2.0 + q2.1 * q2.1, s * (v2.0 * e2.1 - v2.1 * e2.0)),
    ];
    let d = cand.iter().fold((f32::MAX, f32::MAX), |m, c| (m.0.min(c.0), m.1.min(c.1)));
    -d.0.sqrt() * if d.1 >= 0.0 { 1.0 } else { -1.0 }
}

/// Rectangle, ellipse ou ligne d'un menu : meme dessin que les formes de l'editeur de jaquette (`paint` deja a l'echelle de `im`).
fn draw_paint(im: &mut RgbaImage, shape: bdauthor_core::cover::ShapeKind, rect: (i32, i32, i32, i32), paint: &bdauthor_core::design::Paint) {
    if shape == bdauthor_core::cover::ShapeKind::Line {
        if let Some(c) = paint.line_color() { draw_segment_a(im, rect, paint.flip, c, paint.stroke_px, paint.style, paint.arrow, paint.alpha()); }
        return;
    }
    draw_shape_a(im, shape, rect, paint.fill, paint.stroke, if paint.stroke.is_some() { paint.stroke_px } else { 0 }, paint.radius, paint.alpha());
}

/// Segment d'epaisseur `stroke_px` entre deux coins opposes du cadre `rect` (`flip` : bas gauche -> haut droit), avec son type de trait
/// (plein a bouts arrondis, tirets, pointilles, tirets et points, double) et d'eventuelles pointes de fleche.
fn draw_segment(im: &mut RgbaImage, rect: (i32, i32, i32, i32), flip: bool, rgb: [u8; 3], stroke_px: i32,
                style: bdauthor_core::cover::LineStyle, arrow: bdauthor_core::cover::LineArrow) {
    draw_segment_a(im, rect, flip, rgb, stroke_px, style, arrow, 1.0);
}

/// Comme `draw_segment`, avec une opacite `alpha` (0..1).
#[allow(clippy::too_many_arguments)]
fn draw_segment_a(im: &mut RgbaImage, rect: (i32, i32, i32, i32), flip: bool, rgb: [u8; 3], stroke_px: i32,
                  style: bdauthor_core::cover::LineStyle, arrow: bdauthor_core::cover::LineArrow, alpha: f32) {
    use bdauthor_core::cover::LineStyle as S;
    let (x, y, w, h) = rect;
    let (x0, x1) = (x as f32, (x + w) as f32);
    let ((ax, ay), (bx, by)) = if flip { ((x0, (y + h) as f32), (x1, y as f32)) } else { ((x0, y as f32), (x1, (y + h) as f32)) };
    let (dx, dy) = (bx - ax, by - ay);
    let len = dx.hypot(dy);
    let (ux, uy) = if len > 0.0 { (dx / len, dy / len) } else { (1.0, 0.0) };
    let u = stroke_px.max(1) as f32;
    let width = if style == S::Double { u.max(3.0) } else { u };
    let half = width / 2.0;
    let (arrow_a, arrow_b) = arrow.flags();
    let head_len = (width * 4.0).max(8.0).min(len * 0.6);
    let head_half = head_len * 0.45;
    // le fut s'arrete a la base des pointes
    let s0 = if arrow_a { head_len * 0.9 } else { 0.0 };
    let s1 = len - if arrow_b { head_len * 0.9 } else { 0.0 };
    // motifs (debut, fin) en unites d'epaisseur, sur une periode
    let (period, dashes): (f32, Vec<(f32, f32)>) = match style {
        S::Dashed => (7.0 * u, vec![(0.0, 4.0 * u)]),
        S::DashDot => (9.0 * u, vec![(0.0, 4.0 * u), (6.0 * u, 7.0 * u)]),
        _ => (0.0, Vec::new()),
    };
    let m = (half.max(head_half)).ceil() as i32 + 2;
    let tip_a = (ax, ay);
    let tip_b = (bx, by);
    let base_a = (ax + ux * head_len, ay + uy * head_len);
    let base_b = (bx - ux * head_len, by - uy * head_len);
    let (nx, ny) = (-uy, ux);
    for py in (y - m).max(0)..(y + h + m).min(im.height as i32) {
        for px in (x - m).max(0)..(x + w + m).min(im.width as i32) {
            let (qx, qy) = (px as f32 + 0.5, py as f32 + 0.5);
            let (rx, ry) = (qx - ax, qy - ay);
            let s = rx * ux + ry * uy;
            let o = rx * nx + ry * ny;
            let ends = (s - s0 + 0.5).clamp(0.0, 1.0).min((s1 - s + 0.5).clamp(0.0, 1.0));
            let mut cov = match style {
                S::Solid => {
                    let t = ((s).clamp(s0, s1.max(s0))) - s;
                    (0.5 - ((t * t + o * o).sqrt() - half)).clamp(0.0, 1.0)
                }
                S::Double => (0.5 - ((o.abs() - width / 3.0).abs() - width / 6.0)).clamp(0.0, 1.0) * ends,
                S::Dashed | S::DashDot => {
                    let r = (s - s0).rem_euclid(period);
                    let on = dashes.iter().map(|&(a, b)| (r - a + 0.5).clamp(0.0, 1.0).min((b - r + 0.5).clamp(0.0, 1.0))).fold(0.0_f32, f32::max);
                    (0.5 - (o.abs() - half)).clamp(0.0, 1.0) * on * ends
                }
                S::Dotted => {
                    let p = 2.0 * width;
                    let r = (s - s0).rem_euclid(p) - p / 2.0;
                    (0.5 - ((r * r + o * o).sqrt() - half)).clamp(0.0, 1.0) * ends
                }
            };
            if arrow_a { cov = cov.max((0.5 - sd_triangle((qx, qy), tip_a, (base_a.0 + nx * head_half, base_a.1 + ny * head_half), (base_a.0 - nx * head_half, base_a.1 - ny * head_half))).clamp(0.0, 1.0)); }
            if arrow_b { cov = cov.max((0.5 - sd_triangle((qx, qy), tip_b, (base_b.0 + nx * head_half, base_b.1 + ny * head_half), (base_b.0 - nx * head_half, base_b.1 - ny * head_half))).clamp(0.0, 1.0)); }
            over_pixel(im, px, py, rgb, cov * alpha);
        }
    }
}

/// Rectangle (arrondi), ellipse ou trait horizontal, avec remplissage et bordure interieure, bords lisses.
fn draw_shape(im: &mut RgbaImage, shape: bdauthor_core::cover::ShapeKind, rect: (i32, i32, i32, i32), fill: Option<[u8; 3]>, stroke: Option<[u8; 3]>, stroke_px: i32, radius_px: i32) {
    draw_shape_a(im, shape, rect, fill, stroke, stroke_px, radius_px, 1.0);
}

/// Comme `draw_shape`, avec une opacite `alpha` (0..1) pour le remplissage et la bordure.
#[allow(clippy::too_many_arguments)]
fn draw_shape_a(im: &mut RgbaImage, shape: bdauthor_core::cover::ShapeKind, rect: (i32, i32, i32, i32), fill: Option<[u8; 3]>, stroke: Option<[u8; 3]>, stroke_px: i32, radius_px: i32, alpha: f32) {
    use bdauthor_core::cover::ShapeKind;
    let (x, y, w, h) = rect;
    if w <= 0 || h <= 0 { return; }
    let (hw, hh) = (w as f32 / 2.0, h as f32 / 2.0);
    let sw = stroke_px.max(0) as f32;
    let line_h = sw.max(1.0);
    let cover = |d: f32| (0.5 - d).clamp(0.0, 1.0);
    for py in y.max(0)..(y + h).min(im.height as i32) {
        for px in x.max(0)..(x + w).min(im.width as i32) {
            let (qx, qy) = (px as f32 + 0.5 - (x as f32 + hw), py as f32 + 0.5 - (y as f32 + hh));
            let d = match shape {
                ShapeKind::Rect => sd_round_rect(qx, qy, hw, hh, radius_px as f32),
                ShapeKind::Ellipse => (((qx / hw).powi(2) + (qy / hh).powi(2)).sqrt() - 1.0) * hw.min(hh),
                ShapeKind::Line => sd_round_rect(qx, qy, hw, line_h / 2.0, line_h / 2.0),
            };
            let co = cover(d);
            if co <= 0.0 { continue; }
            if shape == ShapeKind::Line {
                if let Some(c) = stroke.or(fill) { over_pixel(im, px, py, c, co * alpha); }
                continue;
            }
            // remplissage sous la bordure seulement la ou elle ne couvre pas : l'opacite reste uniforme
            let edge = if let (Some(_), true) = (stroke, sw > 0.0) { (co - cover(d + sw)).max(0.0) } else { 0.0 };
            if let Some(c) = fill { over_pixel(im, px, py, c, (co - if alpha < 1.0 { edge } else { 0.0 }).max(0.0) * alpha); }
            if let (Some(c), true) = (stroke, sw > 0.0) { over_pixel(im, px, py, c, edge * alpha); }
        }
    }
}

/// Couverture (0..1) du pixel (x, y) par un rectangle arrondi de `w` x `h` (rayon `r`) decale de (ox, oy).
fn rr_cover(x: f32, y: f32, ox: f32, oy: f32, w: f32, h: f32, r: f32) -> f32 {
    if w <= 0.0 || h <= 0.0 {
        return 0.0;
    }
    let r = r.min(w / 2.0).min(h / 2.0).max(0.0);
    let (hw, hh) = (w / 2.0, h / 2.0);
    let (px, py) = (x + 0.5 - ox - hw, y + 0.5 - oy - hh);
    let (qx, qy) = (px.abs() - (hw - r), py.abs() - (hh - r));
    let d = (qx.max(0.0).powi(2) + qy.max(0.0).powi(2)).sqrt() + qx.max(qy).min(0.0) - r;
    (0.5 - d).clamp(0.0, 1.0)
}

/// Coins arrondis : l'alpha de `im` est multiplie par la couverture du rectangle arrondi.
fn round_corners(im: &mut RgbaImage, r: i32) {
    if r <= 0 {
        return;
    }
    let (w, h) = (im.width as f32, im.height as f32);
    for y in 0..im.height {
        for x in 0..im.width {
            let c = rr_cover(x as f32, y as f32, 0.0, 0.0, w, h, r as f32);
            if c < 1.0 {
                let mut p = im.pixel(x, y);
                p[3] = (p[3] as f32 * c).round() as u8;
                im.set(x, y, p);
            }
        }
    }
}

/// Cadre d'epaisseur `bw` (interieur au bouton), arrondi `r`, couleur `rgb`.
fn draw_frame(im: &mut RgbaImage, r: i32, bw: i32, rgb: [u8; 3]) {
    if bw <= 0 {
        return;
    }
    let (w, h, b) = (im.width as f32, im.height as f32, bw as f32);
    for y in 0..im.height {
        for x in 0..im.width {
            let (fx, fy) = (x as f32, y as f32);
            let outer = rr_cover(fx, fy, 0.0, 0.0, w, h, r as f32);
            let inner = rr_cover(fx, fy, b, b, w - 2.0 * b, h - 2.0 * b, (r as f32 - b).max(0.0));
            let a = outer * (1.0 - inner);
            if a > 0.0 {
                let d = im.pixel(x, y);
                let mix = |dc: u8, sc: u8| (dc as f32 * (1.0 - a) + sc as f32 * a).round() as u8;
                let na = (d[3] as f32 + (255.0 - d[3] as f32) * a).round() as u8;
                im.set(x, y, [mix(d[0], rgb[0]), mix(d[1], rgb[1]), mix(d[2], rgb[2]), na]);
            }
        }
    }
}

/// Assombrit les couleurs de `pct` %.
fn darken(im: &mut RgbaImage, pct: i32) {
    if pct <= 0 {
        return;
    }
    let k = (100 - pct.clamp(0, 100)) as u32;
    for p in im.data.chunks_exact_mut(4) {
        for c in &mut p[..3] {
            *c = (*c as u32 * k / 100) as u8;
        }
    }
}

/// Flou rapide d'un canal (moyenne glissante horizontale puis verticale), rayon `r`.
fn box_blur(a: &mut [f32], w: usize, h: usize, r: usize) {
    if r == 0 || w == 0 || h == 0 {
        return;
    }
    let mut tmp = vec![0.0f32; a.len()];
    let n = (2 * r + 1) as f32;
    for y in 0..h {
        let row = &a[y * w..(y + 1) * w];
        let mut acc: f32 = (0..=r).map(|i| row[i.min(w - 1)]).sum::<f32>() + r as f32 * row[0];
        for x in 0..w {
            tmp[y * w + x] = acc / n;
            acc += row[(x + r + 1).min(w - 1)] - row[x.saturating_sub(r)];
        }
    }
    for x in 0..w {
        let col = |y: usize| tmp[y.min(h - 1) * w + x];
        let mut acc: f32 = (0..=r).map(col).sum::<f32>() + r as f32 * col(0);
        for y in 0..h {
            a[y * w + x] = acc / n;
            acc += col(y + r + 1) - col(y.saturating_sub(r));
        }
    }
}

/// Image d'un etat a partir de la face du bouton (`w` x `h`) : arrondi et cadre (`framed`), assombrissement (normal), agrandissement
/// et halo (selectionne / active), le tout centre dans une image elargie des marges du style.
fn finish_state(face: RgbaImage, st: &ButtonStyle, state: BtnState, framed: bool) -> RgbaImage {
    let (w, h) = (face.width as i32, face.height as i32);
    let (_, _, border, bw) = st.colors(state);
    let mut f = face;
    if framed {
        round_corners(&mut f, st.radius);
        draw_frame(&mut f, st.radius, bw, border);
    }
    if state == BtnState::Normal {
        darken(&mut f, st.dim);
    }
    let (mx, my) = st.margins(w, h);
    let (cw, ch) = ((w + 2 * mx).max(1) as u32, (h + 2 * my).max(1) as u32);
    let mut canvas = RgbaImage::new(cw, ch);
    let lit = state != BtnState::Normal;
    if lit && st.zoom > 0 {
        let z = 100 + st.zoom.clamp(0, 20);
        f = resize(&f, ((w * z + 50) / 100).max(1) as u32, ((h * z + 50) / 100).max(1) as u32);
    }
    let (fx, fy) = ((cw as i32 - f.width as i32) / 2, (ch as i32 - f.height as i32) / 2);
    if lit && st.glow > 0 {
        // halo : silhouette du bouton elargie puis floutee, dans la couleur du halo
        let (ww, hh) = (cw as usize, ch as usize);
        let mut a = vec![0.0f32; ww * hh];
        for y in 0..f.height as i32 {
            for x in 0..f.width as i32 {
                let (tx, ty) = (x + fx, y + fy);
                if tx >= 0 && ty >= 0 && (tx as usize) < ww && (ty as usize) < hh {
                    a[ty as usize * ww + tx as usize] = f.pixel(x as u32, y as u32)[3] as f32 / 255.0;
                }
            }
        }
        let r = (st.glow as usize / 2).max(1);
        box_blur(&mut a, ww, hh, r);
        box_blur(&mut a, ww, hh, r);
        let c = st.glow_color;
        for (i, v) in a.iter().enumerate() {
            let al = (v * 1.8).min(1.0) * 230.0;
            if al >= 1.0 {
                canvas.data[i * 4..i * 4 + 4].copy_from_slice(&[c[0], c[1], c[2], al.round() as u8]);
            }
        }
    }
    blit(&mut canvas, &f, fx, fy, 1.0);
    canvas
}

/// Trois etats : l'etat active partage l'image selectionnee quand le style ne le distingue pas (et que `own_act` est faux).
fn three(make: &dyn Fn(BtnState) -> RgbaImage, st: &ButtonStyle, own_act: bool) -> [Arc<RgbaImage>; 3] {
    let n = Arc::new(make(BtnState::Normal));
    let s = Arc::new(make(BtnState::Selected));
    let a = if st.activated || own_act { Arc::new(make(BtnState::Activated)) } else { s.clone() };
    [n, s, a]
}

/// Ajuste `src` (sans deformation) au centre d'un cadre transparent de `w` x `h`.
fn contain(src: &RgbaImage, w: u32, h: u32) -> RgbaImage {
    let k = (w as f64 / src.width.max(1) as f64).min(h as f64 / src.height.max(1) as f64);
    let (nw, nh) = (((src.width as f64 * k).round() as u32).max(1), ((src.height as f64 * k).round() as u32).max(1));
    let mut out = RgbaImage::new(w, h);
    blit(&mut out, &resize(src, nw, nh), (w as i32 - nw as i32) / 2, (h as i32 - nh as i32) / 2, 1.0);
    out
}

impl FontRenderer {
    /// Face d'un texte ou d'une image cliquable : l'element seul a l'etat normal ; ensuite fond translucide, cadre ou soulignement.
    fn decor_face(&self, content: &RgbaImage, st: &ButtonStyle, state: BtnState) -> RgbaImage {
        let (w, h) = (content.width, content.height);
        if state == BtnState::Normal {
            return content.clone();
        }
        let (bg, _, border, bw) = st.colors(state);
        let mut im = if st.backdrop > 0 {
            let mut b = rounded_rect(w, h, st.radius as f32, bg);
            for p in b.data.chunks_exact_mut(4) {
                p[3] = (p[3] as u32 * st.backdrop.clamp(0, 100) as u32 / 100) as u8;
            }
            b
        } else {
            RgbaImage::new(w, h)
        };
        blit(&mut im, content, 0, 0, 1.0);
        if st.underline {
            let t = bw.max(2) as u32;
            for y in h.saturating_sub(t)..h {
                for x in 0..w {
                    im.set(x, y, [border[0], border[1], border[2], 255]);
                }
            }
        } else {
            draw_frame(&mut im, st.radius, bw, border);
        }
        im
    }
}

impl bdauthor_core::project::Renderer for FontRenderer {
    fn thumb_button(&self, thumb: &RgbaImage, label: &str, w: u32, h_total: u32, h_thumb: u32, st: &ButtonStyle) -> [Arc<RgbaImage>; 3] {
        let face = |state: BtnState| {
            let (bg, fg, _, _) = st.colors(state);
            let mut im = fill_rect(w, h_total, bg);
            for y in 0..h_thumb.min(thumb.height).min(h_total) {
                for x in 0..w.min(thumb.width) {
                    im.set(x, y, thumb.pixel(x, y));
                }
            }
            let (txt, sz) = fit_text(&self.font, label, w, ((h_total - h_thumb) as f32 * 0.55).max(14.0));
            draw_text_in(&mut im, &self.font, &txt, sz, fg, (0, h_thumb, w, h_total - h_thumb));
            finish_state(im, st, state, true)
        };
        three(&face, st, false)
    }

    fn text_button(&self, label: &str, w: u32, h: u32, st: &ButtonStyle) -> [Arc<RgbaImage>; 3] {
        let face = |state: BtnState| {
            let (bg, fg, _, _) = st.colors(state);
            let mut im = fill_rect(w, h, bg);
            let (txt, sz) = fit_text(&self.font, label, w, (h as f32 * 0.5).max(14.0));
            draw_text_centered(&mut im, &self.font, &txt, sz, fg);
            finish_state(im, st, state, true)
        };
        three(&face, st, false)
    }

    fn decor_button(&self, decor: &Decor, w: u32, h: u32, st: &ButtonStyle) -> [Arc<RgbaImage>; 3] {
        let content = self.compose_on(RgbaImage::new(w, h), std::slice::from_ref(decor)).unwrap_or_else(|_| RgbaImage::new(w, h));
        let face = |state: BtnState| finish_state(self.decor_face(&content, st, state), st, state, false);
        three(&face, st, false)
    }

    fn skin_button(&self, skin: &[Option<PathBuf>; 3], w: u32, h: u32, st: &ButtonStyle) -> std::result::Result<[Arc<RgbaImage>; 3], String> {
        let normal_path = skin[0].as_ref().ok_or("image de l'etat normal manquante")?;
        let ni = self.cached_image(normal_path)?;
        let normal = contain(&ni, w, h);
        let own: Vec<Option<RgbaImage>> = skin[1..].iter().map(|p| p.as_ref().map(|p| self.cached_image(p).map(|i| contain(&i, w, h)))).map(|r| r.transpose()).collect::<Result<_>>()?;
        let face = |state: BtnState| {
            let mine = match state { BtnState::Normal => None, BtnState::Selected => own[0].as_ref(), BtnState::Activated => own[1].as_ref().or(own[0].as_ref()) };
            match mine {
                // image fournie pour cet etat : prise telle quelle (seuls halo et agrandissement s'appliquent)
                Some(im) => finish_state(im.clone(), st, state, false),
                None => finish_state(self.decor_face(&normal, st, state), st, state, false),
            }
        };
        Ok(three(&face, st, own[1].is_some()))
    }

    fn compose_layer(&self, size: (u32, u32), decor: &[Decor]) -> std::result::Result<RgbaImage, String> {
        self.compose_on(RgbaImage::new(size.0, size.1), decor)
    }

    fn overlay(&self, states: [Arc<RgbaImage>; 3], decor: &[Decor]) -> [Arc<RgbaImage>; 3] {
        let one = |a: &Arc<RgbaImage>| Arc::new(self.compose_on(a.as_ref().clone(), decor).unwrap_or_else(|_| a.as_ref().clone()));
        let n = one(&states[0]);
        let s = if Arc::ptr_eq(&states[1], &states[0]) { n.clone() } else { one(&states[1]) };
        let a = if Arc::ptr_eq(&states[2], &states[1]) { s.clone() } else { one(&states[2]) };
        [n, s, a]
    }

    fn subtitle(&self, text: &str, style: &bdauthor_core::subtitle::SubStyle, max_w: u32) -> Option<RgbaImage> {
        use bdauthor_core::subtitle::{parse_runs, HAlign};
        let src_lines = parse_runs(text);
        if src_lines.iter().all(|l| l.iter().all(|r| r.text.trim().is_empty())) {
            return None;
        }
        let custom = self.custom_font(&style.font, Path::new(""));
        let font: &FontVec = custom.as_deref().unwrap_or(&self.font);
        let outline = style.outline.max(0);
        let shadow = style.shadow.max(0);
        let boxed = style.bg_opacity > 0;
        let bpad = if boxed { style.bg_pad.max(0) } else { 0 };
        // marge transparente autour du texte : contour, ombre et boite doivent tenir dans l'image
        let pad = outline + 4 + shadow + bpad;
        let avail = (max_w as f32 * style.max_width.clamp(30, 100) as f32 / 100.0 - 2.0 * pad as f32).max(40.0);
        let mut size = style.size.max(8) as f32;
        let lines = if style.wrap { wrap_runs(font, &src_lines, size, avail) } else { src_lines };
        // une ligne qui depasse encore (mot trop long, ou coupure desactivee) : la taille est reduite
        let widest = |sz: f32| lines.iter().map(|l| line_width(font, l, sz)).fold(0.0f32, f32::max);
        let wide = widest(size);
        if wide > avail {
            size = (size * avail / wide).max(8.0);
        }
        let align = match style.halign { HAlign::Left => Align::Left, HAlign::Center => Align::Center, HAlign::Right => Align::Right };
        let lh = text_extent(font, "Ag", size).1 * 1.1;
        // l'italique deborde a droite du dernier caractere
        let slant = if lines.iter().flatten().any(|r| r.italic) { (size * SLANT * 0.8).ceil() as i32 } else { 0 };
        let (tw, th) = (widest(size).ceil() as i32 + 2 + slant, (lh * lines.len() as f32).ceil() as i32);
        let (w, h) = (tw + 2 * pad, th + 2 * pad);
        let mut im = RgbaImage::new(w as u32, h as u32);
        if boxed {
            let (bw, bh) = ((tw + 2 * bpad) as u32, (th + 2 * bpad) as u32);
            let radius = (style.bg_radius.max(0) as f32).min(bw.min(bh) as f32 / 2.0);
            let mut b = rounded_rect(bw, bh, radius, style.bg_color);
            let k = style.bg_opacity.clamp(0, 100) as u32;
            for p in b.data.chunks_exact_mut(4) {
                p[3] = (p[3] as u32 * k / 100) as u8;
            }
            blit(&mut im, &b, pad - bpad, pad - bpad, 1.0);
        }
        // texte et contour sur une couche a part : l'ombre en reprend la silhouette
        let mut fg = RgbaImage::new(w as u32, h as u32);
        let rect = |dx: i32, dy: i32| (pad + dx, pad + dy, tw, th);
        if outline > 0 {
            // contour : le texte redessine a tous les decalages entiers du disque de rayon `outline`
            for dy in -outline..=outline {
                for dx in -outline..=outline {
                    if dx * dx + dy * dy <= outline * outline + outline / 2 {
                        draw_runs_block(&mut fg, font, &lines, size, style.outline_color, true, rect(dx, dy), align);
                    }
                }
            }
        }
        draw_runs_block(&mut fg, font, &lines, size, style.color, false, rect(0, 0), align);
        if shadow > 0 {
            let mut sh = RgbaImage::new(w as u32, h as u32);
            let k = style.shadow_opacity.clamp(0, 100) as u32;
            for (d, s) in sh.data.chunks_exact_mut(4).zip(fg.data.chunks_exact(4)) {
                d[..3].copy_from_slice(&style.shadow_color);
                d[3] = (s[3] as u32 * k / 100) as u8;
            }
            // decalee vers le bas et la droite
            let mut moved = RgbaImage::new(w as u32, h as u32);
            blit(&mut moved, &sh, shadow, shadow, 1.0);
            blit(&mut im, &moved, 0, 0, 1.0);
        }
        blit(&mut im, &fg, 0, 0, 1.0);
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
            // composition « par-dessus » sur une destination qui peut etre transparente : la couleur de la destination ne compte
            // qu'a hauteur de sa propre opacite (sinon un pixel translucide pose sur du vide serait assombri vers le noir)
            let da = d[3] as f32 / 255.0;
            let oa = a + da * (1.0 - a);
            if oa <= 0.0 { continue; }
            let mix = |dc: u8, sc: u8| ((sc as f32 * a + dc as f32 * da * (1.0 - a)) / oa).round().clamp(0.0, 255.0) as u8;
            let na = (oa * 255.0).round().clamp(0.0, 255.0) as u8;
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

/// Dossier de polices supplementaires choisi dans les reglages (rien : aucun).
static EXTRA_FONT_DIR: std::sync::Mutex<Option<PathBuf>> = std::sync::Mutex::new(None);

/// Choisit le dossier de polices supplementaires ; vide : aucun.
pub fn set_extra_font_dir(dir: &str) {
    let d = dir.trim();
    if let Ok(mut c) = EXTRA_FONT_DIR.lock() { *c = if d.is_empty() { None } else { Some(PathBuf::from(d)) }; }
}

/// Dossier de polices supplementaires choisi dans les reglages.
pub fn extra_font_dir() -> Option<PathBuf> { EXTRA_FONT_DIR.lock().ok().and_then(|c| c.clone()) }

/// Extensions des polices lues : TrueType et OpenType (les collections .ttc/.otc, les polices web .woff/.woff2 et les anciennes
/// polices PostScript ne le sont pas).
pub const FONT_EXTENSIONS: [&str; 2] = ["ttf", "otf"];

/// Polices lisibles du dossier supplementaire : (libelle = nom du fichier sans extension, chemin relatif au dossier). Les
/// sous-dossiers sont parcourus sur deux niveaux ; resultat trie, limite a 500 polices.
pub fn extra_fonts() -> Vec<(String, String)> {
    let Some(root) = extra_font_dir() else { return vec![] };
    fn walk(root: &Path, dir: &Path, depth: u32, out: &mut Vec<(String, String)>) {
        let Ok(rd) = std::fs::read_dir(dir) else { return };
        let mut entries: Vec<_> = rd.flatten().collect();
        entries.sort_by_key(|e| e.file_name());
        for e in entries {
            if out.len() >= 500 { return; }
            let p = e.path();
            if p.is_dir() { if depth < 2 { walk(root, &p, depth + 1, out); } continue; }
            let ext_ok = p.extension().and_then(|x| x.to_str()).is_some_and(|x| FONT_EXTENSIONS.iter().any(|f| f.eq_ignore_ascii_case(x)));
            if !ext_ok { continue; }
            let (Some(stem), Ok(rel)) = (p.file_stem().and_then(|s| s.to_str()), p.strip_prefix(root)) else { continue };
            out.push((stem.to_string(), rel.to_string_lossy().replace('\\', "/")));
        }
    }
    let mut out = Vec::new();
    walk(&root, &root, 1, &mut out);
    out.sort_by(|a, b| a.0.to_lowercase().cmp(&b.0.to_lowercase()));
    out
}

/// Polices proposees dans les listes de l'interface : (libelle, fichier) ; les polices usuelles installees, puis celles du dossier
/// supplementaire.
pub fn font_choices() -> Vec<(String, String)> {
    let mut v: Vec<(String, String)> = common_fonts().into_iter().map(|(l, f)| (l.to_string(), f.to_string())).collect();
    for (l, f) in extra_fonts() { if !v.iter().any(|(_, g)| *g == f) { v.push((l, f)); } }
    v
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
    // le dossier choisi dans les reglages passe en premier
    if let Some(d) = extra_font_dir() { v.insert(0, d); }
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
        let key = format!("{}|{}|{}", project_dir.display(), name, extra_font_dir().map(|d| d.display().to_string()).unwrap_or_default());
        if let Some(f) = self.fonts.lock().unwrap().get(&key) {
            return f.clone();
        }
        let direct = if Path::new(name).is_absolute() { PathBuf::from(name) } else { project_dir.join(name) };
        let path = if direct.is_file() { Some(direct) } else { system_font_dirs().into_iter().map(|d| d.join(name)).find(|p| p.is_file()) };
        let f = path.and_then(|p| load_font(&p).ok()).map(Arc::new);
        self.fonts.lock().unwrap().insert(key, f.clone());
        f
    }

    /// Le texte `text` ecrit avec la police `name` (voir `custom_font`), en blanc sur fond transparent, au corps `size` px (la largeur
    /// est limitee a `max_w` px en reduisant le corps). `None` si la police est introuvable ou n'a pas tous les caracteres du texte
    /// (police de symboles) : l'appelant affiche alors le texte avec la police de l'interface.
    pub fn font_sample(&self, name: &str, project_dir: &Path, text: &str, size: f32, max_w: u32) -> Option<RgbaImage> {
        let font = self.custom_font(name, project_dir)?;
        if text.is_empty() || text.chars().any(|c| !c.is_whitespace() && font.glyph_id(c).0 == 0) {
            return None;
        }
        let (mut w, mut h) = text_extent(&font, text, size);
        let mut size = size;
        if w > max_w as f32 {
            size *= max_w as f32 / w;
            (w, h) = text_extent(&font, text, size);
        }
        let (iw, ih) = ((w.ceil() as u32 + 4).max(1), (h.ceil() as u32 + 2).max(1));
        let mut im = RgbaImage::new(iw, ih);
        draw_line_f(&mut im, &font, text, size, [255, 255, 255], (2.0, 1.0, (iw - 4) as f32, (ih - 2) as f32), Align::Left);
        Some(im)
    }

    /// Retourne l'element en cache pour `key`, ou le dessine avec `make`. Le cache est vide quand il depasse 160 elements.
    fn cached_sprite(&self, key: u64, make: impl FnOnce() -> RgbaImage) -> Arc<RgbaImage> {
        if let Some(i) = self.sprites.lock().unwrap().get(&key) {
            return i.clone();
        }
        let im = Arc::new(make());
        let mut m = self.sprites.lock().unwrap();
        // vide quand il y a trop d'elements ou trop de memoire (grandes images redimensionnees)
        if m.len() >= 160 || m.values().map(|i| i.data.len()).sum::<usize>() + im.data.len() > 256 << 20 {
            m.clear();
        }
        m.insert(key, im.clone());
        im
    }

    /// Texte dessine sur fond transparent, dans une image un peu plus grande que son cadre (le texte peut deborder) ;
    /// retourne l'image et la marge (px, py) a retrancher a la position du cadre.
    #[allow(clippy::too_many_arguments)]
    #[allow(clippy::too_many_arguments)]
    fn text_sprite(&self, text: &str, size: i32, color: [u8; 3], align: Align, font_name: &str, project_dir: &Path, w: i32, h: i32, deco: &TextDeco) -> (Arc<RgbaImage>, i32, i32) {
        let custom = self.custom_font(font_name, project_dir);
        let font: &FontVec = custom.as_deref().unwrap_or(&self.font);
        let (mx, my) = text_overflow(font, text, size as f32, w, h);
        let (px, py) = (mx + deco.outline.max(0), my + deco.outline.max(0));
        let (cw, ch) = ((w + 2 * px).clamp(1, 8192), (h + 2 * py).clamp(1, 8192));
        let key = hkey(|hh| ("texte", text, size, color, align.name(), font_name, w, h, deco).hash(hh));
        let spr = self.cached_sprite(key, || {
            let mut im = RgbaImage::new(cw as u32, ch as u32);
            draw_deco_text(&mut im, font, text, size as f32, color, (px, py, w, h), align, deco);
            im
        });
        (spr, px, py)
    }

    /// `base` : fond deja redimensionne a `size` (ou `None` pour le degrade par defaut).
    /// Apercu fidele d'une page (meme code de dessin que la construction du disque), a la taille `size`.
    /// `project_dir` sert a resoudre les chemins relatifs des images. Les boutons de navigation sans objet sur cette page sont attenues.
    /// `style` : apparence des boutons du projet ; `selected` : element (indice dans la page) montre dans son etat selectionne,
    /// ou active si `activated`. `slots` : contenu de chaque vignette de la page, dans l'ordre des vignettes.
    #[allow(clippy::too_many_arguments)]
    pub fn render_page_preview(&self, design: &PageDesign, page: usize, pages: usize, slots: &[PreviewSlot], base: Option<&RgbaImage>, size: (u32, u32), project_dir: &Path, style: &ButtonStyle, selected: Option<usize>, activated: bool) -> RgbaImage {
        use bdauthor_core::project::Renderer;
        let s = size.0 as f64 / 1920.0;
        let r = |v: i32| (v as f64 * s).round() as i32;
        let abs = |f: &str| { let q = Path::new(f); if q.is_absolute() { q.to_path_buf() } else { project_dir.join(q) } };
        let mut im = match base {
            Some(b) => b.clone(),
            None => self.cached_sprite(hkey(|h| ("degrade", size).hash(h)), || gradient(size)).as_ref().clone(),
        };
        // dans l'ordre de superposition de la mise en page (le dernier est au premier plan) ; `k` : rang de la vignette
        let mut k = 0usize;
        for (idx, it) in design.items.iter().enumerate() {
            let (x, y, w, h) = (r(it.x), r(it.y), r(it.w).max(1), r(it.h).max(1));
            let st = it.look.style.as_ref().unwrap_or(style).scaled(s);
            let (mx, my) = st.margins(w, h);
            let state = if selected == Some(idx) { if activated { 2usize } else { 1 } } else { 0 };
            let skin = &it.look.skin;
            let skinned = skin.has_normal() && !matches!(it.kind, Kind::Slot { .. });
            // etat choisi d'un bouton dessine par l'utilisateur ; None si ses images sont illisibles
            let skin_state = || -> Option<Arc<RgbaImage>> {
                let key = hkey(|hh| ("habillage", skin, project_dir, w, h, &st, state).hash(hh));
                let one = |p: &str| if p.is_empty() { None } else { Some(abs(p)) };
                // etat selectionne anime : premiere image de l'animation (l'editeur fait defiler les suivantes)
                let sel = if skin.selected.is_empty() { skin.sel_frames.first().map_or("", |f| f.as_str()) } else { skin.selected.as_str() };
                let states = self.skin_button(&[one(skin.normal_file()), one(sel), one(&skin.activated)], w as u32, h as u32, &st).ok()?;
                Some(self.cached_sprite(key, || states[state].as_ref().clone()))
            };
            match &it.kind {
                Kind::Text { text, size: sz, color, align, font, link, deco } => {
                    let deco = deco.scaled(s);
                    if link.is_none() {
                        let (spr, px, py) = self.text_sprite(text, r(*sz), *color, *align, font, project_dir, w, h, &deco);
                        blit(&mut im, &spr, x - px, y - py, 1.0);
                        continue;
                    }
                    let spr = if skinned { skin_state() } else { None }.unwrap_or_else(|| {
                        let key = hkey(|hh| ("texte cliquable", text, r(*sz), color, align.name(), font, project_dir, w, h, &st, state, deco).hash(hh));
                        self.cached_sprite(key, || {
                            let f = if font.is_empty() { String::new() } else { let c = abs(font); if c.is_file() { c.to_string_lossy().into_owned() } else { font.clone() } };
                            let d = Decor::Text { text: text.clone(), x: 0, y: 0, w, h, size: r(*sz), color: *color, align: *align, font: f, deco };
                            self.decor_button(&d, w as u32, h as u32, &st)[state].as_ref().clone()
                        })
                    });
                    blit(&mut im, &spr, x - mx, y - my, 1.0);
                }
                Kind::Image { path, link, .. } => {
                    // image pas encore choisie (ou image de video pas encore extraite) : simple cadre sombre
                    if path.is_empty() {
                        if w > 0 && h > 0 { blit(&mut im, &rounded_rect(w as u32, h as u32, 0.0, [44, 46, 54]), x, y, 0.85); }
                        continue;
                    }
                    let Ok(src) = self.cached_image(&abs(path)) else { continue };
                    if link.is_none() {
                        let sc = (w as f64 / src.width as f64).min(h as f64 / src.height as f64);
                        let (nw, nh) = (((src.width as f64 * sc).round() as u32).max(1), ((src.height as f64 * sc).round() as u32).max(1));
                        let rs = self.cached_sprite(hkey(|hh| ("image", Arc::as_ptr(&src) as usize, nw, nh).hash(hh)), || resize(&src, nw, nh));
                        blit(&mut im, &rs, x + (w - nw as i32) / 2, y + (h - nh as i32) / 2, 1.0);
                        continue;
                    }
                    let spr = if skinned { skin_state() } else { None }.unwrap_or_else(|| {
                        let key = hkey(|hh| ("image cliquable", Arc::as_ptr(&src) as usize, w, h, &st, state).hash(hh));
                        self.cached_sprite(key, || {
                            let d = Decor::Image { path: abs(path), x: 0, y: 0, w, h };
                            self.decor_button(&d, w as u32, h as u32, &st)[state].as_ref().clone()
                        })
                    });
                    blit(&mut im, &spr, x - mx, y - my, 1.0);
                }
                Kind::Shape { shape, paint } => draw_paint(&mut im, *shape, (x, y, w, h), &paint.scaled(s)),
                Kind::Slot { thumb_h, .. } => {
                    let th = r(*thumb_h).clamp(1, h);
                    let rank = k;
                    k += 1;
                    let slot = slots.get(rank);
                    let key = hkey(|hh| ("vignette", slot.map(|s| (&s.label, s.key)), rank, w, h, th, &st, state).hash(hh));
                    let n = self.cached_sprite(key, || {
                        let (label, thumb) = match slot {
                            Some(sl) => (sl.label.clone(), sl.thumb.as_ref().map(|t| cover_resize(t, w as u32, th as u32))),
                            None => ("(vide)".to_string(), None),
                        };
                        let thumb = thumb.unwrap_or_else(|| placeholder_thumb(w as u32, th as u32, rank + 1, &self.font));
                        self.thumb_button(&thumb, &label, w as u32, h as u32, th as u32, &st)[state].as_ref().clone()
                    });
                    blit(&mut im, &n, x - mx, y - my, 1.0);
                }
                Kind::Nav { action, text, .. } => {
                    let n = if skinned { skin_state() } else { None }.unwrap_or_else(|| {
                        self.cached_sprite(hkey(|hh| ("bouton", text, w, h, &st, state).hash(hh)), || self.text_button(text, w as u32, h as u32, &st)[state].as_ref().clone())
                    });
                    blit(&mut im, &n, x - mx, y - my, if nav_applicable(*action, page, pages) { 1.0 } else { 0.35 });
                }
            }
        }
        im
    }
}
