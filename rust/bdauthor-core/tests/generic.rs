//! Pages generiques : conversion des anciens projets, edition des pages, simulation des vignettes de chapitre.
use bdauthor_core::convert::{generate_pages, GenVideo};
use bdauthor_core::design::*;
use bdauthor_core::editor::*;
use bdauthor_core::model::*;
use bdauthor_core::style::ButtonStyle;
use std::collections::HashMap;

fn gv(title: &str, ch: &[f64]) -> GenVideo {
    GenVideo { title: title.into(), chapters: ch.iter().map(|t| (*t, String::new())).collect() }
}

fn slots(d: &PageDesign) -> Vec<(Option<Option<usize>>, Option<f64>, Option<Target>)> {
    d.items.iter().filter_map(|i| match &i.kind { Kind::Slot { video, start, link, .. } => Some((*video, *start, *link)), _ => None }).collect()
}

#[test]
fn conversion_of_automatic_menus() {
    // 3 videos, 2 par page ; la video 2 a 3 chapitres
    let v = vec![gv("A", &[]), gv("B", &[0.0, 60.0, 125.0]), gv("C", &[])];
    let d = generate_pages(&v, Some(2), "Disque", &Designs::default()).unwrap();
    assert!(d.generic);
    // pages : videos 1-2, video 3, chapitres de B (2 + 1)
    assert_eq!(d.free.len(), 4);
    assert_eq!(d.free[0].name, "Menu principal (1/2)");
    assert_eq!(d.free[2].name, "Chapitres : B (1/2)");
    // page 1 : la vignette de A lit A ; celle de B (a chapitres) ouvre la page de ses chapitres
    let s0 = slots(&d.free[0]);
    assert_eq!(s0, vec![(Some(Some(0)), None, None), (Some(Some(1)), None, Some(Target::Main(2)))]);
    // chapitres : vignettes de chapitre (video, instant de depart), nommees « m:ss »
    let s2 = slots(&d.free[2]);
    assert_eq!(s2, vec![(Some(Some(1)), Some(0.0), None), (Some(Some(1)), Some(60.0), None)]);
    assert_eq!(slots(&d.free[3]), vec![(Some(Some(1)), Some(125.0), None)]);
    let label = |p: &PageDesign, k: usize| p.items.iter().filter_map(|i| match &i.kind { Kind::Slot { label, .. } => Some(label.clone()), _ => None }).nth(k).unwrap();
    assert_eq!(label(&d.free[3], 0), " (2:05)");
    // « Tout lire » d'un menu de chapitres -> lire la video ; « Retour » -> page de la video
    let navs: Vec<(NavAction, Option<Target>)> = d.free[2].items.iter().filter_map(|i| match &i.kind { Kind::Nav { action, target, .. } => Some((*action, *target)), _ => None }).collect();
    assert!(navs.contains(&(NavAction::Goto, Some(Target::Video(1)))), "{navs:?}");
    assert!(navs.contains(&(NavAction::Goto, Some(Target::Main(0)))), "{navs:?}");
    // toutes les pages generees sont valides
    for (k, p) in d.free.iter().enumerate() {
        let errs: Vec<_> = p.validate_free(k, d.free.len(), 3).into_iter().filter(|x| x.error).collect();
        assert!(errs.is_empty(), "page {k} : {errs:?}");
    }
}

#[test]
fn conversion_keeps_custom_pages_and_targets() {
    let mut legacy = Designs::default();
    // page libre avec un lien vers le menu de chapitres de la video 1, accueil = page libre
    let mut free = PageDesign::blank_free(1);
    free.items.push(Item::nav(NavAction::Goto, 100, 100, 300, 80));
    if let Kind::Nav { target, .. } = &mut free.items.last_mut().unwrap().kind { *target = Some(Target::Chapters(0, 0)); }
    legacy.free.push(free);
    legacy.home = Some(Target::Main(1));
    legacy.style = ButtonStyle { dim: 9, ..ButtonStyle::default() };
    let v = vec![gv("A", &[0.0, 10.0]), gv("B", &[])];
    let d = generate_pages(&v, None, "Disque", &legacy).unwrap();
    // page principale (1), page libre (2), chapitres de A (3)
    assert_eq!(d.free.len(), 3);
    assert_eq!(d.home, Some(Target::Main(1)));
    assert_eq!(d.style.dim, 9);
    let goto = d.free[1].items.iter().find_map(|i| match &i.kind { Kind::Nav { action: NavAction::Goto, target, .. } => *target, _ => None });
    assert_eq!(goto, Some(Target::Main(2)));
}

fn model(n: usize) -> ProjectModel {
    let mut m = ProjectModel::default();
    for i in 0..n { m.videos.push(VideoEntry::new(&format!("v{i}.mp4"))); }
    m
}

#[test]
fn editing_generic_pages() {
    let mut m = model(2);
    m.videos[0].chapters = vec![ChapterEntry { t: 0.0, title: "Debut".into() }, ChapterEntry { t: 30.0, title: "Suite".into() }];
    // chapitres automatiques sans duree connue : conversion differee
    m.videos[1].auto_s = Some(60.0);
    assert!(convert_to_generic(&mut m, &HashMap::new()).is_err());
    assert!(!m.designs.generic);
    m.videos[1].auto_s = None;
    convert_to_generic(&mut m, &HashMap::new()).unwrap();
    assert!(m.designs.generic);
    let pages = list_pages(&m, &HashMap::new());
    assert_eq!(pages.len(), 2);
    assert!(pages.iter().all(|p| p.menu == MenuType::Main && p.is_free()));
    assert_eq!(pages[1].label(&m), "2. Chapitres : v0");
    // le modele (enregistre) reste valide
    let pb: Vec<String> = m.validate(std::path::Path::new("/nonexistent")).into_iter().filter(|p| !p.contains("introuvable")).collect();
    assert!(pb.is_empty(), "{pb:?}");
    // nouvelle page : ajoutee a la fin, « Suivant » ajoute a la precedente
    let k = add_free_page(&mut m);
    assert_eq!(k, 2);
    assert_eq!(m.designs.free[2].name, "Page 3");
    assert!(m.designs.free[1].items.iter().any(|i| matches!(i.kind, Kind::Nav { action: NavAction::Next, .. })));
    // deplacement : les liens suivent leur page
    m.designs.home = Some(Target::Main(1));
    if let Kind::Slot { ret, .. } = &mut m.designs.free[0].items.iter_mut().find(|i| matches!(i.kind, Kind::Slot { .. })).unwrap().kind { *ret = SlotReturn::Page(1); }
    swap_pages(&mut m, 1, 2);
    assert_eq!(m.designs.home, Some(Target::Main(2)));
    assert_eq!(m.designs.free[2].name, "Chapitres : v0");
    let s = m.designs.free[0].items.iter().find_map(|i| match &i.kind { Kind::Slot { ret, link: Some(l), .. } => Some((*ret, *l)), _ => None }).unwrap();
    assert_eq!(s, (SlotReturn::Page(2), Target::Main(2)), "la vignette de v0 ouvre ses chapitres, deplaces en page 3");
    // suppression d'une page : retours et accueil renumerotes
    let pages = list_pages(&m, &HashMap::new());
    pages[1].remove_design(&mut m.designs);
    assert_eq!(m.designs.free.len(), 2);
    assert_eq!(m.designs.home, Some(Target::Main(1)));
    // pages generees ajoutees a la fin, liens decales
    let added = append_generated_pages(&mut m, &HashMap::new()).unwrap();
    assert_eq!(added, 2);
    assert_eq!(m.designs.free.len(), 4);
    let link = m.designs.free[2].items.iter().find_map(|i| match &i.kind { Kind::Slot { link: Some(l), .. } => Some(*l), _ => None });
    assert_eq!(link, Some(Target::Main(3)));
}

#[test]
fn simulation_of_chapter_thumbnails() {
    let mut m = model(2);
    m.designs.generic = true;
    let mut d = PageDesign::blank_free(0);
    d.items.push(Item::video_slot(100, 100, 400, 260, 225, 1, Some(42.0), "Scene"));
    d.items.push(Item::video_slot(600, 100, 400, 260, 225, 0, None, ""));
    if let Kind::Slot { ret, .. } = &mut d.items[1].kind { *ret = SlotReturn::Home; }
    m.designs.free.push(d);
    let pages = list_pages(&m, &HashMap::new());
    let b = sim_buttons(&pages[0], &m.designs.free[0], &ButtonStyle::default(), 2, &|_| false);
    assert_eq!(b.len(), 2);
    assert_eq!(b[0].action, SimAction::PlayFrom(1, 42.0));
    assert_eq!(b[0].ret, SlotReturn::Origin);
    assert_eq!(b[1].action, SimAction::Play(0));
    assert_eq!(b[1].ret, SlotReturn::Home);
}
