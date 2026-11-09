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
    assert!(text.contains("\"image_video\"") && text.contains("\"video\": 2") && text.contains("\"instant\": 2.5"), "{text}");
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
    let mut m = ProjectModel::from_json(r#"{"videos": [{"fichier": "a.mp4"}, {"fichier": "b.mp4"}]}"#).unwrap();
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
