use bdauthor_render::*;
use std::sync::Arc;

#[test]
fn rounded_rect_corners_transparent() {
    let im = rounded_rect(100, 40, 16.0, [10, 20, 30]);
    assert_eq!(im.pixel(0, 0)[3], 0);
    assert_eq!(im.pixel(50, 20), [10, 20, 30, 255]);
}

#[test]
fn text_button_renders_when_font_available() {
    let Some(path) = find_font() else { eprintln!("aucune police trouvee : test ignore"); return };
    let font = load_font(&path).unwrap();
    let st = text_button_states(&font, "Film 1", 480, 96);
    assert_eq!(st.len(), 3);
    // du texte blanc doit etre present au milieu
    assert!(st[0].data.chunks_exact(4).any(|p| p[0] > 200 && p[1] > 200 && p[2] > 200 && p[3] == 255));
}

#[test]
fn png_roundtrip_and_menu_clip() {
    use bdauthor_core::menu::*;
    use std::sync::Arc;
    let dir = std::env::temp_dir().join("bdauthor_render_test");
    std::fs::create_dir_all(&dir).unwrap();
    let p = dir.join("b.png");
    let src = rounded_rect(200, 80, 12.0, [200, 40, 40]);
    image::RgbaImage::from_raw(200, 80, src.data.clone()).unwrap().save(&p).unwrap();
    let back = load_image(&p).unwrap();
    assert_eq!(back, src);
    let b = vec![MenuButton::new("x", 1, 100, 100, vec![Arc::new(back)])];
    let clip = build_menu_clip(&b, &MenuOptions::default()).unwrap();
    assert!(clip.m2ts.len() > 192 * 10);
}

#[test]
fn font_renderer_builds_menu_elements() {
    use bdauthor_core::project::Renderer;
    let Ok(r) = FontRenderer::new() else { eprintln!("aucune police : test ignore"); return };
    let thumb = rounded_rect(240, 135, 0.0, [200, 100, 50]);
    let st = bdauthor_core::style::ButtonStyle::default();
    let [n, s, a] = r.thumb_button(&thumb, "Une video avec un titre vraiment tres tres long pour tester la troncature", 240, 167, 135, &st);
    assert_eq!((n.width, n.height), (240, 167));
    assert_ne!(n, s);
    assert!(Arc::ptr_eq(&s, &a), "sans etat active distinct, l'image selectionnee est partagee");
    let [t, _, _] = r.text_button("Tout lire", 360, 80, &st);
    assert!(t.data.chunks_exact(4).any(|p| p[0] == 255 && p[1] == 255 && p[2] == 255));
    let bg = r.background("Mes videos", (1280, 720));
    assert_eq!((bg.width, bg.height), (1280, 720));
    assert!(bg.data.chunks_exact(4).all(|p| p[3] == 255));
}

#[test]
fn compose_and_preview() {
    use bdauthor_core::design::*;
    use bdauthor_core::project::{Decor, Renderer};
    let Ok(r) = FontRenderer::new() else { return };
    let size = (960, 540);
    let bg = r.compose_background(None, size, &[Decor::Text { text: "Titre\nSous-titre".into(), x: 40, y: 20, w: 880, h: 100, size: 40, color: [255, 200, 0], align: Align::Left, font: String::new(), deco: Default::default() }]).unwrap();
    assert!(bg.data.chunks_exact(4).any(|p| p[0] > 200 && p[1] > 150 && p[2] < 80), "du texte jaune doit etre dessine");
    let mut d = PageDesign::blank_free(0);
    for k in 0..3 { d.items.push(Item::video_slot(100 + 560 * k, 300, 500, 330, 280, 0, Some(60.0 * k as f64), "Intro")); }
    let slots = vec![PreviewSlot { label: "Intro (0:00)".into(), thumb: None, key: 0 }; 3];
    let st = bdauthor_core::style::ButtonStyle::default();
    let p = r.render_page_preview(&d, 0, 2, &slots, None, size, std::path::Path::new("."), &st, None, false);
    assert_eq!((p.width, p.height), size);
    assert_ne!(p, gradient(size));
    // element selectionne : l'apercu change
    let p2 = r.render_page_preview(&d, 0, 2, &slots, None, size, std::path::Path::new("."), &st, Some(1), false);
    assert_ne!(p, p2);
    // cover_resize : dimensions exactes
    let c = cover_resize(&rounded_rect(200, 100, 0.0, [1, 2, 3]), 50, 50);
    assert_eq!((c.width, c.height), (50, 50));
    // image decorative absente : erreur propre et non plantage
    assert!(r.compose_background(None, size, &[Decor::Image { path: "inexistante.png".into(), x: 0, y: 0, w: 10, h: 10 }]).is_err());
}

#[test]
fn button_styles() {
    use bdauthor_core::project::{Decor, Renderer};
    use bdauthor_core::style::ButtonStyle;
    let Ok(r) = FontRenderer::new() else { return };
    let mut st = ButtonStyle::default();
    st.glow = 12; st.zoom = 10; st.activated = true; st.radius = 16; st.dim = 40; st.sel_border = [255, 0, 0]; st.act_bg = [0, 200, 0];
    let (mx, my) = st.margins(300, 80);
    let [n, s, a] = r.text_button("OK", 300, 80, &st);
    for im in [&n, &s, &a] { assert_eq!((im.width as i32, im.height as i32), (300 + 2 * mx, 80 + 2 * my)); }
    assert!(!Arc::ptr_eq(&s, &a));
    // normal : marges transparentes (pas de halo), coins arrondis transparents, couleur assombrie
    assert_eq!(n.pixel(0, 0)[3], 0);
    assert_eq!(n.pixel(mx as u32, my as u32)[3], 0, "coin arrondi");
    let c = n.pixel((mx + 150) as u32, (my + 10) as u32);
    assert!(c[2] < 58 && c[3] == 255, "fond assombri : {c:?}");
    // selectionne : halo dans la marge, cadre rouge
    assert!(s.pixel(6, (my + 40) as u32)[3] > 0, "halo");
    assert!(s.data.chunks_exact(4).any(|p| p[0] > 200 && p[1] < 40 && p[2] < 40 && p[3] == 255), "cadre rouge");
    // active : fond vert
    assert!(a.data.chunks_exact(4).any(|p| p[1] > 150 && p[0] < 60 && p[3] == 255));
    // texte cliquable souligne, sans fond
    let mut u = ButtonStyle::default();
    u.underline = true; u.backdrop = 0;
    let d = Decor::Text { text: "Lire".into(), x: 0, y: 0, w: 200, h: 60, size: 40, color: [255, 255, 0], align: bdauthor_core::design::Align::Center, font: String::new(), deco: Default::default() };
    let [dn, ds, _] = r.decor_button(&d, 200, 60, &u);
    assert_eq!(dn.pixel(100, 58)[3], 0, "normal : rien sous le texte");
    assert_eq!(ds.pixel(100, 58), [255, 255, 255, 255], "selectionne : soulignement");
    assert_eq!(ds.pixel(2, 2)[3], 0, "pas de fond");
    // bouton dessine par l'utilisateur
    let dir = std::env::temp_dir().join(format!("bdr_skin_{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("n.png"), bdauthor_core::png::write_png(&rounded_rect(100, 40, 0.0, [10, 20, 30]))).unwrap();
    std::fs::write(dir.join("s.png"), bdauthor_core::png::write_png(&rounded_rect(100, 40, 0.0, [200, 210, 220]))).unwrap();
    let base = ButtonStyle::default();
    let [kn, ks, ka] = r.skin_button(&[Some(dir.join("n.png")), Some(dir.join("s.png")), None], 200, 80, &base).unwrap();
    assert_eq!(kn.pixel(100, 40), [10, 20, 30, 255]);
    assert_eq!(ks.pixel(100, 40), [200, 210, 220, 255]);
    assert!(Arc::ptr_eq(&ks, &ka));
    assert!(r.skin_button(&[Some(dir.join("absente.png")), None, None], 10, 10, &base).is_err());
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn cover_text_spans_change_colour_of_part_only() {
    use bdauthor_core::cover::{Cover, CoverItem, CoverKind};
    use bdauthor_core::design::Align;
    use bdauthor_core::rich::{SpanStyle, TextSpan};
    let Ok(r) = FontRenderer::new() else { return };
    let red = [230, 20, 20];
    let mut c = Cover::default();
    c.colors = [[0, 0, 0]; 3];
    let mut t = CoverItem::text("AAAA BBBB", 0.0, 0.0, 120.0, 40.0, 60.0);
    if let CoverKind::Text { align, fx, .. } = &mut t.kind {
        *align = Align::Left;
        fx.spans = vec![TextSpan { start: 5, end: 9, style: SpanStyle { color: Some(red), bold: Some(true), ..Default::default() } }];
    }
    c.items.push(t);
    let img = r.render_sheet(&c, 100, std::path::Path::new("."), &Default::default()).unwrap();
    // pixels rouges et blancs : les premiers seulement a droite des seconds (mot « BBBB » apres « AAAA »)
    let (mut white_max, mut red_min, mut reds) = (0u32, u32::MAX, 0);
    for y in 0..img.height {
        for x in 0..img.width {
            let p = img.pixel(x, y);
            if p[0] > 200 && p[1] > 200 && p[2] > 200 { white_max = white_max.max(x); }
            if p[0] > 180 && p[1] < 60 && p[2] < 60 { red_min = red_min.min(x); reds += 1; }
        }
    }
    assert!(reds > 50, "aucun pixel rouge");
    assert!(red_min > white_max, "rouge des x={red_min}, blanc jusqu'a x={white_max}");
}

#[test]
fn cover_text_carets_follow_lines_and_alignment() {
    use bdauthor_core::cover::{item_decor, CoverItem, CoverKind};
    use bdauthor_core::design::Align;
    let Ok(r) = FontRenderer::new() else { return };
    let vars = Default::default();
    let base = std::path::Path::new(".");
    // deux lignes alignees a gauche : abscisses croissantes sur la premiere, retour au bord sur la seconde
    let mut t = CoverItem::text("ABC\nD", 0.0, 0.0, 100.0, 40.0, 30.0);
    if let CoverKind::Text { align, .. } = &mut t.kind { *align = Align::Left; }
    let (d, _) = item_decor(&t, 0.0, 100, base, &vars).unwrap();
    let (_, _, w, h) = d.rect();
    let c = r.cover_text_carets(&d.with_rect(0, 0, w, h)).unwrap();
    assert_eq!(c.carets.len(), 6);
    assert_eq!(c.lines, 2);
    let xs: Vec<f32> = c.carets[..4].iter().map(|e| e.1).collect();
    assert!(c.carets[..4].iter().all(|e| e.0 == 0) && xs.windows(2).all(|p| p[1] > p[0]), "{:?}", c.carets);
    assert!(xs[0].abs() < 0.5);
    assert_eq!(c.carets[4].0, 1);
    assert!(c.carets[4].1.abs() < 0.5 && c.carets[5].1 > 1.0);
    // centre : la ligne est centree dans le cadre ; le gras d'une partie elargit la ligne
    let mut t = CoverItem::text("ABC", 0.0, 0.0, 100.0, 40.0, 30.0);
    let (d, _) = item_decor(&t, 0.0, 100, base, &vars).unwrap();
    let plain = r.cover_text_carets(&d).unwrap();
    let mid = (plain.carets[0].1 + plain.carets[3].1) / 2.0;
    assert!((mid - d.rect().2 as f32 / 2.0).abs() < 1.0, "{mid}");
    if let CoverKind::Text { fx, .. } = &mut t.kind { fx.spans = bdauthor_core::rich::apply(&[], 3, 0..3, |s| s.bold = Some(true)); }
    let (d, _) = item_decor(&t, 0.0, 100, base, &vars).unwrap();
    let bold = r.cover_text_carets(&d).unwrap();
    assert!(bold.carets[3].1 - bold.carets[0].1 > plain.carets[3].1 - plain.carets[0].1);
    assert!(bold.carets[0].1 < plain.carets[0].1);
    // justifie sur plusieurs lignes : chaque position a une ligne, croissante
    let txt = "un deux trois quatre cinq six sept huit neuf dix onze douze treize";
    let mut t = CoverItem::text(txt, 0.0, 0.0, 40.0, 60.0, 12.0);
    if let CoverKind::Text { fx, .. } = &mut t.kind { fx.justify = true; }
    let (d, _) = item_decor(&t, 0.0, 100, base, &vars).unwrap();
    let j = r.cover_text_carets(&d).unwrap();
    assert!(j.lines >= 3);
    assert_eq!(j.carets.len(), txt.chars().count() + 1);
    assert!(j.carets.windows(2).all(|p| p[1].0 >= p[0].0));
}

#[test]
fn cover_text_span_size_enlarges_its_line_only() {
    use bdauthor_core::cover::{item_decor, CoverItem, CoverKind};
    let Ok(r) = FontRenderer::new() else { return };
    let (vars, base) = (Default::default(), std::path::Path::new("."));
    let carets = |t: &CoverItem| { let (d, _) = item_decor(t, 0.0, 100, base, &vars).unwrap(); r.cover_text_carets(&d).unwrap() };
    let plain = CoverItem::text("AB CD\nEF", 0.0, 0.0, 120.0, 60.0, 20.0);
    let mut big = plain.clone();
    // « CD » a 40 pt dans un texte de 20 pt
    if let CoverKind::Text { fx, .. } = &mut big.kind { fx.spans = bdauthor_core::rich::apply(&[], 8, 3..5, |s| s.size = Some(40.0)); }
    let (p, b) = (carets(&plain), carets(&big));
    assert!((b.line_h[0] / p.line_h[0] - 2.0).abs() < 0.05, "{:?} {:?}", b.line_h, p.line_h);
    assert!((b.line_h[1] - p.line_h[1]).abs() < 0.01);
    // « CD » deux fois plus large ; « AB » garde sa largeur
    let w = |c: &bdauthor_render::TextCarets, a: usize, z: usize| c.carets[z].1 - c.carets[a].1;
    assert!((w(&b, 3, 5) / w(&p, 3, 5) - 2.0).abs() < 0.15);
    assert!((w(&b, 0, 2) - w(&p, 0, 2)).abs() < 0.5);
    // ajuste au cadre : la taille de la portion suit la reduction du texte
    let mut fit = big.clone();
    fit.w = 15.0;
    if let CoverKind::Text { fx, .. } = &mut fit.kind { fx.autofit = true; }
    let f = carets(&fit);
    assert!(f.line_h[0] / f.line_h[1] > 1.9);
    // le dessin suit : rendu sans erreur, plus de pixels allumes qu'avec le texte simple
    let mut c = bdauthor_core::cover::Cover::default();
    c.colors = [[0, 0, 0]; 3];
    c.items.push(big.clone());
    let img = r.render_sheet(&c, 100, base, &vars).unwrap();
    let lit = |im: &bdauthor_core::palette::RgbaImage| im.data.chunks_exact(4).filter(|p| p[0] > 128).count();
    let mut c2 = c.clone();
    c2.items[0] = plain;
    assert!(lit(&img) > lit(&r.render_sheet(&c2, 100, base, &vars).unwrap()));
}
