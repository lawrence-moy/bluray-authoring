//! Habillage du widget Texte : fond (couleur, opacite, arrondi) et contour des lettres, lecture / ecriture JSON et mise a l'echelle.
use bdauthor_core::design::*;

fn text_deco(d: &PageDesign) -> TextDeco {
    match &d.items[0].kind { Kind::Text { deco, .. } => *deco, _ => panic!("texte attendu") }
}

#[test]
fn plain_text_writes_no_deco_fields() {
    let d = PageDesign { items: vec![Item::text("Titre", 100, 90, 800, 110, 60)], ..Default::default() };
    assert!(text_deco(&d).is_plain());
    let j = d.to_value().to_pretty();
    assert!(!j.contains("fond_") && !j.contains("outline"), "{j}");
}

#[test]
fn deco_roundtrip() {
    let mut d = PageDesign { items: vec![Item::text("Titre", 100, 90, 800, 110, 60)], ..Default::default() };
    if let Kind::Text { deco, .. } = &mut d.items[0].kind {
        *deco = TextDeco { bg_color: [10, 20, 30], bg_opacity: 65, bg_radius: 16, outline: 4, outline_color: [255, 0, 128] };
    }
    let v = d.to_value();
    let j = v.to_pretty();
    assert!(j.contains("\"background_color\": \"#0a141e\"") && j.contains("\"background_opacity\": 65") && j.contains("\"background_radius\": 16"), "{j}");
    assert!(j.contains("\"outline\": 4") && j.contains("\"outline_color\": \"#ff0080\""), "{j}");
    let back = PageDesign::from_value(&v).unwrap();
    assert_eq!(text_deco(&back), text_deco(&d));
    // fond seul, sans arrondi : pas de champ inutile
    if let Kind::Text { deco, .. } = &mut d.items[0].kind { deco.outline = 0; deco.bg_radius = 0; }
    let j = d.to_value().to_pretty();
    assert!(j.contains("background_opacity") && !j.contains("background_radius") && !j.contains("outline"), "{j}");
}

#[test]
fn deco_bad_color_and_limits() {
    let mut d = PageDesign { items: vec![Item::text("T", 0, 0, 100, 50, 30)], ..Default::default() };
    if let Kind::Text { deco, .. } = &mut d.items[0].kind { deco.bg_opacity = 50; }
    let mut v = d.to_value();
    let s = v.to_pretty().replace("\"#000000\"", "\"rouge\"");
    assert!(PageDesign::from_value(&bdauthor_core::json::parse(&s).unwrap()).is_err(), "couleur invalide refusee");
    // valeurs hors bornes ramenees dans les limites
    let s = d.to_value().to_pretty().replace("\"background_opacity\": 50", "\"background_opacity\": 250");
    v = bdauthor_core::json::parse(&s).unwrap();
    assert_eq!(text_deco(&PageDesign::from_value(&v).unwrap()).bg_opacity, 100);
}

#[test]
fn deco_scaling() {
    let t = TextDeco { bg_color: [0, 0, 0], bg_opacity: 40, bg_radius: 20, outline: 3, outline_color: [0, 0, 0] };
    let s = t.scaled(0.5);
    assert_eq!((s.bg_radius, s.outline, s.bg_opacity), (10, 2, 40));
    // un contour fin reste visible a petite echelle
    assert_eq!(TextDeco { outline: 1, ..t }.scaled(0.25).outline, 1);
    assert_eq!(TextDeco::default().scaled(2.0), TextDeco::default());
}
