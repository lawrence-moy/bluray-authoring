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
    let bg = r.compose_background(None, size, &[Decor::Text { text: "Titre\nSous-titre".into(), x: 40, y: 20, w: 880, h: 100, size: 40, color: [255, 200, 0], align: Align::Left, font: String::new() }]).unwrap();
    assert!(bg.data.chunks_exact(4).any(|p| p[0] > 200 && p[1] > 150 && p[2] < 80), "du texte jaune doit etre dessine");
    let d = PageDesign::starting_point(MenuType::Chapters, 3, 0, 2, "Mes chapitres").unwrap();
    let slots = vec![PreviewSlot { label: "Intro (0:00)".into(), thumb: None, key: 0 }; 3];
    let st = bdauthor_core::style::ButtonStyle::default();
    let p = r.render_page_preview(&d, MenuType::Chapters, 0, 2, &slots, None, size, std::path::Path::new("."), &st, None, false);
    assert_eq!((p.width, p.height), size);
    assert_ne!(p, gradient(size));
    // element selectionne : l'apercu change
    let p2 = r.render_page_preview(&d, MenuType::Chapters, 0, 2, &slots, None, size, std::path::Path::new("."), &st, Some(1), false);
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
    let d = Decor::Text { text: "Lire".into(), x: 0, y: 0, w: 200, h: 60, size: 40, color: [255, 255, 0], align: bdauthor_core::design::Align::Center, font: String::new() };
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
