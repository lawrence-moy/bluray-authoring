//! Chapitres d'une video : identifiants stables et vignettes liees a un chapitre.
use bdauthor_core::design::*;
use bdauthor_core::editor::*;
use bdauthor_core::model::*;
use std::collections::HashMap;

fn slot_of(m: &ProjectModel, k: usize) -> (Option<usize>, Option<f64>, Option<u32>) {
    let it = m.designs.free[0].items.iter().filter(|i| matches!(i.kind, Kind::Slot { .. })).nth(k).unwrap();
    match &it.kind { Kind::Slot { video, start, chapter, .. } => (*video, *start, *chapter), _ => unreachable!() }
}

fn model() -> ProjectModel {
    let mut m = ProjectModel::default();
    for i in 0..2 { m.videos.push(VideoEntry::new(&format!("v{i}.mp4"))); }
    m.designs.free.push(PageDesign::blank_free(0));
    m
}

#[test]
fn link_follow_and_unlink() {
    let mut m = model();
    let a = m.videos[0].add_chapter(60.0, "Scene 2");
    let b = m.videos[0].add_chapter(120.0, "");
    assert_eq!((a, b), (1, 2));
    // un chapitre existant a la meme seconde est reutilise
    assert_eq!(m.videos[0].add_chapter(60.005, "autre"), a);
    assert_eq!(m.videos[0].chapters.len(), 2);
    // vignette de chapitre liee, puis instant libre sur l'autre video
    m.designs.free[0].items.push(Item::video_slot(0, 0, 400, 300, 240, 0, Some(60.0), ""));
    m.designs.free[0].items.push(Item::video_slot(500, 0, 400, 300, 240, 0, Some(120.0), ""));
    m.designs.free[0].items.push(Item::video_slot(1000, 0, 400, 300, 240, 1, Some(5.0), ""));
    for k in 0..2 { if let Kind::Slot { chapter, .. } = &mut m.designs.free[0].items[k].kind { *chapter = Some(k as u32 + 1); } }
    assert_eq!(m.chapter_uses(0, a), 1);
    // l'instant du chapitre change : la vignette suit, l'autre non
    m.videos[0].chapters[0].t = 75.5;
    assert!(m.sync_chapters());
    assert!(!m.sync_chapters(), "rien a changer ensuite");
    assert_eq!(slot_of(&m, 0), (Some(0), Some(75.5), Some(a)));
    assert_eq!(slot_of(&m, 1), (Some(0), Some(120.0), Some(b)));
    assert_eq!(slot_of(&m, 2), (Some(1), Some(5.0), None));
    // les identifiants ne sont pas des rangs : retirer le premier chapitre ne change pas le second
    m.videos[0].chapters.remove(0);
    assert!(m.sync_chapters());
    assert_eq!(slot_of(&m, 0), (Some(0), Some(75.5), None), "chapitre retire : la vignette garde son instant, sans lien");
    assert_eq!(slot_of(&m, 1), (Some(0), Some(120.0), Some(b)));
    m.videos[0].chapters[0].t = 130.0;
    m.sync_chapters();
    assert_eq!(slot_of(&m, 1).1, Some(130.0));
    // changer la video d'une vignette la delie (les identifiants sont propres a chaque video : l'identifiant 2 de la video 1
    // serait un autre chapitre) ; l'interface le fait au moment du choix de la video, le modele ne peut pas le deviner
    m.videos[1].add_chapter(9.0, "");
    m.videos[1].add_chapter(19.0, "");
    if let Kind::Slot { video, chapter, .. } = &mut m.designs.free[0].items[1].kind { *video = Some(1); *chapter = None; }
    m.sync_chapters();
    assert_eq!(slot_of(&m, 1), (Some(1), Some(130.0), None), "garde son instant, libre");
}

#[test]
fn saved_with_ids_only_when_linked() {
    let mut m = model();
    m.videos[0].add_chapter(10.0, "");
    let used = m.videos[0].add_chapter(20.0, "Fin");
    m.videos[0].add_chapter(30.0, "Titre seul");
    m.designs.free[0].items.push(Item::video_slot(0, 0, 400, 300, 240, 0, Some(20.0), ""));
    if let Kind::Slot { chapter, .. } = &mut m.designs.free[0].items[0].kind { *chapter = Some(used); }
    let j = m.to_json();
    let v = bdauthor_core::json::parse_project(&j).unwrap();
    let ch = v.get("videos").and_then(|x| x.as_array()).unwrap()[0].get("chapitres").and_then(|x| x.as_array()).unwrap().to_vec();
    assert_eq!(ch[0].as_f64(), Some(10.0), "chapitre simple : un nombre");
    assert_eq!(ch[1].get("id").and_then(|x| x.as_f64()), Some(used as f64), "chapitre lie : son identifiant est ecrit");
    assert!(ch[2].get("id").is_none() && ch[2].get("titre").is_some());
    // rechargement : le lien tient, meme si on deplace le chapitre dans le fichier (instant de la vignette recalcule)
    let back = ProjectModel::from_json(&j).unwrap();
    assert_eq!(slot_of(&back, 0), (Some(0), Some(20.0), Some(used)));
    assert_eq!(back.to_json(), j, "aller-retour identique");
    let moved = j.replace("\"t\": 20", "\"t\": 25");
    assert_ne!(moved, j);
    let back = ProjectModel::from_json(&moved).unwrap();
    assert_eq!(slot_of(&back, 0).1, Some(25.0));
    // fichier sans identifiants (ancien projet) : ids attribues, vignettes non liees
    let old = r#"{"videos": [{"fichier": "a.mp4", "chapitres": [5, {"t": 9, "titre": "x"}]}],
                  "mise_en_page": {"pages": [{"nom": "p", "elements": [{"type": "vignette", "x": 0, "y": 0, "w": 300, "h": 200, "vignette_h": 150, "video": 1, "depart_s": 9}]}]}}"#;
    let o = ProjectModel::from_json(old).unwrap();
    assert_eq!(o.videos[0].chapters.iter().map(|c| c.id).collect::<Vec<_>>(), vec![1, 2]);
    assert_eq!(slot_of(&o, 0).2, None);
}

#[test]
fn starter_pages_link_their_chapters() {
    let mut m = ProjectModel::default();
    m.videos.push(VideoEntry::new("a.mp4"));
    m.videos[0].add_chapter(0.0, "Debut");
    let b = m.videos[0].add_chapter(30.0, "Suite");
    assert!(ensure_pages(&mut m, &HashMap::new()).unwrap());
    let linked: Vec<(Option<f64>, Option<u32>)> = m.designs.free.iter().flat_map(|p| p.items.iter()).filter_map(|i| match i.kind {
        Kind::Slot { start: Some(t), chapter, .. } => Some((Some(t), chapter)), _ => None }).collect();
    assert_eq!(linked, vec![(Some(0.0), Some(1)), (Some(30.0), Some(b))]);
}
