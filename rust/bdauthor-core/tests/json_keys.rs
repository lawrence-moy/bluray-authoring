use bdauthor_core::json;
use bdauthor_core::model::ProjectModel;

const PROJECT: &str = r##"{
  "title": "T", "language": "fra",
  "videos": [{"file": "a.mkv", "thumbnail_s": 3.5, "chapters": [{"t": 1.0, "title": "C"}],
              "subtitles": [{"file": "s.srt", "language": "fra", "language_id": 1, "style": {"size": 50, "outline_color": "#000000"}}],
              "audio_tracks": [{"track": 0, "language": "fra", "language_id": 1}]}],
  "encoding": {"crf": 20, "preset": "fast"},
  "menu": {"background_seconds": 4, "loop": true, "size": "1080", "use_cache": false},
  "languages": [{"id": 1, "name": "Francais", "code": "fra"}],
  "cover": {"face_width_mm": 129, "height_mm": 148, "spine_mm": 14, "bleed_mm": 3, "colors": {"back": "#ececec", "spine": "#e6e6e6", "front": "#e6e3de"},
            "items": [{"type": "texte", "text": "Hello", "size_pt": 20, "color": "#000000", "align": "centre", "bold": true, "x": 1, "y": 2, "w": 30, "h": 10}]}
}"##;

fn keys(v: &json::Value, out: &mut Vec<String>) {
    match v {
        json::Value::Arr(a) => a.iter().for_each(|x| keys(x, out)),
        json::Value::Obj(o) => o.iter().for_each(|(k, x)| { out.push(k.clone()); keys(x, out); }),
        _ => {}
    }
}

#[test]
fn project_file_uses_english_keys() {
    let m = ProjectModel::from_json(PROJECT).unwrap();
    assert_eq!(m.title, "T");
    assert_eq!(m.videos[0].subs[0].lang, "fra");
    assert_eq!(m.videos[0].audio.len(), 1);
    assert_eq!(m.cover.items.len(), 1);
    assert!(m.loop_menu && !m.cache);
    // ce qui est ecrit : cles anglaises seulement, et relecture identique
    let out = m.to_json();
    let mut ks = vec![];
    keys(&json::parse(&out).unwrap(), &mut ks);
    for fr in ["titre", "langue", "fichier", "soustitres", "pistes_audio", "mise_en_page", "jaquette", "encodage", "taille_pt", "contour_couleur", "fond_secondes", "face_mm", "couleurs", "dos", "texte", "gras", "alignement"] {
        assert!(!ks.iter().any(|k| k == fr), "cle francaise ecrite : {fr}");
    }
    for en in ["title", "language", "file", "subtitles", "audio_tracks", "cover", "encoding", "size_pt", "outline_color", "background_seconds", "face_width_mm", "colors", "back", "text", "bold", "align"] {
        assert!(ks.iter().any(|k| k == en), "cle anglaise absente : {en}");
    }
    assert_eq!(ProjectModel::from_json(&out).unwrap().to_json(), out);
}

#[test]
fn old_french_keys_are_no_longer_read() {
    // un ancien fichier (cles francaises) n'est plus compris : la liste des videos est introuvable
    assert!(ProjectModel::from_json(r#"{"titre": "T", "videos": [{"fichier": "a.mkv"}]}"#).is_err());
}
