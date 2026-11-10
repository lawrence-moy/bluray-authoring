//! Images d'une video pour les fenetres d'apercu : taille, sondage, extraction avec memoire, frise.
use bdauthor_core::ffmpeg::Tools;
use bdauthor_core::framesrc::*;
use bdauthor_core::framesrc::{STRIP_N, VIEW_W, prefetch_strips, strip_thumb_size};
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

/// Cache des frises des tests : un dossier temporaire, jamais celui de l'utilisateur.
fn test_cache() -> PathBuf {
    static ONCE: std::sync::Once = std::sync::Once::new();
    let d = std::env::temp_dir().join(format!("bdauthor_framesrc_cache_{}", std::process::id()));
    ONCE.call_once(|| std::env::set_var("BDAUTHOR_CACHE_DIR", &d));
    d
}

fn source(name: &str) -> Option<(Tools, PathBuf, PathBuf)> {
    test_cache();
    let tools = Tools::locate().ok()?;
    let dir = std::env::temp_dir().join(format!("bdauthor_framesrc_{name}_{}", std::process::id()));
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
    let Some((_tools, src, dir)) = source("loader") else { eprintln!("ffmpeg absent : test ignore"); return };
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

#[test]
fn stream_cover_fills_requested_size() {
    use bdauthor_core::ffmpeg::FrameStream;
    let Some((tools, src, dir)) = source("stream") else { eprintln!("ffmpeg absent : test ignore"); return };
    // source 16/9 rognee en carre : la taille demandee est respectee
    let fs = FrameStream::start_fit(&tools, &src, 1.0, (72, 72), true).unwrap();
    let img = fs.rx.recv_timeout(Duration::from_secs(20)).unwrap();
    assert_eq!((img.width, img.height, img.data.len()), (72, 72, 72 * 72 * 4));
    drop(fs);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn strip_range_covers_only_the_range() {
    let Some((_tools, src, dir)) = source("range") else { eprintln!("ffmpeg absent : test ignore"); return };
    // 4 vignettes exactes entre 1 s et 2 s : quatre images differentes, numerotees de 0 a 3
    let mut st = Strip::start_range(&src, 1.0, 2.0, 4, (64, 36), true);
    let mut got: Vec<(usize, Vec<u8>)> = Vec::new();
    assert!(wait(|| { let (n, busy) = st.poll(); got.extend(n.into_iter().map(|(i, im)| (i, im.data))); !busy }));
    got.sort_by_key(|g| g.0);
    assert_eq!(got.iter().map(|g| g.0).collect::<Vec<_>>(), vec![0, 1, 2, 3]);
    for w in got.windows(2) { assert_ne!(w[0].1, w[1].1, "images exactes differentes"); }
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn audio_peaks_follow_the_loudness_and_playback_starts() {
    use bdauthor_core::ffmpeg::{audio_peaks, AudioPlayback};
    let Ok(tools) = Tools::locate() else { return };
    let dir = std::env::temp_dir().join(format!("bdauthor_audio_{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let p = dir.join("a.wav");
    // 2 s de sinus fort puis 2 s de sinus faible
    let ok = Command::new(&tools.ffmpeg)
        .args(["-y", "-v", "error", "-f", "lavfi", "-i", "sine=frequency=440:duration=2", "-f", "lavfi", "-i", "sine=frequency=440:duration=2,volume=0.1", "-filter_complex", "[0][1]concat=n=2:v=0:a=1"])
        .arg(&p).status().unwrap().success();
    assert!(ok);
    let peaks = audio_peaks(&tools, &p, None, 4.0, 8).unwrap();
    assert_eq!(peaks.len(), 8);
    assert!(peaks[1] > 0.1 && peaks[6] < 0.03 && peaks[6] > 0.005, "{peaks:?}");
    if tools.ffplay().is_some() {
        let mut pb = AudioPlayback::start(&tools, &p, None, 1.0).unwrap();
        pb.stop();
        assert!(pb.finished());
    }
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn ffmpeg_reports_detailed_progress() {
    use bdauthor_core::ffmpeg::{make_still_ts, set_progress_detail, EncodeOpts, FfmpegProgress};
    let Some((tools, _src, dir)) = source("detail") else { return };
    let img = dir.join("i.png");
    assert!(Command::new(&tools.ffmpeg).args(["-y", "-v", "error", "-f", "lavfi", "-i", "color=c=red:s=320x180:d=1", "-frames:v", "1"]).arg(&img).status().unwrap().success());
    let got = std::sync::Arc::new(std::sync::Mutex::new(Vec::<FfmpegProgress>::new()));
    let g = got.clone();
    set_progress_detail(Some(Box::new(move |d| g.lock().unwrap().push(d))));
    let opts = EncodeOpts { preset: "ultrafast".into(), ..Default::default() };
    make_still_ts(&tools, &img, &dir.join("o.ts"), 3.0, (1920, 1080), "25", &opts, &std::sync::atomic::AtomicBool::new(false)).unwrap();
    set_progress_detail(None);
    let v = got.lock().unwrap().clone();
    assert!(!v.is_empty());
    let last = v.last().unwrap();
    assert!((last.duration_s - 3.0).abs() < 1e-9 && last.time_s > 2.0, "{last:?}");
    // un autre fil n'est pas concerne
    std::thread::spawn(move || make_still_ts(&tools, &img, &dir.join("p.ts"), 1.0, (320, 180), "25", &EncodeOpts { preset: "ultrafast".into(), ..Default::default() }, &std::sync::atomic::AtomicBool::new(false)).unwrap()).join().unwrap();
    assert_eq!(got.lock().unwrap().len(), v.len());
}

#[test]
fn strip_order_is_a_permutation_serving_the_focus_or_spreading() {
    use bdauthor_core::framesrc::strip_order;
    for n in [0usize, 1, 2, 5, 8, 48, 80] {
        for focus in [None, Some(3.0)] {
            let mut o = strip_order(n, 0.0, 10.0, focus);
            o.sort();
            assert_eq!(o, (0..n).collect::<Vec<_>>(), "n={n} focus={focus:?}");
        }
    }
    // vers la vue : la vignette sous `focus` d'abord, puis ses voisines
    let o = strip_order(10, 0.0, 10.0, Some(5.2));
    assert_eq!(o[0], 5);
    assert!(o[1] == 4 || o[1] == 6);
    // sans vue : debut, milieu, quarts...
    assert_eq!(strip_order(8, 0.0, 8.0, None), vec![0, 4, 2, 6, 1, 3, 5, 7]);
}

#[test]
fn keyframe_range_is_read_in_one_pass() {
    let Some(tools) = Tools::locate().ok() else { eprintln!("ffmpeg absent : test ignore"); return };
    let dir = std::env::temp_dir().join(format!("bdauthor_framesrc_onepass_{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let src = dir.join("src.mp4");
    // une image cle par seconde pendant 8 s
    assert!(Command::new(&tools.ffmpeg)
        .args(["-y", "-v", "error", "-f", "lavfi", "-i", "testsrc2=size=320x180:rate=25:duration=8", "-c:v", "libx264", "-g", "25", "-pix_fmt", "yuv420p"])
        .arg(&src).status().unwrap().success());
    let mut got: Vec<(usize, Vec<u8>)> = Vec::new();
    let n = bdauthor_core::ffmpeg::stream_range_keyframes(&tools, &src, 0.0, 8.0, 8, (64, 36), |i, im| { assert_eq!(im.data.len(), 64 * 36 * 4); got.push((i, im.data)); true }).unwrap();
    assert!(n >= 1, "au moins une image recue");
    assert_eq!(got.iter().map(|g| g.0).collect::<Vec<_>>(), (0..8).collect::<Vec<_>>(), "toujours 8 vignettes numerotees dans l'ordre");
    assert!(got.windows(2).any(|w| w[0].1 != w[1].1), "des images differentes");
    // par la frise : memes numeros, rien en double
    let mut st = Strip::start_range(&src, 0.0, 8.0, 8, (64, 36), false);
    let mut seen = Vec::new();
    assert!(wait(|| { let (v, busy) = st.poll(); seen.extend(v.into_iter().map(|x| x.0)); !busy }));
    seen.sort();
    assert_eq!(seen, (0..8).collect::<Vec<_>>());
    // images exactes : plusieurs ffmpeg en parallele, toutes les vignettes arrivent une seule fois
    let mut ex = Strip::start_range_focus(&src, 0.0, 8.0, 8, (64, 36), true, Some(5.0));
    let mut seen = Vec::new();
    assert!(wait(|| { let (v, busy) = ex.poll(); seen.extend(v.into_iter().map(|x| x.0)); !busy }));
    seen.sort();
    assert_eq!(seen, (0..8).collect::<Vec<_>>());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn whole_strip_is_kept_in_the_disk_cache() {
    let Some((_tools, src, dir)) = source("cache") else { eprintln!("ffmpeg absent : test ignore"); return };
    let collect = |st: &mut Strip| {
        let mut got: Vec<(usize, Vec<u8>)> = Vec::new();
        assert!(wait(|| { let (v, busy) = st.poll(); got.extend(v.into_iter().map(|(i, im)| (i, im.data))); !busy }));
        got.sort_by_key(|g| g.0);
        got
    };
    let first = collect(&mut Strip::start(&src, 4.0, 6, (64, 36)));
    assert_eq!(first.iter().map(|g| g.0).collect::<Vec<_>>(), (0..6).collect::<Vec<_>>());
    let cache = test_cache().join("frises");
    let files: Vec<_> = std::fs::read_dir(&cache).unwrap().flatten().filter(|e| e.path().extension().is_some_and(|x| x == "frise")).collect();
    assert!(!files.is_empty(), "frise enregistree dans le cache");
    // deuxieme ouverture : lue du cache, memes images, sans ffmpeg (meme si ffmpeg devenait introuvable)
    let t0 = Instant::now();
    let second = collect(&mut Strip::start(&src, 4.0, 6, (64, 36)));
    assert_eq!(first, second);
    assert!(t0.elapsed() < Duration::from_millis(500), "lecture du cache : {:?}", t0.elapsed());
    // une video modifiee n'est pas confondue avec l'ancienne
    std::thread::sleep(Duration::from_millis(20));
    std::fs::write(&src, b"plus une video").unwrap();
    let mut st = Strip::start(&src, 4.0, 6, (64, 36));
    let third = collect(&mut st);
    assert!(third.is_empty(), "fichier modifie : le cache ne sert plus");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn prefetch_fills_the_cache_used_by_the_windows() {
    let Some((tools, src, dir)) = source("prefetch") else { eprintln!("ffmpeg absent : test ignore"); return };
    prefetch_strips(vec![src.clone()]);
    let i = bdauthor_core::ffmpeg::probe(&tools, &src).unwrap();
    let size = strip_thumb_size((VIEW_W, scaled_height((i.width, i.height), VIEW_W)));
    // la frise apparait dans le cache
    let cache = test_cache().join("frises");
    assert!(wait(|| std::fs::read_dir(&cache).map(|r| r.flatten().count() > 0).unwrap_or(false)));
    std::thread::sleep(Duration::from_secs(1));
    assert!(wait(|| {
        let mut st = Strip::start(&src, i.duration_s, STRIP_N, size);
        let mut n = 0;
        let t0 = Instant::now();
        // complete en moins de 300 ms : lue du cache
        while t0.elapsed() < Duration::from_millis(300) {
            let (v, busy) = st.poll();
            n += v.len();
            if !busy { break; }
            std::thread::sleep(Duration::from_millis(10));
        }
        n == STRIP_N
    }));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn batch_extraction_matches_single_extraction() {
    let Some(tools) = Tools::locate().ok() else { eprintln!("ffmpeg absent : test ignore"); return };
    let dir = std::env::temp_dir().join(format!("bdauthor_framesrc_batch_{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let src = dir.join("src.mp4");
    // images cles espacees de 4 s : l'image cle n'est pas l'image a l'instant demande
    assert!(Command::new(&tools.ffmpeg)
        .args(["-y", "-v", "error", "-f", "lavfi", "-i", "testsrc2=size=320x180:rate=25:duration=10", "-c:v", "libx264", "-g", "100", "-sc_threshold", "0", "-pix_fmt", "yuv420p"])
        .arg(&src).status().unwrap().success());
    let times = [1.3, 2.9, 5.5, 7.1, 9.0];
    for exact in [false, true] {
        let batch = bdauthor_core::ffmpeg::extract_frames_rgba(&tools, &src, &times, (64, 36), exact).unwrap();
        assert_eq!(batch.len(), times.len());
        for (t, b) in times.iter().zip(&batch) {
            let one = bdauthor_core::ffmpeg::extract_frame_rgba(&tools, &src, *t, (64, 36), exact).unwrap();
            assert_eq!(one.data.len(), b.data.len());
            if !exact { assert_eq!(one.data, b.data, "image cle de {t} s : la meme par lot ou seule"); }
        }
        if !exact {
            // images cles : 1,3 et 2,9 s tombent sur l'image cle de 0 s, 5,5 et 7,1 sur celle de 4 s
            assert_eq!(batch[0].data, batch[1].data);
            assert_eq!(batch[2].data, batch[3].data);
            assert_ne!(batch[1].data, batch[2].data);
        } else {
            assert_ne!(batch[0].data, batch[1].data, "images exactes : differentes");
        }
    }
    let _ = std::fs::remove_dir_all(&dir);
}
