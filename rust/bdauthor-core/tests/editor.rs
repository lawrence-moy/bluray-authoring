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
    let it = |x, y, w, h| Item { x, y, w, h, kind: Kind::Slot { thumb_h: h - 40, video: None, link: None, start: None, chapter: None, label: String::new(), thumb_at: None, ret: Default::default() }, look: Default::default(), layer: 0, sub: Default::default(), id: 0, nav: Default::default(), name: String::new() };
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
    src.items.push(Item { x: 10, y: 10, w: 100, h: 100, kind: Kind::Image { path: "logo.png".into(), link: None, frame: None }, look: Default::default(), layer: 0, sub: Default::default(), id: 0, nav: Default::default(), name: String::new() });
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
    d.items.push(Item { x: 1500, y: 60, w: 200, h: 120, kind: Kind::Slot { thumb_h: 100, video: None, link: None, start: None, chapter: None, label: String::new(), thumb_at: None, ret: Default::default() }, look: Default::default(), layer: 0, sub: Default::default(), id: 0, nav: Default::default(), name: String::new() });
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

#[test]
fn proportional_resize_keeps_ratio_and_anchor() {
    use bdauthor_core::editor::dragged_rect_ratio;
    // 300x200 (3:2) : coin bas droit tire de +60 en x seulement -> 360x240, coin haut gauche fixe
    assert_eq!(dragged_rect_ratio((100, 100, 300, 200), Handle::BottomRight, 60.0, 0.0, 0, 16), (100, 100, 360, 240));
    // en y seulement : +40 -> 240 de haut, donc 360 de large
    assert_eq!(dragged_rect_ratio((100, 100, 300, 200), Handle::BottomRight, 0.0, 40.0, 0, 16), (100, 100, 360, 240));
    // coin haut gauche : le coin bas droit (400, 300) reste fixe
    let r = dragged_rect_ratio((100, 100, 300, 200), Handle::TopLeft, -30.0, -5.0, 0, 16);
    assert_eq!((r.0 + r.2, r.1 + r.3), (400, 300));
    assert!((r.2 as f32 / r.3 as f32 - 1.5).abs() < 0.02);
    // reduction : plancher a la taille minimale en gardant le ratio
    let r = dragged_rect_ratio((100, 100, 300, 200), Handle::BottomRight, -1000.0, -1000.0, 0, 16);
    assert!(r.2 >= 16 && r.3 >= 16 && (r.2 as f32 / r.3 as f32 - 1.5).abs() < 0.1, "{r:?}");
    // le plan borne : jamais hors de 1920x1080
    let r = dragged_rect_ratio((1500, 800, 300, 200), Handle::BottomRight, 5000.0, 5000.0, 0, 16);
    assert!(r.0 + r.2 <= 1920 && r.1 + r.3 <= 1080, "{r:?}");
    // le deplacement n'est pas touche
    assert_eq!(dragged_rect_ratio((100, 100, 300, 200), Handle::Body, 33.0, -17.0, 10, 16), dragged_rect((100, 100, 300, 200), Handle::Body, 33.0, -17.0, 10, 16));
}

#[test]
fn side_handles_resize_one_dimension() {
    use bdauthor_core::editor::{dragged_rect, dragged_rect_ratio, hit_test, Handle};
    let items = vec![Item::text("T", 100, 100, 400, 200, 40)];
    // milieu du bord droit / bas, element selectionne : poignees de cote ; non selectionne : corps
    assert_eq!(hit_test(&items, 500.0, 200.0, Some(0), 10.0), Some((0, Handle::Right)));
    assert_eq!(hit_test(&items, 100.0, 200.0, Some(0), 10.0), Some((0, Handle::Left)));
    assert_eq!(hit_test(&items, 300.0, 100.0, Some(0), 10.0), Some((0, Handle::Top)));
    assert_eq!(hit_test(&items, 300.0, 299.0, Some(0), 10.0), Some((0, Handle::Bottom)));
    assert_eq!(hit_test(&items, 499.0, 200.0, None, 10.0), Some((0, Handle::Body)));
    // les angles restent prioritaires
    assert_eq!(hit_test(&items, 500.0, 300.0, Some(0), 10.0), Some((0, Handle::BottomRight)));
    let o = (100, 100, 400, 200);
    assert_eq!(dragged_rect(o, Handle::Right, 50.0, 80.0, 0, 16), (100, 100, 450, 200));
    assert_eq!(dragged_rect(o, Handle::Left, 50.0, 80.0, 0, 16), (150, 100, 350, 200));
    assert_eq!(dragged_rect(o, Handle::Top, 50.0, -40.0, 0, 16), (100, 60, 400, 240));
    assert_eq!(dragged_rect(o, Handle::Bottom, 50.0, 1000.0, 0, 16), (100, 100, 400, 980), "borne au plan");
    assert_eq!(dragged_rect_ratio(o, Handle::Right, 50.0, 0.0, 0, 16), (100, 100, 450, 200), "Maj sans effet sur un cote");
}

#[test]
fn snapping_to_screen_and_items() {
    use bdauthor_core::editor::{snap_item_rect, snap_lines, Handle};
    let other = Item::text("A", 600, 300, 200, 100, 30);
    let (xs, ys) = snap_lines(std::iter::once(&other));
    // deplacement : le bord gauche (603) s'aimante sur le bord gauche de l'autre element (600)
    let (r, gx, gy) = snap_item_rect(Handle::Body, (603, 512, 100, 50), &xs, &ys, 6, 16);
    assert_eq!((r.0, gx), (600, Some(600)));
    // centre vertical : 512 + 25 = 537 -> 540 (centre de l'ecran)
    assert_eq!((r.1, gy), (515, Some(540)));
    // trop loin : rien ne bouge
    let (r, gx, gy) = snap_item_rect(Handle::Body, (1100, 700, 100, 50), &xs, &ys, 6, 16);
    assert_eq!((r, gx, gy), ((1100, 700, 100, 50), None, None));
    // redimensionnement par le cote droit : seul le bord droit s'aimante (797 -> 800), la hauteur ne change pas
    let (r, gx, gy) = snap_item_rect(Handle::Right, (500, 700, 297, 53), &xs, &ys, 6, 16);
    assert_eq!((r, gx, gy), ((500, 700, 300, 53), Some(800), None));
    // coin haut gauche : bords gauche et haut
    let (r, _, _) = snap_item_rect(Handle::TopLeft, (958, 4, 100, 100), &xs, &ys, 6, 16);
    assert_eq!(r, (960, 0, 98, 104));
}

#[test]
fn full_screen_item_keeps_grabbable_handles() {
    // un element plein ecran : poignees rentrees dans le plan, saisissables depuis l'interieur
    let items = vec![Item::text("Plein", 0, 0, 1920, 1080, 60)];
    let (l, t, r, b) = handle_edges(&items[0], 6.0);
    assert_eq!((l, t, r, b), (6.0, 6.0, 1914.0, 1074.0));
    assert_eq!(hit_test(&items, 6.0, 6.0, Some(0), 12.0), Some((0, Handle::TopLeft)));
    assert_eq!(hit_test(&items, 1914.0, 1074.0, Some(0), 12.0), Some((0, Handle::BottomRight)));
    assert_eq!(hit_test(&items, 6.0, 540.0, Some(0), 12.0), Some((0, Handle::Left)));
    assert_eq!(hit_test(&items, 960.0, 1074.0, Some(0), 12.0), Some((0, Handle::Bottom)));
    // un element au milieu n'est pas modifie
    let mid = Item::text("M", 100, 100, 400, 200, 40);
    assert_eq!(handle_edges(&mid, 6.0), (100.0, 100.0, 500.0, 300.0));
}

#[test]
fn paste_goes_to_free_spot() {
    // origine libre : on ne bouge pas
    assert_eq!(free_spot(&[(0, 0, 100, 100)], (500, 500, 200, 100), 20), Some((500, 500)));
    // origine occupee : l'emplacement trouve ne recouvre rien et reste dans le plan
    let existing = [(400, 400, 600, 400)];
    let (x, y) = free_spot(&existing, (500, 500, 200, 100), 20).unwrap();
    assert!(x + 200 <= existing[0].0 || x >= existing[0].0 + existing[0].2 || y + 100 <= existing[0].1 || y >= existing[0].1 + existing[0].3);
    assert!(x >= 0 && y >= 0 && x + 200 <= 1920 && y + 100 <= 1080);
    // plan plein : aucun emplacement
    assert_eq!(free_spot(&[(0, 0, 1920, 1080)], (10, 10, 100, 100), 20), None);
    // un element sans surface (ligne) ne gene pas
    assert_eq!(free_spot(&[(0, 500, 1920, 0)], (100, 500, 100, 100), 20), Some((100, 500)));
}
