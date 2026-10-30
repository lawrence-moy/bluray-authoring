//! Langues de sous-titres du projet : liste, rattachement des pistes, conversion des anciens projets, validation.
use bdauthor_core::design::*;
use bdauthor_core::model::*;

const LEGACY: &str = r##"{ "titre": "T", "langue": "fra",
  "videos": [ {"fichier": "a.mp4", "soustitres": [ {"fichier":"f.srt","langue":"fra"}, {"fichier":"e.srt","langue":"eng"} ]},
              {"fichier": "b.mp4", "soustitres": [ {"fichier":"e.srt","langue":"eng"}, {"fichier":"f.srt","langue":"fra"}, {"fichier":"d.srt","langue":"deu"} ]} ],
  "mise_en_page": { "pages": [ { "nom": "P", "elements": [
     {"type":"texte","texte":"EN","x":0,"y":0,"w":300,"h":80,"taille":40,"cible":{"rester":true},"soustitres":2} ] } ] } }"##;

#[test]
fn legacy_project_gets_languages() {
    let mut m = ProjectModel::from_json(LEGACY).unwrap();
    assert!(m.languages.is_empty());
    m.ensure_languages();
    // une langue par code, dans l'ordre de rencontre
    let codes: Vec<&str> = m.languages.iter().map(|l| l.code.as_str()).collect();
    assert_eq!(codes, ["fra", "eng", "deu"]);
    assert_eq!(m.languages[0].name, "Français");
    // chaque piste est rattachee a la langue de son code
    assert_eq!(m.videos[1].subs.iter().map(|t| t.lang_id).collect::<Vec<_>>(), [2, 1, 3]);
    // le bouton « piste 2 » (anglais dans la premiere video) devient « anglais »
    let it = &m.designs.free[0].items[0];
    assert_eq!(it.sub, SubChoice::Lang(2));
    // sans effet la deuxieme fois
    let again = m.clone();
    m.ensure_languages();
    assert_eq!(m, again);
    // enregistrement et relecture
    let back = ProjectModel::from_json(&m.to_json()).unwrap();
    assert_eq!(back.languages, m.languages);
    assert_eq!(back.videos[1].subs[2].lang_id, 3);
    assert_eq!(back.designs.free[0].items[0].sub, SubChoice::Lang(2));
}

#[test]
fn validation_of_languages() {
    let mut m = ProjectModel::from_json(LEGACY).unwrap();
    m.ensure_languages();
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
    let t = r#"{"videos":[{"fichier":"a"}],"langues":[{"id":4,"nom":"Anglais","code":"eng"}],
      "mise_en_page":{"pages":[{"nom":"P","elements":[{"type":"texte","texte":"x","x":0,"y":0,"w":10,"h":10,"taille":40,"cible":{"rester":true},"soustitres":{"langue":4}}]}]}}"#;
    let m = ProjectModel::from_json(t).unwrap();
    assert_eq!(m.designs.free[0].items[0].sub, SubChoice::Lang(4));
    assert!(m.to_json().contains("\"langue\": 4"));
    assert!(ProjectModel::from_json(r#"{"videos":[{"fichier":"a"}],"langues":[{"id":1,"code":"fra"},{"id":1,"code":"eng"}]}"#).is_err());
}
