use bdauthor_core::cover::{Cover, CoverItem, CoverKind};
use bdauthor_core::rich::*;
use std::collections::BTreeMap;

fn bold(start: usize, end: usize) -> TextSpan { TextSpan { start, end, style: SpanStyle { bold: Some(true), ..Default::default() } } }

#[test]
fn apply_merges_and_clears() {
    let s = apply(&[], 10, 2..5, |c| c.bold = Some(true));
    assert_eq!(s, vec![bold(2, 5)]);
    // une portion voisine de meme reglage est fusionnee
    let s = apply(&s, 10, 5..7, |c| c.bold = Some(true));
    assert_eq!(s, vec![bold(2, 7)]);
    // couleur sur une partie : la portion est coupee en trois
    let s = apply(&s, 10, 3..4, |c| c.color = Some([1, 2, 3]));
    assert_eq!(s.len(), 3);
    assert_eq!(common(&s, 10, 2..7), None);
    assert_eq!(common(&s, 10, 4..7), Some(SpanStyle { bold: Some(true), ..Default::default() }));
    // effacer la mise en forme
    assert!(apply(&s, 10, 0..10, |c| *c = SpanStyle::default()).is_empty());
}

#[test]
fn spans_follow_text_edits() {
    let s = vec![bold(6, 10)]; // « Mon <film> super »
    // texte tape avant : la portion se decale
    assert_eq!(after_edit(&s, "Mon : film super", "Le Mon : film super"), vec![bold(9, 13)]);
    // texte tape a la fin de la portion : il prend son reglage
    assert_eq!(after_edit(&s, "Mon : film super", "Mon : films super"), vec![bold(6, 11)]);
    // suppression dans la portion
    assert_eq!(after_edit(&s, "Mon : film super", "Mon : fm super"), vec![bold(6, 8)]);
    // remplacement de tout le texte
    assert_eq!(after_edit(&s, "Mon : film super", "x"), vec![]);
    // portions hors du texte : ignorees
    assert_eq!(normalize(&[bold(3, 50)], 5), vec![bold(3, 5)]);
}

#[test]
fn fields_are_replaced_inside_spans() {
    let mut vars = BTreeMap::new();
    vars.insert("title".to_string(), "Le Grand Film".to_string());
    // « Titre : {title} » avec {title} en gras
    let (t, s) = substitute("Titre : {title}", &[bold(8, 15)], &vars);
    assert_eq!(t, "Titre : Le Grand Film");
    assert_eq!(s, vec![bold(8, 21)]);
    // sans mise en forme : meme resultat que le remplacement simple
    assert_eq!(substitute("{title} !", &[], &vars).0, "Le Grand Film !");
}

#[test]
fn spans_round_trip_in_json_and_reach_the_layout() {
    let mut c = Cover::default();
    let mut t = CoverItem::text("Un deux trois", 10.0, 10.0, 100.0, 20.0, 20.0);
    if let CoverKind::Text { fx, .. } = &mut t.kind {
        fx.spans = vec![bold(3, 7), TextSpan { start: 8, end: 13, style: SpanStyle { italic: Some(true), color: Some([255, 0, 0]), bold: Some(false), size: None } }];
    }
    c.items.push(t);
    let back = Cover::from_value(c.to_value().as_ref()).unwrap();
    assert_eq!(back, c);
    match &c.layout(100, std::path::Path::new(".")).items[0].0 {
        bdauthor_core::cover::CoverDecor::Text { spans, .. } => assert_eq!(spans.len(), 2),
        _ => panic!(),
    }
    // l'edition groupee ne recopie pas la mise en forme d'un texte sur un autre (elle depend du texte)
    let mut items = vec![c.items[0].clone(), CoverItem::text("Autre", 0.0, 0.0, 10.0, 10.0, 12.0)];
    let before = { let mut b = items[0].clone(); if let CoverKind::Text { fx, .. } = &mut b.kind { fx.spans.clear(); } b };
    bdauthor_core::cover::apply_to_like(&mut items, 0, &before, &[1]);
    match &items[1].kind { CoverKind::Text { fx, .. } => assert!(fx.spans.is_empty()), _ => panic!() }
}

#[test]
fn substitute_map_tracks_positions_and_fields() {
    let mut vars = BTreeMap::new();
    vars.insert("titre".to_string(), "Film".to_string());
    // « Le {titre} ! » : le champ (positions 3 a 10) devient « Film »
    let s = substitute_map("Le {titre} !", &[], &vars);
    assert_eq!(s.text, "Le Film !");
    assert_eq!(s.fields, vec![3..10]);
    assert_eq!(s.disp[3], 3);
    assert_eq!(s.disp[10], 7);
    assert_eq!(*s.disp.last().unwrap(), 9);
    // champ inconnu : reste tel quel ; une valeur n'est pas relue
    vars.insert("a".to_string(), "{titre}".to_string());
    assert_eq!(substitute_map("{a} {zz}", &[], &vars).text, "{titre} {zz}");
    // un champ coupe par un changement de mise en forme est remplace, avec le reglage de son accolade
    let sp = [TextSpan { start: 3, end: 6, style: SpanStyle { bold: Some(true), ..Default::default() } }];
    let s = substitute_map("Le {titre}", &sp, &vars);
    assert_eq!(s.text, "Le Film");
    assert_eq!(s.spans, vec![TextSpan { start: 3, end: 7, style: SpanStyle { bold: Some(true), ..Default::default() } }]);
}

#[test]
fn span_size_round_trips_and_becomes_a_factor_in_the_layout() {
    let mut c = Cover::default();
    let mut t = CoverItem::text("petit GRAND", 10.0, 10.0, 100.0, 20.0, 20.0);
    if let CoverKind::Text { fx, .. } = &mut t.kind { fx.spans = apply(&[], 11, 6..11, |s| s.size = Some(30.0)); }
    c.items.push(t);
    let back = Cover::from_value(c.to_value().as_ref()).unwrap();
    assert_eq!(back, c);
    match &c.layout(100, std::path::Path::new(".")).items[0].0 {
        bdauthor_core::cover::CoverDecor::Text { spans, .. } => assert_eq!(spans[0].style.size, Some(1.5)),
        _ => panic!(),
    }
}
