//! Halo anime : style, images de l'animation, ecriture dans le menu (objets consecutifs, boucle, cadence).
mod common;
use bdauthor_core::menu::{anim_rate_code, build_menu, MenuButton, MenuOptions};
use bdauthor_core::palette::RgbaImage;
use bdauthor_core::style::{ButtonStyle, PULSE_FRAMES};
use std::sync::Arc;

fn img(w: u32, h: u32, c: [u8; 4]) -> Arc<RgbaImage> {
    Arc::new(RgbaImage { width: w, height: h, data: c.iter().copied().cycle().take((w * h * 4) as usize).collect() })
}

#[test]
fn pulse_style_and_json() {
    common::fr();
    let st = ButtonStyle { glow: 20, glow_pulse: true, ..ButtonStyle::default() };
    assert_eq!(st.pulse_frames(), PULSE_FRAMES);
    assert_eq!(st.state_count(), 2 + PULSE_FRAMES as u64 - 1);
    // image 0 = halo complet ; le plus petit a mi-parcours (35 %) ; retour symetrique
    let g: Vec<i32> = (0..PULSE_FRAMES).map(|i| st.pulse_style(i).glow).collect();
    assert_eq!(g[0], 20);
    assert_eq!(g[PULSE_FRAMES / 2], 7);
    assert_eq!(g[1], g[PULSE_FRAMES - 1]);
    // sans halo : pas d'animation
    assert_eq!(ButtonStyle { glow: 0, glow_pulse: true, ..ButtonStyle::default() }.pulse_frames(), 1);
    let back = ButtonStyle::from_value(&st.to_value()).unwrap();
    assert!(back.glow_pulse);
    assert!(!ButtonStyle::from_value(&ButtonStyle::default().to_value()).unwrap().glow_pulse);
}

#[test]
fn animated_selected_state_in_menu() {
    common::fr();
    let (n, s, a) = (img(40, 20, [10, 10, 10, 255]), img(40, 20, [200, 30, 30, 255]), img(40, 20, [30, 200, 30, 255]));
    let mut b0 = MenuButton::new("", 1, 100, 100, vec![n.clone(), s.clone(), a.clone()]);
    b0.sel_frames = (1..4).map(|k| img(40, 20, [200, 30 + 40 * k, 30, 255])).collect();
    let b1 = MenuButton::new("", 2, 300, 100, vec![n.clone(), s.clone(), a.clone()]);
    let o = MenuOptions { frame_rate_code: 3, ..Default::default() };
    let m = build_menu(&[b0, b1], &o).unwrap();
    let page = &m.ics.pages[0];
    assert_eq!(page.frame_rate_code, anim_rate_code(3, 8.0));
    assert_eq!(anim_rate_code(3, 8.0), 2); // 25 i/s : une image sur 3 (8,3 i/s)
    assert_eq!(anim_rate_code(3, 25.0), 0);
    assert_eq!(anim_rate_code(1, 4.0), 5);
    let btn = |i: usize| &page.bogs[i].buttons[0];
    // bouton anime : 4 objets consecutifs, en boucle ; le bouton fixe partage toujours ses images
    let sel = btn(0).selected;
    assert_eq!((sel.2 - sel.1, sel.3), (3, 0x80));
    assert_eq!(btn(1).selected.1, btn(1).selected.2);
    assert_eq!(btn(1).selected.3, 0);
    assert!(m.ods.iter().any(|o| o.object_id == sel.2));
    // sans animation : pas de cadence
    let plain = build_menu(&[MenuButton::new("", 1, 0, 0, vec![n, s, a])], &o).unwrap();
    assert_eq!(plain.ics.pages[0].frame_rate_code, 0);
    // relecture : le format ecrit se relit a l'identique
    let packed = m.ics.pack();
    let re = bdauthor_core::ig::Ics::parse(&packed).unwrap();
    assert_eq!(re.pages[0].bogs[0].buttons[0].selected, sel);
}

#[test]
fn custom_animation_json_and_weight() {
    common::fr();
    use bdauthor_core::style::Skin;
    let sk = Skin { normal: "n.png".into(), sel_frames: vec!["a1.png".into(), "a2.png".into()], sel_once: true, ..Default::default() };
    assert_eq!(Skin::from_value(&sk.to_value()), sk);
    assert_eq!(sk.anim_frames(), 2);
    // sans image normale, l'animation n'est pas utilisee
    assert_eq!(Skin { normal: String::new(), ..sk.clone() }.anim_frames(), 0);
    // cadence de page : JSON
    let t = r#"{"videos":[{"fichier":"a"}],"mise_en_page":{"pages":[{"nom":"P","animation_ips":12,"elements":[
        {"type":"navigation","action":"aller_a","texte":"x","cible":{"page":1},"x":0,"y":0,"w":200,"h":80,
         "images_etats":{"normal":"n.png","animation_selection":["a1.png","a2.png","a3.png"]}}]}]}}"#;
    let m = bdauthor_core::model::ProjectModel::from_json(t).unwrap();
    let pg = &m.designs.free[0];
    assert_eq!(pg.anim_fps, 12);
    assert!(m.to_json().contains("\"animation_ips\": 12"));
    assert_eq!(pg.items[0].look.skin.sel_frames.len(), 3);
    // poids : image normale + 3 images d'animation (pas d'etat active distinct)
    let w = pg.weight(&m.designs.style, 0, 1, (1920, 1080));
    assert_eq!(w.bytes, 200 * 80 * 4);
    // validation : images introuvables signalees
    let probs = m.validate(std::path::Path::new("/nonexistent"));
    assert!(probs.iter().any(|p| p.contains("Animation d'un élément : image introuvable (a2.png)")), "{probs:?}");
}

#[test]
fn selected_images_one_or_many() {
    common::fr();
    use bdauthor_core::style::Skin;
    let mut sk = Skin { normal: "n.png".into(), ..Default::default() };
    sk.set_selected(vec!["s.png".into()]);
    assert_eq!((sk.selected.as_str(), sk.sel_frames.len(), sk.anim_frames()), ("s.png", 0, 0));
    sk.set_selected(vec!["a.png".into(), "b.png".into()]);
    assert_eq!((sk.selected.as_str(), sk.anim_frames()), ("", 2));
    assert_eq!(sk.selected_files(), ["a.png", "b.png"]);
    sk.sel_once = true;
    let back = Skin::from_value(&sk.to_value());
    assert_eq!(back, sk);
    sk.set_selected(vec![]);
    assert!(sk.selected_files().is_empty() && !sk.sel_once);
    // anciens projets : image « selectionne » + animation -> l'animation gagne ; animation d'une seule image -> image fixe
    let v = bdauthor_core::json::parse(r#"{"normal":"n.png","selectionne":"s.png","animation_selection":["a.png","b.png"],"animation_une_fois":true}"#).unwrap();
    let old = Skin::from_value(&v);
    assert_eq!((old.selected.as_str(), old.sel_frames.len(), old.sel_once), ("", 2, true));
    let v = bdauthor_core::json::parse(r#"{"normal":"n.png","animation_selection":["a.png"]}"#).unwrap();
    let one = Skin::from_value(&v);
    assert_eq!((one.selected.as_str(), one.sel_frames.len()), ("a.png", 0));
}

#[test]
fn animated_normal_state() {
    common::fr();
    use bdauthor_core::style::Skin;
    // modele : une image = fixe, plusieurs = animation ; relecture
    let mut sk = Skin::default();
    sk.set_normal(vec!["n1.png".into(), "n2.png".into(), "n3.png".into()]);
    assert!(sk.has_normal() && sk.normal.is_empty());
    assert_eq!((sk.normal_file(), sk.normal_anim_frames()), ("n1.png", 3));
    sk.normal_once = true;
    sk.set_selected(vec!["a.png".into(), "b.png".into()]);
    assert_eq!(sk.anim_frames(), 2); // l'image normale animee suffit pour animer la selection
    assert_eq!(Skin::from_value(&sk.to_value()), sk);
    sk.set_normal(vec!["n.png".into()]);
    assert_eq!((sk.normal.as_str(), sk.normal_anim_frames(), sk.normal_once), ("n.png", 0, false));
    // menu : objets consecutifs pour l'etat normal, en boucle ou une fois
    let (n, s, a) = (img(40, 20, [10, 10, 10, 255]), img(40, 20, [200, 30, 30, 255]), img(40, 20, [30, 200, 30, 255]));
    let mut b0 = MenuButton::new("", 1, 100, 100, vec![n.clone(), s.clone(), a.clone()]);
    b0.normal_frames = vec![img(40, 20, [20, 20, 20, 255]), img(40, 20, [30, 30, 30, 255])];
    let mut b1 = MenuButton::new("", 2, 300, 100, vec![n.clone(), s.clone(), a.clone()]);
    b1.normal_frames = vec![img(40, 20, [40, 40, 40, 255])];
    b1.normal_once = true;
    let o = MenuOptions { frame_rate_code: 3, ..Default::default() };
    let m = build_menu(&[b0, b1], &o).unwrap();
    let page = &m.ics.pages[0];
    let (n0, n1) = (page.bogs[0].buttons[0].normal, page.bogs[1].buttons[0].normal);
    assert_eq!((n0.1 - n0.0, n0.2), (2, 0x80));
    assert_eq!((n1.1 - n1.0, n1.2), (1, 0));
    assert_eq!(page.frame_rate_code, anim_rate_code(3, 8.0));
    // l'etat selectionne (fixe) reste partage par les deux boutons
    assert_eq!(page.bogs[0].buttons[0].selected.1, page.bogs[1].buttons[0].selected.1);
}
