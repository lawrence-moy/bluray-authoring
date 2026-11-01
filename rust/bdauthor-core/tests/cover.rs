use bdauthor_core::cover::*;
use bdauthor_core::deflate::{zlib_compress, zlib_decompress_fixed};
use bdauthor_core::model::ProjectModel;
use bdauthor_core::palette::RgbaImage;
use std::path::Path;

#[test]
fn deflate_roundtrip() {
    let mut cases: Vec<Vec<u8>> = vec![vec![], vec![7], b"abcabcabcabcabcabcabc".to_vec(), vec![0u8; 100_000]];
    // pseudo-aleatoire + motifs repetes
    let mut x = 12345u32;
    let mut mixed = Vec::new();
    for i in 0..60_000u32 {
        x = x.wrapping_mul(1664525).wrapping_add(1013904223);
        mixed.push(if i % 7 < 4 { (i / 7 % 251) as u8 } else { (x >> 24) as u8 });
    }
    cases.push(mixed);
    for c in cases {
        let z = zlib_compress(&c);
        assert_eq!(zlib_decompress_fixed(&z).unwrap(), c, "len {}", c.len());
    }
    // un aplat se compresse tres bien
    assert!(zlib_compress(&vec![9u8; 1_000_000]).len() < 20_000);
}

#[test]
fn layout_and_geometry() {
    let c = Cover::default();
    assert_eq!(c.trim_mm(), (272.0, 148.0));
    assert_eq!(c.sheet_mm(), (278.0, 154.0));
    let l = c.layout(300, Path::new("."));
    assert_eq!(l.size, (3283, 1819));
    // les trois zones couvrent toute la feuille sans trou
    let bg = l.background();
    assert!((0..bg.height).all(|y| (0..bg.width).all(|x| bg.pixel(x, y)[3] == 255)));
    assert_eq!(bg.pixel(0, 0), [24, 28, 40, 255]);
    assert_eq!(bg.pixel(l.size.0 / 2, 10), [40, 60, 110, 255]);
}

#[test]
fn items_to_pixels() {
    let mut c = Cover::default();
    c.items.push(CoverItem::text("TITRE", 143.0, 10.0, 129.0, 30.0, 36.0));
    let mut spine = CoverItem::text("Tranche", 129.0, 0.0, 14.0, 148.0, 18.0);
    spine.rot = 90;
    c.items.push(spine);
    c.items.push(CoverItem::text("", 0.0, 0.0, 10.0, 10.0, 12.0)); // texte vide : ignore
    c.items.push(CoverItem::image("img/logo.png", -3.0, -3.0, 20.0, 20.0));
    let l = c.layout(300, Path::new("/proj"));
    assert_eq!(l.items.len(), 3);
    let (d, rot) = &l.items[0];
    assert_eq!(*rot, 0);
    // 143 mm + fond perdu 3 mm a 300 dpi ; 36 pt = 150 px
    match d { bdauthor_core::project::Decor::Text { x, w, size, .. } => assert_eq!((*x, *w, *size), (1724, 1524, 150)), _ => panic!() }
    assert_eq!(l.items[1].1, 90);
    match &l.items[2].0 { bdauthor_core::project::Decor::Image { path, x, .. } => { assert_eq!(*x, 0); assert!(path.ends_with("img/logo.png") && path.starts_with("/proj")); } _ => panic!() }
}

#[test]
fn json_roundtrip_in_project() {
    assert!(Cover::default().to_value().is_none());
    let mut c = Cover::default();
    c.set_format(1);
    c.marks = true;
    c.colors[2] = [200, 10, 30];
    c.bg_image = "fond.png".into();
    let mut t = CoverItem::text("Mon film\nsuite", 140.0, 20.0, 100.0, 40.0, 30.0);
    t.rot = 270;
    t.name = "Titre".into();
    c.items.push(t);
    c.items.push(CoverItem::image("a.png", 1.5, 2.25, 30.0, 40.0));
    let back = Cover::from_value(c.to_value().as_ref()).unwrap();
    assert_eq!(back, c);
    // dans le projet
    let mut m = ProjectModel::default();
    m.cover = c.clone();
    let m2 = ProjectModel::from_json(&m.to_json()).unwrap();
    assert_eq!(m2.cover, c);
    assert!(!ProjectModel::default().to_json().contains("jaquette"));
}

#[test]
fn rotation_and_crop() {
    let mut im = RgbaImage::new(3, 2);
    for y in 0..2 { for x in 0..3 { im.set(x, y, [x as u8, y as u8, 0, 255]); } }
    let r = rotate_quarters(&im, 1); // horaire : 2 de large, 3 de haut
    assert_eq!((r.width, r.height), (2, 3));
    assert_eq!(r.pixel(1, 0), [0, 0, 0, 255]); // ancien (0,0) en haut a droite
    assert_eq!(r.pixel(0, 0), [0, 1, 0, 255]);
    assert_eq!(rotate_quarters(&rotate_quarters(&im, 1), 3), im);
    assert_eq!(rotate_quarters(&im, 2).pixel(0, 0), [2, 1, 0, 255]);
    let c = crop(&im, 1, 0, 5, 5);
    assert_eq!((c.width, c.height), (2, 2));
    assert_eq!(crop(&im, -5, -5, 2, 2).width, 0);
}

#[test]
fn pdf_structure() {
    let mut c = Cover::default();
    c.dpi = 72;
    let l = c.layout(72, Path::new("."));
    let sheet = l.background();
    let pdf = c.to_pdf(&sheet, "Mon (film)");
    let text = String::from_utf8_lossy(&pdf).into_owned();
    assert!(text.starts_with("%PDF-1.4"));
    assert!(text.contains("/MediaBox [0 0 788.031 436.535]"), "{}", &text[..300.min(text.len())]);
    assert!(text.contains("/Title (Mon film)"));
    // table des references : chaque decalage pointe sur « n 0 obj » (calcule sur les octets : le flux d'image est binaire)
    let at = |hay: &[u8], pat: &[u8]| hay.windows(pat.len()).rposition(|w| w == pat).unwrap();
    let sx = at(&pdf, b"startxref\n") + 10;
    let xref_at: usize = std::str::from_utf8(&pdf[sx..]).unwrap().lines().next().unwrap().parse().unwrap();
    assert!(pdf[xref_at..].starts_with(b"xref\n0 7\n"));
    let table = std::str::from_utf8(&pdf[xref_at..sx - 10]).unwrap();
    for (n, line) in table.lines().skip(3).take(6).enumerate() {
        let off: usize = line[..10].parse().unwrap();
        assert!(pdf[off..].starts_with(format!("{} 0 obj", n + 1).as_bytes()), "objet {}", n + 1);
    }
    // l'image decompressee retrouve les pixels (RVB)
    let key = b"/Filter /FlateDecode /Length ";
    let p = pdf.windows(key.len()).position(|w| w == key).unwrap() + key.len();
    let end = pdf[p..].iter().position(|&b| b == b' ').unwrap();
    let len: usize = std::str::from_utf8(&pdf[p..p + end]).unwrap().parse().unwrap();
    let start = p + pdf[p..].windows(7).position(|w| w == b"stream\n").unwrap() + 7;
    let raw = zlib_decompress_fixed(&pdf[start..start + len]).unwrap();
    assert_eq!(raw.len(), (sheet.width * sheet.height * 3) as usize);
    assert_eq!(&raw[..3], &[24, 28, 40]);
    // traits de coupe : page agrandie et lignes dessinees
    c.marks = true;
    let with = String::from_utf8_lossy(&c.to_pdf(&sheet, "t")).into_owned();
    assert!(with.contains("/MediaBox [0 0 844.724 493.228]") && with.contains(" l S"));
}

#[test]
fn history_undo_redo() {
    let mut cur = 0i32;
    let mut h = EditHistory::new(&cur);
    cur = 1; h.observe(&cur, 1000);
    cur = 2; h.observe(&cur, 1100); // fusionne avec la precedente
    cur = 5; h.observe(&cur, 5000);
    assert!(h.can_undo() && !h.can_redo());
    assert!(h.undo(&mut cur)); assert_eq!(cur, 2);
    assert!(h.undo(&mut cur)); assert_eq!(cur, 0);
    assert!(!h.undo(&mut cur));
    assert!(h.redo(&mut cur)); assert_eq!(cur, 2);
    cur = 9; h.observe(&cur, 9000);
    assert!(!h.can_redo());
}

#[test]
fn label_json_layout_pdf() {
    assert!(DiscLabel::default().to_value().is_none());
    let mut l = DiscLabel::default();
    l.marks = true;
    l.color = [10, 20, 30];
    l.items.push(CoverItem::text("{titre}", 20.0, 10.0, 80.0, 20.0, 20.0));
    let back = DiscLabel::from_value(l.to_value().as_ref()).unwrap();
    assert_eq!(back, l);
    let mut m = ProjectModel::default();
    m.label = l.clone();
    assert_eq!(ProjectModel::from_json(&m.to_json()).unwrap().label, l);
    assert_eq!(l.sheet_mm(), (122.0, 122.0));
    let lay = l.layout_with(72, Path::new("."), &Default::default());
    let pdf = l.to_pdf(&lay.background(), "etiquette");
    assert!(pdf.starts_with(b"%PDF-1.4"));
    std::fs::write(std::env::temp_dir().join("bd_label_demo.pdf"), &pdf).unwrap();
}

#[test]
fn token_substitution_and_vars() {
    let mut v = std::collections::BTreeMap::new();
    v.insert("titre".to_string(), "Mon film".to_string());
    assert_eq!(apply_vars("« {titre} » {inconnu}", &v), "« Mon film » {inconnu}");
    assert_eq!(clock(3725.0), "1:02:05");
    assert_eq!(clock(65.0), "1:05");
    let mut m = ProjectModel::default();
    m.title = "Titre".into();
    let vars = m.cover_vars(&Default::default(), 2026);
    assert_eq!(vars["titre"], "Titre");
    assert_eq!(vars["nb_videos"], "0");
    assert_eq!(vars["annee"], "2026");
}
