//! Pistes audio : lecture/ecriture JSON, regle de numerotation par langue.
use bdauthor_core::audio::*;
use bdauthor_core::model::*;
use bdauthor_core::subtitle::Language;

fn langs(codes: &[&str]) -> Vec<Language> {
    codes.iter().enumerate().map(|(i, c)| Language { id: i as u32 + 1, name: c.to_uppercase(), code: c.to_string() }).collect()
}
fn tr(id: u32) -> AudioTrackCfg { AudioTrackCfg { source: AudioSource::Embedded(id), lang_id: id, lang: "und".into(), offset_ms: 0 } }

#[test]
fn plan_orders_tracks_by_language() {
    let l = langs(&["fra", "eng", "deu"]);
    // aucune piste configuree, ou pas de langues : ancien fonctionnement
    assert_eq!(plan_audio(&l, "fra", &[vec![], vec![]]).unwrap(), None);
    assert_eq!(plan_audio(&[], "fra", &[vec![tr(1)]]).unwrap(), None);
    // video 1 : anglais puis francais -> rangees fra, eng ; video 2 : francais seul ; video 3 sans reglage : francais par defaut
    let p = plan_audio(&l, "fra", &[vec![tr(2), tr(1)], vec![tr(1)], vec![]]).unwrap().unwrap();
    assert_eq!(p.slots, vec![1, 2]);
    assert_eq!(p.videos[0].langs, vec![*b"fra", *b"eng"]);
    assert_eq!(p.videos[0].tracks.iter().map(|t| t.lang_id).collect::<Vec<_>>(), vec![1, 2]);
    assert_eq!(p.videos[1].langs, vec![*b"fra"]);
    assert!(p.videos[2].tracks.is_empty() && p.videos[2].langs == vec![*b"fra"]);
    // une langue reservee aux sous-titres (deu) n'occupe pas de numero audio ; l'allemand audio le ferait
    let p = plan_audio(&l, "fra", &[vec![tr(3), tr(1)]]).unwrap().unwrap();
    assert_eq!(p.slots, vec![1, 3]);
}

#[test]
fn plan_rejects_inconsistent_numbers() {
    let l = langs(&["fra", "eng"]);
    // l'anglais seul serait la piste 1 ici et la piste 2 ailleurs
    let e = plan_audio(&l, "fra", &[vec![tr(1), tr(2)], vec![tr(2)]]).unwrap_err();
    assert!(e.contains("video 2") && e.contains("meme numero"), "{e}");
    // langue en double, langue non choisie, langue du disque absente de la liste
    assert!(plan_audio(&l, "fra", &[vec![tr(1), tr(1)]]).unwrap_err().contains("deja utilisee"));
    let mut t = tr(1);
    t.lang_id = 0;
    assert!(plan_audio(&l, "fra", &[vec![t]]).unwrap_err().contains("langue a choisir"));
    assert!(plan_audio(&l, "jpn", &[vec![tr(1)], vec![]]).unwrap_err().contains("n'est pas dans la liste"));
}

#[test]
fn audio_tracks_json_and_validation() {
    let t = r#"{"langue":"fra","langues":[{"id":1,"nom":"Francais","code":"fra"},{"id":2,"nom":"Anglais","code":"eng"}],
      "videos":[{"fichier":"a.mp4","pistes_audio":[{"piste":0,"langue":"xxx","langue_id":1,"decalage_ms":-120},{"fichier":"en.wav","langue_id":2}]}]}"#;
    let mut m = ProjectModel::from_json(t).unwrap();
    assert_eq!(m.videos[0].audio.len(), 2);
    assert_eq!(m.videos[0].audio[0].offset_ms, -120);
    assert_eq!(m.videos[0].audio[1].source, AudioSource::File("en.wav".into()));
    // le code ecrit suit la langue du projet
    let out = m.to_json();
    assert!(out.contains("\"langue\": \"fra\"") && out.contains("\"langue_id\": 2") && out.contains("\"decalage_ms\": -120"), "{out}");
    assert_eq!(ProjectModel::from_json(&out).unwrap().videos[0].audio, m.to_value_audio_for_test());
    let probs = |m: &ProjectModel| -> Vec<String> { m.validate(std::path::Path::new("/nonexistent")).into_iter().filter(|p| !p.contains("introuvable")).collect() };
    assert!(probs(&m).is_empty(), "{:?}", probs(&m));
    // piste audio sans langue : signalee ; langue retiree : idem
    m.remove_language(2);
    assert!(probs(&m).iter().any(|p| p.to_lowercase().contains("langue a choisir")), "{:?}", probs(&m));
    // pistes audio sans aucune langue dans le projet
    m.languages.clear();
    assert!(probs(&m).iter().any(|p| p.contains("aucune langue")), "{:?}", probs(&m));
}

trait AudioForTest { fn to_value_audio_for_test(&self) -> Vec<AudioTrackCfg>; }
impl AudioForTest for ProjectModel {
    fn to_value_audio_for_test(&self) -> Vec<AudioTrackCfg> {
        let back = ProjectModel::from_json(&self.to_json()).unwrap();
        back.videos[0].audio.clone()
    }
}
