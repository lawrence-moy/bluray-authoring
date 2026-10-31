//! Calques de l'editeur : regroupement des elements, ordre, glisser-deposer, JSON.
use bdauthor_core::design::*;
use bdauthor_core::editor::*;

fn names(d: &PageDesign) -> Vec<String> {
    d.items.iter().map(|i| match &i.kind { Kind::Text { text, .. } => text.clone(), _ => "?".into() }).collect()
}
fn txt(t: &str) -> Item { Item::text(t, 0, 0, 100, 50, 30) }

#[test]
fn layers_group_and_order_items() {
    let mut d = PageDesign::default();
    for t in ["a", "b", "c"] { d.items.push(txt(t)); }
    // sans calques : liste simple
    assert_eq!(d.layer_range(0), 0..3);
    assert_eq!(to_front(&mut d, 0), 2);
    assert_eq!(names(&d), ["b", "c", "a"]);
    // premier calque : « Calque 1 » recoit tout, le nouveau est au premier plan et vide
    let top = add_layer(&mut d);
    assert_eq!(d.layers.len(), 2);
    assert_eq!((d.layers[0].name.as_str(), d.layers[1].name.as_str()), ("Calque 1", "Calque 2"));
    assert!(d.items.iter().all(|i| i.layer == 1));
    assert_eq!(d.layer_range(1), 3..3);
    // ajout dans le calque du dessus, puis dans celui du dessous (au premier plan de ce calque)
    assert_eq!(insert_item(&mut d, txt("t"), top), 3);
    assert_eq!(insert_item(&mut d, txt("d"), 1), 3);
    assert_eq!(names(&d), ["b", "c", "a", "d", "t"]);
    // les deplacements restent dans le calque
    assert_eq!(to_front(&mut d, 0), 3);
    assert_eq!(names(&d), ["c", "a", "d", "b", "t"]);
    assert_eq!(move_in_list(&mut d, 3, false), 3, "« b » est deja devant tout son calque");
    assert_eq!(to_back(&mut d, 4), 4, "« t » est seul dans son calque");
    // glisser « c » dans le calque du dessus, derriere « t »
    let k = move_item(&mut d, 0, 4, top);
    assert_eq!(names(&d), ["a", "d", "b", "c", "t"]);
    assert_eq!((k, d.items[k].layer), (3, top));
    // glisser « t » au fond du calque 1
    let k = move_item(&mut d, 4, 0, 1);
    assert_eq!(names(&d), ["t", "a", "d", "b", "c"]);
    assert_eq!(k, 0);
    // calque du dessus passe dessous : ses elements suivent
    move_layer(&mut d, 1, 0);
    assert_eq!(names(&d), ["c", "t", "a", "d", "b"]);
    // masquer / verrouiller
    d.layers[0].hidden = true;
    d.layers[1].locked = true;
    assert!(d.item_hidden(0) && !d.item_hidden(1) && d.item_locked(1) && !d.item_locked(0));
    // JSON aller-retour
    let back = PageDesign::from_value(&d.to_value()).unwrap();
    assert_eq!(back, d);
    // suppression d'un calque en gardant ses elements : ils vont au calque voisin, a la meme place a l'ecran
    let id0 = d.layers[0].id;
    remove_layer(&mut d, id0, true);
    assert_eq!(d.layers.len(), 1);
    assert_eq!(names(&d), ["c", "t", "a", "d", "b"]);
    // suppression avec ses elements
    let lid = add_layer(&mut d);
    insert_item(&mut d, txt("x"), lid);
    remove_layer(&mut d, lid, false);
    assert_eq!(names(&d), ["c", "t", "a", "d", "b"]);
    // dernier calque supprime : liste simple
    let id = d.layers[0].id;
    remove_layer(&mut d, id, true);
    assert!(d.layers.is_empty() && d.items.iter().all(|i| i.layer == 0));
    assert!(!d.to_value().to_pretty().contains("calque"));
}

#[test]
fn unknown_layers_are_regrouped() {
    // JSON edite a la main : elements de calques melanges ou inconnus
    let v = bdauthor_core::json::parse(r#"{ "calques": [ {"id": 4, "nom": "Fond"}, {"id": 9, "nom": "Boutons"} ], "elements": [
      {"type":"texte","texte":"b1","x":0,"y":0,"w":10,"h":10,"taille":20,"calque":9},
      {"type":"texte","texte":"f1","x":0,"y":0,"w":10,"h":10,"taille":20,"calque":4},
      {"type":"texte","texte":"z","x":0,"y":0,"w":10,"h":10,"taille":20,"calque":77},
      {"type":"texte","texte":"b2","x":0,"y":0,"w":10,"h":10,"taille":20,"calque":9} ] }"#).unwrap();
    let d = PageDesign::from_value(&v).unwrap();
    assert_eq!(names(&d), ["f1", "z", "b1", "b2"]);
    assert_eq!(d.items[1].layer, 4);
}

#[test]
fn pages_reorder_and_transfers() {
    use bdauthor_core::model::*;
    let mut m = ProjectModel::default();
    m.videos.push(VideoEntry::new("a.mp4"));
    for k in 0..3 {
        let mut p = PageDesign::blank_free(k);
        p.name = format!("P{}", k + 1);
        p.items.push(txt(&format!("t{}", k + 1)));
        m.designs.free.push(p);
    }
    // lien de P1 vers P3, retour d'une vignette de P2 vers P1, accueil = P3
    let mut g = Item::nav(NavAction::Goto, 0, 0, 100, 50);
    if let Kind::Nav { target, .. } = &mut g.kind { *target = Some(Target::Main(2)); }
    m.designs.free[0].items.push(g);
    let mut v = Item::video_slot(0, 0, 100, 80, 60, 0, None, "");
    if let Kind::Slot { ret, .. } = &mut v.kind { *ret = SlotReturn::Page(0); }
    m.designs.free[1].items.push(v);
    m.designs.home = Some(Target::Main(2));
    // P3 en tete : [P3, P1, P2]
    move_page(&mut m, 2, 0);
    let order: Vec<_> = m.designs.free.iter().map(|p| p.name.clone()).collect();
    assert_eq!(order, ["P3", "P1", "P2"]);
    assert_eq!(m.designs.home, Some(Target::Main(0)));
    let goto = m.designs.free[1].items.iter().find_map(|i| match &i.kind { Kind::Nav { action: NavAction::Goto, target, .. } => *target, _ => None });
    assert_eq!(goto, Some(Target::Main(0)));
    let ret = m.designs.free[2].items.iter().find_map(|i| match &i.kind { Kind::Slot { ret, .. } => Some(*ret), _ => None });
    assert_eq!(ret, Some(SlotReturn::Page(1)));
    // retour a l'ordre initial
    move_page(&mut m, 0, 2);
    assert_eq!(m.designs.free.iter().map(|p| p.name.clone()).collect::<Vec<_>>(), ["P1", "P2", "P3"]);
    assert_eq!(m.designs.home, Some(Target::Main(2)));
    // element copie puis deplace vers une autre page
    let d = &mut m.designs;
    let n1 = d.free[1].items.len();
    let k = transfer_item(d, 0, 0, 1, true).unwrap();
    assert_eq!((k, d.free[1].items.len(), d.free[0].items.len()), (n1, n1 + 1, 2));
    let k = transfer_item(d, 0, 0, 2, false).unwrap();
    assert_eq!(d.free[0].items.len(), 1);
    assert_eq!(names(&d.free[2])[k], "t1");
    // calque deplace vers une page sans calques : elle en recoit (« Calque 1 » pour l'existant)
    let lid = add_layer(&mut d.free[0]);
    insert_item(&mut d.free[0], txt("haut"), lid);
    let before = d.free[1].items.len();
    let nid = transfer_layer(d, 0, lid, 1, false).unwrap();
    assert_eq!(d.free[1].layers.len(), 2);
    assert_eq!(d.free[1].layers[1].name, "Calque 2");
    assert_eq!(d.free[1].layers[1].id, nid);
    assert_eq!(names(&d.free[1]).last().unwrap(), "haut");
    assert_eq!(d.free[1].items.len(), before + 1);
    assert_eq!(d.free[0].layers.len(), 1);
    assert!(!names(&d.free[0]).contains(&"haut".to_string()));
}

#[test]
fn subtitle_choice_per_button() {
    use bdauthor_core::mobj::{set_pg_stream, SET_STREAM_INSN};
    let mut d = PageDesign::default();
    let mut b = Item::nav(NavAction::Goto, 0, 0, 100, 50);
    if let Kind::Nav { target, .. } = &mut b.kind { *target = Some(Target::Video(1)); }
    b.sub = SubChoice::Lang(4);
    d.items.push(b);
    let mut t = txt("off");
    if let Kind::Text { link, .. } = &mut t.kind { *link = Some(Target::Stay(None)); }
    t.sub = SubChoice::Off;
    d.items.push(t);
    let back = PageDesign::from_value(&d.to_value()).unwrap();
    assert_eq!(back, d);
    assert_eq!(d.items[0].played_video(), Some(1));
    assert_eq!(d.items[1].played_video(), None);
    // commande : piste 2 affichee, ou affichage coupe sans changer de piste
    let c = set_pg_stream(Some(1));
    assert_eq!((c.insn, c.dst, c.src), (SET_STREAM_INSN, 0xC002, 0));
    assert!(c.text().starts_with("SET_STREAM"), "{}", c.text());
    assert!(c.imm_op1() && c.imm_op2());
    assert_eq!(set_pg_stream(None).dst, 0);
}
