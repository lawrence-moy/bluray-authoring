//! Pages du menu : pages de depart, edition des pages, simulation des vignettes de chapitre.
mod common;
use bdauthor_core::convert::{generate_pages, GenVideo};
use bdauthor_core::design::*;
use bdauthor_core::editor::*;
use bdauthor_core::model::*;
use bdauthor_core::style::ButtonStyle;
use std::collections::HashMap;

fn gv(title: &str, ch: &[f64]) -> GenVideo {
    GenVideo { title: title.into(), chapters: ch.iter().map(|t| (*t, String::new())).collect() }
}

fn slots(d: &PageDesign) -> Vec<(Option<usize>, Option<f64>, Option<Target>)> {
    d.items.iter().filter_map(|i| match &i.kind { Kind::Slot { video, start, link, .. } => Some((*video, *start, *link)), _ => None }).collect()
}

#[test]
fn starter_pages() {
    common::fr();
    // 3 videos sur une page ; la video 2 a 3 chapitres (une page de chapitres)
    let v = vec![gv("A", &[]), gv("B", &[0.0, 60.0, 125.0]), gv("C", &[])];
    let st = ButtonStyle { dim: 9, ..ButtonStyle::default() };
    let d = generate_pages(&v, "Disque", &st).unwrap();
    assert_eq!(d.style.dim, 9);
    assert_eq!(d.free.len(), 2);
    assert_eq!(d.free[0].name, "Vidéos");
    assert_eq!(d.free[1].name, "Chapitres : B");
    // page 1 : la vignette de A lit A ; celle de B (a chapitres) ouvre la page de ses chapitres
    assert_eq!(slots(&d.free[0]), vec![(Some(0), None, None), (Some(1), None, Some(Target::Main(1))), (Some(2), None, None)]);
    // chapitres : vignettes de chapitre (video, instant de depart), nommees « nom (m:ss) »
    assert_eq!(slots(&d.free[1]), vec![(Some(1), Some(0.0), None), (Some(1), Some(60.0), None), (Some(1), Some(125.0), None)]);
    let label = |p: &PageDesign, k: usize| p.items.iter().filter_map(|i| match &i.kind { Kind::Slot { label, .. } => Some(label.clone()), _ => None }).nth(k).unwrap();
    assert_eq!(label(&d.free[1], 2), " (2:05)");
    // « Tout lire » -> lire la video ; « Retour » -> page de la video
    let navs: Vec<(NavAction, Option<Target>, String)> = d.free[1].items.iter().filter_map(|i| match &i.kind { Kind::Nav { action, target, text } => Some((*action, *target, text.clone())), _ => None }).collect();
    assert!(navs.contains(&(NavAction::Goto, Some(Target::Video(1)), "Tout lire".into())), "{navs:?}");
    assert!(navs.contains(&(NavAction::Goto, Some(Target::Main(0)), "Retour".into())), "{navs:?}");
    // toutes les pages generees sont valides
    for (k, p) in d.free.iter().enumerate() {
        let errs: Vec<_> = p.validate(k, d.free.len(), 3).into_iter().filter(|x| x.error).collect();
        assert!(errs.is_empty(), "page {k} : {errs:?}");
    }
    // plus de 12 videos : plusieurs pages avec « Precedent » / « Suivant »
    let many: Vec<GenVideo> = (0..13).map(|i| gv(&format!("V{i}"), &[])).collect();
    let d = generate_pages(&many, "Disque", &ButtonStyle::default()).unwrap();
    assert_eq!(d.free.len(), 2);
    assert_eq!(d.free[1].name, "Vidéos (2/2)");
    assert!(d.free[1].items.iter().any(|i| matches!(i.kind, Kind::Nav { action: NavAction::Prev, .. })));
}

fn model(n: usize) -> ProjectModel {
    let mut m = ProjectModel::default();
    for i in 0..n { m.videos.push(VideoEntry::new(&format!("v{i}.mp4"))); }
    m
}

#[test]
fn editing_pages() {
    common::fr();
    let mut m = model(2);
    m.videos[0].chapters = vec![ChapterEntry { id: 1, t: 0.0, title: "Debut".into() }, ChapterEntry { id: 2, t: 30.0, title: "Suite".into() }];
    // chapitres automatiques sans duree connue : pages de depart differees
    m.videos[1].auto_s = Some(60.0);
    assert!(ensure_pages(&mut m, &HashMap::new()).is_err());
    assert!(m.designs.free.is_empty());
    m.videos[1].auto_s = None;
    assert!(ensure_pages(&mut m, &HashMap::new()).unwrap());
    // deja des pages : rien ne change
    assert!(!ensure_pages(&mut m, &HashMap::new()).unwrap());
    let pages = list_pages(&m);
    assert_eq!(pages.len(), 2);
    assert_eq!(pages[1].label(&m), "2. Chapitres : v0");
    // le modele (enregistre) reste valide
    let pb: Vec<String> = m.validate(std::path::Path::new("/nonexistent")).into_iter().filter(|p| !p.contains("introuvable")).collect();
    assert!(pb.is_empty(), "{pb:?}");
    // nouvelle page : ajoutee a la fin, sans toucher aux autres pages
    let before = m.designs.free[1].items.len();
    let k = add_free_page(&mut m);
    assert_eq!(k, 2);
    assert_eq!(m.designs.free[2].name, "Page 3");
    assert_eq!(m.designs.free[1].items.len(), before, "pas de bouton « Suivant » ajoute a la page precedente");
    // deplacement : les liens suivent leur page
    m.designs.home = Some(Target::Main(1));
    if let Kind::Slot { ret, .. } = &mut m.designs.free[0].items.iter_mut().find(|i| matches!(i.kind, Kind::Slot { .. })).unwrap().kind { *ret = SlotReturn::Page(1); }
    swap_pages(&mut m, 1, 2);
    assert_eq!(m.designs.home, Some(Target::Main(2)));
    assert_eq!(m.designs.free[2].name, "Chapitres : v0");
    let s = m.designs.free[0].items.iter().find_map(|i| match &i.kind { Kind::Slot { ret, link: Some(l), .. } => Some((*ret, *l)), _ => None }).unwrap();
    assert_eq!(s, (SlotReturn::Page(2), Target::Main(2)), "la vignette de v0 ouvre ses chapitres, deplaces en page 3");
    // suppression d'une page : retours et accueil renumerotes
    let pages = list_pages(&m);
    pages[1].remove_design(&mut m.designs);
    assert_eq!(m.designs.free.len(), 2);
    assert_eq!(m.designs.home, Some(Target::Main(1)));
    // retrait d'une video : sa vignette reste sans video, la suivante est renumerotee
    m.remove_video(0);
    let v: Vec<Option<usize>> = slots(&m.designs.free[0]).into_iter().map(|s| s.0).collect();
    assert_eq!(v, vec![None, Some(0)]);
}

#[test]
fn simulation_of_chapter_thumbnails() {
    common::fr();
    let mut m = model(2);
    let mut d = PageDesign::blank_free(0);
    d.items.push(Item::video_slot(100, 100, 400, 260, 225, 1, Some(42.0), "Scene"));
    d.items.push(Item::video_slot(600, 100, 400, 260, 225, 0, None, ""));
    if let Kind::Slot { ret, .. } = &mut d.items[1].kind { *ret = SlotReturn::Home; }
    m.designs.free.push(d);
    let pages = list_pages(&m);
    let b = sim_buttons(&pages[0], &m.designs.free[0], &ButtonStyle::default(), 2);
    assert_eq!(b.len(), 2);
    assert_eq!(b[0].action, SimAction::PlayFrom(1, 42.0));
    assert_eq!(b[0].ret, SlotReturn::Origin);
    assert_eq!(b[1].action, SimAction::Play(0));
    assert_eq!(b[1].ret, SlotReturn::Home);
}
