mod common;
use bdauthor_core::json::{parse, Value};
use bdauthor_core::model::*;

const EX: &str = r#"{ "titre": "Mes vidéos", "langue": "fra", "inconnu": {"a": 1},
  "videos": [ {"fichier": "a.mp4", "titre": "Première", "chapitres": [0, 120, {"t": 300, "titre": "Le pont"}], "extra": true},
              {"fichier": "b.mp4", "vignette_s": 3.0, "chapitres_auto_s": 60} ],
  "encodage": {"crf": 20, "preset": "fast", "maxrate": 25000}, "menu": {"fond": "f.png", "fond_secondes": 5, "par_page": 8} }"#;

#[test]
fn roundtrip_preserves_everything() {
    common::fr();
    let m = ProjectModel::from_json(EX).unwrap();
    assert_eq!(m.videos.len(), 2);
    assert_eq!(m.videos[0].chapters.len(), 3);
    assert_eq!(m.videos[0].chapters[2].title, "Le pont");
    assert_eq!((m.crf, m.preset.as_str()), (20, "fast"));
    let out = m.to_json();
    let back = bdauthor_core::json::parse_project(&out).unwrap();
    let orig = bdauthor_core::json::parse_project(EX).unwrap();
    // champs inconnus conserves ; ancien reglage des menus automatiques retire
    assert_eq!(back.get("inconnu"), orig.get("inconnu"));
    assert!(back.get("menu").unwrap().get("par_page").is_none());
    assert_eq!(back.get("videos").unwrap().as_array().unwrap()[0].get("extra"), Some(&Value::Bool(true)));
    assert_eq!(back.get("encodage").unwrap().get("maxrate").unwrap().as_i64(), Some(25000));
    // le modele relu est identique
    // idempotent : relire puis réécrire redonne exactement le même texte
    assert_eq!(ProjectModel::from_json(&out).unwrap().to_json(), out);
}

#[test]
fn edits_and_validation() {
    common::fr();
    let mut m = ProjectModel::from_json(EX).unwrap();
    m.videos[1].title = "Seconde".into();
    m.videos[1].auto_s = None;
    m.videos[1].chapters.push(ChapterEntry { id: 1, t: 10.0, title: String::new() });
    let v = bdauthor_core::json::parse_project(&m.to_json()).unwrap();
    let v1 = &v.get("videos").unwrap().as_array().unwrap()[1];
    assert_eq!(v1.get("titre").unwrap().as_str(), Some("Seconde"));
    assert!(v1.get("chapitres_auto_s").is_none());
    assert_eq!(v1.get("chapitres").unwrap().as_array().unwrap()[0].as_f64(), Some(10.0));
    assert!(v.get("menu").unwrap().get("par_page").is_none());
    let probs = m.validate(std::path::Path::new("/nonexistent"));
    assert!(probs.iter().any(|p| p.contains("introuvable")), "{probs:?}");
    m.lang = "fr".into();
    assert!(m.validate(std::path::Path::new(".")).iter().any(|p| p.contains("langue")));
    assert!(ProjectModel::from_json("{}").is_err());
    assert!(ProjectModel::default().validate(std::path::Path::new(".")).iter().any(|p| p.contains("Aucune")));
}

#[test]
fn json_writer_roundtrip() {
    common::fr();
    let v = parse(r#"{"a":"é\"\n\u0001","b":[1,2.5,[],{}],"c":null}"#).unwrap();
    assert_eq!(parse(&v.to_pretty()).unwrap(), v);
}

#[test]
fn designs_persist_and_follow_videos() {
    common::fr();
    use bdauthor_core::design::*;
    let mut m = ProjectModel::from_json(EX).unwrap();
    let mut p = PageDesign::blank_free(0);
    p.items.push(Item::video_slot(100, 100, 400, 260, 225, 0, None, ""));
    p.items.push(Item::video_slot(600, 100, 400, 260, 225, 1, Some(30.0), "Suite"));
    m.designs.free.push(p);
    let out = m.to_json();
    let back = ProjectModel::from_json(&out).unwrap();
    assert_eq!(back.designs, m.designs);
    assert_eq!(back.to_json(), out);
    let vids = |m: &ProjectModel| -> Vec<Option<usize>> { m.designs.free[0].items.iter().filter_map(|i| i.fixed_video().map(Some).or(if matches!(i.kind, Kind::Slot { .. }) { Some(None) } else { None })).collect() };
    m.swap_videos(0, 1);
    assert_eq!(vids(&m), vec![Some(1), Some(0)]);
    assert_eq!(m.videos[0].file, "b.mp4");
    m.remove_video(1);
    assert_eq!(vids(&m), vec![None, Some(0)]);
}

#[test]
fn chapter_list_matches_builder_rules() {
    common::fr();
    let m = ProjectModel::from_json(EX).unwrap();
    let a = m.videos[0].chapter_list(Some(600.0));
    assert_eq!(a.iter().map(|c| c.0).collect::<Vec<_>>(), vec![0.0, 120.0, 300.0]);
    assert_eq!(a[2].1, "Le pont");
    assert_eq!(a[0].1, "Chapitre 1");
    // automatiques : necessitent la duree ; 200 s toutes les 60 s -> 0, 60, 120 (180 est a moins d'un demi-intervalle de la fin)
    assert!(m.videos[1].chapter_list(None).is_empty());
    assert_eq!(m.videos[1].chapter_list(Some(200.0)).iter().map(|c| c.0).collect::<Vec<_>>(), vec![0.0, 60.0, 120.0]);
    let mut v = VideoEntry::new("x.mp4");
    v.chapters.push(ChapterEntry { id: 1, t: 30.0, title: String::new() });
    assert_eq!(v.chapter_list(Some(100.0)).len(), 2); // 0 insere automatiquement
    assert!(v.chapter_list(Some(20.0)).is_empty());   // 30 s au-dela de la fin : un seul chapitre reste
}
