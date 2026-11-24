//! Pistes audio : lecture/ecriture JSON, regle de numerotation par langue.
mod common;
use bdauthor_core::audio::*;
use bdauthor_core::model::*;
use bdauthor_core::subtitle::Language;

fn langs(codes: &[&str]) -> Vec<Language> {
    codes.iter().enumerate().map(|(i, c)| Language { id: i as u32 + 1, name: c.to_uppercase(), code: c.to_string() }).collect()
}
fn tr(id: u32) -> AudioTrackCfg { AudioTrackCfg { source: AudioSource::Embedded(id), lang_id: id, lang: "und".into(), offset_ms: 0, format: AudioFormat::Ac3, bitrate: 0, sample_rate: 0, lpcm_channels: Default::default(), gain_db: 0.0, normalize: None, measured: None } }

#[test]
fn plan_orders_tracks_by_language() {
    common::fr();
    let l = langs(&["fra", "eng", "deu"]);
    // aucune piste configuree, ou pas de langues : ancien fonctionnement
    assert_eq!(plan_audio(&l, "fra", &[vec![], vec![]]).unwrap(), None);
    assert_eq!(plan_audio(&[], "fra", &[vec![tr(1)]]).unwrap(), None);
    // video 1 : anglais puis francais (ordre garde) ; video 2 : francais seul ; video 3 sans reglage : francais par defaut
    let p = plan_audio(&l, "fra", &[vec![tr(2), tr(1)], vec![tr(1)], vec![]]).unwrap().unwrap();
    assert_eq!(p.slots, vec![1, 2]);
    assert_eq!(p.videos[0].langs, vec![*b"eng", *b"fra"]);
    assert_eq!(p.videos[0].tracks.iter().map(|t| t.lang_id).collect::<Vec<_>>(), vec![2, 1]);
    assert_eq!(p.videos[1].langs, vec![*b"fra"]);
    assert!(p.videos[2].tracks.is_empty() && p.videos[2].langs == vec![*b"fra"]);
    // une langue reservee aux sous-titres (deu) n'occupe pas de numero audio ; l'allemand audio le ferait
    let p = plan_audio(&l, "fra", &[vec![tr(3), tr(1)]]).unwrap().unwrap();
    assert_eq!(p.slots, vec![1, 3]);
}

#[test]
fn plan_lets_each_video_keep_its_languages() {
    common::fr();
    let l = langs(&["deu", "fra", "eng"]);
    // video 1 : allemand et francais ; video 2 : anglais seul (piste 1 ici, alors que l'anglais est la 3e langue)
    let p = plan_audio(&l, "fra", &[vec![tr(2), tr(1)], vec![tr(3)]]).unwrap().unwrap();
    assert_eq!(p.slots, vec![1, 2, 3]);
    assert_eq!(p.videos[0].ids, vec![2, 1]);
    assert_eq!(p.videos[0].langs, vec![*b"fra", *b"deu"]);
    assert_eq!(p.videos[1].ids, vec![3]);
    assert_eq!(p.videos[1].langs, vec![*b"eng"]);
    // langue en double, langue non choisie, langue du disque absente de la liste
    let l = langs(&["fra", "eng"]);
    assert!(plan_audio(&l, "fra", &[vec![tr(1), tr(1)]]).unwrap_err().contains("deja utilisee"));
    let mut t = tr(1);
    t.lang_id = 0;
    assert!(plan_audio(&l, "fra", &[vec![t]]).unwrap_err().contains("langue a choisir"));
    assert!(plan_audio(&l, "jpn", &[vec![tr(1)], vec![]]).unwrap_err().contains("n'est pas dans la liste"));
}

#[test]
fn audio_tracks_json_and_validation() {
    common::fr();
    let t = r#"{"language":"fra","languages":[{"id":1,"name":"Francais","code":"fra"},{"id":2,"name":"Anglais","code":"eng"}],
      "videos":[{"file":"a.mp4","audio_tracks":[{"track":0,"language":"xxx","language_id":1,"offset_ms":-120},{"file":"en.wav","language_id":2}]}]}"#;
    let mut m = ProjectModel::from_json(t).unwrap();
    assert_eq!(m.videos[0].audio.len(), 2);
    assert_eq!(m.videos[0].audio[0].offset_ms, -120);
    assert_eq!(m.videos[0].audio[1].source, AudioSource::File("en.wav".into()));
    // le code ecrit suit la langue du projet
    let out = m.to_json();
    let fro = m.to_json();
    assert!(fro.contains("\"language\": \"fra\"") && fro.contains("\"language_id\": 2") && fro.contains("\"offset_ms\": -120"), "{fro}");
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

#[test]
fn track_level_is_saved_validated_and_sent_to_ffmpeg() {
    use bdauthor_core::audio::{AudioFormat, AudioTrackCfg};
    use bdauthor_core::ffmpeg::audio_codec_args;
    let mut t = AudioTrackCfg::embedded(0, "fra");
    // 0 dB : rien n'est ecrit et la ligne de commande reste celle d'avant
    assert!(t.to_value().get("gain_db").is_none());
    assert!(!audio_codec_args("448k", &[t.clone()]).iter().any(|a| a.contains("volume")));
    t.gain_db = -3.5;
    let back = AudioTrackCfg::from_value(&t.to_value()).unwrap();
    assert_eq!(back.gain_db, -3.5);
    let args = audio_codec_args("448k", &[t.clone()]);
    let k = args.iter().position(|a| a == "-filter:a:0").expect("filtre de la piste");
    assert_eq!(args[k + 1], "volume=-3.50dB");
    // LPCM stereo : le mixage des canaux vient avant le niveau
    t.format = AudioFormat::Lpcm16;
    t.lpcm_channels = bdauthor_core::audio::LpcmChannels::Stereo;
    let args = audio_codec_args("448k", &[t.clone()]);
    let k = args.iter().position(|a| a == "-filter:a:0").unwrap();
    assert!(args[k + 1].starts_with("aresample=") && args[k + 1].ends_with(",volume=-3.50dB"), "{}", args[k + 1]);
    // piste recopiee : pas de filtre possible, signale
    t.format = AudioFormat::Ac3Copy;
    assert!(!t.problems().is_empty());
    assert!(t.gain_filter().is_none());
    t.format = AudioFormat::Ac3;
    t.gain_db = 40.0;
    assert!(!t.problems().is_empty());
}

#[test]
fn track_normalization_is_saved_and_sent_to_ffmpeg() {
    use bdauthor_core::audio::{AudioFormat, AudioTrackCfg, Loudness};
    use bdauthor_core::ffmpeg::audio_codec_args;
    let mut t = AudioTrackCfg::embedded(0, "fra");
    // « comme le projet » : rien n'est ecrit
    assert!(t.to_value().get("normalize").is_none());
    assert_eq!(AudioTrackCfg::from_value(&t.to_value()).unwrap().normalize, None);
    t.normalize = Some(false);
    assert_eq!(AudioTrackCfg::from_value(&t.to_value()).unwrap().normalize, Some(false));
    assert!(t.gain_filter().is_none());
    t.normalize = Some(true);
    t.gain_db = 5.0; // la normalisation remplace le niveau manuel
    assert_eq!(AudioTrackCfg::from_value(&t.to_value()).unwrap().normalize, Some(true));
    // sans mesure : une passe, apres le mixage stereo de la sortie
    let args = audio_codec_args("448k", &[t.clone()]);
    let k = args.iter().position(|a| a == "-filter:a:0").expect("filtre de la piste");
    assert_eq!(args[k + 1], "aformat=channel_layouts=stereo,loudnorm=I=-23:LRA=7:TP=-1.5");
    // avec la mesure : seconde passe lineaire
    t.measured = Loudness::from_json("noise {
	\"input_i\" : \"-27.61\",
	\"input_tp\" : \"-4.47\",
	\"input_lra\" : \"18.06\",
	\"input_thresh\" : \"-39.20\",
	\"output_i\" : \"-16.58\",
	\"target_offset\" : \"0.58\"
}
");
    assert_eq!(t.measured.unwrap().input_i, -27.61);
    let f = t.gain_filter().unwrap();
    assert!(f.contains("measured_I=-27.61") && f.contains("measured_thresh=-39.20") && f.contains("offset=0.58") && f.ends_with("linear=true:print_format=none"), "{f}");
    // piste muette : pas de mesure
    assert!(Loudness::from_json("{ \"input_i\" : \"-inf\", \"input_tp\" : \"-inf\", \"input_lra\" : \"0.00\", \"input_thresh\" : \"-70.00\", \"target_offset\" : \"0.00\" }").is_none());
    // piste recopiee : pas de filtre possible, signale
    t.format = AudioFormat::Ac3Copy;
    assert!(!t.problems().is_empty());
    assert!(t.gain_filter().is_none());
}

#[test]
fn project_level_normalization_without_audio_tracks_filters_the_first_track() {
    use bdauthor_core::ffmpeg::{encode_args, EncodeOpts, VideoInfo};
    let info = VideoInfo { width: 1920, height: 1080, fps_num: 25, fps_den: 1, pix_fmt: "yuv420p".into(), codec: "h264".into(), duration_s: 60.0, has_audio: true, ..Default::default() };
    let mut o = EncodeOpts::default();
    let args = |o: &EncodeOpts| encode_args(std::path::Path::new("in.mkv"), std::path::Path::new("out.ts"), &info, o).0;
    assert!(!args(&o).iter().any(|a| a.contains("loudnorm")));
    o.normalize_audio = true;
    let a = args(&o);
    let k = a.iter().position(|x| x == "-filter:a:0").expect("filtre de la piste");
    assert!(a[k + 1].ends_with("loudnorm=I=-23:LRA=7:TP=-1.5"), "{}", a[k + 1]);
}
