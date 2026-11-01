use bdauthor_core::design::*;
use bdauthor_core::editor::*;
use bdauthor_core::style::ButtonStyle;

fn page() -> PageDesign {
    let mut items = Vec::new();
    for k in 0..4 { items.push(Item::video_slot(100 + 420 * k as i32, 300, 400, 260, 225, k, None, "")); }
    items.push(Item::nav(NavAction::Next, 1000, 940, 360, 80));
    PageDesign { items, ..Default::default() }
}

#[test]
fn manual_neighbors_override_automatic_ones() {
    let info = PageInfo { r: PageRef(0), page: 0, pages: 2 };
    let st = ButtonStyle::default();
    let mut d = page();
    let auto = sim_buttons(&info, &d, &st, 4);
    assert_eq!(auto[0].neighbors.3, 1);
    // vignette 1 -> droite : la 4e ; vignette 2 -> gauche : aucun
    let id3 = d.ensure_id(3);
    assert_eq!(id3, 1);
    assert_eq!(d.ensure_id(3), 1, "identifiant stable");
    d.items[0].nav.set(3, NavLink::Item(id3));
    d.items[1].nav.set(2, NavLink::Stay);
    let b = sim_buttons(&info, &d, &st, 4);
    assert_eq!(b[0].neighbors.3, 3);
    assert_eq!(b[1].neighbors.2, 1);
    assert_eq!(b[0].neighbors.0, auto[0].neighbors.0, "les autres fleches restent automatiques");
    // lien vers un element disparu : automatique + avertissement
    d.items[0].nav.set(3, NavLink::Item(99));
    assert_eq!(sim_buttons(&info, &d, &st, 4)[0].neighbors.3, 1);
    let probs = d.validate(0, 2, 4);
    assert!(probs.iter().any(|p| p.msg.contains("supprimé")), "{probs:?}");
    d.prune_nav();
    assert_eq!(d.items[0].nav, NavLinks::default());
}

#[test]
fn json_roundtrip_and_duplicates() {
    let mut d = page();
    let id = d.ensure_id(2);
    d.items[0].nav.set(3, NavLink::Item(id));
    d.items[1].nav.set(1, NavLink::Stay);
    let v = d.to_value();
    let back = PageDesign::from_value(&v).unwrap();
    assert_eq!(back.items[2].id, id);
    assert_eq!(back.items[0].nav.right, NavLink::Item(id));
    assert_eq!(back.items[1].nav.down, NavLink::Stay);
    assert!(back.items[3].nav.is_auto());
    // doublons d'identifiants (fichier modifie a la main) : le second est remis a zero
    let mut e = page();
    e.items[0].id = 5;
    e.items[1].id = 5;
    e.normalize_ids();
    assert_eq!((e.items[0].id, e.items[1].id), (5, 0));
}

#[test]
fn moving_an_item_to_another_page_drops_its_links() {
    let mut ds = Designs::default();
    ds.free = vec![page(), page()];
    let id = ds.free[0].ensure_id(1);
    ds.free[0].items[0].nav.set(3, NavLink::Item(id));
    let k = transfer_item(&mut ds, 0, 1, 1, false).unwrap();
    assert_eq!(ds.free[1].items[k].id, 0);
    assert_eq!(ds.free[0].items[0].nav, NavLinks::default(), "le lien vers l'element parti est efface");
}

#[test]
fn item_name_roundtrip() {
    let mut d = page();
    d.items[1].name = "Bouton « Film 2 »".into();
    let back = PageDesign::from_value(&d.to_value()).unwrap();
    assert_eq!(back.items[1].name, "Bouton « Film 2 »");
    assert!(back.items[0].name.is_empty());
}
