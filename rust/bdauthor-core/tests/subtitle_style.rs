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

fn c(s: i64, e: i64, t: &str) -> Cue { Cue { start_ms: s, end_ms: e, text: t.into(), pos: None } }

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

// ------------------------------------------------------------------ positions donnees par le fichier
fn pos(h: HAlign, v: VPos) -> Option<CuePos> { Some(CuePos { h, v }) }

#[test]
fn an_tags_all_nine() {
    let want = [
        (7, HAlign::Left, VPos::Top), (8, HAlign::Center, VPos::Top), (9, HAlign::Right, VPos::Top),
        (4, HAlign::Left, VPos::Middle), (5, HAlign::Center, VPos::Middle), (6, HAlign::Right, VPos::Middle),
        (1, HAlign::Left, VPos::Bottom), (2, HAlign::Center, VPos::Bottom), (3, HAlign::Right, VPos::Bottom),
    ];
    for (n, h, v) in want {
        assert_eq!(an_tag(&format!("{{\\an{n}}}Texte")), pos(h, v), "an{n}");
    }
    // balise melangee a d'autres, derniere gagnante, absence, valeurs invalides
    assert_eq!(an_tag("{\\i1\\an9\\b1}x"), pos(HAlign::Right, VPos::Top));
    assert_eq!(an_tag("{\\an7}a{\\an3}b"), pos(HAlign::Right, VPos::Bottom));
    assert_eq!(an_tag("rien"), None);
    assert_eq!(an_tag("{\\an0}x"), None);
    assert_eq!(an_tag("{\\anX}x"), None);
}

#[test]
fn positions_read_from_ass_and_srt() {
    let ass = "[Script Info]\nPlayResX: 1920\n\n[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\nDialogue: 0,0:00:01.00,0:00:03.00,Default,,0,0,0,,{\\an8}Haut\\NDeux lignes\nDialogue: 0,0:00:04.00,0:00:05.00,Default,,0,0,0,,Sans balise\n";
    let c = parse_ass(ass);
    assert_eq!(c.len(), 2);
    assert_eq!((c[0].text.as_str(), c[0].pos), ("Haut\nDeux lignes", pos(HAlign::Center, VPos::Top)));
    assert_eq!(c[1].pos, None);
    let srt = "1\n00:00:01,000 --> 00:00:02,000\n{\\an7}Haut gauche\n\n2\n00:00:03,000 --> 00:00:04,000\nNormal\n";
    let c = parse_srt(srt);
    assert_eq!((c[0].text.as_str(), c[0].pos), ("Haut gauche", pos(HAlign::Left, VPos::Top)));
    assert_eq!(c[1].pos, None);
    // le decalage et le retrait des indications gardent la position
    assert_eq!(shift(&c, 500)[0].pos, c[0].pos);
    assert_eq!(strip_notes(&[Cue { start_ms: 0, end_ms: 1000, text: "[x] Salut".into(), pos: pos(HAlign::Right, VPos::Top) }])[0].pos, pos(HAlign::Right, VPos::Top));
    assert!(clear_positions(&c).iter().all(|x| x.pos.is_none()));
}

#[test]
fn merge_keeps_first_requested_position() {
    let a = Cue { start_ms: 0, end_ms: 3000, text: "Radio".into(), pos: pos(HAlign::Center, VPos::Top) };
    let b = Cue { start_ms: 1000, end_ms: 4000, text: "Dialogue".into(), pos: None };
    let m = merge(&[a, b]);
    assert_eq!(m.len(), 3);
    assert_eq!(m[0].pos, pos(HAlign::Center, VPos::Top));
    assert_eq!(m[1].text, "Radio\nDialogue");
    assert_eq!(m[1].pos, pos(HAlign::Center, VPos::Top));
    assert_eq!(m[2].pos, None);
    // deux intervalles contigus de meme texte mais de positions differentes ne sont pas soudes
    let x = Cue { start_ms: 0, end_ms: 1000, text: "Pareil".into(), pos: None };
    let y = Cue { start_ms: 1000, end_ms: 2000, text: "Pareil".into(), pos: pos(HAlign::Left, VPos::Top) };
    assert_eq!(merge(&[x, y]).len(), 2);
}

#[test]
fn cue_position_overrides_track_in_pgs() {
    let st = SubStyle::default();
    let at = |p| render_cue_at("Texte", &st, p, &Flat, (1920, 1080)).unwrap().unwrap();
    let base = at(None);
    assert_eq!(base.y as u32 + base.h as u32, 1080 - 70);
    let tl = at(pos(HAlign::Left, VPos::Top));
    assert_eq!((tl.x, tl.y), (100, 70));
    let mr = at(pos(HAlign::Right, VPos::Middle));
    assert_eq!(mr.x as u32 + mr.w as u32, 1920 - 100);
    assert_eq!(mr.y as u32, (1080 - mr.h as u32) / 2);
    let bc = at(pos(HAlign::Center, VPos::Bottom));
    assert_eq!((bc.x, bc.y), (base.x, base.y));
    // piste complete : une position par replique
    let cues = vec![
        Cue { start_ms: 1000, end_ms: 2000, text: "A".into(), pos: pos(HAlign::Center, VPos::Top) },
        Cue { start_ms: 3000, end_ms: 4000, text: "B".into(), pos: None },
    ];
    let specs = encode_track(&cues, &st, &Flat, (1920, 1080), 1, 0, 0).unwrap();
    assert!(specs.len() >= 4);
}

#[test]
fn file_positions_flag_json() {
    let mut t = SubTrackCfg::from_file("a.ass", "fra");
    assert!(t.file_positions);
    assert!(!t.to_value().to_pretty().contains("positions_fichier"));
    t.file_positions = false;
    let j = t.to_value().to_pretty();
    assert!(j.contains("\"positions_fichier\": false"), "{j}");
    assert!(!SubTrackCfg::from_value(&t.to_value()).unwrap().file_positions);
}
