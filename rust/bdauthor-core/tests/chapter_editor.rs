//! Outils de l'editeur de chapitres : image a un instant, lecture, images cles, images cles forcees aux chapitres.
use bdauthor_core::ffmpeg::*;
use bdauthor_core::model::ProjectModel;
use std::path::{Path, PathBuf};
use std::process::Command;

fn workdir(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("bdauthor_chap_{name}_{}", std::process::id()));
    std::fs::create_dir_all(&d).unwrap();
    d
}

#[test]
fn force_keys_arguments() {
    assert!(force_keys_args(&[]).is_empty());
    assert!(force_keys_args(&[0.0]).is_empty());
    assert_eq!(force_keys_args(&[12.5, 30.0]), vec!["-force_key_frames", "12.4995,29.9995", "-forced-idr", "1"]);
    let info = VideoInfo { width: 1920, height: 1080, fps_num: 25, fps_den: 1, pix_fmt: "yuv420p".into(), full_range: false, codec: "h264".into(), duration_s: 60.0, has_audio: true, field_order: String::new() };
    let o = EncodeOpts { force_keys: vec![10.0, 20.04], ..Default::default() };
    let (a, _) = encode_args(Path::new("in.mp4"), Path::new("out.ts"), &info, &o);
    let k = a.iter().position(|x| x == "-force_key_frames").expect("images cles forcees");
    assert_eq!(a[k + 1], "9.9995,20.0395");
    let (b, _) = encode_args(Path::new("in.mp4"), Path::new("out.ts"), &info, &EncodeOpts::default());
    assert!(!b.iter().any(|x| x == "-force_key_frames"));
}

#[test]
fn frame_arguments() {
    let a = frame_args(Path::new("v.mkv"), 12.04, (960, 540), true);
    assert!(a.windows(2).any(|w| w[0] == "-ss" && w[1] == "12.0400"));
    assert!(!a.iter().any(|x| x == "-noaccurate_seek"));
    assert!(a.windows(2).any(|w| w[0] == "-vf" && w[1] == "scale=960:540"));
    assert_eq!(a.last().unwrap(), "pipe:1");
    let b = frame_args(Path::new("v.mkv"), 12.0, (160, 90), false);
    let ss = b.iter().position(|x| x == "-noaccurate_seek").unwrap();
    assert!(ss < b.iter().position(|x| x == "-i").unwrap(), "option d'entree avant -i");
}

#[test]
fn keyframes_from_compact_output() {
    let out = "frame|pts_time=1.400000\nframe|pts_time=3.400000\nframe|pts_time=5.400000\nformat|start_time=1.400000\n";
    let k = parse_keyframes_compact(out);
    assert_eq!(k.len(), 3);
    assert!((k[0] - 0.0).abs() < 1e-9 && (k[1] - 2.0).abs() < 1e-9 && (k[2] - 4.0).abs() < 1e-9);
    // horodatage manquant (N/A) : ignore ; sans start_time : la premiere image cle sert d'origine
    let k = parse_keyframes_compact("frame|pts_time=N/A|best_effort_timestamp_time=0.5\nframe|pts_time=2.5\n");
    assert_eq!(k, vec![0.0, 2.0]);
}

#[test]
fn exact_chapters_flag_json() {
    let m = ProjectModel::from_json(r#"{"videos":[{"fichier":"a.mp4","chapitres":[0,12.5],"chapitres_exacts":true},{"fichier":"b.mp4"}]}"#).unwrap();
    assert!(m.videos[0].exact_chapters && !m.videos[1].exact_chapters);
    let j = m.to_json();
    assert_eq!(j.matches("chapitres_exacts").count(), 1);
    let mut m2 = m.clone();
    m2.videos[0].exact_chapters = false;
    assert!(!m2.to_json().contains("chapitres_exacts"));
}

fn source(tools: &Tools, dir: &Path) -> PathBuf {
    let p = dir.join("src.mp4");
    // GOP de 50 images (2 s a 25 i/s) : sans images cles forcees, aucune image cle a 1,48 s
    let ok = Command::new(&tools.ffmpeg)
        .args(["-y", "-v", "error", "-f", "lavfi", "-i", "testsrc=size=320x180:rate=25:duration=4", "-c:v", "libx264", "-g", "50", "-keyint_min", "50", "-sc_threshold", "0", "-pix_fmt", "yuv420p"])
        .arg(&p).status().unwrap().success();
    assert!(ok);
    p
}

#[test]
fn frames_stream_and_forced_keyframes() {
    let Ok(tools) = Tools::locate() else { eprintln!("ffmpeg absent : test ignore"); return };
    let dir = workdir("ff");
    let src = source(&tools, &dir);

    // image exacte et image cle : tailles demandees ; deux instants differents donnent deux images differentes
    let a = extract_frame_rgba(&tools, &src, 1.0, (160, 90), true).unwrap();
    let b = extract_frame_rgba(&tools, &src, 1.04, (160, 90), true).unwrap();
    assert_eq!((a.width, a.height, a.data.len()), (160, 90, 160 * 90 * 4));
    assert_ne!(a.data, b.data, "image suivante differente (compteur de testsrc)");
    let c = extract_frame_rgba(&tools, &src, 1.0, (160, 90), false).unwrap();
    assert_eq!(c.data.len(), a.data.len());
    assert!(extract_frame_rgba(&tools, &src, 99.0, (160, 90), true).is_err(), "au-dela de la fin");

    // images cles de la source : 0 et 2 s
    let k = list_keyframes(&tools, &src).unwrap();
    assert_eq!(k.len(), 2, "{k:?}");
    assert!(k[0].abs() < 0.01 && (k[1] - 2.0).abs() < 0.01, "{k:?}");

    // lecture : des images arrivent, de la bonne taille
    let fs = FrameStream::start(&tools, &src, 3.0, (64, 36)).unwrap();
    let first = fs.rx.recv_timeout(std::time::Duration::from_secs(10)).unwrap();
    assert_eq!((first.width, first.height), (64, 36));
    drop(fs);

    // encodage avec une image cle forcee a 1,48 s
    let info = probe(&tools, &src).unwrap();
    let ts = dir.join("out.ts");
    let o = EncodeOpts { preset: "ultrafast".into(), force_keys: vec![1.48], ..Default::default() };
    encode_to_ts(&tools, &src, &ts, &info, &o, &mut |_| {}, &std::sync::atomic::AtomicBool::new(false)).unwrap();
    let k = list_keyframes(&tools, &ts).unwrap();
    assert!(k.iter().any(|t| (t - 1.48).abs() < 0.021), "image cle a 1,48 s : {k:?}");
    let _ = std::fs::remove_dir_all(&dir);
}
