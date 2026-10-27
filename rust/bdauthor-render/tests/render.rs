use bdauthor_render::*;

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
    let [n, s] = r.thumb_button(&thumb, "Une video avec un titre vraiment tres tres long pour tester la troncature", 240, 167, 135);
    assert_eq!((n.width, n.height), (240, 167));
    assert_ne!(n, s);
    let [t, _] = r.text_button("Tout lire", 360, 80);
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
    let bg = r.compose_background(None, size, &[Decor::Text { text: "Titre\nSous-titre".into(), x: 40, y: 20, w: 880, h: 100, size: 40, color: [255, 200, 0], align: Align::Left, font: String::new() }]).unwrap();
    assert!(bg.data.chunks_exact(4).any(|p| p[0] > 200 && p[1] > 150 && p[2] < 80), "du texte jaune doit etre dessine");
    let d = PageDesign::starting_point(MenuType::Chapters, 3, 0, 2, "Mes chapitres").unwrap();
    let slots = vec![PreviewSlot { label: "Intro (0:00)".into(), thumb: None, key: 0 }; 3];
    let p = r.render_page_preview(&d, MenuType::Chapters, 0, 2, &slots, None, size, std::path::Path::new("."));
    assert_eq!((p.width, p.height), size);
    assert_ne!(p, gradient(size));
    // cover_resize : dimensions exactes
    let c = cover_resize(&rounded_rect(200, 100, 0.0, [1, 2, 3]), 50, 50);
    assert_eq!((c.width, c.height), (50, 50));
    // image decorative absente : erreur propre et non plantage
    assert!(r.compose_background(None, size, &[Decor::Image { path: "inexistante.png".into(), x: 0, y: 0, w: 10, h: 10 }]).is_err());
}
