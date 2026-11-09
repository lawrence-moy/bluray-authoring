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
    let bad = bdauthor_core::json::parse(r#"{"file":"a.srt","style":{"align":"travers"}}"#).unwrap();
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
fn pos(h: HAlign, v: VPos) -> Option<CuePos> { Some(CuePos { h: Some(h), v: Some(v) }) }

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
    assert!(j.contains("\"file_positions\": false"), "{j}");
    assert!(!SubTrackCfg::from_value(&t.to_value()).unwrap().file_positions);
}

// ------------------------------------------------------------------ alignement des styles ASS / SSA
#[test]
fn ass_style_alignment() {
    let ass = "[V4+ Styles]\nFormat: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding\n\
Style: Default,Arial,56,&H00FFFFFF,&H000000FF,&H00000000,&H00000000,0,0,0,0,100,100,0,0,1,3,0,2,60,60,60,1\n\
Style: *Panneau,Arial,56,&H00FFFFFF,&H000000FF,&H00000000,&H00000000,0,0,0,0,100,100,0,0,1,3,0,8,60,60,60,1\n\
Style: Pensees,Arial,56,&H00FFFFFF,&H000000FF,&H00000000,&H00000000,0,0,0,0,100,100,0,0,1,3,0,7,60,60,60,1\n\
Style: Droite,Arial,56,&H00FFFFFF,&H000000FF,&H00000000,&H00000000,0,0,0,0,100,100,0,0,1,3,0,6,60,60,60,1\n\
\n[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\n\
Dialogue: 0,0:00:01.00,0:00:02.00,Default,,0,0,0,,Normal\n\
Dialogue: 0,0:00:03.00,0:00:04.00,panneau,,0,0,0,,Haut\n\
Dialogue: 0,0:00:05.00,0:00:06.00,Pensees,,0,0,0,,Pensee\n\
Dialogue: 0,0:00:07.00,0:00:08.00,Droite,,0,0,0,,Droite, avec virgule\n\
Dialogue: 0,0:00:09.00,0:00:10.00,Panneau,,0,0,0,,{\\an2}Force le bas\n\
Dialogue: 0,0:00:11.00,0:00:12.00,Inconnu,,0,0,0,,Style inexistant\n";
    let c = parse_ass(ass);
    let p: Vec<Option<CuePos>> = c.iter().map(|x| x.pos).collect();
    assert_eq!(c.len(), 6);
    assert_eq!(p[0], None, "bas centre = defaut : la piste decide");
    assert_eq!(p[1], pos(HAlign::Center, VPos::Top), "etoile et casse ignorees");
    assert_eq!(p[2], pos(HAlign::Left, VPos::Top));
    assert_eq!((p[3], c[3].text.as_str()), (pos(HAlign::Right, VPos::Middle), "Droite, avec virgule"));
    assert_eq!(p[4], pos(HAlign::Center, VPos::Bottom), "la balise l'emporte sur le style");
    assert_eq!(p[5], None);
}

#[test]
fn ssa_legacy_alignment_numbers() {
    let ssa = "[V4 Styles]\nFormat: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, TertiaryColour, BackColour, Bold, Italic, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, AlphaLevel, Encoding\n\
Style: Bas,Arial,56,16777215,255,0,0,0,0,1,3,0,2,60,60,60,0,1\n\
Style: HG,Arial,56,16777215,255,0,0,0,0,1,3,0,5,60,60,60,0,1\n\
Style: HC,Arial,56,16777215,255,0,0,0,0,1,3,0,6,60,60,60,0,1\n\
Style: HD,Arial,56,16777215,255,0,0,0,0,1,3,0,7,60,60,60,0,1\n\
Style: MG,Arial,56,16777215,255,0,0,0,0,1,3,0,9,60,60,60,0,1\n\
Style: MD,Arial,56,16777215,255,0,0,0,0,1,3,0,11,60,60,60,0,1\n\
\n[Events]\nFormat: Marked, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\n\
Dialogue: Marked=0,0:00:01.00,0:00:02.00,Bas,,0,0,0,,a\n\
Dialogue: Marked=0,0:00:03.00,0:00:04.00,HG,,0,0,0,,b\n\
Dialogue: Marked=0,0:00:05.00,0:00:06.00,HC,,0,0,0,,c\n\
Dialogue: Marked=0,0:00:07.00,0:00:08.00,HD,,0,0,0,,d\n\
Dialogue: Marked=0,0:00:09.00,0:00:10.00,MG,,0,0,0,,e\n\
Dialogue: Marked=0,0:00:11.00,0:00:12.00,MD,,0,0,0,,f\n";
    let p: Vec<Option<CuePos>> = parse_ass(ssa).iter().map(|x| x.pos).collect();
    assert_eq!(p, vec![None, pos(HAlign::Left, VPos::Top), pos(HAlign::Center, VPos::Top), pos(HAlign::Right, VPos::Top), pos(HAlign::Left, VPos::Middle), pos(HAlign::Right, VPos::Middle)]);
}

// ------------------------------------------------------------------ encodages
use bdauthor_core::charset::{decode, Charset};

#[test]
fn legacy_charsets() {
    let d = |b: &[u8], cs| decode(b, cs, Charset::Auto);
    assert_eq!(d(&[0xCF, 0xF0, 0xE8, 0xE2, 0xE5, 0xF2], Charset::Cp1251), "Привет");
    assert_eq!(d(&[0x5A, 0x61, 0xBF, 0xF3, 0xB3, 0xE6], Charset::Cp1250), "Zażółć");
    assert_eq!(d(&[0xC3, 0xE5, 0xE9, 0xDC], Charset::Cp1253), "Γειά");
    assert_eq!(d(&[0xF9, 0xEC, 0xE5, 0xED], Charset::Cp1255), "שלום");
    assert_eq!(d(&[0xE3, 0xD1, 0xCD, 0xC8, 0xC7], Charset::Cp1256), "مرحبا");
    assert_eq!(d(&[0xCA, 0xC7, 0xD1, 0xCA, 0xB4, 0xD5], Charset::Cp874), "สวัสดี");
    assert_eq!(d(&[0xC7, 0x61, 0xF0, 0x72, 0xFD], Charset::Cp1254), "Çaðrý".replace('ð', "ğ").replace('ý', "ı"));
    // doubles octets, melanges a de l'ASCII et a des demi-chasses
    assert_eq!(d(&[0x93, 0xFA, 0x96, 0x7B, 0x8C, 0xEA, b'!'], Charset::ShiftJis), "日本語!");
    assert_eq!(d(&[0xB1, 0xB2], Charset::ShiftJis), "ｱｲ");
    assert_eq!(d(&[0xD6, 0xD0, 0xCE, 0xC4], Charset::Gbk), "中文");
    assert_eq!(d(&[0xA4, 0xA4, 0xA4, 0xE5], Charset::Big5), "中文");
    assert_eq!(d(&[0xC7, 0xD1, 0xB1, 0xB9, 0xBE, 0xEE], Charset::Uhc), "한국어");
    // octet de tete sans suite : caractere de remplacement, pas de plantage
    assert_eq!(d(&[b'a', 0x93], Charset::ShiftJis), "a\u{FFFD}");
}

#[test]
fn auto_charset_uses_utf8_then_language() {
    let ru = [0xCF, 0xF0, 0xE8, 0xE2, 0xE5, 0xF2];
    assert_eq!(decode("é".as_bytes(), Charset::Auto, Charset::Cp1251), "é", "UTF-8 reste prioritaire");
    assert_eq!(decode(&ru, Charset::Auto, Charset::for_language("rus")), "Привет");
    assert_ne!(decode(&ru, Charset::Auto, Charset::Auto), "Привет");
    assert_eq!(Charset::for_language("pol"), Charset::Cp1250);
    assert_eq!(Charset::for_language("jpn"), Charset::ShiftJis);
    assert_eq!(Charset::for_language("fra"), Charset::Cp1252);
    for c in Charset::ALL { assert_eq!(Charset::from_name(c.name()), Some(c)); }
    // BOM : UTF-16 reconnu quel que soit le choix
    assert_eq!(decode(&[0xFF, 0xFE, b'a', 0], Charset::Cp1251, Charset::Auto), "a");
}

#[test]
fn charset_via_file_and_json() {
    let dir = std::env::temp_dir().join(format!("bdauthor_cs_{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let f = dir.join("ru.srt");
    let mut b = b"1\n00:00:01,000 --> 00:00:02,000\n".to_vec();
    b.extend([0xCF, 0xF0, 0xE8, 0xE2, 0xE5, 0xF2, b'\n']);
    std::fs::write(&f, &b).unwrap();
    let by_lang = load_cues_enc(&f, None, Charset::Auto, "rus").unwrap();
    assert_eq!(by_lang[0].text, "Привет");
    let forced = load_cues_enc(&f, None, Charset::Cp1251, "fra").unwrap();
    assert_eq!(forced[0].text, "Привет");
    let wrong = load_cues_enc(&f, None, Charset::Auto, "fra").unwrap();
    assert_ne!(wrong[0].text, "Привет");
    let _ = std::fs::remove_dir_all(&dir);
    let mut t = SubTrackCfg::from_file("a.srt", "rus");
    assert!(!t.to_value().to_pretty().contains("encodage"));
    t.charset = Charset::Cp1251;
    t.keep_format = false;
    let back = SubTrackCfg::from_value(&t.to_value()).unwrap();
    assert_eq!((back.charset, back.keep_format), (Charset::Cp1251, false));
    let bad = bdauthor_core::json::parse(r#"{"file":"a.srt","encoding":"klingon"}"#).unwrap();
    assert!(SubTrackCfg::from_value(&bad).is_err());
}

// ------------------------------------------------------------------ entites HTML
#[test]
fn html_entities() {
    assert_eq!(decode_entities("caf&eacute; &amp; cr&egrave;me"), "café & crème");
    assert_eq!(decode_entities("&#233;t&#xE9; &#8364; &#x20AC;"), "été € €");
    assert_eq!(decode_entities("&amp;lt; reste &lt;"), "&lt; reste <");
    assert_eq!(decode_entities("a&nbsp;b&lrm;c&rlm;d"), "a bcd");
    assert_eq!(decode_entities("&hellip; &mdash; &laquo;x&raquo; &oelig;"), "… — «x» œ");
    assert_eq!(decode_entities("AT&T, R&D; &inconnu; &#0; &#xZZ; &"), "AT&T, R&D; &inconnu; &#0; &#xZZ; &");
    // a travers un fichier SRT
    let c = parse_srt("1\n00:00:01,000 --> 00:00:02,000\nCaf&eacute; &#169; 2020 &lt;b&gt;\n");
    assert_eq!(c[0].text, "Café © 2020 <b>", "un < issu d'une entite n'est pas une balise");
}

// ------------------------------------------------------------------ mise en forme
fn runs(t: &str) -> Vec<Vec<(String, bool, bool, bool, Option<[u8; 3]>)>> {
    parse_runs(t).into_iter().map(|l| l.into_iter().map(|r| (r.text, r.italic, r.bold, r.underline, r.color)).collect()).collect()
}

#[test]
fn markup_html_tags() {
    let c = parse_srt("1\n00:00:01,000 --> 00:00:02,000\n<i>Italique</i> <b>gras</b> <u>sous</u> <font color=\"#ff0000\">rouge</font> fin\n");
    let r = runs(&c[0].text);
    assert_eq!(plain_text(&c[0].text), "Italique gras sous rouge fin");
    let find = |w: &str| r[0].iter().find(|x| x.0 == w).cloned().unwrap_or_else(|| panic!("{w} absent : {r:?}"));
    assert_eq!(find("Italique"), ("Italique".into(), true, false, false, None));
    assert_eq!(find("gras"), ("gras".into(), false, true, false, None));
    assert_eq!(find("sous"), ("sous".into(), false, false, true, None));
    assert_eq!(find("rouge"), ("rouge".into(), false, false, false, Some([255, 0, 0])));
    assert!(r[0].iter().any(|x| x.0.contains("fin") && !x.1 && x.4.is_none()));
    // balise ouverte sur une ligne, fermee sur la suivante ; texte sans balise ; couleurs nommees et courtes
    let c = parse_srt("1\n00:00:01,000 --> 00:00:02,000\n<i>Premiere\nseconde</i>\nhors\n\n2\n00:00:03,000 --> 00:00:04,000\n<font color=lime>a</font><font color=\"#0f0\">b</font><c.yellow>c</c><i>non fermee\n");
    let r = runs(&c[0].text);
    assert!(r[0][0].1 && r[1][0].1 && !r[2][0].1, "{r:?}");
    let r = runs(&c[1].text);
    assert_eq!(r[0][0].4, Some([0, 255, 0]));
    assert_eq!(r[0][1].4, Some([0, 255, 0]));
    assert_eq!(r[0][2].4, Some([255, 255, 0]));
    assert!(r[0][3].1);
    assert!(c[1].text.ends_with("\u{1}I"), "mise en forme refermee en fin de replique");
    // les repliques fusionnees ne se contaminent pas
    let m = merge(&[Cue { start_ms: 0, end_ms: 2000, text: c[1].text.clone(), pos: None }, Cue { start_ms: 0, end_ms: 2000, text: "normal".into(), pos: None }]);
    assert!(!runs(&m[0].text)[1][0].1);
}

#[test]
fn markup_ass_overrides() {
    let a = parse_ass("[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\n\
Dialogue: 0,0:00:01.00,0:00:02.00,Default,,0,0,0,,{\\i1\\b1}Gras italique{\\b0} italique{\\r} normal {\\c&H00FFFF&}jaune{\\1c&HFF0000&}bleu{\\c}fin\n\
Dialogue: 0,0:00:03.00,0:00:04.00,Default,,0,0,0,,{\\t(0,100,\\i1)\\fs40\\bord2\\blur1\\an8}Texte{\\iclip(0,0,1,1)}\n");
    let r = runs(&a[0].text);
    let w = |t: &str| r[0].iter().find(|x| x.0.trim() == t || x.0.starts_with(t)).cloned().unwrap_or_else(|| panic!("{t} absent : {r:?}"));
    assert_eq!((w("Gras").1, w("Gras").2), (true, true));
    assert_eq!((w("italique").1, w("italique").2), (true, false), "{r:?}");
    assert_eq!((w("normal").1, w("normal").2, w("normal").4), (false, false, None));
    assert_eq!(w("jaune").4, Some([255, 255, 0]));
    assert_eq!(w("bleu").4, Some([0, 0, 255]), "ASS ecrit BBGGRR");
    assert_eq!(w("fin").4, None);
    // transformation animee, taille, contour : aucun effet de mise en forme
    let r = runs(&a[1].text);
    assert_eq!(r[0].len(), 1);
    assert_eq!((r[0][0].1, r[0][0].2, r[0][0].4), (false, false, None));
    assert_eq!(a[1].pos, Some(CuePos { h: Some(HAlign::Center), v: Some(VPos::Top) }));
}

#[test]
fn strip_format_and_notes_keep_markup() {
    let c = parse_srt("1\n00:00:01,000 --> 00:00:02,000\n<i>[Musique]\nSalut (rires) toi</i>\n");
    assert_eq!(plain_text(&c[0].text), "[Musique]\nSalut (rires) toi");
    let n = strip_notes(&c);
    assert_eq!(plain_text(&n[0].text), "Salut toi");
    assert!(runs(&n[0].text)[0][0].1, "l'italique de la ligne supprimee passe a la suivante : {:?}", n[0].text);
    let p = strip_format(&n);
    assert_eq!(p[0].text, "Salut toi");
    // qualite : les marqueurs ne comptent pas comme des caracteres
    let long = Cue { start_ms: 0, end_ms: 4000, text: format!("\u{1}i{}\u{1}I", "x".repeat(40)), pos: None };
    assert!(check_quality(&[long], &QualityLimits::default()).iter().all(|i| i.kind != IssueKind::LongLine));
}

// ------------------------------------------------------------------ WebVTT : positions
#[test]
fn webvtt_cue_settings() {
    let vtt = std::fs::read_to_string("../exemples_soustitres/test_webvtt.vtt").ok();
    let src = vtt.unwrap_or_else(|| "WEBVTT\n\n00:01.000 --> 00:02.000\nx\n".into());
    let c = parse_file_text("vtt", &src);
    assert!(!c.is_empty());
    let one = |s: &str| parse_file_text("vtt", &format!("WEBVTT\n\n00:01.000 --> 00:02.000 {s}\nx\n")).remove(0).pos;
    let p = |h: Option<HAlign>, v: Option<VPos>| Some(CuePos { h, v });
    assert_eq!(one(""), None);
    assert_eq!(one("line:0"), p(None, Some(VPos::Top)));
    assert_eq!(one("line:10% align:start position:10%"), p(Some(HAlign::Left), Some(VPos::Top)));
    assert_eq!(one("line:10% align:end position:90%"), p(Some(HAlign::Right), Some(VPos::Top)));
    assert_eq!(one("line:50%"), p(None, Some(VPos::Middle)));
    assert_eq!(one("line:-1"), p(None, Some(VPos::Bottom)));
    assert_eq!(one("line:90% align:start position:5%"), p(Some(HAlign::Left), Some(VPos::Bottom)));
    assert_eq!(one("align:start"), p(Some(HAlign::Left), None));
    assert_eq!(one("align:center"), p(Some(HAlign::Center), None));
    assert_eq!(one("size:40% position:50%"), p(Some(HAlign::Center), None));
    assert_eq!(one("line:auto"), None);
    // les coordonnees SubRip (X1:10) ne sont pas des reglages WebVTT
    assert_eq!(parse_srt("1\n00:00:01,000 --> 00:00:02,000  X1:10 X2:20\nx\n")[0].pos, None);
    // seul le sens demande est impose : la verticale de la piste reste
    let st = SubStyle { pos: VPos::Top, ..SubStyle::default() };
    let w = bdauthor_core::pgs::with_pos(&st, p(Some(HAlign::Right), None));
    assert_eq!((w.pos, w.halign), (VPos::Top, HAlign::Right));
}
