use bdauthor_core::design::*;
use bdauthor_core::editor::*;
use bdauthor_core::model::*;
use std::collections::HashMap;

fn model(n: usize) -> ProjectModel {
    let mut m = ProjectModel::default();
    for i in 0..n { m.videos.push(VideoEntry::new(&format!("v{i}.mp4"))); }
    m
}

#[test]
fn page_list() {
    let mut m = model(13);
    let p = list_pages(&m, &HashMap::new());
    assert_eq!(p.len(), 2);
    assert_eq!((p[0].count(), p[1].count()), (7, 6));
    m.videos[2].auto_s = Some(60.0);
    assert_eq!(list_pages(&m, &HashMap::new()).len(), 2, "duree inconnue : chapitres automatiques ignores");
    let d: HashMap<String, f64> = [("v2.mp4".to_string(), 1500.0)].into(); // 25 chapitres -> 4 pages (au-dela de 12 : pages de 8)
    let p = list_pages(&m, &d);
    assert_eq!(p.iter().filter(|x| x.menu == MenuType::Chapters).count(), 4);
    assert_eq!(p[2].video, Some(2));
    assert!(p[2].label(&m).contains("Chapitres de"));
    assert!(p[0].starting_point(&m).unwrap().slot_count() == 7);
}

#[test]
fn drag_and_hit() {
    let it = |x, y, w, h| Item { x, y, w, h, kind: Kind::Slot { thumb_h: h - 40, assign: None } };
    let items = vec![it(100, 100, 300, 200), it(250, 150, 300, 200)];
    assert_eq!(hit_test(&items, 260.0, 160.0, None, 8.0), Some((1, Handle::Body)), "l'element du dessus gagne");
    assert_eq!(hit_test(&items, 120.0, 120.0, None, 8.0), Some((0, Handle::Body)));
    assert_eq!(hit_test(&items, 1000.0, 900.0, None, 8.0), None);
    assert_eq!(hit_test(&items, 401.0, 301.0, Some(0), 8.0), Some((0, Handle::BottomRight)));
    // deplacement aimante a 10 et borne a l'ecran
    assert_eq!(dragged_rect((100, 100, 300, 200), Handle::Body, 33.0, -17.0, 10, 16), (130, 80, 300, 200));
    assert_eq!(dragged_rect((100, 100, 300, 200), Handle::Body, 5000.0, 5000.0, 0, 16), (1620, 880, 300, 200));
    // redimensionnement : taille minimale respectee
    assert_eq!(dragged_rect((100, 100, 300, 200), Handle::BottomRight, -1000.0, -1000.0, 0, 16), (100, 100, 16, 16));
    assert_eq!(dragged_rect((100, 100, 300, 200), Handle::TopLeft, 50.0, 20.0, 0, 16), (150, 120, 250, 180));
    // vignette : la hauteur du libelle est conservee
    let mut a = items[0].clone();
    set_rect(&mut a, &items[0], (100, 100, 300, 300));
    assert_eq!(a.kind, Kind::Slot { thumb_h: 260, assign: None });
}

#[test]
fn ordering_and_decor_copy() {
    let mut d = PageDesign::starting_point(MenuType::Main, 3, 0, 2, "T").unwrap(); // [texte, v, v, v, suivant]
    assert!(matches!(d.items[0].kind, Kind::Text { .. }));
    let up = move_in_list(&mut d, 0, true);
    assert_eq!(up, 0);
    assert_eq!(move_in_list(&mut d, 0, false), 1, "le texte passe devant la 1re vignette");
    assert_eq!(to_front(&mut d, 1), 4);
    assert!(matches!(d.items[4].kind, Kind::Text { .. }) && matches!(d.items[0].kind, Kind::Slot { .. }));
    assert_eq!(to_back(&mut d, 4), 0);
    assert!(matches!(d.items[0].kind, Kind::Text { .. }));
    let mut src = PageDesign::starting_point(MenuType::Main, 3, 0, 2, "Titre commun").unwrap();
    src.items.push(Item { x: 10, y: 10, w: 100, h: 100, kind: Kind::Image { path: "logo.png".into() } });
    let target = PageDesign::default_main(2, 1, 2).unwrap();
    let out = copy_decor(&src, target.clone(), &target);
    assert!(out.items.iter().any(|i| matches!(&i.kind, Kind::Nav { action: NavAction::Prev, .. })), "Precedent repris de la page cible");
    assert!(out.items.iter().any(|i| matches!(&i.kind, Kind::Nav { action: NavAction::Next, .. })));
    assert_eq!(out.slot_count(), 2);
    assert!(out.items.iter().any(|i| matches!(&i.kind, Kind::Text { text, .. } if text == "Titre commun")));
    assert!(out.items.iter().any(|i| matches!(i.kind, Kind::Image { .. })));
    assert!(matches!(out.items[0].kind, Kind::Text { .. } | Kind::Image { .. }), "decors derriere");
}

#[test]
fn history_undo_redo() {
    let mut d = Designs::default();
    let mut h = History::default();
    h.reset(&d);
    let a = PageDesign::default_main(2, 0, 1).unwrap();
    d.main.insert(0, a.clone());
    h.observe(&d, 1000);
    // changement rapproche : fusionne avec le precedent ? non, c'est la 1re etape (pile vide) -> nouvelle etape
    d.main.get_mut(&0).unwrap().items[0].x += 5;
    h.observe(&d, 1100); // fusion
    d.main.get_mut(&0).unwrap().items[0].x += 5;
    h.observe(&d, 3000); // nouvelle etape
    assert!(h.can_undo() && !h.can_redo());
    let x_final = d.main[&0].items[0].x;
    assert!(h.undo(&mut d));
    assert_eq!(d.main[&0].items[0].x, x_final - 5);
    assert!(h.undo(&mut d));
    assert!(d.main.is_empty(), "retour a l'etat initial");
    assert!(!h.undo(&mut d));
    assert!(h.redo(&mut d) && h.redo(&mut d));
    assert_eq!(d.main[&0].items[0].x, x_final);
    assert!(!h.redo(&mut d));
    // un nouveau changement vide « refaire »
    h.undo(&mut d);
    d.main.get_mut(&0).unwrap().items[0].y += 7;
    h.observe(&d, 9000);
    assert!(!h.can_redo());
    h.reset(&d);
    assert!(!h.can_undo());
}

#[test]
fn slot_assignment_and_swap() {
    let mut d = PageDesign::default_main(4, 0, 1).unwrap(); // 4 vignettes automatiques
    assert_eq!(d.slot_assignment(4), vec![0, 1, 2, 3]);
    d.assign_slot(2, Some(0), 4); // la 3e vignette montre l'element 1 : echange avec la 1re
    assert_eq!(d.slot_assignment(4), vec![2, 1, 0, 3]);
    d.assign_slot(3, Some(1), 4);
    assert_eq!(d.slot_assignment(4), vec![2, 3, 0, 1]);
    let mut seen = d.slot_assignment(4);
    seen.sort();
    assert_eq!(seen, vec![0, 1, 2, 3], "toujours une permutation");
    d.assign_slot(2, None, 4);
    let a = d.slot_assignment(4);
    let mut s = a.clone(); s.sort(); s.dedup();
    assert_eq!(s.len(), 4, "pas de doublon apres retour a l'automatique : {a:?}");
    // affectation invalide ignoree (page plus petite)
    let mut e = PageDesign::default_main(3, 0, 1).unwrap();
    if let Kind::Slot { assign, .. } = &mut e.items[0].kind { *assign = Some(7); }
    assert_eq!(e.slot_assignment(3), vec![0, 1, 2]);
    // aller-retour JSON
    let mut ds = Designs::default();
    ds.main.insert(0, d.clone());
    let back = Designs::from_value(&bdauthor_core::json::parse(&ds.to_value().to_pretty()).unwrap()).unwrap();
    assert_eq!(back, ds);
}

#[test]
fn free_pages_listing_and_add() {
    let mut m = model(5);
    m.par_page = Some(3); // 2 pages automatiques
    assert_eq!(list_pages(&m, &HashMap::new()).len(), 2);
    // pas de personnalisation : l'ajout d'une page libre n'ajoute rien a la page automatique precedente
    let f = add_free_page(&mut m);
    assert_eq!(f, 0);
    let pages = list_pages(&m, &HashMap::new());
    assert_eq!(pages.len(), 3);
    assert!(pages.iter().all(|p| p.pages == 3), "les pages automatiques voient la page libre dans la suite");
    let last = pages.last().unwrap();
    assert!(last.is_free() && last.page == 2 && last.items.len() == 5);
    assert!(last.custom(&m.designs));
    assert_eq!(m.designs.free[0].items.len(), 1, "page vierge = bouton Precedent seulement");
    // une 2e page libre : la 1re recoit un bouton Suivant
    add_free_page(&mut m);
    assert!(m.designs.free[0].items.iter().any(|i| matches!(&i.kind, Kind::Nav { action: NavAction::Next, .. })));
    // suppression
    pages.last().unwrap().remove_design(&mut m.designs);
    assert_eq!(m.designs.free.len(), 1);
    // page automatique personnalisee sans « Suivant » : l'ajout le lui donne
    let mut m2 = model(2);
    m2.designs.main.insert(0, PageDesign::default_main(2, 0, 1).unwrap());
    add_free_page(&mut m2);
    assert!(m2.designs.main[&0].items.iter().any(|i| matches!(&i.kind, Kind::Nav { action: NavAction::Next, .. })));
}
