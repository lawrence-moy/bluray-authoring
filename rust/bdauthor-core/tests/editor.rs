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
    let it = |x, y, w, h| Item { x, y, w, h, kind: Kind::Slot { thumb_h: h - 40 } };
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
    assert_eq!(a.kind, Kind::Slot { thumb_h: 260 });
}

#[test]
fn ordering_and_decor_copy() {
    let mut d = PageDesign::starting_point(MenuType::Main, 3, 0, 2, "T").unwrap(); // [texte, v, v, v, suivant]
    assert!(matches!(d.items[0].kind, Kind::Text { .. }));
    let first_slot = 1;
    let new = move_slot_order(&mut d, first_slot, false);
    assert_eq!(new, 2);
    let n2 = move_slot_order(&mut d, 3, false); // la derniere vignette ne peut plus descendre
    assert_eq!(n2, 3);
    let up = move_in_list(&mut d, 0, true);
    assert_eq!(up, 0);
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
