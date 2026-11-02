//! Reglages avances des sous-titres : alignement, ombre, boite, largeur, indications retirees, controle de qualite.
use bdauthor_core::palette::RgbaImage;
use bdauthor_core::pgs::*;
use bdauthor_core::project::Renderer;
use bdauthor_core::subtitle::*;
use std::path::Path;
use std::sync::Arc;

struct Flat;
impl Renderer for Flat {
    fn thumb_button(&self, _: &RgbaImage, _: &str, _: u32, _: u32, _: u32, _: &bdauthor_core::style::ButtonStyle) -> [Arc<RgbaImage>; 3] { unimplemented!() }
    fn text_button(&self, _: &str, _: u32, _: u32, _: &bdauthor_core::style::ButtonStyle) -> [Arc<RgbaImage>; 3] { unimplemented!() }
    fn background(&self, _: &str, _: (u32, u32)) -> RgbaImage { unimplemented!() }
    fn compose_background(&self, _: Option<&Path>, _: (u32, u32), _: &[bdauthor_core::project::Decor]) -> Result<RgbaImage, String> { unimplemented!() }
    fn subtitle(&self, text: &str, _: &SubStyle, max_w: u32) -> Option<RgbaImage> {
        let w = (text.chars().count() as u32 * 10 + 20).min(max_w);
        let mut im = RgbaImage::new(w, 40);
        for y in 0..40 { for x in 0..w { im.set(x, y, [255, 255, 255, 255]); } }
        // deux pixels translucides : ils prennent les opacites de la boite (50 %) et de l'ombre (40 %)
        im.set(2, 2, [0, 0, 0, 127]);
        im.set(3, 2, [90, 90, 90, 102]);
        Some(im)
    }
}

fn c(s: i64, e: i64, t: &str) -> Cue { Cue { start_ms: s, end_ms: e, text: t.into() } }

#[test]
fn horizontal_placement() {
    let mut st = SubStyle::default();
    let mid = render_cue("Texte", &st, &Flat, (1920, 1080)).unwrap().unwrap();
    assert_eq!(mid.x as u32, (1920 - mid.w as u32) / 2);
    st.halign = HAlign::Left;
    st.margin_x = 100;
    let l = render_cue("Texte", &st, &Flat, (1920, 1080)).unwrap().unwrap();
    assert_eq!(l.x, 100);
    st.halign = HAlign::Right;
    let r = render_cue("Texte", &st, &Flat, (1920, 1080)).unwrap().unwrap();
    assert_eq!(r.x as u32 + r.w as u32, 1920 - 100);
    // en 720 la marge suit l'echelle
    let r7 = render_cue("Texte", &st, &Flat, (1280, 720)).unwrap().unwrap();
    assert_eq!(r7.x as u32 + r7.w as u32, 1280 - 67);
}

#[test]
fn style_json_roundtrip_and_defaults_stay_compact() {
    let mut t = SubTrackCfg::from_file("a.srt", "fra");
    let j = t.to_value().to_pretty();
    for k in ["alignement", "ombre", "fond_opacite", "cesure", "largeur_max", "retirer_indications"] {
        assert!(!j.contains(&format!("\"{k}\"")), "{k} ecrit par defaut");
    }
    t.strip_notes = true;
    t.style.halign = HAlign::Right;
    t.style.margin_x = 150;
    t.style.max_width = 70;
    t.style.wrap = true;
    t.style.shadow = 4;
    t.style.shadow_color = [10, 20, 30];
    t.style.shadow_opacity = 55;
    t.style.bg_opacity = 60;
    t.style.bg_color = [1, 2, 3];
    t.style.bg_pad = 20;
    t.style.bg_radius = 0;
    let back = SubTrackCfg::from_value(&t.to_value()).unwrap();
    assert_eq!(back, t);
    let bad = bdauthor_core::json::parse(r#"{"fichier":"a.srt","style":{"alignement":"travers"}}"#).unwrap();
    assert!(SubTrackCfg::from_value(&bad).is_err());
}

#[test]
fn box_and_shadow_use_exact_opacity_levels() {
    let mut st = SubStyle::default();
    st.bg_opacity = 50;
    st.shadow = 3;
    st.shadow_opacity = 40;
    let im = render_cue("Texte", &st, &Flat, (1920, 1080)).unwrap().unwrap();
    let alphas: std::collections::BTreeSet<u8> = im.entries.values().map(|e| e.3).collect();
    assert!(alphas.contains(&127) && alphas.contains(&102), "{alphas:?}");
}

#[test]
fn strip_notes_removes_hearing_impaired_marks() {
    let src = vec![
        c(0, 1000, "[Musique]"),
        c(1000, 2000, "Bonjour (rires) tout le monde"),
        c(2000, 3000, "- (soupir)\n- Oui"),
        c(3000, 4000, "[Il pleut]\nSalut"),
        c(4000, 5000, "(\u{266a}) "),
    ];
    let o = strip_notes(&src);
    assert_eq!(o, vec![c(1000, 2000, "Bonjour tout le monde"), c(2000, 3000, "- Oui"), c(3000, 4000, "Salut")]);
}

#[test]
fn quality_checks() {
    let lim = QualityLimits::default();
    let ok = vec![c(0, 2000, "Bonjour"), c(2500, 5000, "Tout va bien\nici")];
    assert!(check_quality(&ok, &lim).is_empty());
    let bad = vec![
        c(0, 500, "Trop court"),                                   // court (500 ms) ; 10 car / 0,5 s = 20 : pas trop rapide
        c(1000, 2000, &"x".repeat(60)),                              // rapide (60/s) + ligne longue
        c(2030, 2900, "a\nb\nc"),                                    // 3 lignes ; intervalle de 30 ms avec la precedente
        c(3000, 12000, "Longue"),                                    // trop long
    ];
    let k: Vec<(usize, IssueKind)> = check_quality(&bad, &lim).into_iter().map(|i| (i.index, i.kind)).collect();
    assert!(k.contains(&(0, IssueKind::TooShort)));
    assert!(!k.contains(&(0, IssueKind::TooFast)));
    assert!(k.contains(&(1, IssueKind::TooFast)) && k.contains(&(1, IssueKind::LongLine)));
    assert!(k.contains(&(1, IssueKind::TightGap)));
    assert!(k.contains(&(2, IssueKind::TooManyLines)));
    assert!(k.contains(&(3, IssueKind::TooLong)));
}
