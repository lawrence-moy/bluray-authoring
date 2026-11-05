//! Images d'une video pour les fenetres d'apercu : taille, sondage, extraction avec memoire, frise.
use bdauthor_core::ffmpeg::Tools;
use bdauthor_core::framesrc::*;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};

#[test]
fn scaled_height_is_even_and_proportional() {
    assert_eq!(scaled_height((1920, 1080), 960), 540);
    assert_eq!(scaled_height((1280, 720), 640), 360);
    assert_eq!(scaled_height((720, 576), 960) % 2, 0);
    assert_eq!(scaled_height((0, 0), 100), 100);
}

fn source() -> Option<(Tools, PathBuf, PathBuf)> {
    let tools = Tools::locate().ok()?;
    let dir = std::env::temp_dir().join(format!("bdauthor_framesrc_{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let p = dir.join("src.mp4");
    let ok = Command::new(&tools.ffmpeg)
        .args(["-y", "-v", "error", "-f", "lavfi", "-i", "testsrc=size=320x180:rate=25:duration=4", "-c:v", "libx264", "-pix_fmt", "yuv420p"])
        .arg(&p).status().unwrap().success();
    assert!(ok);
    Some((tools, p, dir))
}

fn wait<F: FnMut() -> bool>(mut f: F) -> bool {
    let t0 = Instant::now();
    while t0.elapsed() < Duration::from_secs(20) {
        if f() { return true; }
        std::thread::sleep(Duration::from_millis(20));
    }
    false
}

#[test]
fn probe_loader_and_strip() {
    let Some((_tools, src, dir)) = source() else { eprintln!("ffmpeg absent : test ignore"); return };
    let m = probe_async(&src, 160).recv_timeout(Duration::from_secs(20)).unwrap().unwrap();
    assert_eq!((m.size, m.fps.round() as i32), ((160, 90), 25));
    assert!((m.duration - 4.0).abs() < 0.1);

    // image exacte puis memoire : la deuxieme demande est immediate
    let mut fl = FrameLoader::start(&src, Fit::Stretch, 2);
    assert!(!fl.want(25, 1.0, m.size), "pas encore en memoire");
    assert!(fl.busy());
    assert!(wait(|| { fl.poll(); fl.get(25, m.size).is_some() }));
    assert!(!fl.busy());
    assert_eq!(fl.get(25, m.size).unwrap().data.len(), 160 * 90 * 4);
    assert!(fl.want(25, 1.0, m.size));
    // les images plus anciennes sont oubliees au-dela de la capacite
    for n in [30, 35] {
        fl.want(n, n as f64 / 25.0, m.size);
        assert!(wait(|| { fl.poll(); fl.get(n, m.size).is_some() }));
    }
    assert!(fl.get(25, m.size).is_none() && fl.get(35, m.size).is_some());
    // au-dela de la fin : echec retenu, pas redemande
    fl.want(9999, 999.0, m.size);
    assert!(wait(|| { fl.poll(); fl.error(9999, m.size).is_some() }));
    assert!(!fl.want(9999, 999.0, m.size) && !fl.busy());

    // fond 16/9 d'une video d'autres proportions : taille exacte demandee
    let mut cv = FrameLoader::start(&src, Fit::Cover, 2);
    cv.want(1000, 1.0, (128, 72));
    assert!(wait(|| { cv.poll(); cv.get(1000, (128, 72)).is_some() }));
    assert_eq!(cv.get(1000, (128, 72)).unwrap().data.len(), 128 * 72 * 4);

    // frise : toutes les vignettes arrivent
    let mut st = Strip::start(&src, m.duration, 6, (64, 36));
    let mut seen = Vec::new();
    assert!(wait(|| { let (n, busy) = st.poll(); seen.extend(n.into_iter().map(|x| x.0)); !busy }));
    seen.sort();
    assert_eq!(seen, vec![0, 1, 2, 3, 4, 5]);
    let _ = std::fs::remove_dir_all(&dir);
    let _: &Path = &src;
}
