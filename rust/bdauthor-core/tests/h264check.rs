//! Verificateur de conformite H.264 Blu-ray : un flux encode par l'appli passe, un flux mal regle est signale (ffmpeg requis).
use bdauthor_core::ffmpeg::{self, EncodeOpts, Tools};
use bdauthor_core::h264check;
use std::path::PathBuf;
use std::process::Command;
use std::sync::atomic::AtomicBool;

fn workdir(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("bdauthor_test_{name}_{}", std::process::id()));
    std::fs::create_dir_all(&d).unwrap();
    d
}

#[test]
fn app_encode_is_compliant_and_bad_settings_are_reported() {
    let Ok(tools) = Tools::locate() else { eprintln!("ffmpeg absent : test ignore"); return };
    let dir = workdir("h264check");
    let src = dir.join("src.mkv");
    assert!(Command::new(&tools.ffmpeg)
        .args(["-y", "-v", "error", "-f", "lavfi", "-i", "testsrc2=size=1920x1080:rate=25:duration=3", "-c:v", "libx264", "-preset", "ultrafast", "-pix_fmt", "yuv420p"])
        .arg(&src).status().unwrap().success());
    // encodage de l'appli (reglages Blu-ray de x264)
    let info = ffmpeg::probe(&tools, &src).unwrap();
    let ts = dir.join("ok.ts");
    let opts = EncodeOpts { preset: "veryfast".into(), keyint: 25, ..Default::default() };
    ffmpeg::encode_to_ts_tracks(&tools, &src, &ts, &info, &opts, &[], &mut |_| {}, &AtomicBool::new(false)).unwrap();
    let enc = bdauthor_core::film::load_encoded(&std::fs::read(&ts).unwrap()).unwrap();
    let r = h264check::check(&enc.video, 25);
    assert!(r.ok(), "{:?}", r.issues);
    let s = r.sps.unwrap();
    assert_eq!((s.profile, s.level, s.width_mbs, s.height_mbs), (100, 41, 120, 68));
    assert!(s.max_refs <= 4 && s.hrd.is_some_and(|(_, cpb)| cpb <= 30_000_000), "{s:?}");
    assert!(r.gop_max <= 25 && r.b_run_max <= 3 && r.frames == 75, "{} {} {}", r.gop_max, r.b_run_max, r.frames);

    // flux mal regle : sans HRD, 8 references, 8 images B d'affilee, une seule image cle, niveau 5.1
    let bad = dir.join("bad.ts");
    assert!(Command::new(&tools.ffmpeg)
        .args(["-y", "-v", "error", "-i"]).arg(&src)
        .args(["-c:v", "libx264", "-preset", "veryfast", "-level", "5.1", "-x264-params", "bframes=8:b-adapt=0:ref=8:keyint=250:scenecut=0", "-f", "mpegts"])
        .arg(&bad).status().unwrap().success());
    let enc = bdauthor_core::film::load_encoded(&std::fs::read(&bad).unwrap()).unwrap();
    let r = h264check::check(&enc.video, 25);
    let all = r.issues.join(" | ");
    eprintln!("{all}");
    for want in ["level 5.1", "HRD", "B pictures in a row", "between key pictures", "reference pictures", "buffering period"] {
        assert!(all.contains(want), "{want} absent : {all}");
    }
    let _ = std::fs::remove_dir_all(dir);
}
