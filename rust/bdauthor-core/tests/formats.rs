//! Formats PAL / entrelaces / sans son avec un vrai ffmpeg (ignore si ffmpeg est absent).
use bdauthor_core::ffmpeg::*;
use bdauthor_core::film::*;
use bdauthor_core::tsclip::scan_clip;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::AtomicBool;

fn dir(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("bdauthor_fmt_{name}_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn make(tools: &Tools, out: &Path, args: &[&str]) {
    let ok = Command::new(&tools.ffmpeg).args(["-y", "-v", "error"]).args(args).arg(out).status().unwrap().success();
    assert!(ok, "generation de {}", out.display());
}

fn film(tools: &Tools, src: &Path, d: &Path) -> (Vec<u8>, FilmStats) {
    make_film_m2ts(tools, src, *b"fra", &EncodeOpts { preset: "ultrafast".into(), ..Default::default() }, Some(d), &mut |_| {}, &AtomicBool::new(false)).unwrap()
}

#[test]
fn pal_entrelace_et_sans_son() {
    let Ok(tools) = Tools::locate() else { eprintln!("ffmpeg absent : test ignore"); return };
    let d = dir("pal");
    // 1080p25 -> 1080i50 en PsF : 25 images/s conservees
    let p25 = d.join("p25.mp4");
    make(&tools, &p25, &["-f", "lavfi", "-i", "testsrc=size=1920x1080:rate=25:duration=2", "-f", "lavfi", "-i", "sine=duration=2", "-c:v", "libx264", "-preset", "ultrafast", "-pix_fmt", "yuv420p", "-c:a", "aac", "-shortest"]);
    let (m, st) = film(&tools, &p25, &d);
    let sc = scan_clip(&m).unwrap();
    assert!(st.cible.contains("PsF"), "{}", st.cible);
    assert_eq!((sc.height, sc.progressive, sc.fps), (1080, false, 25.0));
    assert_eq!(sc.video_code, 0x43, "1080i, 25 i/s");
    assert_eq!(st.images, 50);

    // 1080i50 vrai entrelace (trame du haut en premier) -> conserve
    let i50 = d.join("i50.mp4");
    make(&tools, &i50, &["-f", "lavfi", "-i", "testsrc=size=1920x1080:rate=25:duration=2", "-f", "lavfi", "-i", "sine=duration=2", "-c:v", "libx264", "-preset", "ultrafast",
        "-flags", "+ildct+ilme", "-x264-params", "tff=1", "-field_order", "tt", "-pix_fmt", "yuv420p", "-c:a", "aac", "-shortest"]);
    let info = probe(&tools, &i50).unwrap();
    assert!(info.interlaced(), "ffprobe : {:?}", info.field_order);
    let (m, st) = film(&tools, &i50, &d);
    let sc = scan_clip(&m).unwrap();
    assert_eq!(st.cible, "1080i50");
    assert_eq!((sc.progressive, sc.fps, sc.video_code), (false, 25.0, 0x43));

    // video sans son -> piste muette ajoutee ; 25p SD -> 720p50
    let muet = d.join("muet.mp4");
    make(&tools, &muet, &["-f", "lavfi", "-i", "testsrc=size=640x360:rate=25:duration=2", "-c:v", "libx264", "-preset", "ultrafast", "-pix_fmt", "yuv420p"]);
    assert!(!probe(&tools, &muet).unwrap().has_audio);
    let (m, st) = film(&tools, &muet, &d);
    let sc = scan_clip(&m).unwrap();
    assert!(st.trames_audio > 0 && !sc.audio_codes.is_empty(), "piste muette absente");
    assert_eq!((sc.height, sc.fps), (720, 50.0));
    assert!((95..=105).contains(&st.images), "cadence doublee : {}", st.images);

    // image fixe 1080 a 25 i/s : PsF aussi
    let png = d.join("i.png");
    make(&tools, &png, &["-f", "lavfi", "-i", "color=c=blue:size=320x180", "-frames:v", "1"]);
    let (m, _) = make_still_m2ts(&tools, &png, 1.0, (1920, 1080), "25", &EncodeOpts { preset: "ultrafast".into(), ..Default::default() }, Some(&d), &AtomicBool::new(false)).unwrap();
    let sc = scan_clip(&m).unwrap();
    assert_eq!((sc.progressive, sc.fps, sc.video_code), (false, 25.0, 0x43));
    let _ = std::fs::remove_dir_all(d);
}
