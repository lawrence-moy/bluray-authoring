//! Tests avec un vrai ffmpeg (ignores si ffmpeg est absent). Une source synthetique est generee par ffmpeg lui-meme.
use bdauthor_core::ffmpeg::*;
use std::path::PathBuf;
use std::process::Command;
use std::sync::atomic::{AtomicBool, Ordering};

fn workdir(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("bdauthor_test_{name}_{}", std::process::id()));
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn source(tools: &Tools, dir: &PathBuf) -> PathBuf {
    let p = dir.join("src.mp4");
    let ok = Command::new(&tools.ffmpeg)
        .args(["-y", "-v", "error", "-f", "lavfi", "-i", "testsrc=size=640x360:rate=25:duration=2", "-f", "lavfi", "-i", "sine=duration=2",
               "-c:v", "libx264", "-pix_fmt", "yuv420p", "-c:a", "aac", "-shortest"])
        .arg(&p).status().unwrap().success();
    assert!(ok);
    p
}

#[test]
fn bout_en_bout() {
    let Ok(tools) = Tools::locate() else { eprintln!("ffmpeg absent : test ignore"); return };
    let dir = workdir("e2e");
    let src = source(&tools, &dir);
    let info = probe(&tools, &src).unwrap();
    assert_eq!((info.width, info.height, info.fps_num), (640, 360, 25));
    assert!(info.has_audio && !info.full_range);

    let ts = dir.join("out.ts");
    let mut max = 0.0f64;
    let tg = encode_to_ts(&tools, &src, &ts, &info, &EncodeOpts { preset: "ultrafast".into(), ..Default::default() },
                          &mut |p| max = max.max(p), &AtomicBool::new(false)).unwrap();
    assert_eq!(tg.label, "720p50 (cadence doublee)"); // 25 i/s en definition standard -> 720p50 (chaque image doublee)
    assert_eq!(max, 1.0);
    assert_eq!(std::fs::metadata(&ts).unwrap().len() % 188, 0);

    let png = dir.join("t.png");
    extract_thumbnail(&tools, &src, &png, (320, 180), None, info.duration_s).unwrap();
    assert!(std::fs::metadata(&png).unwrap().len() > 100);

    let still = dir.join("still.ts");
    make_still_ts(&tools, &png, &still, 1.0, (1280, 720), "60000/1001", &EncodeOpts { preset: "ultrafast".into(), ..Default::default() },
                  &AtomicBool::new(false)).unwrap();
    assert!(std::fs::metadata(&still).unwrap().len() > 188);
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn annulation_et_erreurs() {
    let Ok(tools) = Tools::locate() else { return };
    let dir = workdir("err");
    let src = source(&tools, &dir);
    let info = probe(&tools, &src).unwrap();

    let cancel = AtomicBool::new(true);
    let r = encode_to_ts(&tools, &src, &dir.join("c.ts"), &info, &EncodeOpts::default(), &mut |_| {}, &cancel);
    assert!(matches!(r, Err(Error::Cancelled)), "{r:?}");

    let bad = dir.join("pas_une_video.mp4");
    std::fs::write(&bad, b"ceci n'est pas une video").unwrap();
    assert!(matches!(probe(&tools, &bad), Err(Error::Probe(_))));
    assert!(probe(&tools, &dir.join("absent.mp4")).is_err());

    // source sans piste audio : une piste muette est ajoutee
    let muet = dir.join("muet.mp4");
    assert!(Command::new(&tools.ffmpeg).args(["-y", "-v", "error", "-i"]).arg(&src).args(["-an", "-c:v", "copy"]).arg(&muet).status().unwrap().success());
    let info2 = probe(&tools, &muet).unwrap();
    assert!(!info2.has_audio);
    let r = encode_to_ts(&tools, &muet, &dir.join("m.ts"), &info2, &EncodeOpts::default(), &mut |_| {}, &AtomicBool::new(false));
    assert!(r.is_ok(), "{r:?}");
    cancel.store(false, Ordering::Relaxed);
    let _ = std::fs::remove_dir_all(dir);
}
