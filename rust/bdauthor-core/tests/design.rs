use bdauthor_core::design::*;
use bdauthor_core::json::parse;

#[test]
fn default_grids_match_layout() {
    let d = PageDesign::default_main(5, 0, 1).unwrap();
    assert_eq!(d.slot_count(), 5);
    assert_eq!(d.items.len(), 5);
    let d = PageDesign::default_main(8, 1, 3).unwrap();
    let navs: Vec<_> = d.items.iter().filter_map(|i| if let Kind::Nav { action, .. } = i.kind { Some(action) } else { None }).collect();
    assert_eq!(navs, vec![NavAction::Prev, NavAction::Next]);
    let c = PageDesign::default_chapter(4, 0, 1).unwrap();
    assert_eq!(c.items.iter().filter(|i| matches!(i.kind, Kind::Nav { .. })).count(), 2);
    let c = PageDesign::default_chapter(4, 0, 2).unwrap();
    assert_eq!(c.items.iter().filter(|i| matches!(i.kind, Kind::Nav { .. })).count(), 3); // pas de « Précédent » en page 1
    assert!(PageDesign::default_main(13, 0, 1).is_err());
}

#[test]
fn validation() {
    let mut d = PageDesign::starting_point(MenuType::Main, 3, 0, 2, "Titre").unwrap();
    assert!(d.validate(3, MenuType::Main, 0, 2).iter().all(|p| !p.error), "{:?}", d.validate(3, MenuType::Main, 0, 2));
    assert!(d.validate(4, MenuType::Main, 0, 2).iter().any(|p| !p.error && p.msg.contains("sans vignette"))); // 4e element non montre
    assert!(d.validate(2, MenuType::Main, 0, 2).iter().any(|p| p.error && p.msg.contains("sans élément"))); // une vignette de trop
    d.items.push(Item::nav(NavAction::Back, 10, 10, 200, 80)); // sans objet dans le menu principal
    assert!(d.validate(3, MenuType::Main, 0, 2).iter().any(|p| p.msg.contains("n'apparaîtra pas")));
    d.items[1].x = -5;
    assert!(d.validate(3, MenuType::Main, 0, 2).iter().any(|p| p.error && p.msg.contains("dépasse")));
    let mut d2 = PageDesign::default_main(2, 0, 1).unwrap();
    d2.items[1].x = d2.items[0].x; d2.items[1].y = d2.items[0].y;
    assert!(d2.validate(2, MenuType::Main, 0, 1).iter().any(|p| p.msg.contains("chevauchent")));
}

#[test]
fn json_roundtrip_and_remap() {
    let mut ds = Designs::default();
    let mut p = PageDesign::starting_point(MenuType::Chapters, 3, 0, 1, "Chapitres").unwrap();
    p.items.push(Item { x: 50, y: 60, w: 300, h: 200, kind: Kind::Image { path: "logo.png".into(), link: None }, look: Default::default(), layer: 0, sub: Default::default() });
    if let Kind::Text { color, align, font, .. } = &mut p.items[0].kind { *color = [10, 200, 30]; *align = Align::Left; *font = "georgia.ttf".into(); }
    ds.main.insert(1, PageDesign::default_main(4, 1, 3).unwrap());
    ds.chapters.insert((2, 0), p.clone());
    ds.chapters.insert((0, 1), p.clone());
    let text = ds.to_value().to_pretty();
    let back = Designs::from_value(&parse(&text).unwrap()).unwrap();
    assert_eq!(back, ds);
    assert!(text.contains("georgia.ttf"), "police enregistree");
    assert!(text.contains("\"3\""), "videos numerotees a partir de 1");
    let mut x = ds.clone();
    x.swap_videos(0, 2);
    assert!(x.chapters.contains_key(&(0, 0)) && x.chapters.contains_key(&(2, 1)));
    let mut y = ds.clone();
    y.remove_video(0);
    assert_eq!(y.chapters.keys().copied().collect::<Vec<_>>(), vec![(1, 0)]);
    assert!(PageDesign::from_value(&parse(r##"{"elements":[{"type":"zzz","x":1,"y":1,"w":1,"h":1}]}"##).unwrap()).is_err());
}

#[test]
fn free_pages_json_swap_remove_validate() {
    let mut ds = Designs::default();
    let mut f = PageDesign::blank_free(1);
    f.items.push(Item { x: 100, y: 200, w: 400, h: 300, kind: Kind::Slot { thumb_h: 240, assign: None, video: Some(Some(2)), link: None, start: None, label: String::new(), thumb_at: None, ret: Default::default() }, look: Default::default(), layer: 0, sub: Default::default() });
    f.items.push(Item { x: 600, y: 200, w: 400, h: 300, kind: Kind::Slot { thumb_h: 240, assign: None, video: Some(Some(0)), link: None, start: None, label: String::new(), thumb_at: None, ret: Default::default() }, look: Default::default(), layer: 0, sub: Default::default() });
    ds.free.push(f);
    let text = ds.to_value().to_pretty();
    assert!(text.contains("\"libres\""));
    let back = Designs::from_value(&parse(&text).unwrap()).unwrap();
    assert_eq!(back, ds);
    // validation : videos 3 -> ok ; 2 -> la video 3 n'existe pas
    assert!(ds.free[0].validate_free(1, 2, 3).iter().all(|p| !p.error));
    assert!(ds.free[0].validate_free(1, 2, 2).iter().any(|p| p.error && p.msg.contains("n'existe pas")));
    // vignette sans video
    let mut g = ds.free[0].clone();
    if let Kind::Slot { video, .. } = &mut g.items[1].kind { *video = None; } // vignette d'element sur une page libre : erreur
    assert!(g.validate_free(1, 2, 3).iter().any(|p| p.error));
    // echange de videos 0 et 2 : les affectations suivent
    let mut x = ds.clone();
    x.swap_videos(0, 2);
    let a: Vec<Option<usize>> = x.free[0].items.iter().filter_map(|i| if let Kind::Slot { .. } = i.kind { Some(i.fixed_video()) } else { None }).collect();
    assert_eq!(a, vec![Some(0), Some(2)]);
    // retrait de la video 0 : sa vignette reste en place sans video, la video 2 devient 1
    let mut y = ds.clone();
    y.remove_video(0);
    let a: Vec<Option<usize>> = y.free[0].items.iter().filter_map(|i| if let Kind::Slot { .. } = i.kind { Some(i.fixed_video()) } else { None }).collect();
    assert_eq!(a, vec![Some(1), None]);
    assert!(y.free[0].items.iter().filter(|i| i.is_video_slot()).count() == 2);
    let pr = y.free[0].validate_free(1, 2, 2);
    assert!(pr.iter().all(|p| !p.error) && pr.iter().any(|p| p.msg.contains("sans vidéo")), "{pr:?}");
    let back = Designs::from_value(&parse(&y.to_value().to_pretty()).unwrap()).unwrap();
    assert_eq!(back, y, "une vignette sans video survit a l'enregistrement");
}

#[test]
fn goto_buttons_json_remap_and_check() {
    let mut ds = Designs::default();
    let mut p = PageDesign::default_main(2, 0, 2).unwrap();
    p.items.push(Item::nav(NavAction::Goto, 100, 900, 300, 80)); // cible par defaut : page 1 du menu principal
    p.items.push(Item { x: 500, y: 900, w: 300, h: 80, kind: Kind::Nav { action: NavAction::Goto, text: "Chapitres".into(), target: Some(Target::Chapters(2, 1)) }, look: Default::default(), layer: 0, sub: Default::default() });
    ds.main.insert(0, p);
    let text = ds.to_value().to_pretty();
    assert!(text.contains("aller_a") && text.contains("\"cible\""));
    let back = Designs::from_value(&parse(&text).unwrap()).unwrap();
    assert_eq!(back, ds);
    let targets = |d: &Designs| -> Vec<Option<Target>> { d.main[&0].items.iter().filter_map(|i| if let Kind::Nav { action: NavAction::Goto, target, .. } = i.kind { Some(target) } else { None }).collect() };
    assert_eq!(targets(&ds), vec![Some(Target::Main(0)), Some(Target::Chapters(2, 1))]);
    // echange des videos 0 et 2 : la cible de chapitres suit ; retrait de la video 2 : la cible disparait
    let mut x = ds.clone();
    x.swap_videos(0, 2);
    assert_eq!(targets(&x)[1], Some(Target::Chapters(0, 1)));
    let mut y = ds.clone();
    y.remove_video(2);
    assert_eq!(targets(&y)[1], None);
    assert!(y.main[&0].validate(2, MenuType::Main, 0, 2).iter().any(|p| p.error && p.msg.contains("cible")));
    // suppression de la page 1 : le bouton qui y menait perd sa cible, les pages suivantes sont decalees
    let mut z = ds.clone();
    z.main.get_mut(&0).unwrap().items.push(Item { x: 900, y: 900, w: 200, h: 80, kind: Kind::Nav { action: NavAction::Goto, text: "p3".into(), target: Some(Target::Main(2)) }, look: Default::default(), layer: 0, sub: Default::default() });
    z.page_removed(0);
    assert_eq!(targets(&z), vec![None, Some(Target::Chapters(2, 1)), Some(Target::Main(1))]);
    // verification des cibles
    let ok = |t: Target| matches!(t, Target::Main(0..=1));
    assert!(ds.main[&0].check_targets(&ok).iter().any(|p| p.error), "chapitres inexistants");
}

#[test]
fn clickable_text_image_and_video_targets() {
    let mut ds = Designs::default();
    let mut p = PageDesign::default_main(2, 0, 1).unwrap();
    let mut t = Item::text("Lire", 100, 900, 300, 80, 50);
    if let Kind::Text { link, .. } = &mut t.kind { *link = Some(Target::Video(2)); }
    p.items.push(t);
    p.items.push(Item { x: 500, y: 900, w: 300, h: 80, kind: Kind::Image { path: "logo.png".into(), link: Some(Target::Main(1)) }, look: Default::default(), layer: 0, sub: Default::default() });
    p.items.push(Item { x: 900, y: 900, w: 300, h: 80, kind: Kind::Nav { action: NavAction::Goto, text: "b".into(), target: Some(Target::Video(1)) }, look: Default::default(), layer: 0, sub: Default::default() });
    p.items.push(Item { x: 1300, y: 900, w: 300, h: 80, kind: Kind::Image { path: "deco.png".into(), link: None }, look: Default::default(), layer: 0, sub: Default::default() });
    ds.main.insert(0, p);
    let text = ds.to_value().to_pretty();
    assert!(text.contains("\"lire\": \"video\""), "{text}");
    let back = Designs::from_value(&parse(&text).unwrap()).unwrap();
    assert_eq!(back, ds);
    let items = &ds.main[&0].items;
    let n = items.len();
    assert!(items[n - 4].is_button() && items[n - 3].is_button() && !items[n - 1].is_button());
    let links = |d: &Designs| -> Vec<Option<Target>> { d.main[&0].items.iter().rev().take(4).rev().map(|i| match i.kind { Kind::Nav { target, .. } => target, _ => i.link() }).collect() };
    assert_eq!(links(&ds), vec![Some(Target::Video(2)), Some(Target::Main(1)), Some(Target::Video(1)), None]);
    let mut x = ds.clone();
    x.swap_videos(1, 2);
    assert_eq!(links(&x), vec![Some(Target::Video(1)), Some(Target::Main(1)), Some(Target::Video(2)), None]);
    let mut y = ds.clone();
    y.remove_video(1);
    assert_eq!(links(&y), vec![Some(Target::Video(1)), Some(Target::Main(1)), None, None]);
    let mut z = ds.clone();
    z.page_removed(1);
    assert_eq!(links(&z)[1], None);
    // chevauchement : un texte cliquable compte comme un bouton
    let mut o = PageDesign::default();
    o.items.push(Item { x: 0, y: 0, w: 400, h: 100, kind: Kind::Image { path: "a.png".into(), link: Some(Target::Video(0)) }, look: Default::default(), layer: 0, sub: Default::default() });
    o.items.push(Item::nav(NavAction::Goto, 100, 50, 300, 80));
    assert!(o.validate(0, MenuType::Main, 0, 1).iter().any(|p| p.msg.contains("chevauchent")));
    // destinations inexistantes
    let ok = |t: Target| matches!(t, Target::Main(0) | Target::Video(0..=1));
    let probs = ds.main[&0].check_targets(&ok);
    assert_eq!(probs.iter().filter(|p| p.error).count(), 2, "{probs:?}");
    // cible illisible
    assert!(PageDesign::from_value(&parse(r#"{"elements":[{"type":"texte","texte":"x","taille":20,"x":0,"y":0,"w":50,"h":50,"cible":{"menu":"???"}}]}"#).unwrap()).is_err());
}

#[test]
fn fixed_video_slots_with_destination() {
    // anciens fichiers : « element » d'une page libre devient la video
    let old = parse(r#"{"principal":{},"chapitres":{},"libres":[{"elements":[{"type":"vignette","vignette_h":200,"x":0,"y":0,"w":300,"h":250,"element":2}]}]}"#).unwrap();
    let d = Designs::from_value(&old).unwrap();
    assert!(matches!(d.free[0].items[0].kind, Kind::Slot { assign: None, video: Some(Some(1)), .. }));
    // page automatique : 2 elements + une vignette de la video 3 qui ouvre la page 2
    let mut p = PageDesign::default_main(2, 0, 2).unwrap();
    let before = p.slot_count();
    p.items.push(Item { x: 100, y: 850, w: 250, h: 200, kind: Kind::Slot { thumb_h: 150, assign: Some(0), video: Some(Some(2)), link: Some(Target::Main(1)), start: None, label: String::new(), thumb_at: None, ret: Default::default() }, look: Default::default(), layer: 0, sub: Default::default() });
    assert_eq!(p.slot_count(), before, "une vignette de video choisie ne compte pas parmi les elements");
    assert_eq!(p.all_slot_count(), before + 1);
    assert!(p.validate(2, MenuType::Main, 0, 2).iter().all(|x| !x.error), "{:?}", p.validate(2, MenuType::Main, 0, 2));
    assert_eq!(p.slot_assignment(2), vec![0, 1]);
    let mut ds = Designs::default();
    ds.main.insert(0, p);
    let text = ds.to_value().to_pretty();
    let back = Designs::from_value(&parse(&text).unwrap()).unwrap();
    assert!(matches!(back.main[&0].items.last().unwrap().kind, Kind::Slot { assign: None, video: Some(Some(2)), link: Some(Target::Main(1)), .. }));
    let fixed = |d: &Designs| d.main[&0].items.last().map(|i| (i.fixed_video(), i.link()));
    let mut x = back.clone();
    x.swap_videos(2, 0);
    assert_eq!(fixed(&x), Some((Some(0), Some(Target::Main(1)))));
    let mut y = back.clone();
    y.remove_video(2);
    assert!(y.main[&0].items.last().map_or(false, |i| i.is_video_slot() && i.fixed_video().is_none()), "la vignette de la video retiree reste, sans video");
    let mut z = back.clone();
    z.page_removed(1);
    assert_eq!(fixed(&z), Some((Some(2), None)));
    let ok = |t: Target| matches!(t, Target::Main(0..=1) | Target::Video(0..=1));
    assert!(back.main[&0].check_targets(&ok).iter().any(|p| p.error && p.msg.contains("vidéo 3")));
}

#[test]
fn button_style_json_scale_weight() {
    use bdauthor_core::style::*;
    let mut st = ButtonStyle::default();
    st.glow = 20; st.zoom = 10; st.activated = true; st.sel_border = [1, 2, 3]; st.underline = true; st.dim = 30;
    let back = ButtonStyle::from_value(&parse(&st.to_value().to_pretty()).unwrap()).unwrap();
    assert_eq!(back, st);
    // valeurs hors plage bornees, champs absents = defaut
    let b = ButtonStyle::from_value(&parse(r#"{"halo": 999, "agrandir": -4}"#).unwrap()).unwrap();
    assert_eq!((b.glow, b.zoom, b.sel_bg), (60, 0, ButtonStyle::default().sel_bg));
    assert!(ButtonStyle::from_value(&parse(r##"{"fond": "#12"}"##).unwrap()).is_err());
    // echelle 720 : 20 px -> 13 px ; une epaisseur de 1 reste 1
    let k = st.scaled(1280.0 / 1920.0);
    assert_eq!((k.glow, k.normal_border_w), (13, 1));
    assert_eq!(st.margins(400, 200), (40, 30)); // halo 20 + 5 % de 400 / de 200
    assert_eq!(st.state_count(), 3);
    // style du projet et style/images propres a un element : aller-retour JSON
    let mut ds = Designs::default();
    ds.style = st.clone();
    let mut p = PageDesign::default_main(2, 0, 1).unwrap();
    p.items[0].look.style = Some(ButtonStyle { glow: 0, ..ButtonStyle::default() });
    p.items.push(Item::nav(NavAction::Goto, 100, 900, 300, 80));
    p.items.last_mut().unwrap().look.skin = Skin { normal: "b.png".into(), selected: "".into(), activated: "c.svg".into() };
    ds.main.insert(0, p.clone());
    let text = ds.to_value().to_pretty();
    assert!(text.contains("style_boutons") && text.contains("images_etats"));
    assert_eq!(Designs::from_value(&parse(&text).unwrap()).unwrap(), ds);
    assert!(!Designs { style: st.clone(), ..Default::default() }.is_empty(), "un style seul doit etre enregistre");
    // poids : 2 vignettes (style propre sans halo, l'autre avec) + « Aller a » (images : 3 etats)
    let w = p.weight(2, &st, MenuType::Main, 0, 1, (1920, 1080));
    assert_eq!(w.buttons, 3);
    let plain = p.weight(2, &ButtonStyle::default(), MenuType::Main, 0, 1, (1920, 1080));
    assert!(w.bytes > plain.bytes && w.seconds > plain.seconds);
    let (a, b2) = (&p.items[0], &p.items[1]);
    let sz = |it: &Item, s: &ButtonStyle, n: u64| { let (mx, my) = s.margins(it.w, it.h); (it.w + 2 * mx) as u64 * (it.h + 2 * my) as u64 * n };
    let own = ButtonStyle { glow: 0, ..ButtonStyle::default() };
    assert_eq!(plain.bytes, sz(a, &own, 2) + sz(b2, &ButtonStyle::default(), 2) + 300 * 80 * 3);
    assert!(DECODED_LIMIT > w.bytes);
}

#[test]
fn home_page_json_and_remap() {
    let mut ds = Designs::default();
    assert!(ds.is_empty());
    ds.home = Some(Target::Chapters(2, 0));
    assert!(!ds.is_empty());
    let back = Designs::from_value(&parse(&ds.to_value().to_pretty()).unwrap()).unwrap();
    assert_eq!(back.home, Some(Target::Chapters(2, 0)));
    let mut a = ds.clone();
    a.swap_videos(2, 0);
    assert_eq!(a.home, Some(Target::Chapters(0, 0)));
    let mut b = ds.clone();
    b.remove_video(1);
    assert_eq!(b.home, Some(Target::Chapters(1, 0)));
    b.remove_video(1);
    assert_eq!(b.home, None);
    let mut c = Designs { home: Some(Target::Main(3)), ..Default::default() };
    c.page_removed(1);
    assert_eq!(c.home, Some(Target::Main(2)));
    c.page_removed(2);
    assert_eq!(c.home, None);
    // « Tout lire » existe aussi dans le menu principal
    assert!(nav_applicable(NavAction::PlayAll, MenuType::Main, 0, 1));
}

#[test]
fn page_background_json() {
    let mut p = PageDesign::default_main(1, 0, 1).unwrap();
    assert!(!p.to_value().to_pretty().contains("fond_page"));
    p.bg = PageBg { visual: PageVisual::Video("fond.mp4".into(), true), music: PageMusic::Silent };
    let t = p.to_value().to_pretty();
    assert!(t.contains("fond_page") && t.contains("son_video"));
    assert_eq!(PageDesign::from_value(&parse(&t).unwrap()).unwrap(), p);
    p.bg = PageBg { visual: PageVisual::Image("a.png".into()), music: PageMusic::File("m.mp3".into()) };
    assert_eq!(PageDesign::from_value(&parse(&p.to_value().to_pretty()).unwrap()).unwrap(), p);
    p.bg = PageBg { visual: PageVisual::Project, music: PageMusic::File("m.mp3".into()) };
    assert_eq!(PageDesign::from_value(&parse(&p.to_value().to_pretty()).unwrap()).unwrap(), p);
}
