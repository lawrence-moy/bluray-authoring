use bdauthor_core::ffmpeg::{FrameStream, Tools};
use bdauthor_core::playback::{PcmFeed, PlayClock};
use std::process::Command;
use std::time::{Duration, Instant};

#[test]
fn clock_follows_audio_then_wall_time_without_jumps() {
    let t0 = Instant::now();
    let mut c = PlayClock::new(10.0);
    assert_eq!(c.time(Some(0.0), t0), 10.0);
    assert!((c.time(Some(1.5), t0) - 11.5).abs() < 1e-9);
    // le son s'arrete : le temps ecoule prend la suite depuis 11.5
    assert!((c.time(None, t0) - 11.5).abs() < 1e-9);
    assert!((c.time(None, t0 + Duration::from_millis(500)) - 12.0).abs() < 1e-6);
    // un nouveau son a 12.0
    c.restart_audio(12.0);
    assert!((c.time(Some(0.25), t0 + Duration::from_millis(600)) - 12.25).abs() < 1e-9);
    // jamais en arriere
    assert!((c.time(Some(0.20), t0 + Duration::from_millis(610)) - 12.25).abs() < 1e-9);
}

#[test]
fn pcm_feed_decodes_the_chosen_track_at_the_output_format() {
    let Ok(tools) = Tools::locate() else { eprintln!("ffmpeg absent : test ignore"); return };
    let dir = std::env::temp_dir().join(format!("bdauthor_pcm_{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let f = dir.join("a.mkv");
    // piste 0 : silence, piste 1 : sinus fort, 3 s
    assert!(Command::new(&tools.ffmpeg).args(["-y", "-v", "error", "-f", "lavfi", "-i", "anullsrc=r=48000:cl=stereo", "-f", "lavfi", "-i", "sine=frequency=440:sample_rate=44100:duration=3",
        "-t", "3", "-map", "0", "-map", "1", "-c:a", "flac"]).arg(&f).status().unwrap().success());
    let feed = PcmFeed::start(&tools, &f, Some(1), 1.0, 48000, 6).unwrap();
    let src = feed.source();
    src.set_latency(Duration::from_millis(20));
    // une « carte son » qui tire 10 ms a la fois
    let mut buf = vec![0f32; 480 * 6];
    let (mut total, mut peak) = (0usize, 0f32);
    let deadline = Instant::now() + Duration::from_secs(20);
    while !feed.finished() && Instant::now() < deadline {
        src.fill(&mut buf);
        total += buf.chunks_exact(6).filter(|f| f.iter().any(|s| *s != 0.0)).count();
        peak = buf.iter().fold(peak, |m, s| m.max(s.abs()));
        std::thread::sleep(Duration::from_millis(1));
    }
    assert!(feed.finished() && feed.started());
    // 2 s restantes apres -ss 1 ; la position tient compte du delai de sortie
    assert!((total as f64 / 48000.0 - 2.0).abs() < 0.05, "{total}");
    assert!((feed.position() - (2.0 - 0.02)).abs() < 0.05, "{}", feed.position());
    assert!(peak > 0.1, "piste 1 attendue (sinus), pic {peak}");
    // piste absente : fini tout de suite, sans son
    let none = PcmFeed::start(&tools, &f, Some(5), 0.0, 48000, 2).unwrap();
    let deadline = Instant::now() + Duration::from_secs(10);
    while !none.finished() && Instant::now() < deadline { std::thread::sleep(Duration::from_millis(10)); }
    assert!(none.finished() && !none.started());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn paced_frames_arrive_faster_than_real_time() {
    let Ok(tools) = Tools::locate() else { return };
    let dir = std::env::temp_dir().join(format!("bdauthor_paced_{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let f = dir.join("v.mkv");
    assert!(Command::new(&tools.ffmpeg).args(["-y", "-v", "error", "-f", "lavfi", "-i", "testsrc=size=320x180:rate=25:duration=4", "-c:v", "libx264", "-preset", "ultrafast"]).arg(&f).status().unwrap().success());
    let t = Instant::now();
    let fs = FrameStream::start_paced(&tools, &f, 1.0, (64, 36), false, false).unwrap();
    let n = fs.rx.iter().count();
    assert_eq!(n, 75);
    assert!(t.elapsed() < Duration::from_secs(2), "{:?}", t.elapsed());
    let _ = std::fs::remove_dir_all(&dir);
}
