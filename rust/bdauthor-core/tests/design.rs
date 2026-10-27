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
    p.items.push(Item { x: 50, y: 60, w: 300, h: 200, kind: Kind::Image { path: "logo.png".into() } });
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
    f.items.push(Item { x: 100, y: 200, w: 400, h: 300, kind: Kind::Slot { thumb_h: 240, assign: Some(2) } });
    f.items.push(Item { x: 600, y: 200, w: 400, h: 300, kind: Kind::Slot { thumb_h: 240, assign: Some(0) } });
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
    if let Kind::Slot { assign, .. } = &mut g.items[1].kind { *assign = None; }
    assert!(g.validate_free(1, 2, 3).iter().any(|p| p.error));
    // echange de videos 0 et 2 : les affectations suivent
    let mut x = ds.clone();
    x.swap_videos(0, 2);
    let a: Vec<Option<usize>> = x.free[0].items.iter().filter_map(|i| if let Kind::Slot { assign, .. } = i.kind { Some(assign) } else { None }).collect();
    assert_eq!(a, vec![Some(0), Some(2)]);
    // retrait de la video 0 : sa vignette disparait, la video 2 devient 1
    let mut y = ds.clone();
    y.remove_video(0);
    let a: Vec<Option<usize>> = y.free[0].items.iter().filter_map(|i| if let Kind::Slot { assign, .. } = i.kind { Some(assign) } else { None }).collect();
    assert_eq!(a, vec![Some(1)]);
}
