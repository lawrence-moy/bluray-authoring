use bdauthor_core::barcode::BarcodeKind;
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
    assert_eq!(c.trim_mm(), (269.0, 148.0));
    assert_eq!(c.sheet_mm(), (275.0, 154.0));
    let l = c.layout(300, Path::new("."));
    assert_eq!(l.size, (3248, 1819));
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
    match d { CoverDecor::Text { x, w, size, .. } => assert_eq!((*x, *w, *size), (1724, 1524, 150)), _ => panic!() }
    assert_eq!(l.items[1].1, 90);
    match &l.items[2].0 { CoverDecor::Image { path, x, .. } => { assert_eq!(*x, 0); assert!(path.ends_with("img/logo.png") && path.starts_with("/proj")); } _ => panic!() }
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
    assert!(!ProjectModel::default().to_json().contains("\"cover\""));
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
    assert!(text.contains("/MediaBox [0 0 779.528 436.535]"), "{}", &text[..300.min(text.len())]);
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
    assert!(with.contains("/MediaBox [0 0 836.220 493.228]") && with.contains(" l S"));
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

#[test]
fn layers_order_visibility_json() {
    let mut c = Cover::default();
    let mut a = CoverItem::text("A", 0.0, 0.0, 10.0, 10.0, 12.0); a.layer = 1;
    let b = CoverItem::text("B", 0.0, 0.0, 10.0, 10.0, 12.0);
    let mut d = CoverItem::text("C", 0.0, 0.0, 10.0, 10.0, 12.0); d.layer = 1;
    c.items = vec![a, b, d];
    // sans calques declares : tout au meme niveau, ordre de la liste... ici les numeros comptent
    assert_eq!(draw_order(&c.items, &c.layers), vec![1, 0, 2]);
    c.layers = vec![CoverLayer::new("Fond"), CoverLayer { name: "Haut".into(), visible: false, locked: true }];
    assert_eq!(draw_order(&c.items, &c.layers), vec![1]);
    assert_eq!(layer_state(&c.layers, 1), (false, true));
    let back = Cover::from_value(c.to_value().as_ref()).unwrap();
    assert_eq!(back, c);
    assert_eq!(normalize_layers(vec![CoverLayer::new("Calque 1")]), vec![]);
    assert_eq!(layers_or_default(&[]).len(), 1);
    // un calque masque disparait du rendu
    let l1 = c.layout(72, Path::new(".")).items.len();
    c.layers[1].visible = true;
    assert_eq!(l1 + 2, c.layout(72, Path::new(".")).items.len());
}

#[test]
fn a4_and_png_outputs() {
    // jaquette Blu-ray standard : 269 x 148 mm finis -> A4 paysage
    let c = Cover::default();
    let sheet = c.layout(72, Path::new(".")).background();
    let pdf = c.to_pdf_a4(&sheet, "A4").unwrap();
    let text = String::from_utf8_lossy(&pdf).into_owned();
    assert!(text.contains("/MediaBox [0 0 841.890 595.276]"), "{}", &text[..300.min(text.len())]);
    std::fs::write(std::env::temp_dir().join("bd_cover_a4.pdf"), &pdf).unwrap();
    // trop grande : refusee
    let mut big = Cover::default();
    big.face_mm = 200.0;
    assert!(big.to_pdf_a4(&sheet, "x").is_err());
    // etiquette : A4 portrait
    let l = DiscLabel::default();
    let ls = l.layout_with(72, Path::new("."), &Default::default()).background();
    let pdf = l.to_pdf_a4(&ls, "e").unwrap();
    assert!(String::from_utf8_lossy(&pdf).contains("/MediaBox [0 0 595.276 841.890]"));
    std::fs::write(std::env::temp_dir().join("bd_label_a4.pdf"), &pdf).unwrap();
    // PNG : signature, pHYs (300 dpi = 11811 px/m), IDAT decompressible
    let png = bdauthor_core::png::write_png_print(&sheet, 300);
    assert_eq!(&png[..8], &[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A]);
    assert!(png.windows(4).any(|w| w == b"pHYs"));
    assert!(png.windows(4).any(|w| w == 11811u32.to_be_bytes()));
    std::fs::write(std::env::temp_dir().join("bd_cover.png"), &png).unwrap();
}

#[test]
fn shapes_and_text_fx_json_layout() {
    let mut c = Cover::default();
    let mut t = CoverItem::text("Titre", 10.0, 10.0, 100.0, 30.0, 30.0);
    if let CoverKind::Text { fx, .. } = &mut t.kind {
        fx.outline_mm = 0.4; fx.outline_color = [255, 0, 0]; fx.shadow_mm = 0.6; fx.spacing = 1.3; fx.valign = VAlign::Bottom; fx.autofit = true; fx.bold = true; fx.italic = true;
    }
    c.items.push(t);
    let mut r = CoverItem::shape(ShapeKind::Rect, 0.0, 0.0, 50.0, 20.0);
    if let CoverKind::Shape { stroke, stroke_mm, radius_mm, .. } = &mut r.kind { *stroke = Some([10, 20, 30]); *stroke_mm = 1.0; *radius_mm = 3.0; }
    c.items.push(r);
    c.items.push(CoverItem::shape(ShapeKind::Ellipse, 5.0, 5.0, 20.0, 20.0));
    c.items.push(CoverItem::shape(ShapeKind::Line, 5.0, 30.0, 80.0, 2.0));
    let back = Cover::from_value(c.to_value().as_ref()).unwrap();
    assert_eq!(back, c);
    let l = c.layout(300, Path::new("."));
    assert_eq!(l.items.len(), 4);
    match &l.items[0].0 { CoverDecor::Text { outline, shadow, spacing, valign, autofit, bold, italic, .. } => { assert_eq!((*outline, *shadow, *autofit, *valign, *bold, *italic), (5, 7, true, VAlign::Bottom, true, true)); assert!((*spacing - 1.3).abs() < 1e-6); } _ => panic!() };
    match &l.items[1].0 { CoverDecor::Shape { stroke_px, radius_px, fill, .. } => assert_eq!((*stroke_px, *radius_px, *fill), (12, 35, Some([255, 255, 255]))), _ => panic!() }
    // un texte simple n'ecrit aucun effet
    let plain = CoverItem::text("x", 0.0, 0.0, 10.0, 10.0, 12.0);
    let mut c2 = Cover::default();
    c2.items.push(plain);
    let json = c2.to_value().unwrap().to_pretty();
    assert!(!json.contains("contour") && !json.contains("ombre") && !json.contains("interligne"));
}

#[test]
fn snapping_and_alignment() {
    // deplacement : le bord gauche (x = 0.8) s'aimante sur 0 ; pas de ligne a portee en y
    let (r, gx, gy) = snap_rect(0, (0.8, 40.0, 20.0, 10.0), &[0.0, 129.0], &[0.0], 1.5);
    assert_eq!((r, gx, gy), ((0.0, 40.0, 20.0, 10.0), Some(0.0), None));
    // le centre s'aimante sur 64.5 (centre de la face) : x + w/2 = 65 -> x = 54.5
    let (r, gx, _) = snap_rect(0, (55.0, 0.0, 20.0, 10.0), &[64.5], &[], 1.0);
    assert!((r.0 - 54.5).abs() < 1e-9 && gx == Some(64.5));
    // coin bas droit : seuls le bord droit et le bord bas bougent
    let (r, gx, gy) = snap_rect(4, (10.0, 10.0, 49.0, 29.0), &[60.0], &[40.0], 2.0);
    assert_eq!(r, (10.0, 10.0, 50.0, 30.0));
    assert_eq!((gx, gy), (Some(60.0), Some(40.0)));
    // trop loin : rien ne change
    assert_eq!(snap_rect(0, (10.0, 10.0, 5.0, 5.0), &[100.0], &[100.0], 1.0), ((10.0, 10.0, 5.0, 5.0), None, None));
    // alignement
    use bdauthor_core::editor::ItemAlign;
    let mut items = vec![CoverItem::text("a", 10.0, 5.0, 20.0, 10.0, 12.0), CoverItem::text("b", 50.0, 30.0, 10.0, 10.0, 12.0), CoverItem::text("c", 100.0, 60.0, 30.0, 10.0, 12.0)];
    assert!(align_cover(&mut items, &[0, 1, 2], ItemAlign::Left, (272.0, 148.0)));
    assert!(items.iter().all(|i| i.x == 10.0));
    assert!(align_cover(&mut items, &[0, 1, 2], ItemAlign::DistributeV, (272.0, 148.0)));
    assert_eq!(items[1].y, (5.0 + 5.0 + 60.0 + 5.0) / 2.0 - 5.0);
    // un seul : centre sur la feuille
    let mut one = vec![CoverItem::text("a", 0.0, 0.0, 20.0, 10.0, 12.0)];
    assert!(align_cover(&mut one, &[0], ItemAlign::CenterH, (272.0, 148.0)));
    assert_eq!(one[0].x, 126.0);
    // guides de securite : jaquette (dos, face, tranche) et etiquette (cercles)
    let c = Cover::default();
    assert_eq!(c.safe_guides().0.len(), 3);
    assert!(c.snap_lines().0.contains(&129.0) && c.snap_lines().0.contains(&143.0));
    assert_eq!(DiscLabel::default().safe_guides().1.len(), 2);
}

#[test]
fn line_endpoints() {
    let mut it = CoverItem::shape(ShapeKind::Line, 0.0, 0.0, 10.0, 0.0);
    it.set_line_ends((50.0, 10.0), (20.0, 40.0));
    assert_eq!((it.x, it.y, it.w, it.h), (20.0, 10.0, 30.0, 30.0));
    assert_eq!(it.line_ends(), Some(((20.0, 40.0), (50.0, 10.0))));
    it.set_line_ends((5.0, 7.0), (45.0, 7.0));
    assert_eq!((it.w, it.h), (40.0, 0.0));
    let mut c = Cover::default();
    it.set_line_ends((60.0, 80.0), (10.0, 20.0));
    c.items.push(it);
    let c2 = Cover::from_value(c.to_value().as_ref()).unwrap();
    assert_eq!(c2.items[0].line_ends(), c.items[0].line_ends());
    let mut h = CoverItem::shape(ShapeKind::Line, 0.0, 0.0, 10.0, 0.0);
    h.set_line_ends((5.0, 7.0), (45.0, 7.0));
    c.items.push(h);
    let c3 = Cover::from_value(c.to_value().as_ref()).unwrap();
    assert_eq!(c3.items[1].h, 0.0);
    let lay = c3.layout(96, std::path::Path::new("."));
    assert!(lay.items.iter().any(|(d, rot)| matches!(d, CoverDecor::Shape { h: 0, .. }) && *rot == 0));
}

#[test]
fn line_styles_arrows_json() {
    let mut it = CoverItem::shape(ShapeKind::Line, 0.0, 0.0, 10.0, 0.0);
    it.set_line_ends((10.0, 10.0), (50.0, 10.0));
    if let CoverKind::Shape { style, arrow, .. } = &mut it.kind { *style = LineStyle::DashDot; *arrow = LineArrow::End; }
    // on tire le bout de droite (fin) au-dela du bout de gauche : la fleche reste sur le bout deplace
    it.move_line_end(false, (10.0, 10.0), (-20.0, 10.0));
    assert_eq!(it.line_ends(), Some(((-20.0, 10.0), (10.0, 10.0))));
    assert!(matches!(&it.kind, CoverKind::Shape { arrow: LineArrow::Start, .. }));
    let mut c = Cover::default();
    c.items.push(it);
    let back = Cover::from_value(c.to_value().as_ref()).unwrap();
    assert_eq!(back, c);
    let plain = CoverItem::shape(ShapeKind::Line, 0.0, 0.0, 10.0, 0.0);
    let mut c2 = Cover::default();
    c2.items.push(plain);
    let json = c2.to_value().unwrap().to_pretty();
    assert!(!json.contains("\"trait\"") && !json.contains("\"fleche\""), "{json}");
}

#[test]
fn video_image_item_json_layout_and_frame() {
    use bdauthor_core::design::VideoFrame;
    let mut c = Cover::default();
    let mut it = CoverItem::video_image(10.0, 20.0, 60.0, 40.0);
    if let CoverKind::VideoImage { frame } = &mut it.kind { *frame = VideoFrame { video: Some(1), at: 2.5, crop: Some([0.1, 0.2, 0.5, 0.4]) }; }
    let sans_video = CoverItem::video_image(0.0, 0.0, 30.0, 30.0);
    c.items = vec![it.clone(), sans_video];
    // enregistrement : video numerotee a partir de 1, image sans video gardee telle quelle
    let text = c.to_value().unwrap().to_pretty();
    assert!(text.contains("\"image_video\"") && text.contains("\"video\": 2") && text.contains("\"time_s\": 2.5"), "{text}");
    let back = Cover::from_value(Some(&bdauthor_core::json::parse(&text).unwrap())).unwrap();
    assert_eq!(back.items, c.items);
    // mise en page : sans image extraite l'element est ignore ; avec elle (cle donnee par `vars`) il devient une image
    assert_eq!(c.layout(72, Path::new(".")).items.len(), 0);
    let CoverKind::VideoImage { frame } = &it.kind else { unreachable!() };
    let key = frame_key(frame).unwrap();
    let mut vars = std::collections::BTreeMap::new();
    vars.insert(key.clone(), "/tmp/image.png".to_string());
    let lay = c.layout_with(72, Path::new("."), &vars);
    assert_eq!(lay.items.len(), 1);
    assert!(matches!(&lay.items[0].0, CoverDecor::Image { path, .. } if path == Path::new("/tmp/image.png")));
    // le modele liste les images a extraire (sans doublon, video existante seulement)
    let mut m = ProjectModel::from_json(r#"{"videos": [{"file": "a.mp4"}, {"file": "b.mp4"}]}"#).unwrap();
    m.cover = c.clone();
    m.label.items.push(it.clone());
    let fr = m.cover_frames();
    assert_eq!(fr.len(), 1);
    assert_eq!((fr[0].0.as_str(), fr[0].1), (key.as_str(), 1));
}

#[test]
fn video_frame_png_is_written_cropped() {
    use bdauthor_core::design::VideoFrame;
    use bdauthor_core::ffmpeg::Tools;
    let Ok(tools) = Tools::locate() else { eprintln!("ffmpeg absent : test ignore"); return };
    let dir = std::env::temp_dir().join(format!("bdauthor_vframe_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let src = dir.join("v.mp4");
    assert!(std::process::Command::new(&tools.ffmpeg).args(["-y", "-v", "error", "-f", "lavfi", "-i", "testsrc=size=640x360:rate=25:duration=3", "-pix_fmt", "yuv420p"]).arg(&src).status().unwrap().success());
    let size = bdauthor_core::framesrc::full_frame_size(&tools, &src).unwrap();
    assert_eq!(size, (1920, 1080));
    let (png, png2) = (dir.join("a.png"), dir.join("b.png"));
    bdauthor_core::framesrc::write_video_frame_png(&tools, &src, size, 3.0, &VideoFrame { video: Some(0), at: 1.0, crop: None }, &png).unwrap();
    bdauthor_core::framesrc::write_video_frame_png(&tools, &src, size, 3.0, &VideoFrame { video: Some(0), at: 1.0, crop: Some([0.0, 0.0, 0.5, 0.5]) }, &png2).unwrap();
    let dim = |p: &Path| { let b = std::fs::read(p).unwrap(); (u32::from_be_bytes(b[16..20].try_into().unwrap()), u32::from_be_bytes(b[20..24].try_into().unwrap())) };
    assert_eq!(dim(&png), (1920, 1080));
    assert_eq!(dim(&png2), (960, 540));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn cover_video_images_follow_video_removal_and_swap() {
    use bdauthor_core::cover::{CoverItem, CoverKind};
    let mut m = ProjectModel::from_json(r#"{"videos":[{"file":"a.mkv"},{"file":"b.mkv"},{"file":"c.mkv"}]}"#).unwrap();
    for v in [0usize, 1, 2] {
        let mut it = CoverItem::video_image(0.0, 0.0, 10.0, 10.0);
        if let CoverKind::VideoImage { frame } = &mut it.kind { frame.video = Some(v); }
        m.cover.items.push(it);
    }
    let vids = |m: &ProjectModel| m.cover.items.iter().map(|i| match &i.kind { CoverKind::VideoImage { frame } => frame.video, _ => unreachable!() }).collect::<Vec<_>>();
    m.swap_videos(0, 2);
    assert_eq!(vids(&m), vec![Some(2), Some(1), Some(0)]);
    m.remove_video(1);
    assert_eq!(vids(&m), vec![Some(1), None, Some(0)]);
}

#[test]
fn info_table_round_trips_and_lays_out() {
    use bdauthor_core::cover::{info_key, InfoField, InfoTable, CoverDecor, INFO_COUNT_KEY};
    let mut c = Cover::default();
    let t = InfoTable { videos: vec![1], fields: vec![InfoField::Duration, InfoField::Languages], header: true, transpose: false, header_fill: Some([10, 20, 30]), grid: None, ..InfoTable::default() };
    c.items.push(CoverItem { kind: CoverKind::InfoTable { table: t.clone() }, x: 10.0, y: 20.0, w: 80.0, h: 20.0, rot: 0, name: String::new(), layer: 0 });
    let v = c.to_value().unwrap();
    let back = Cover::from_value(Some(&v)).unwrap();
    assert_eq!(back.items[0].kind, CoverKind::InfoTable { table: t });
    // valeurs : la video 2 seulement ; en-tete avec fond
    let mut vars = std::collections::BTreeMap::new();
    vars.insert(INFO_COUNT_KEY.to_string(), "2".to_string());
    vars.insert(info_key(0, InfoField::Duration), "10 min".to_string());
    vars.insert(info_key(1, InfoField::Duration), "95 min".to_string());
    vars.insert(info_key(1, InfoField::Languages), "Français, English".to_string());
    vars.insert("@info:label:duration".to_string(), "Durée".to_string());
    let lay = c.layout_with(300, std::path::Path::new("."), &vars);
    let texts: Vec<String> = lay.items.iter().filter_map(|(d, _)| match d { CoverDecor::Text { text, .. } => Some(text.clone()), _ => None }).collect();
    assert_eq!(texts, vec!["Durée", "languages", "95 min", "Français, English"]);
    let fills = lay.items.iter().filter(|(d, _)| matches!(d, CoverDecor::Shape { fill: Some([10, 20, 30]), .. })).count();
    assert_eq!(fills, 2, "fond des deux intitules");
    // les cases restent dans le cadre de l'element
    let (x0, y0) = (((3.0 + 10.0) / 25.4 * 300.0) as i32, ((3.0 + 20.0) / 25.4 * 300.0) as i32);
    for (d, rot) in &lay.items {
        assert_eq!(*rot, 0);
        let (x, y, w, h) = d.rect();
        assert!(x >= x0 - 1 && y >= y0 - 1 && x + w <= x0 + 946 + 2 && y + h <= y0 + 237 + 2, "{:?}", d.rect());
    }
}

#[test]
fn info_table_transposed_with_grid() {
    use bdauthor_core::cover::{info_key, InfoField, InfoTable, CoverDecor, INFO_COUNT_KEY};
    let t = InfoTable { fields: vec![InfoField::Format, InfoField::Sound], transpose: true, ..InfoTable::default() };
    let mut vars = std::collections::BTreeMap::new();
    vars.insert(INFO_COUNT_KEY.to_string(), "1".to_string());
    vars.insert(info_key(0, InfoField::Format), "HD 1080p · 16/9".to_string());
    vars.insert(info_key(0, InfoField::Sound), "Dolby Digital 2.0".to_string());
    let cells = t.cells(&vars);
    assert_eq!(cells.len(), 2);
    assert_eq!(cells[0], vec![("format".to_string(), true), ("HD 1080p · 16/9".to_string(), false)]);
    let mut c = Cover::default();
    c.items.push(CoverItem { kind: CoverKind::InfoTable { table: t }, x: 0.0, y: 0.0, w: 60.0, h: 12.0, rot: 90, name: String::new(), layer: 0 });
    let lay = c.layout_with(150, std::path::Path::new("."), &vars);
    // grille : 3 traits horizontaux et 3 verticaux (2 lignes, 2 colonnes) ; le tableau reste a l'horizontale
    let lines = lay.items.iter().filter(|(d, _)| matches!(d, CoverDecor::Shape { shape: ShapeKind::Line, .. })).count();
    assert_eq!(lines, 6);
    assert!(lay.items.iter().all(|(_, r)| *r == 0));
    // sans video : rien
    let mut none = vars.clone();
    none.insert(INFO_COUNT_KEY.to_string(), "0".to_string());
    assert!(c.layout_with(150, std::path::Path::new("."), &none).items.is_empty());
}

#[test]
fn info_vars_describe_each_video() {
    use bdauthor_core::cover::{info_key, InfoField};
    let mut m = ProjectModel::default();
    m.videos.push(bdauthor_core::model::VideoEntry::new("a.mkv"));
    m.videos[0].title = "Film".into();
    let fr = m.add_language("Français", "fra");
    let mut d = std::collections::HashMap::new();
    d.insert("a.mkv".to_string(), 5730.0);
    let mut s = std::collections::HashMap::new();
    s.insert("a.mkv".to_string(), (1080u32, 25.0));
    let mut a = bdauthor_core::audio::AudioTrackCfg { source: bdauthor_core::audio::AudioSource::Embedded(0), lang_id: 0, lang: String::new(), offset_ms: 0 };
    a.lang_id = fr;
    m.videos[0].audio.push(a.clone());
    m.videos[0].audio.push(a);
    let v = m.cover_info_vars(&d, &s);
    assert_eq!(v[&info_key(0, InfoField::Title)], "Film");
    assert_eq!(v[&info_key(0, InfoField::Duration)], "96 min");
    assert_eq!(v[&info_key(0, InfoField::Languages)], "Français");
    assert_eq!(v[&info_key(0, InfoField::Subtitles)], "—");
    assert_eq!(v[&info_key(0, InfoField::Format)], "HD 1080p · 16/9");
    assert_eq!(v[&info_key(0, InfoField::Sound)], "Dolby Digital 2.0");
    // duree et format inconnus : vides
    let v = m.cover_info_vars(&Default::default(), &Default::default());
    assert!(!v.contains_key(&info_key(0, InfoField::Duration)) && !v.contains_key(&info_key(0, InfoField::Format)));
}

#[test]
fn info_table_videos_follow_swaps_and_removals() {
    use bdauthor_core::cover::InfoTable;
    let mut m = ProjectModel::default();
    for f in ["a.mkv", "b.mkv", "c.mkv"] { m.videos.push(bdauthor_core::model::VideoEntry::new(f)); }
    m.cover.items.push(CoverItem { kind: CoverKind::InfoTable { table: InfoTable { videos: vec![0, 2], ..InfoTable::default() } }, x: 0.0, y: 0.0, w: 50.0, h: 10.0, rot: 0, name: String::new(), layer: 0 });
    let vids = |m: &ProjectModel| match &m.cover.items[0].kind { CoverKind::InfoTable { table } => table.videos.clone(), _ => unreachable!() };
    m.swap_videos(0, 1);
    assert_eq!(vids(&m), vec![1, 2]);
    m.remove_video(1);
    assert_eq!(vids(&m), vec![1]);
}

#[test]
fn info_table_body_background_is_separate_from_headings() {
    use bdauthor_core::cover::{info_key, InfoField, InfoTable, CoverDecor, INFO_COUNT_KEY};
    let t = InfoTable { fields: vec![InfoField::Duration], header_fill: Some([1, 2, 3]), fill: Some([200, 100, 50]), grid: None, ..InfoTable::default() };
    let mut c = Cover::default();
    c.items.push(CoverItem { kind: CoverKind::InfoTable { table: t.clone() }, x: 0.0, y: 0.0, w: 40.0, h: 30.0, rot: 0, name: String::new(), layer: 0 });
    let back = Cover::from_value(Some(&c.to_value().unwrap())).unwrap();
    assert_eq!(back.items[0].kind, CoverKind::InfoTable { table: t });
    let mut vars = std::collections::BTreeMap::new();
    vars.insert(INFO_COUNT_KEY.to_string(), "2".to_string());
    for v in 0..2 { vars.insert(info_key(v, InfoField::Duration), format!("{} min", 10 + v)); }
    let lay = c.layout_with(100, std::path::Path::new("."), &vars);
    let n = |col: [u8; 3]| lay.items.iter().filter(|(d, _)| matches!(d, CoverDecor::Shape { fill: Some(f), .. } if *f == col)).count();
    assert_eq!((n([1, 2, 3]), n([200, 100, 50])), (1, 2), "1 intitule, 2 cases de valeurs");
}

#[test]
fn info_table_rows_follow_the_chosen_video_order() {
    use bdauthor_core::cover::{info_key, InfoField, InfoTable, INFO_COUNT_KEY};
    let mut vars = std::collections::BTreeMap::new();
    vars.insert(INFO_COUNT_KEY.to_string(), "3".to_string());
    for v in 0..3 { vars.insert(info_key(v, InfoField::Duration), format!("{} min", 10 * (v + 1))); }
    let t = InfoTable { videos: vec![2, 0, 2, 7], fields: vec![InfoField::Duration], header: false, ..InfoTable::default() };
    // doublons et videos inexistantes ignores ; l'ordre de la liste est celui des lignes
    assert_eq!(t.shown_videos(3), vec![2, 0]);
    let cells = t.cells(&vars);
    assert_eq!(cells.iter().map(|r| r[0].0.clone()).collect::<Vec<_>>(), vec!["30 min", "10 min"]);
    // liste vide : toutes, dans l'ordre naturel
    assert_eq!(InfoTable::default().shown_videos(3), vec![0, 1, 2]);
}

#[test]
fn wrap_words_breaks_paragraphs_at_spaces() {
    // chaque lettre vaut 10, l'espace 10 : « aaa bbb » = 70
    let w = |s: &str| s.chars().count() as f32 * 10.0;
    let lines = wrap_words(&["aaa bbb ccc dd", "", "xx"], 75.0, &w, 10.0);
    let txt: Vec<(String, bool)> = lines.iter().map(|l| (l.words.join(" "), l.last)).collect();
    assert_eq!(txt, vec![("aaa bbb".to_string(), false), ("ccc dd".to_string(), true), (String::new(), true), ("xx".to_string(), true)]);
    // un mot trop long reste seul sur sa ligne
    let lines = wrap_words(&["a bbbbbbbbbb c"], 50.0, &w, 10.0);
    let txt: Vec<String> = lines.iter().map(|l| l.words.join(" ")).collect();
    assert_eq!(txt, vec!["a", "bbbbbbbbbb", "c"]);
    // plusieurs espaces et retours : un seul separateur
    assert_eq!(wrap_words(&["  a   b  "], 100.0, &w, 10.0)[0].words, vec!["a", "b"]);
}

#[test]
fn justify_option_is_saved_and_reaches_the_layout() {
    let mut c = Cover::default();
    let mut it = CoverItem::text("Un long texte a justifier", 10.0, 10.0, 80.0, 30.0, 12.0);
    if let CoverKind::Text { fx, .. } = &mut it.kind { fx.justify = true; }
    c.items.push(it);
    let json = c.to_value().unwrap().to_pretty();
    assert!(json.contains("\"justify\": true"), "{json}");
    let back = Cover::from_value(Some(&c.to_value().unwrap())).unwrap();
    assert_eq!(back.items[0].kind, c.items[0].kind);
    match &back.layout(100, std::path::Path::new(".")).items[0].0 { CoverDecor::Text { justify, .. } => assert!(*justify), _ => panic!() }
    // absent par defaut : rien d'ecrit
    let mut d = Cover::default();
    d.items.push(CoverItem::text("x", 0.0, 0.0, 10.0, 10.0, 12.0));
    assert!(!d.to_value().unwrap().to_pretty().contains("justify"));
}

#[test]
fn barcode_round_trips_and_lays_out() {
    let mut c = Cover::default();
    let spec = BarcodeSpec { kind: BarcodeKind::Ean13, value: "5901234123457".into(), color: [10, 20, 30], background: None, show_text: true, size_pt: 9.0, font: String::new() };
    c.items.push(CoverItem { kind: CoverKind::Barcode { code: spec.clone() }, x: 10.0, y: 10.0, w: 40.0, h: 20.0, rot: 0, name: String::new(), layer: 0 });
    let v = c.to_value().unwrap();
    let json = v.to_pretty();
    assert!(json.contains("\"code_barres\"") && json.contains("\"ean13\""), "{json}");
    let back = Cover::from_value(Some(&v)).unwrap();
    assert_eq!(back.items[0].kind, c.items[0].kind);
    // transparent : pas de fond ; 30 barres au plus pour un EAN-13 (59 barres noires environ), texte au centre
    let lay = back.layout(300, Path::new("."));
    let bars = lay.items.iter().filter(|(d, _)| matches!(d, CoverDecor::Shape { fill: Some(f), .. } if *f == [10, 20, 30])).count();
    assert!(bars >= 25 && bars <= 40, "{bars} barres");
    assert!(lay.items.iter().any(|(d, _)| matches!(d, CoverDecor::Text { text, .. } if text == "5 901234 123457")));
    assert!(!lay.items.iter().any(|(d, _)| matches!(d, CoverDecor::Shape { fill: Some([255, 255, 255]), .. })), "pas de fond");
}

#[test]
fn barcode_bars_stay_inside_the_frame_with_whole_pixel_modules() {
    let spec = BarcodeSpec::default();
    let vars = std::collections::BTreeMap::new();
    let r = (100, 50, 1000, 300);
    let d = spec.decors(r, 300, &vars, Path::new("."));
    // fond blanc d'abord
    assert!(matches!(&d[0], CoverDecor::Shape { fill: Some([255, 255, 255]), x: 100, y: 50, w: 1000, h: 300, .. }));
    let mut widths = std::collections::BTreeSet::new();
    for dec in &d[1..] {
        if let CoverDecor::Shape { x, y, w, h, .. } = dec {
            assert!(*x >= r.0 && x + w <= r.0 + r.2 && *y >= r.1 && y + h <= r.1 + r.3, "barre hors du cadre : {x} {y} {w} {h}");
            widths.insert(*w);
        }
    }
    // Code 128 : largeurs de barres multiples du module entier
    let module = *widths.iter().next().unwrap();
    assert!(widths.iter().all(|w| w % module == 0), "{widths:?}");
    // valeur impossible : seul le fond reste
    let bad = BarcodeSpec { kind: BarcodeKind::Ean13, value: "abc".into(), ..BarcodeSpec::default() };
    assert_eq!(bad.decors(r, 300, &vars, Path::new(".")).len(), 1);
    // les variables sont remplacees
    let mut v2 = std::collections::BTreeMap::new();
    v2.insert("ref".to_string(), "AB12".to_string());
    let s2 = BarcodeSpec { value: "{ref}".into(), ..BarcodeSpec::default() };
    assert_eq!(s2.resolved(&v2), "AB12");
    assert!(s2.decors(r, 300, &v2, Path::new(".")).len() > 5);
}
