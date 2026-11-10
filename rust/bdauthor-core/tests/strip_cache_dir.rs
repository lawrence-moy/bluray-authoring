//! Dossier du cache des frises choisi dans les reglages (fichier de test a part : le reglage vaut pour tout le processus).
use bdauthor_core::ffmpeg::Tools;
use bdauthor_core::framesrc::{clear_strip_cache, default_strip_cache_dir, set_strip_cache_dir, strip_cache_dir, strip_cache_usage, Strip};
use std::process::Command;
use std::time::{Duration, Instant};

#[test]
fn chosen_cache_folder_is_used_measured_and_cleared() {
    let Some(tools) = Tools::locate().ok() else { eprintln!("ffmpeg absent : test ignore"); return };
    let dir = std::env::temp_dir().join(format!("bdauthor_cache_dir_{}", std::process::id()));
    let cache = dir.join("mon cache");
    std::fs::create_dir_all(&dir).unwrap();
    let src = dir.join("src.mp4");
    assert!(Command::new(&tools.ffmpeg)
        .args(["-y", "-v", "error", "-f", "lavfi", "-i", "testsrc=size=320x180:rate=25:duration=4", "-c:v", "libx264", "-pix_fmt", "yuv420p"])
        .arg(&src).status().unwrap().success());
    set_strip_cache_dir(&cache.display().to_string());
    assert_eq!(strip_cache_dir(), Some(cache.clone()));
    let mut st = Strip::start(&src, 4.0, 6, (64, 36));
    let t0 = Instant::now();
    while st.poll().1 && t0.elapsed() < Duration::from_secs(20) { std::thread::sleep(Duration::from_millis(20)); }
    let (n, bytes) = strip_cache_usage();
    assert_eq!(n, 1, "une frise dans le dossier choisi");
    assert!(bytes > 6 * 64 * 36 * 4);
    // un autre fichier du dossier n'est pas touche
    std::fs::write(cache.join("autre.txt"), b"x").unwrap();
    assert_eq!(clear_strip_cache(), 1);
    assert_eq!(strip_cache_usage().0, 0);
    assert!(cache.join("autre.txt").exists());
    // vide : retour a l'emplacement par defaut
    set_strip_cache_dir("  ");
    assert_eq!(strip_cache_dir(), default_strip_cache_dir());
    let _ = std::fs::remove_dir_all(&dir);
}
