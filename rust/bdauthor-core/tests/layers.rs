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
