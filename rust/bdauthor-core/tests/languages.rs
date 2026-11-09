//! Langues de sous-titres du projet : liste, rattachement des pistes, conversion des anciens projets, validation.
mod common;
use bdauthor_core::design::*;
use bdauthor_core::model::*;

const PROJECT: &str = r##"{ "title": "T", "language": "fra",
  "languages": [ {"id":1,"name":"Français","code":"fra"}, {"id":2,"name":"Anglais","code":"eng"}, {"id":3,"name":"Allemand","code":"deu"} ],
  "videos": [ {"file": "a.mp4", "subtitles": [ {"file":"f.srt","language":"fra","language_id":1}, {"file":"e.srt","language":"eng","language_id":2} ]},
              {"file": "b.mp4", "subtitles": [ {"file":"e.srt","language":"eng","language_id":2}, {"file":"f.srt","language":"fra","language_id":1}, {"file":"d.srt","language":"deu","language_id":3} ]} ],
  "layout": { "pages": [ { "name": "P", "items": [
     {"type":"texte","text":"EN","x":0,"y":0,"w":300,"h":80,"size":40,"target":{"stay":true},"subtitles":{"language":2}} ] } ] } }"##;

#[test]
fn validation_of_languages() {
    common::fr();
    let mut m = ProjectModel::from_json(PROJECT).unwrap();
    assert_eq!(m.designs.free[0].items[0].sub, SubChoice::Lang(2));
    // enregistrement et relecture
    let back = ProjectModel::from_json(&m.to_json()).unwrap();
    assert_eq!(back.languages, m.languages);
    assert_eq!(back.videos[1].subs[2].lang_id, 3);
    let probs = |m: &ProjectModel| -> Vec<String> { m.validate(std::path::Path::new("/nonexistent")).into_iter().filter(|p| !p.contains("introuvable")).collect() };
    assert!(probs(&m).is_empty(), "{:?}", probs(&m));
    // deux pistes de la meme langue dans une video
    m.videos[0].subs[1].lang_id = 1;
    assert!(probs(&m).iter().any(|p| p.contains("déjà utilisée")), "{:?}", probs(&m));
    m.videos[0].subs[1].lang_id = 2;
    // piste sans langue
    m.videos[1].subs[2].lang_id = 0;
    assert!(probs(&m).iter().any(|p| p.contains("langue à choisir")), "{:?}", probs(&m));
    m.videos[1].subs[2].lang_id = 3;
    // retrait de la langue anglaise : pistes sans langue, bouton retire
    m.remove_language(2);
    assert_eq!(m.designs.free[0].items[0].sub, SubChoice::Keep);
    assert_eq!(m.videos[0].subs[1].lang_id, 0);
    assert!(probs(&m).iter().any(|p| p.contains("langue à choisir")));
    // un bouton qui regle une langue que personne n'a
    let id = m.add_language("Japonais", "jpn");
    m.videos[0].subs[1].lang_id = 1; // evite le doublon d'avant
    m.videos[0].subs.pop();
    m.videos[1].subs.remove(0);
    m.designs.free[0].items[0].sub = SubChoice::Lang(id);
    assert!(probs(&m).iter().any(|p| p.contains("aucune vidéo n'a de sous-titres en « Japonais »")), "{:?}", probs(&m));
    // ordre des langues = numeros de piste
    m.move_language(2, 0);
    assert_eq!(m.language_rank(id), Some(0));
}

#[test]
fn language_choice_json() {
    common::fr();
    let t = r#"{"videos":[{"file":"a"}],"languages":[{"id":4,"name":"Anglais","code":"eng"}],
      "layout":{"pages":[{"name":"P","items":[{"type":"texte","text":"x","x":0,"y":0,"w":10,"h":10,"size":40,"target":{"stay":true},"subtitles":{"language":4}}]}]}}"#;
    let m = ProjectModel::from_json(t).unwrap();
    assert_eq!(m.designs.free[0].items[0].sub, SubChoice::Lang(4));
    assert!(m.to_json().contains("\"language\": 4"));
    assert!(ProjectModel::from_json(r#"{"videos":[{"file":"a"}],"languages":[{"id":1,"code":"fra"},{"id":1,"code":"eng"}]}"#).is_err());
}

#[test]
fn audio_language_action() {
    common::fr();
    // texte cliquable (pas seulement un bouton) qui regle la langue audio 4
    let t = r#"{"videos":[{"file":"a","audio_tracks":[{"track":1,"language":"eng","language_id":4}]}],"languages":[{"id":4,"name":"Anglais","code":"eng"}],
      "layout":{"pages":[{"name":"P","items":[{"type":"texte","text":"x","x":0,"y":0,"w":10,"h":10,"size":40,"target":{"audio_language":4}}]}]}}"#;
    let mut m = ProjectModel::from_json(t).unwrap();
    let link = |m: &ProjectModel| match &m.designs.free[0].items[0].kind { Kind::Text { link, .. } => *link, _ => None };
    assert_eq!(link(&m), Some(Target::Audio(4, None)));
    assert_eq!(m.designs.free[0].items[0].sub, SubChoice::Keep);
    assert!(m.to_json().contains("\"audio_language\": 4"));
    let probs = |m: &ProjectModel| -> Vec<String> { m.validate(std::path::Path::new("/nonexistent")).into_iter().filter(|p| !p.contains("introuvable")).collect() };
    assert!(!probs(&m).iter().any(|p| p.contains("Élément cliquable")), "{:?}", probs(&m));
    // plus aucune video avec cette langue : avertissement
    m.videos[0].audio[0].lang_id = 0;
    assert!(probs(&m).iter().any(|p| p.contains("aucune vidéo n'a de piste audio en « Anglais »")), "{:?}", probs(&m));
    // retrait de la langue : la destination est a rechoisir
    m.remove_language(4);
    assert_eq!(link(&m), None);
    // commande du lecteur : SET_STREAM, audio seulement (bit 31), sous-titres intacts (bits 15 et 14 a 0)
    let c = bdauthor_core::mobj::set_audio_stream(1);
    assert_eq!(c.dst, 0x8002_0000);
}

#[test]
fn setting_then_exit_page() {
    common::fr();
    // reglage des sous-titres puis page 2 ; reglage audio puis page 3 ; JSON, renumerotation et suppression de page
    let t = r#"{"videos":[{"file":"a"}],"languages":[{"id":4,"name":"Anglais","code":"eng"}],
      "layout":{"pages":[
        {"name":"P1","items":[
          {"type":"texte","text":"st","x":0,"y":0,"w":10,"h":10,"size":40,"target":{"stay":true,"then":2},"subtitles":{"language":4}},
          {"type":"texte","text":"au","x":0,"y":20,"w":10,"h":10,"size":40,"target":{"audio_language":4,"then":3}},
          {"type":"texte","text":"ici","x":0,"y":40,"w":10,"h":10,"size":40,"target":{"stay":true},"subtitles":"aucun"}]},
        {"name":"P2","items":[]},{"name":"P3","items":[]}]}}"#;
    let mut m = ProjectModel::from_json(t).unwrap();
    let links = |m: &ProjectModel| -> Vec<Option<Target>> { m.designs.free[0].items.iter().map(|it| it.link()).collect() };
    assert_eq!(links(&m), [Some(Target::Stay(Some(1))), Some(Target::Audio(4, Some(2))), Some(Target::Stay(None))]);
    let back = ProjectModel::from_json(&m.to_json()).unwrap();
    assert_eq!(links(&back), links(&m));
    assert!(m.to_json().contains("\"then\": 2"));
    assert_eq!(Target::Audio(4, Some(2)).without_exit(), Target::Audio(4, None));
    // echange des pages 2 et 3 : les sorties suivent
    bdauthor_core::editor::swap_pages(&mut m, 1, 2);
    assert_eq!(links(&m)[..2], [Some(Target::Stay(Some(2))), Some(Target::Audio(4, Some(1)))]);
    // suppression de la page 2 (ex-P3) : le reglage qui y sortait reste sur place, l'autre est renumerote
    m.designs.page_removed(2);
    assert_eq!(links(&m)[..2], [Some(Target::Stay(None)), Some(Target::Audio(4, Some(1)))]);
}
