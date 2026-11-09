mod common;
use bdauthor_core::palette::RgbaImage;
use bdauthor_core::pgs::*;
use bdauthor_core::project::Renderer;
use bdauthor_core::subtitle::*;
use std::path::Path;
use std::sync::Arc;

#[test]
fn srt_basic() {
    common::fr();
    let src = "\u{feff}1\r\n00:00:01,500 --> 00:00:03,000\r\n<i>Bonjour</i> &amp; salut\r\nDeuxi\u{e8}me ligne\r\n\r\n2\r\n00:01:02.5 --> 00:01:04,250 X1:10\r\n{\\an8}Haut\r\n\r\n3\r\n00:00:05,000 --> 00:00:04,000\r\nignore\r\n";
    let c = parse_srt(src);
    assert_eq!(c.len(), 2);
    assert_eq!((c[0].start_ms, c[0].end_ms, plain_text(&c[0].text).as_str()), (1500, 3000, "Bonjour & salut\nDeuxième ligne"));
    assert_eq!(c[0].text, "\u{1}iBonjour\u{1}I & salut\nDeuxième ligne");
    assert_eq!((c[1].start_ms, c[1].end_ms, c[1].text.as_str()), (62_500, 64_250, "Haut"));
}

#[test]
fn ass_basic() {
    common::fr();
    let src = "[Script Info]\nTitle: x\n\n[V4+ Styles]\nStyle: a,b,c\n\n[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\n\
Dialogue: 0,0:00:01.50,0:00:03.00,Default,,0,0,0,,{\\i1}Bonjour, monde\\Nligne 2{\\i0}\nComment: 0,0:00:01.50,0:00:03.00,Default,,0,0,0,,non\nDialogue: 0,1:02:03.04,1:02:04.00,Default,,0,0,0,,Fin\n";
    let c = parse_ass(src);
    assert_eq!(c.len(), 2);
    assert_eq!((c[0].start_ms, c[0].end_ms, plain_text(&c[0].text).as_str()), (1500, 3000, "Bonjour, monde\nligne 2"));
    assert_eq!(c[0].text, "\u{1}iBonjour, monde\nligne 2\u{1}I");
    assert_eq!(c[1].start_ms, 3_723_040);
}

#[test]
fn decode_encodings() {
    common::fr();
    assert_eq!(decode_text(&[0x63, 0x61, 0x66, 0xE9, 0x92]), "café’");
    assert_eq!(decode_text("é".as_bytes()), "é");
    assert_eq!(decode_text(&[0xFF, 0xFE, b'a', 0, 0xE9, 0]), "aé");
}

#[test]
fn merge_overlaps() {
    common::fr();
    let c = |s, e, t: &str| Cue { start_ms: s, end_ms: e, text: t.into(), pos: None };
    let m = merge(&[c(0, 3000, "A"), c(1000, 2000, "B"), c(5000, 6000, "C"), c(6000, 7000, "C"), c(8000, 8000, "vide")]);
    assert_eq!(m, vec![c(0, 1000, "A"), c(1000, 2000, "A\nB"), c(2000, 3000, "A"), c(5000, 7000, "C")]);
    let s = shift(&[c(0, 1000, "x"), c(-500, -100, "y"), c(-200, 800, "z")], 100);
    assert_eq!(s, vec![c(100, 1100, "x"), c(0, 900, "z")]);
}

/// Moteur de test : un rectangle blanc a bord noir dont la largeur depend du texte.
struct Flat;
impl Renderer for Flat {
    fn thumb_button(&self, _: &RgbaImage, _: &str, _: u32, _: u32, _: u32, _: &bdauthor_core::style::ButtonStyle) -> [Arc<RgbaImage>; 3] { unimplemented!() }
    fn text_button(&self, _: &str, _: u32, _: u32, _: &bdauthor_core::style::ButtonStyle) -> [Arc<RgbaImage>; 3] { unimplemented!() }
    fn background(&self, _: &str, _: (u32, u32)) -> RgbaImage { unimplemented!() }
    fn compose_background(&self, _: Option<&Path>, _: (u32, u32), _: &[bdauthor_core::project::Decor]) -> Result<RgbaImage, String> { unimplemented!() }
    fn subtitle(&self, text: &str, style: &SubStyle, max_w: u32) -> Option<RgbaImage> {
        let w = (text.chars().count() as u32 * 10 + 20).min(max_w);
        let h = style.size as u32;
        let mut im = RgbaImage::new(w + 8, h + 8); // marge transparente a rogner
        for y in 0..h {
            for x in 0..w {
                let edge = x < 2 || y < 2 || x >= w - 2 || y >= h - 2;
                let c = if edge { [0, 0, 0, 255] } else { [255, 255, 255, 255] };
                im.set(x + 4, y + 4, c);
            }
        }
        Some(im)
    }
}

fn cues() -> Vec<Cue> {
    vec![
        Cue { start_ms: 1000, end_ms: 3000, text: "Bonjour".into(), pos: None },
        Cue { start_ms: 3100, end_ms: 4000, text: "Collee".into(), pos: None }, // trop pres pour un effacement : remplace directement
        Cue { start_ms: 8000, end_ms: 8100, text: "Courte".into(), pos: None },
    ]
}

#[test]
fn pgs_roundtrip() {
    common::fr();
    let base = 54_000_000u64;
    let specs = encode_track(&cues(), &SubStyle::default(), &Flat, (1920, 1080), 1, base, base).unwrap();
    // l'ordre des DTS ne decroit jamais
    let key = |s: &bdauthor_core::mux::PesSpec| s.dts.unwrap_or(s.pts);
    assert!(specs.windows(2).all(|w| key(&w[0]) <= key(&w[1])), "DTS non monotones");
    let sets = decode_sets(&specs).unwrap();
    // 3 images + effacements apres la 1re? non (trop proche), apres la 2e et la 3e
    let states: Vec<(u8, usize)> = sets.iter().map(|s| (s.state, s.objects.len())).collect();
    assert_eq!(states, vec![(0x80, 1), (0x80, 1), (0x00, 0), (0x80, 1), (0x00, 0)]);
    let first = &sets[0];
    assert_eq!(first.pts, base + 1000 * 90);
    let o = &first.objects[0];
    assert_eq!(o.w as u32, 7 * 10 + 20);
    assert_eq!(o.x as u32, (1920 - o.w as u32) / 2);
    assert_eq!(o.y as u32, 1080 - 70 - o.h as u32);
    // couleurs : bord noir, centre blanc, fond transparent
    assert_eq!(o.rgba.pixel(0, 0)[3], 255);
    assert!(o.rgba.pixel(0, 0)[0] < 40);
    let c = o.rgba.pixel(o.w as u32 / 2, o.h as u32 / 2);
    assert!(c[0] > 215 && c[3] == 255, "{c:?}");
    // composition_number croissant
    assert!(sets.windows(2).all(|w| w[1].comp_no == w[0].comp_no + 1));
    // la 2e replique remplace la 1re sans effacement : elle commence une fois le delai de decodage ecoule
    assert!(sets[1].pts >= base + 3100 * 90);
    // duree minimale de la derniere
    assert!(sets[4].pts - sets[3].pts >= MIN_DURATION);
}

#[test]
fn pgs_720_scaling_and_positions() {
    common::fr();
    let mut st = SubStyle::default();
    st.pos = VPos::Top;
    let im = render_cue("Haut", &st, &Flat, (1280, 720)).unwrap().unwrap();
    assert_eq!(im.y as u32, (70.0f64 * 720.0 / 1080.0).round() as u32);
    st.pos = VPos::Middle;
    let im = render_cue("Milieu", &st, &Flat, (1280, 720)).unwrap().unwrap();
    assert!((im.y as i32 * 2 + im.h as i32 - 720).abs() <= 1);
}

#[test]
fn pgs_fragmented_ods() {
    common::fr();
    // image bruitee : le RLE depasse la taille maximale d'un segment
    let (w, h) = (700u16, 200u16);
    let mut x = 12345u32;
    let rows: Vec<Vec<u8>> = (0..h).map(|_| (0..w).map(|_| { x = x.wrapping_mul(1664525).wrapping_add(1013904223); ((x >> 24) % 7 + 1) as u8 }).collect()).collect();
    let rle = bdauthor_core::ig::rle_encode(&rows);
    assert!(rle.len() > 64_000);
    let segs = ods_segments(0, w, h, &rle);
    assert!(segs.len() >= 2);
    assert!(segs.iter().all(|s| s.len() < 65_500));
    let mut entries = std::collections::BTreeMap::new();
    for i in 0..8u8 { entries.insert(i, (16 + i * 20, 128, 128, 255)); }
    let item = Timed { start: 60_000_000, end: 61_000_000, img: SubImage { x: 10, y: 20, w, h, entries, rows: rows.clone() } };
    let specs = display_sets(&[item], (1920, 1080), 1, 54_000_000).unwrap();
    assert!(specs.iter().all(|s| build_pg_pes(s).is_ok()));
    let sets = decode_sets(&specs).unwrap();
    let o = &sets[0].objects[0];
    for (yy, r) in rows.iter().enumerate() {
        for (xx, &i) in r.iter().enumerate() {
            let px = o.rgba.pixel(xx as u32, yy as u32);
            let want = 16 + i as i32 * 20;
            assert!((px[0] as i32 - ((want - 16) as f64 * 1.164).round() as i32).abs() <= 2, "pixel {xx},{yy}");
        }
    }
}

#[test]
fn start_is_delayed_when_too_early() {
    common::fr();
    let c = vec![Cue { start_ms: 0, end_ms: 2000, text: "Tot".into(), pos: None }];
    let base = 54_000_000u64;
    let specs = encode_track(&c, &SubStyle::default(), &Flat, (1920, 1080), 1, base, base).unwrap();
    let min_dts = specs.iter().map(|s| s.dts.unwrap_or(s.pts)).min().unwrap();
    assert!(min_dts >= base);
}

#[test]
fn model_json_roundtrip_with_subtitles() {
    common::fr();
    use bdauthor_core::model::ProjectModel;
    let src = r##"{ "title": "T", "language": "fra", "videos": [
        {"file": "a.mp4", "inconnu": 7, "subtitles": [
            {"file": "a_fr.srt", "language": "fra", "offset_ms": -250, "style": {"size": 64, "color": "#ffee00", "outline": 5, "position": "haut", "margin": 40, "font": "arial.ttf"}},
            {"track": 1, "language": "eng"} ]},
        {"file": "b.mp4"} ] }"##;
    let m = ProjectModel::from_json(src).unwrap();
    assert_eq!(m.videos[0].subs.len(), 2);
    let t = &m.videos[0].subs[0];
    assert_eq!((t.offset_ms, t.style.size, t.style.color, t.style.outline, t.style.pos, t.style.margin), (-250, 64, [255, 238, 0], 5, VPos::Top, 40));
    assert_eq!(t.style.outline_color, [0, 0, 0]);
    assert_eq!(m.videos[0].subs[1].source, SubSource::Embedded(1));
    assert!(m.videos[1].subs.is_empty());
    let again = ProjectModel::from_json(&m.to_json()).unwrap();
    assert_eq!(again.videos[0].subs, m.videos[0].subs);
    assert!(m.to_json().contains("\"inconnu\""), "champ inconnu perdu");
    assert!(!m.to_json().contains("\"subtitles\": []"));
    // validation : fichier manquant, langue invalide
    let mut m2 = m.clone();
    m2.videos[0].subs[1].lang = "fr".into();
    let probs = m2.validate(std::path::Path::new("/nonexistent"));
    assert!(probs.iter().any(|p| p.contains("sous-titres 1") && p.contains("introuvable")), "{probs:?}");
    assert!(probs.iter().any(|p| p.contains("sous-titres 2") && p.contains("langue")), "{probs:?}");
    // une erreur de style est signalee a la lecture
    assert!(ProjectModel::from_json(r#"{"videos":[{"file":"a","subtitles":[{"file":"x.srt","style":{"position":"ailleurs"}}]}]}"#).is_err());
}
