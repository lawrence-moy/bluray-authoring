//! Modification groupee dans l'inspecteur : ce qui change sur l'element principal passe aux autres elements du meme type, sauf
//! ce qui leur est propre (position, nom, contenu, destination).
use bdauthor_core::design::*;
use bdauthor_core::style::ButtonStyle;

#[test]
fn text_changes_go_to_texts_only() {
    let mut d = PageDesign::default();
    d.items.push(Item::text("Un", 100, 100, 400, 60, 40));
    d.items.push(Item::text("Deux", 100, 200, 300, 60, 40));
    d.items.push(Item::text("Trois", 100, 300, 300, 60, 40));
    d.items.push(Item::shape(bdauthor_core::cover::ShapeKind::Rect, 0, 0, 50, 50));
    d.items[2].name = "titre".into();
    let before = d.items[0].clone();
    // l'inspecteur change la couleur, la taille du texte et la largeur du premier
    if let Kind::Text { color, size, .. } = &mut d.items[0].kind { *color = [255, 0, 0]; *size = 52; }
    d.items[0].w = 500;
    let shape_before = d.items[3].clone();
    assert_eq!(apply_to_like(&mut d, 0, &before, &[0, 1, 2, 3]), 2);
    for (k, text, y) in [(1, "Deux", 200), (2, "Trois", 300)] {
        let it = &d.items[k];
        let Kind::Text { color, size, text: t, .. } = &it.kind else { panic!() };
        assert_eq!((*color, *size, it.w), ([255, 0, 0], 52, 500), "element {k}");
        // le contenu et la position restent les leurs
        assert_eq!((t.as_str(), it.x, it.y), (text, 100, y));
    }
    assert_eq!(d.items[2].name, "titre");
    assert_eq!(d.items[3], shape_before, "une forme n'est pas un texte");
    // rien de change : rien a reporter
    let same = d.items[0].clone();
    assert_eq!(apply_to_like(&mut d, 0, &same, &[0, 1, 2]), 0);
}

#[test]
fn a_style_field_changes_alone() {
    let mut d = PageDesign::default();
    for k in 0..3 { d.items.push(Item::video_slot(100 + 400 * k, 300, 360, 260, 200, k as usize, None, &format!("Video {k}"))); }
    // chaque vignette a son propre style, de couleurs differentes
    for (k, it) in d.items.iter_mut().enumerate() {
        it.look.style = Some(ButtonStyle { normal_bg: [10 * k as u8, 0, 0], radius: 4, ..Default::default() });
    }
    let before = d.items[0].clone();
    d.items[0].look.style.as_mut().unwrap().radius = 20;
    assert_eq!(apply_to_like(&mut d, 0, &before, &[0, 1, 2]), 2);
    for k in 1..3 {
        let st = d.items[k].look.style.as_ref().unwrap();
        // l'arrondi passe, la couleur de fond de chacune reste
        assert_eq!((st.radius, st.normal_bg), (20, [10 * k as u8, 0, 0]));
        // chaque vignette garde sa video et sa legende
        assert_eq!(d.items[k].fixed_video(), Some(k));
        assert!(matches!(&d.items[k].kind, Kind::Slot { label, .. } if label == &format!("Video {k}")));
    }
    // style propre retire sur la principale (retour au style du projet) : retire aussi chez les autres
    let before = d.items[0].clone();
    d.items[0].look.style = None;
    assert_eq!(apply_to_like(&mut d, 0, &before, &[0, 1, 2]), 2);
    assert!(d.items.iter().all(|it| it.look.style.is_none()));
}

#[test]
fn cover_items_change_together() {
    use bdauthor_core::cover::{apply_to_like, CoverItem, CoverKind};
    let mut items = vec![
        CoverItem::text("Titre", 10.0, 10.0, 80.0, 20.0, 28.0),
        CoverItem::text("Sous-titre", 10.0, 40.0, 80.0, 20.0, 18.0),
        CoverItem::image("photo.jpg", 0.0, 0.0, 50.0, 50.0),
    ];
    let before = items[0].clone();
    if let CoverKind::Text { color, fx, .. } = &mut items[0].kind { *color = [0, 0, 255]; fx.bold = true; }
    items[0].rot = 90;
    let img = items[2].clone();
    assert_eq!(apply_to_like(&mut items, 0, &before, &[0, 1, 2]), 1);
    let CoverKind::Text { color, fx, text, size_pt, .. } = &items[1].kind else { panic!() };
    assert_eq!((*color, fx.bold, items[1].rot), ([0, 0, 255], true, 90));
    // sa taille de texte n'a pas ete touchee (seuls les reglages changes passent), ni son texte ni sa position
    assert_eq!((text.as_str(), *size_pt, items[1].y), ("Sous-titre", 18.0, 40.0));
    assert_eq!(items[2], img);
}
