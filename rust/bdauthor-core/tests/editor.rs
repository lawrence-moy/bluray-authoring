use bdauthor_core::design::*;
use bdauthor_core::editor::*;


/// Page de test : titre, `n` vignettes de videos, puis « Suivant ».
fn page(n: usize, title: &str) -> PageDesign {
    let mut items = vec![Item::text(title, 100, 90, 1720, 110, 84)];
    for k in 0..n { items.push(Item::video_slot(100 + 420 * k as i32, 300, 400, 260, 225, k, None, "")); }
    items.push(Item::nav(NavAction::Next, 1000, 940, 360, 80));
    PageDesign { items, ..Default::default() }
}

#[test]
fn drag_and_hit() {
    let it = |x, y, w, h| Item { x, y, w, h, kind: Kind::Slot { thumb_h: h - 40, video: None, link: None, start: None, chapter: None, label: String::new(), thumb_at: None, ret: Default::default() }, look: Default::default(), layer: 0, sub: Default::default() };
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
    assert_eq!(a.kind, Kind::Slot { thumb_h: 260, video: None, link: None, start: None, chapter: None, label: String::new(), thumb_at: None, ret: Default::default() });
}

#[test]
fn ordering_and_decor_copy() {
    let mut d = page(3, "T"); // [texte, v, v, v, suivant]
    assert!(matches!(d.items[0].kind, Kind::Text { .. }));
    let up = move_in_list(&mut d, 0, true);
    assert_eq!(up, 0);
    assert_eq!(move_in_list(&mut d, 0, false), 1, "le texte passe devant la 1re vignette");
    assert_eq!(to_front(&mut d, 1), 4);
    assert!(matches!(d.items[4].kind, Kind::Text { .. }) && matches!(d.items[0].kind, Kind::Slot { .. }));
    assert_eq!(to_back(&mut d, 4), 0);
    assert!(matches!(d.items[0].kind, Kind::Text { .. }));
    let mut src = page(3, "Titre commun");
    src.items.push(Item { x: 10, y: 10, w: 100, h: 100, kind: Kind::Image { path: "logo.png".into(), link: None }, look: Default::default(), layer: 0, sub: Default::default() });
    let mut target = page(2, "Autre");
    target.items.retain(|i| matches!(i.kind, Kind::Slot { .. }));
    let out = copy_decor(&src, target.clone());
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
    d.free.push(page(2, "T"));
    h.observe(&d, 1000);
    // changement rapproche : fusionne avec le precedent ? non, c'est la 1re etape (pile vide) -> nouvelle etape
    d.free[0].items[0].x += 5;
    h.observe(&d, 1100); // fusion
    d.free[0].items[0].x += 5;
    h.observe(&d, 3000); // nouvelle etape
    assert!(h.can_undo() && !h.can_redo());
    let x_final = d.free[0].items[0].x;
    assert!(h.undo(&mut d));
    assert_eq!(d.free[0].items[0].x, x_final - 5);
    assert!(h.undo(&mut d));
    assert!(d.free.is_empty(), "retour a l'etat initial");
    assert!(!h.undo(&mut d));
    assert!(h.redo(&mut d) && h.redo(&mut d));
    assert_eq!(d.free[0].items[0].x, x_final);
    assert!(!h.redo(&mut d));
    // un nouveau changement vide « refaire »
    h.undo(&mut d);
    d.free[0].items[0].y += 7;
    h.observe(&d, 9000);
    assert!(!h.can_redo());
    h.reset(&d);
    assert!(!h.can_undo());
}

#[test]
fn remote_simulation() {
    use bdauthor_core::style::ButtonStyle;
    let info = PageInfo { r: PageRef(0), page: 0, pages: 2 };
    let mut d = page(4, "Videos");
    let mut t = Item::text("Page 2", 100, 600, 400, 80, 40);
    if let Kind::Text { link, .. } = &mut t.kind { *link = Some(Target::Main(1)); }
    d.items.push(t);
    d.items.push(Item { x: 1500, y: 60, w: 200, h: 120, kind: Kind::Slot { thumb_h: 100, video: None, link: None, start: None, chapter: None, label: String::new(), thumb_at: None, ret: Default::default() }, look: Default::default(), layer: 0, sub: Default::default() });
    let st = ButtonStyle::default();
    let b = sim_buttons(&info, &d, &st, 4);
    let acts: Vec<SimAction> = b.iter().map(|x| x.action).collect();
    // 4 vignettes, « Suivant », le texte cliquable ; la vignette sans video n'existe pas
    assert_eq!(acts, vec![SimAction::Play(0), SimAction::Play(1), SimAction::Play(2), SimAction::Play(3), SimAction::Next, SimAction::Go(Target::Main(1))]);
    // voisins : meme regle que le disque (bdauthor_core::menu::auto_neighbors)
    let rects: Vec<(i32, i32, i32, i32)> = b.iter().map(|x| x.rect).collect();
    let n = bdauthor_core::menu::auto_neighbors(&rects);
    assert!(b.iter().zip(&n).all(|(x, m)| x.neighbors == (m.0 as usize, m.1 as usize, m.2 as usize, m.3 as usize)));
    assert_eq!(b[0].neighbors.3, 1);
    assert_eq!(b[1].neighbors.2, 0);
    // destination personnalisee d'une vignette ; video inexistante : absente
    if let Kind::Slot { link, .. } = &mut d.items[1].kind { *link = Some(Target::Main(1)); }
    assert_eq!(sim_buttons(&info, &d, &st, 4)[0].action, SimAction::Go(Target::Main(1)));
    assert_eq!(sim_buttons(&info, &d, &st, 2).len(), 4);
    // « Suivant » absent de la derniere page
    let last = PageInfo { r: PageRef(1), page: 1, pages: 2 };
    assert!(!sim_buttons(&last, &d, &st, 4).iter().any(|x| x.action == SimAction::Next));
    // pages visees
    let pages = vec![info.clone(), last.clone()];
    assert_eq!(page_of_target(&pages, Target::Main(1)), Some(PageRef(1)));
    assert_eq!(page_of_target(&pages, Target::Main(5)), None);
    assert_eq!(page_of_target(&pages, Target::Video(0)), None);
}
