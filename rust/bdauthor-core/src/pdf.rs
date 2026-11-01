//! PDF minimal sans dependance : une page contenant une image RVB (compressee) et, au choix, des traits de coupe.
use crate::deflate::zlib_compress;
use crate::palette::RgbaImage;

const MM: f64 = 72.0 / 25.4;

/// Millimetres -> points PDF.
pub fn mm_to_pt(mm: f64) -> f64 { mm * MM }

/// Page de `page_mm` (largeur, hauteur) portant `img` dans le rectangle `place_mm` (x, y depuis le coin haut gauche, largeur,
/// hauteur) ; `lines_mm` : traits (x1, y1, x2, y2) depuis le coin haut gauche, traces en noir (traits de coupe et de pliage).
pub fn image_pdf(page_mm: (f64, f64), img: &RgbaImage, place_mm: (f64, f64, f64, f64), lines_mm: &[(f64, f64, f64, f64)], title: &str) -> Vec<u8> {
    image_pdf_shaped(page_mm, img, place_mm, lines_mm, &[], &[], title)
}

/// Chemin d'un cercle (centre et rayon en points, origine en bas a gauche) en quatre courbes de Bezier.
fn circle_path(cx: f64, cy: f64, r: f64) -> String {
    let k = 0.552_284_749_8 * r;
    format!("{:.3} {:.3} m {:.3} {:.3} {:.3} {:.3} {:.3} {:.3} c {:.3} {:.3} {:.3} {:.3} {:.3} {:.3} c {:.3} {:.3} {:.3} {:.3} {:.3} {:.3} c {:.3} {:.3} {:.3} {:.3} {:.3} {:.3} c h\n",
        cx + r, cy, cx + r, cy + k, cx + k, cy + r, cx, cy + r,
        cx - k, cy + r, cx - r, cy + k, cx - r, cy,
        cx - r, cy - k, cx - k, cy - r, cx, cy - r,
        cx + k, cy - r, cx + r, cy - k, cx + r, cy)
}

/// Comme `image_pdf`, avec en plus : `clip_mm` = cercles (centre x, y depuis le coin haut gauche, rayon) qui decoupent l'image (regle
/// pair-impair : un cercle interieur fait un trou) et `circles_mm` = cercles traces en noir (reperes).
pub fn image_pdf_shaped(page_mm: (f64, f64), img: &RgbaImage, place_mm: (f64, f64, f64, f64), lines_mm: &[(f64, f64, f64, f64)], clip_mm: &[(f64, f64, f64)], circles_mm: &[(f64, f64, f64)], title: &str) -> Vec<u8> {
    let (pw, ph) = (page_mm.0 * MM, page_mm.1 * MM);
    // RVB sur fond blanc
    let mut rgb = Vec::with_capacity((img.width * img.height * 3) as usize);
    for px in img.data.chunks_exact(4) {
        let a = px[3] as u32;
        for c in 0..3 { rgb.push(((px[c] as u32 * a + 255 * (255 - a) + 127) / 255) as u8); }
    }
    let z = zlib_compress(&rgb);
    let (x, y, w, h) = (place_mm.0 * MM, ph - (place_mm.1 + place_mm.3) * MM, place_mm.2 * MM, place_mm.3 * MM);
    let mut content = String::from("q\n");
    if !clip_mm.is_empty() {
        for c in clip_mm { content.push_str(&circle_path(c.0 * MM, ph - c.1 * MM, c.2 * MM)); }
        content.push_str("W* n\n");
    }
    content.push_str(&format!("{w:.3} 0 0 {h:.3} {x:.3} {y:.3} cm /Im0 Do Q\n"));
    if !lines_mm.is_empty() || !circles_mm.is_empty() {
        content.push_str("0 0 0 RG 0.25 w\n");
        for c in circles_mm { content.push_str(&circle_path(c.0 * MM, ph - c.1 * MM, c.2 * MM)); content.push_str("S\n"); }
        for l in lines_mm {
            content.push_str(&format!("{:.3} {:.3} m {:.3} {:.3} l S\n", l.0 * MM, ph - l.1 * MM, l.2 * MM, ph - l.3 * MM));
        }
    }
    let safe: String = title.chars().filter(|c| c.is_ascii() && !matches!(c, '(' | ')' | '\\')).collect();
    let mut out: Vec<u8> = b"%PDF-1.4\n%\xE2\xE3\xCF\xD3\n".to_vec();
    let mut offsets: Vec<usize> = Vec::new();
    let mut obj = |out: &mut Vec<u8>, body: &[u8]| {
        offsets.push(out.len());
        out.extend_from_slice(format!("{} 0 obj\n", offsets.len()).as_bytes());
        out.extend_from_slice(body);
        out.extend_from_slice(b"\nendobj\n");
    };
    obj(&mut out, b"<< /Type /Catalog /Pages 2 0 R >>");
    obj(&mut out, b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>");
    obj(&mut out, format!("<< /Type /Page /Parent 2 0 R /MediaBox [0 0 {pw:.3} {ph:.3}] /Resources << /XObject << /Im0 5 0 R >> >> /Contents 4 0 R >>").as_bytes());
    let mut stream = format!("<< /Length {} >>\nstream\n", content.len()).into_bytes();
    stream.extend_from_slice(content.as_bytes());
    stream.extend_from_slice(b"endstream");
    obj(&mut out, &stream);
    let mut im = format!("<< /Type /XObject /Subtype /Image /Width {} /Height {} /ColorSpace /DeviceRGB /BitsPerComponent 8 /Filter /FlateDecode /Length {} >>\nstream\n", img.width, img.height, z.len()).into_bytes();
    im.extend_from_slice(&z);
    im.extend_from_slice(b"\nendstream");
    obj(&mut out, &im);
    obj(&mut out, format!("<< /Title ({safe}) /Producer (bdauthor) >>").as_bytes());
    let xref = out.len();
    out.extend_from_slice(format!("xref\n0 {}\n0000000000 65535 f \n", offsets.len() + 1).as_bytes());
    for o in &offsets { out.extend_from_slice(format!("{o:010} 00000 n \n").as_bytes()); }
    out.extend_from_slice(format!("trailer\n<< /Size {} /Root 1 0 R /Info 6 0 R >>\nstartxref\n{xref}\n%%EOF\n", offsets.len() + 1).as_bytes());
    out
}
