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
    assert!(d.validate(4, MenuType::Main, 0, 2).iter().any(|p| p.error)); // il en faut 4
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
    p.items.push(Item { x: 50, y: 60, w: 300, h: 200, kind: Kind::Image { path: "logo.png".into(), link: None } });
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
    f.items.push(Item { x: 100, y: 200, w: 400, h: 300, kind: Kind::Slot { thumb_h: 240, assign: None, video: Some(2), link: None } });
    f.items.push(Item { x: 600, y: 200, w: 400, h: 300, kind: Kind::Slot { thumb_h: 240, assign: None, video: Some(0), link: None } });
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
    if let Kind::Slot { video, .. } = &mut g.items[1].kind { *video = None; }
    assert!(g.validate_free(1, 2, 3).iter().any(|p| p.error));
    // echange de videos 0 et 2 : les affectations suivent
    let mut x = ds.clone();
    x.swap_videos(0, 2);
    let a: Vec<Option<usize>> = x.free[0].items.iter().filter_map(|i| if let Kind::Slot { video, .. } = i.kind { Some(video) } else { None }).collect();
    assert_eq!(a, vec![Some(0), Some(2)]);
    // retrait de la video 0 : sa vignette disparait, la video 2 devient 1
    let mut y = ds.clone();
    y.remove_video(0);
    let a: Vec<Option<usize>> = y.free[0].items.iter().filter_map(|i| if let Kind::Slot { video, .. } = i.kind { Some(video) } else { None }).collect();
    assert_eq!(a, vec![Some(1)]);
}

#[test]
fn goto_buttons_json_remap_and_check() {
    let mut ds = Designs::default();
    let mut p = PageDesign::default_main(2, 0, 2).unwrap();
    p.items.push(Item::nav(NavAction::Goto, 100, 900, 300, 80)); // cible par defaut : page 1 du menu principal
    p.items.push(Item { x: 500, y: 900, w: 300, h: 80, kind: Kind::Nav { action: NavAction::Goto, text: "Chapitres".into(), target: Some(Target::Chapters(2, 1)) } });
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
    z.main.get_mut(&0).unwrap().items.push(Item { x: 900, y: 900, w: 200, h: 80, kind: Kind::Nav { action: NavAction::Goto, text: "p3".into(), target: Some(Target::Main(2)) } });
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
    p.items.push(Item { x: 500, y: 900, w: 300, h: 80, kind: Kind::Image { path: "logo.png".into(), link: Some(Target::Main(1)) } });
    p.items.push(Item { x: 900, y: 900, w: 300, h: 80, kind: Kind::Nav { action: NavAction::Goto, text: "b".into(), target: Some(Target::Video(1)) } });
    p.items.push(Item { x: 1300, y: 900, w: 300, h: 80, kind: Kind::Image { path: "deco.png".into(), link: None } });
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
    o.items.push(Item { x: 0, y: 0, w: 400, h: 100, kind: Kind::Image { path: "a.png".into(), link: Some(Target::Video(0)) } });
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
    assert!(matches!(d.free[0].items[0].kind, Kind::Slot { assign: None, video: Some(1), .. }));
    // page automatique : 2 elements + une vignette de la video 3 qui ouvre la page 2
    let mut p = PageDesign::default_main(2, 0, 2).unwrap();
    let before = p.slot_count();
    p.items.push(Item { x: 100, y: 850, w: 250, h: 200, kind: Kind::Slot { thumb_h: 150, assign: Some(0), video: Some(2), link: Some(Target::Main(1)) } });
    assert_eq!(p.slot_count(), before, "une vignette de video choisie ne compte pas parmi les elements");
    assert_eq!(p.all_slot_count(), before + 1);
    assert!(p.validate(2, MenuType::Main, 0, 2).iter().all(|x| !x.error), "{:?}", p.validate(2, MenuType::Main, 0, 2));
    assert_eq!(p.slot_assignment(2), vec![0, 1]);
    let mut ds = Designs::default();
    ds.main.insert(0, p);
    let text = ds.to_value().to_pretty();
    let back = Designs::from_value(&parse(&text).unwrap()).unwrap();
    assert!(matches!(back.main[&0].items.last().unwrap().kind, Kind::Slot { assign: None, video: Some(2), link: Some(Target::Main(1)), .. }));
    let fixed = |d: &Designs| d.main[&0].items.last().map(|i| (i.fixed_video(), i.link()));
    let mut x = back.clone();
    x.swap_videos(2, 0);
    assert_eq!(fixed(&x), Some((Some(0), Some(Target::Main(1)))));
    let mut y = back.clone();
    y.remove_video(2);
    assert!(y.main[&0].items.iter().all(|i| i.fixed_video().is_none()), "la vignette de la video retiree disparait");
    let mut z = back.clone();
    z.page_removed(1);
    assert_eq!(fixed(&z), Some((Some(2), None)));
    let ok = |t: Target| matches!(t, Target::Main(0..=1) | Target::Video(0..=1));
    assert!(back.main[&0].check_targets(&ok).iter().any(|p| p.error && p.msg.contains("vidéo 3")));
}
