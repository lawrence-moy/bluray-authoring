//! Widgets Rectangle et Ligne de l'editeur de menu : valeurs par defaut, JSON, mise a l'echelle, pas de lien.
use bdauthor_core::cover::{LineArrow, LineStyle, ShapeKind};
use bdauthor_core::design::*;

fn paint(d: &PageDesign, i: usize) -> (ShapeKind, Paint) {
    match &d.items[i].kind { Kind::Shape { shape, paint } => (*shape, *paint), _ => panic!("forme attendue") }
}

#[test]
fn defaults_and_no_link() {
    let r = Item::shape(ShapeKind::Rect, 100, 100, 400, 200);
    let l = Item::shape(ShapeKind::Line, 100, 100, 400, 0);
    let (pr, pl) = (Paint::new(ShapeKind::Rect), Paint::new(ShapeKind::Line));
    assert_eq!((pr.fill, pr.stroke), (Some([255, 255, 255]), None));
    assert_eq!((pl.fill, pl.stroke, pl.line_color()), (None, Some([255, 255, 255]), Some([255, 255, 255])));
    assert!(r.link().is_none() && !r.is_button() && !l.is_button());
    // une forme ne compte pas dans les chevauchements
    let d = PageDesign { items: vec![r, Item::shape(ShapeKind::Rect, 150, 150, 400, 200)], ..Default::default() };
    assert!(d.overlapping(0, 1).is_empty());
}

#[test]
fn json_roundtrip() {
    let mut d = PageDesign { items: vec![Item::shape(ShapeKind::Rect, 10, 20, 300, 120), Item::shape(ShapeKind::Line, 0, 0, 500, 80)], ..Default::default() };
    if let Kind::Shape { paint, .. } = &mut d.items[0].kind { *paint = Paint { fill: Some([10, 20, 30]), stroke: Some([255, 0, 128]), stroke_px: 6, radius: 24, ..*paint }; }
    if let Kind::Shape { paint, .. } = &mut d.items[1].kind { *paint = Paint { flip: true, style: LineStyle::Dashed, arrow: LineArrow::End, stroke_px: 9, ..*paint }; }
    let v = d.to_value();
    let j = v.to_pretty();
    assert!(j.contains("\"type\": \"forme\"") && j.contains("\"forme\": \"rectangle\"") && j.contains("\"forme\": \"ligne\""), "{j}");
    assert!(j.contains("\"remplissage\": \"#0a141e\"") && j.contains("\"bordure\": \"#ff0080\"") && j.contains("\"arrondi\": 24"), "{j}");
    assert!(j.contains("\"retourne\": true") && j.contains("\"trait\": \"tirets\"") && j.contains("\"fleche\": \"fin\""), "{j}");
    let back = PageDesign::from_value(&v).unwrap();
    assert_eq!(paint(&back, 0), paint(&d, 0));
    assert_eq!(paint(&back, 1), paint(&d, 1));
    // couleur invalide refusee
    let bad = j.replace("\"#0a141e\"", "\"rouge\"");
    assert!(PageDesign::from_value(&bdauthor_core::json::parse(&bad).unwrap()).is_err());
}

#[test]
fn scaling_keeps_a_visible_stroke() {
    let p = Paint { stroke_px: 4, radius: 20, ..Paint::new(ShapeKind::Rect) };
    let s = p.scaled(0.5);
    assert_eq!((s.stroke_px, s.radius), (2, 10));
    assert_eq!(p.scaled(0.1).stroke_px, 1);
    assert_eq!(Paint::new(ShapeKind::Rect).scaled(0.25).radius, 0);
}

#[test]
fn thin_line_can_be_picked() {
    use bdauthor_core::editor::{hit_test, Handle};
    let items = vec![Item::shape(ShapeKind::Line, 100, 500, 600, 0)];
    assert_eq!(hit_test(&items, 300.0, 503.0, None, 12.0), Some((0, Handle::Body)));
    assert_eq!(hit_test(&items, 300.0, 515.0, None, 12.0), None);
}
