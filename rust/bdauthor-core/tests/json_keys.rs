use bdauthor_core::json;
use bdauthor_core::model::ProjectModel;

const FRENCH: &str = r##"{
  "titre": "T", "langue": "fra",
  "videos": [{"fichier": "a.mkv", "vignette_s": 3.5, "chapitres": [{"t": 1.0, "titre": "C"}],
              "soustitres": [{"fichier": "s.srt", "langue": "fra", "langue_id": 1, "style": {"taille": 50, "contour_couleur": "#000000"}}],
              "pistes_audio": [{"piste": 0, "langue": "fra", "langue_id": 1}]}],
  "encodage": {"crf": 20, "preset": "fast"},
  "menu": {"fond_secondes": 4, "boucle": true, "taille": "1080", "cache": false},
  "langues": [{"id": 1, "nom": "Francais", "code": "fra"}],
  "jaquette": {"face_mm": 129, "hauteur_mm": 148, "tranche_mm": 14, "fond_perdu_mm": 3, "couleurs": {"dos": "#ececec", "tranche": "#e6e6e6", "face": "#e6e3de"},
               "elements": [{"type": "texte", "texte": "Hello", "taille_pt": 20, "couleur": "#000000", "alignement": "centre", "gras": true, "x": 1, "y": 2, "w": 30, "h": 10}]}
}"##;

fn keys(v: &json::Value, out: &mut Vec<String>) {
    match v {
        json::Value::Arr(a) => a.iter().for_each(|x| keys(x, out)),
        json::Value::Obj(o) => o.iter().for_each(|(k, x)| { out.push(k.clone()); keys(x, out); }),
        _ => {}
    }
}

#[test]
fn project_file_keys_are_english_and_old_files_still_load() {
    let m = ProjectModel::from_json(FRENCH).unwrap();
    let out = m.to_json();
    let v = json::parse(&out).unwrap();
    let mut ks = vec![];
    keys(&v, &mut ks);
    for fr in ["titre", "langue", "fichier", "soustitres", "pistes_audio", "mise_en_page", "jaquette", "encodage", "taille_pt", "contour_couleur", "fond_secondes", "face_mm", "couleurs", "dos", "texte", "gras", "alignement"] {
        assert!(!ks.iter().any(|k| k == fr), "cle francaise encore ecrite : {fr}");
    }
    for en in ["title", "language", "file", "subtitles", "audio_tracks", "cover", "encoding", "size_pt", "outline_color", "background_seconds", "front_width_mm", "colors", "back", "text", "bold", "align"] {
        let en = if en == "front_width_mm" { "face_width_mm" } else { en };
        assert!(ks.iter().any(|k| k == en), "cle anglaise absente : {en}");
    }
    // relecture du fichier ecrit : meme contenu
    let m2 = ProjectModel::from_json(&out).unwrap();
    assert_eq!(m2.to_json(), out);
    assert_eq!(m2.title, "T");
    assert_eq!(m2.videos[0].subs[0].lang, "fra");
    assert_eq!(m2.cover.items.len(), 1);
    assert!(m2.loop_menu && !m2.cache);
}
