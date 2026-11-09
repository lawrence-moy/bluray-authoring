mod common;
use bdauthor_core::design::*;
use bdauthor_core::json::parse;

/// Vignette de la video `v` en (x, y).
fn slot(x: i32, y: i32, v: Option<usize>) -> Item {
    Item { x, y, w: 400, h: 300, kind: Kind::Slot { thumb_h: 240, video: v, link: None, start: None, chapter: None, label: String::new(), thumb_at: None, ret: Default::default() }, look: Default::default(), layer: 0, sub: Default::default(), id: 0, nav: Default::default(), name: String::new() }
}

/// Page : titre, `n` vignettes des videos 0..n, « Suivant ».
fn page(n: usize) -> PageDesign {
    let mut items = vec![Item::text("Titre", 100, 90, 1720, 110, 84)];
    for k in 0..n { items.push(slot(100 + 420 * k as i32, 300, Some(k))); }
    items.push(Item::nav(NavAction::Next, 1000, 940, 360, 80));
    PageDesign { items, ..Default::default() }
}

#[test]
fn validation() {
    common::fr();
    let mut d = page(3);
    assert!(d.validate(0, 2, 3).iter().all(|p| !p.error), "{:?}", d.validate(0, 2, 3));
    // « Suivant » sur la derniere page : sans objet
    assert!(d.validate(1, 2, 3).iter().any(|p| p.msg.contains("n'apparaîtra pas")));
    // video inexistante : erreur ; vignette sans video : avertissement
    assert!(d.validate(0, 2, 2).iter().any(|p| p.error && p.msg.contains("n'existe pas")));
    if let Kind::Slot { video, .. } = &mut d.items[1].kind { *video = None; }
    let pr = d.validate(0, 2, 3);
    assert!(pr.iter().all(|p| !p.error) && pr.iter().any(|p| p.msg.contains("sans vidéo")), "{pr:?}");
    d.items[2].x = -5;
    assert!(d.validate(0, 2, 3).iter().any(|p| p.error && p.msg.contains("dépasse")));
    let mut d2 = page(2);
    d2.items[2].x = d2.items[1].x; d2.items[2].y = d2.items[1].y;
    assert!(d2.validate(0, 1, 2).iter().any(|p| p.msg.contains("chevauchent")));
    // page sans sortie (au-dela de la premiere)
    let mut d3 = page(1);
    d3.items.retain(|i| !matches!(i.kind, Kind::Nav { .. }));
    assert!(d3.validate(1, 2, 1).iter().any(|p| p.msg.contains("Aucun bouton vers une autre page")));
}

#[test]
fn pages_json_swap_remove() {
    common::fr();
    let mut ds = Designs::default();
    let mut p = PageDesign::blank_free(1);
    p.items.push(slot(100, 200, Some(2)));
    p.items.push(slot(600, 200, Some(0)));
    let mut t = Item::text("Chapitres", 100, 90, 1720, 110, 84);
    if let Kind::Text { color, align, font, .. } = &mut t.kind { *color = [10, 200, 30]; *align = Align::Left; *font = "georgia.ttf".into(); }
    p.items.push(t);
    p.items.push(Item { x: 50, y: 60, w: 300, h: 200, kind: Kind::Image { path: "logo.png".into(), link: None, frame: None }, look: Default::default(), layer: 0, sub: Default::default(), id: 0, nav: Default::default(), name: String::new() });
    ds.free.push(p);
    let text = ds.to_value().to_pretty();
    assert!(text.contains("\"pages\"") && text.contains("georgia.ttf"));
    assert!(!text.contains("generique") && !text.contains("principal"));
    let back = Designs::from_value(&parse(&text).unwrap()).unwrap();
    assert_eq!(back, ds);
    let vids = |d: &Designs| -> Vec<Option<usize>> { d.free[0].items.iter().filter(|i| matches!(i.kind, Kind::Slot { .. })).map(|i| i.fixed_video()).collect() };
    // echange de videos 0 et 2 : les vignettes suivent
    let mut x = ds.clone();
    x.swap_videos(0, 2);
    assert_eq!(vids(&x), vec![Some(0), Some(2)]);
    // retrait de la video 0 : sa vignette reste en place sans video, la video 2 devient 1
    let mut y = ds.clone();
    y.remove_video(0);
    assert_eq!(vids(&y), vec![Some(1), None]);
    let back = Designs::from_value(&parse(&y.to_value().to_pretty()).unwrap()).unwrap();
    assert_eq!(back, y, "une vignette sans video survit a l'enregistrement");
    assert!(PageDesign::from_value(&parse(r##"{"items":[{"type":"zzz","x":1,"y":1,"w":1,"h":1}]}"##).unwrap()).is_err());
}

#[test]
fn goto_buttons_json_remap_and_check() {
    common::fr();
    let mut ds = Designs::default();
    let mut p = page(2);
    p.items.push(Item::nav(NavAction::Goto, 100, 900, 300, 80)); // cible par defaut : page 1
    p.items.push(Item { x: 500, y: 900, w: 300, h: 80, kind: Kind::Nav { action: NavAction::Goto, text: "Lire".into(), target: Some(Target::Video(2)) }, look: Default::default(), layer: 0, sub: Default::default(), id: 0, nav: Default::default(), name: String::new() });
    ds.free.push(p);
    let text = ds.to_value().to_pretty();
    assert!(text.contains("aller_a") && text.contains("\"target\""));
    assert!(text.contains("\"page\": 1"), "destination de page : {{\"page\": n}}");
    let back = Designs::from_value(&parse(&text).unwrap()).unwrap();
    assert_eq!(back, ds);
    let targets = |d: &Designs| -> Vec<Option<Target>> { d.free[0].items.iter().filter_map(|i| if let Kind::Nav { action: NavAction::Goto, target, .. } = i.kind { Some(target) } else { None }).collect() };
    assert_eq!(targets(&ds), vec![Some(Target::Main(0)), Some(Target::Video(2))]);
    // echange des videos 0 et 2 : la cible suit ; retrait de la video 2 : la cible disparait
    let mut x = ds.clone();
    x.swap_videos(0, 2);
    assert_eq!(targets(&x)[1], Some(Target::Video(0)));
    let mut y = ds.clone();
    y.remove_video(2);
    assert_eq!(targets(&y)[1], None);
    assert!(y.free[0].validate(0, 2, 2).iter().any(|p| p.error && p.msg.contains("cible")));
    // suppression de la page 1 : le bouton qui y menait perd sa cible, les pages suivantes sont decalees
    let mut z = ds.clone();
    z.free[0].items.push(Item { x: 900, y: 900, w: 200, h: 80, kind: Kind::Nav { action: NavAction::Goto, text: "p3".into(), target: Some(Target::Main(2)) }, look: Default::default(), layer: 0, sub: Default::default(), id: 0, nav: Default::default(), name: String::new() });
    z.page_removed(0);
    assert_eq!(targets(&z), vec![None, Some(Target::Video(2)), Some(Target::Main(1))]);
    // verification des cibles
    let ok = |t: Target| matches!(t, Target::Main(0..=1) | Target::Video(0..=1));
    assert!(ds.free[0].check_targets(&ok).iter().any(|p| p.error), "video 3 inexistante");
}

#[test]
fn clickable_text_image_and_video_targets() {
    common::fr();
    let mut ds = Designs::default();
    let mut p = page(2);
    let mut t = Item::text("Lire", 100, 900, 300, 80, 50);
    if let Kind::Text { link, .. } = &mut t.kind { *link = Some(Target::Video(2)); }
    p.items.push(t);
    p.items.push(Item { x: 500, y: 900, w: 300, h: 80, kind: Kind::Image { path: "logo.png".into(), link: Some(Target::Main(1)), frame: None }, look: Default::default(), layer: 0, sub: Default::default(), id: 0, nav: Default::default(), name: String::new() });
    p.items.push(Item { x: 900, y: 900, w: 300, h: 80, kind: Kind::Nav { action: NavAction::Goto, text: "b".into(), target: Some(Target::Video(1)) }, look: Default::default(), layer: 0, sub: Default::default(), id: 0, nav: Default::default(), name: String::new() });
    p.items.push(Item { x: 1300, y: 900, w: 300, h: 80, kind: Kind::Image { path: "deco.png".into(), link: None, frame: None }, look: Default::default(), layer: 0, sub: Default::default(), id: 0, nav: Default::default(), name: String::new() });
    ds.free.push(p);
    let text = ds.to_value().to_pretty();
    assert!(text.contains("\"play\": \"video\""), "{text}");
    let back = Designs::from_value(&parse(&text).unwrap()).unwrap();
    assert_eq!(back, ds);
    let items = &ds.free[0].items;
    let n = items.len();
    assert!(items[n - 4].is_button() && items[n - 3].is_button() && !items[n - 1].is_button());
    let links = |d: &Designs| -> Vec<Option<Target>> { d.free[0].items.iter().rev().take(4).rev().map(|i| match i.kind { Kind::Nav { target, .. } => target, _ => i.link() }).collect() };
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
    o.items.push(Item { x: 0, y: 0, w: 400, h: 100, kind: Kind::Image { path: "a.png".into(), link: Some(Target::Video(0)), frame: None }, look: Default::default(), layer: 0, sub: Default::default(), id: 0, nav: Default::default(), name: String::new() });
    o.items.push(Item::nav(NavAction::Goto, 100, 50, 300, 80));
    assert!(o.validate(0, 1, 1).iter().any(|p| p.msg.contains("chevauchent")));
    // destinations inexistantes
    let ok = |t: Target| matches!(t, Target::Main(0) | Target::Video(0..=1));
    let probs = ds.free[0].check_targets(&ok);
    assert_eq!(probs.iter().filter(|p| p.error).count(), 2, "{probs:?}");
    // cible illisible
    assert!(PageDesign::from_value(&parse(r#"{"items":[{"type":"texte","text":"x","size":20,"x":0,"y":0,"w":50,"h":50,"target":{"menu":"???"}}]}"#).unwrap()).is_err());
}

#[test]
fn slot_with_destination() {
    common::fr();
    let mut p = page(2);
    p.items.push(Item { x: 100, y: 850, w: 250, h: 200, kind: Kind::Slot { thumb_h: 150, video: Some(2), link: Some(Target::Main(1)), start: None, chapter: None, label: String::new(), thumb_at: None, ret: Default::default() }, look: Default::default(), layer: 0, sub: Default::default(), id: 0, nav: Default::default(), name: String::new() });
    assert_eq!(p.slot_count(), 3);
    let mut ds = Designs::default();
    ds.free.push(p);
    let back = Designs::from_value(&parse(&ds.to_value().to_pretty()).unwrap()).unwrap();
    assert!(matches!(back.free[0].items.last().unwrap().kind, Kind::Slot { video: Some(2), link: Some(Target::Main(1)), .. }));
    let fixed = |d: &Designs| d.free[0].items.last().map(|i| (i.fixed_video(), i.link()));
    let mut x = back.clone();
    x.swap_videos(2, 0);
    assert_eq!(fixed(&x), Some((Some(0), Some(Target::Main(1)))));
    let mut y = back.clone();
    y.remove_video(2);
    assert_eq!(fixed(&y), Some((None, Some(Target::Main(1)))), "la vignette de la video retiree reste, sans video");
    let mut z = back.clone();
    z.page_removed(1);
    assert_eq!(fixed(&z), Some((Some(2), None)));
    let ok = |t: Target| matches!(t, Target::Main(0..=1) | Target::Video(0..=1));
    assert!(back.free[0].check_targets(&ok).iter().any(|p| p.error && p.msg.contains("vidéo 3")));
}

#[test]
fn button_style_json_scale_weight() {
    common::fr();
    use bdauthor_core::style::*;
    let mut st = ButtonStyle::default();
    st.glow = 20; st.zoom = 10; st.activated = true; st.sel_border = [1, 2, 3]; st.underline = true; st.dim = 30;
    let back = ButtonStyle::from_value(&parse(&st.to_value().to_pretty()).unwrap()).unwrap();
    assert_eq!(back, st);
    // valeurs hors plage bornees, champs absents = defaut
    let b = ButtonStyle::from_value(&parse(r#"{"glow": 999, "zoom": -4}"#).unwrap()).unwrap();
    assert_eq!((b.glow, b.zoom, b.sel_bg), (60, 0, ButtonStyle::default().sel_bg));
    assert!(ButtonStyle::from_value(&parse(r##"{"background": "#12"}"##).unwrap()).is_err());
    // echelle 720 : 20 px -> 13 px ; une epaisseur de 1 reste 1
    let k = st.scaled(1280.0 / 1920.0);
    assert_eq!((k.glow, k.normal_border_w), (13, 1));
    assert_eq!(st.margins(400, 200), (40, 30)); // halo 20 + 5 % de 400 / de 200
    assert_eq!(st.state_count(), 3);
    // style du projet et style/images propres a un element : aller-retour JSON
    let mut ds = Designs::default();
    ds.style = st.clone();
    let mut p = PageDesign::default();
    p.items.push(slot(100, 300, Some(0)));
    p.items.push(slot(600, 300, Some(1)));
    p.items[0].look.style = Some(ButtonStyle { glow: 0, ..ButtonStyle::default() });
    p.items.push(Item::nav(NavAction::Goto, 100, 900, 300, 80));
    p.items.last_mut().unwrap().look.skin = Skin { normal: "b.png".into(), selected: "".into(), activated: "c.svg".into(), ..Default::default() };
    ds.free.push(p.clone());
    let text = ds.to_value().to_pretty();
    assert!(text.contains("button_style") && text.contains("state_images"));
    assert_eq!(Designs::from_value(&parse(&text).unwrap()).unwrap(), ds);
    assert!(!Designs { style: st.clone(), ..Default::default() }.is_empty(), "un style seul doit etre enregistre");
    // poids : 2 vignettes (style propre sans halo, l'autre avec) + « Aller a » (images : 3 etats)
    let w = p.weight(&st, 0, 1, (1920, 1080));
    assert_eq!(w.buttons, 3);
    let plain = p.weight(&ButtonStyle::default(), 0, 1, (1920, 1080));
    assert!(w.bytes > plain.bytes && w.seconds > plain.seconds);
    let (a, b2) = (&p.items[0], &p.items[1]);
    let sz = |it: &Item, s: &ButtonStyle, n: u64| { let (mx, my) = s.margins(it.w, it.h); (it.w + 2 * mx) as u64 * (it.h + 2 * my) as u64 * n };
    let own = ButtonStyle { glow: 0, ..ButtonStyle::default() };
    assert_eq!(plain.bytes, sz(a, &own, 2) + sz(b2, &ButtonStyle::default(), 2) + 300 * 80 * 3);
    assert!(DECODED_LIMIT > w.bytes);
}

#[test]
fn home_page_json_and_remap() {
    common::fr();
    let mut ds = Designs::default();
    assert!(ds.is_empty());
    ds.home = Some(Target::Main(2));
    assert!(!ds.is_empty());
    let back = Designs::from_value(&parse(&ds.to_value().to_pretty()).unwrap()).unwrap();
    assert_eq!(back.home, Some(Target::Main(2)));
    let mut c = Designs { home: Some(Target::Main(3)), ..Default::default() };
    c.page_removed(1);
    assert_eq!(c.home, Some(Target::Main(2)));
    c.page_removed(2);
    assert_eq!(c.home, None);
    assert!(nav_applicable(NavAction::PlayAll, 0, 1));
    assert!(!nav_applicable(NavAction::Prev, 0, 2) && nav_applicable(NavAction::Next, 0, 2));
}

#[test]
fn page_background_json() {
    common::fr();
    let mut p = page(1);
    assert!(!p.to_value().to_pretty().contains("page_background"));
    p.bg = PageBg { visual: PageVisual::Video("fond.mp4".into(), true), music: PageMusic::Silent };
    let t = p.to_value().to_pretty();
    assert!(t.contains("page_background") && t.contains("video_sound"));
    assert_eq!(PageDesign::from_value(&parse(&t).unwrap()).unwrap(), p);
    p.bg = PageBg { visual: PageVisual::Image("a.png".into()), music: PageMusic::File("m.mp3".into()) };
    assert_eq!(PageDesign::from_value(&parse(&p.to_value().to_pretty()).unwrap()).unwrap(), p);
    p.bg = PageBg { visual: PageVisual::Project, music: PageMusic::File("m.mp3".into()) };
    assert_eq!(PageDesign::from_value(&parse(&p.to_value().to_pretty()).unwrap()).unwrap(), p);
}

#[test]
fn automatic_launch_is_saved_and_checked() {
    common::fr();
    let mut d = page(2);
    d.items[1].id = 5;
    d.idle = Some(Idle { seconds: 30, item: 5 });
    assert!(d.validate(0, 1, 2).iter().all(|p| !p.error), "{:?}", d.validate(0, 1, 2));
    let back = PageDesign::from_value(&parse(&d.to_value().to_pretty()).unwrap()).unwrap();
    assert_eq!(back.idle, Some(Idle { seconds: 30, item: 5 }));
    // delai borne, element inconnu ou qui n'est pas un bouton : erreur
    let long = parse(r##"{"auto_launch":{"seconds":99999,"item":5},"items":[]}"##).unwrap();
    assert_eq!(PageDesign::from_value(&long).unwrap().idle.unwrap().seconds, MAX_IDLE_SECONDS);
    d.idle = Some(Idle { seconds: 30, item: 77 });
    assert!(d.validate(0, 1, 2).iter().any(|p| p.error && p.msg.contains("Lancement automatique")));
    d.items[0].id = 9; // le titre n'est pas un bouton
    d.idle = Some(Idle { seconds: 30, item: 9 });
    assert!(d.validate(0, 1, 2).iter().any(|p| p.error));
    // sans reglage : rien d'ecrit
    d.idle = None;
    assert!(!d.to_value().to_pretty().contains("lancement_auto"));
}
