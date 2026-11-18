//! Mesure de vitesse : x264 mesure ; un encodeur materiel absent donne un message, pas un plantage (ffmpeg requis).
use bdauthor_core::bench::*;
use bdauthor_core::ffmpeg::{self, EncodeOpts, Tools};
use bdauthor_core::hwenc::VideoEncoder;
use std::process::Command;
use std::sync::atomic::AtomicBool;

#[test]
fn bench_line_is_read() {
    let err = "frame=  100 fps=50\nbench: utime=3.250s stime=0.250s rtime=2.000s\nbench: maxrss=123456KiB\n";
    assert_eq!(parse_bench(err), Some((3.5, 2.0)));
    assert_eq!(parse_bench("nothing"), None);
}

#[test]
fn bench_args_follow_the_build_pipeline() {
    let info = ffmpeg::VideoInfo { width: 1920, height: 1080, fps_num: 24000, fps_den: 1001, duration_s: 60.0, codec: "h264".into(), pix_fmt: "yuv420p".into(), ..Default::default() };
    let o = EncodeOpts { encoder: VideoEncoder::Qsv, ..Default::default() };
    let (a, _, hw) = ffmpeg::bench_args(std::path::Path::new("in.mkv"), &info, &o, 20.0, 10.0, true);
    let j = a.join(" ");
    assert!(hw && j.starts_with("-hwaccel auto -ss 20.000 -t 10.000 -i in.mkv -map 0:v:0 -an -sn -vf") && j.contains("format=nv12") && j.contains("h264_qsv") && j.contains("-f null"), "{j}");
    let (a, _, hw) = ffmpeg::bench_args(std::path::Path::new("in.mkv"), &info, &EncodeOpts::default(), 0.0, 5.0, false);
    assert!(!hw && a.join(" ").contains("libx264") && !a.contains(&"-hwaccel".to_string()));
}

#[test]
fn measure_runs_x264_and_reports_missing_hardware() {
    let Ok(tools) = Tools::locate() else { eprintln!("ffmpeg absent : test ignore"); return };
    let dir = std::env::temp_dir().join(format!("bdauthor_bench_{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let src = dir.join("src.mkv");
    assert!(Command::new(&tools.ffmpeg).args(["-y", "-v", "error", "-f", "lavfi", "-i", "testsrc2=size=1920x1080:rate=24000/1001:duration=10", "-c:v", "libx264", "-preset", "ultrafast", "-pix_fmt", "yuv420p"])
        .arg(&src).status().unwrap().success());
    let mut steps = Vec::new();
    let (r, note) = measure(&tools, &src, 4.0, "ultrafast", 18, Some((VideoEncoder::Qsv, true)), &mut |f, s| steps.push((f, s.to_string())), &AtomicBool::new(false)).unwrap();
    eprintln!("{r:#?}");
    assert!(note.is_none());
    assert_eq!(r.iter().map(|c| c.case).collect::<Vec<_>>(), vec![Case::X264, Case::Hw, Case::HwGpuDecode]);
    let m = r[0].result.as_ref().unwrap();
    // 4 s a 23,976 i/s : 95 ou 96 images
    assert!((94..=97).contains(&m.frames) && m.fps > 0.0 && m.speed > 0.0 && m.cpu_cores > 0.0 && m.cpu_share <= 1.0, "{m:?}");
    assert!(r[1].result.as_ref().is_err_and(|e| e.contains("MFX")), "{:?}", r[1].result);
    assert!(steps.windows(2).all(|w| w[1].0 >= w[0].0 - 1e-9) && steps.last().unwrap().0 == 1.0);
    // sans encodeur materiel : x264 seul, avec une note
    let (r, note) = measure(&tools, &src, 2.0, "ultrafast", 18, None, &mut |_, _| {}, &AtomicBool::new(false)).unwrap();
    assert!(r.len() == 1 && note.is_some());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_test_pattern_can_be_measured_without_any_video() {
    let Ok(tools) = Tools::locate() else { eprintln!("ffmpeg absent : test ignore"); return };
    assert!(pattern_filter(true).contains("rate=25") && pattern_filter(false).contains("rate=24000/1001"));
    let mut last = 0.0;
    let (r, _) = measure_source(&tools, &Source::Pattern { fps25: false }, 2.0, "ultrafast", 18, None, &mut |f, _| { assert!(f >= last - 1e-9); last = f; }, &AtomicBool::new(false)).unwrap();
    let m = r[0].result.as_ref().unwrap();
    // 2 s a 23,976 i/s
    assert!((47..=49).contains(&m.frames), "{m:?}");
    assert_eq!(last, 1.0);
    // mire a 25 i/s : cible 1080i (PsF) ; sans encodeur materiel, x264 seul
    let (r, _) = measure_source(&tools, &Source::Pattern { fps25: true }, 2.0, "ultrafast", 18, Some((VideoEncoder::Qsv, false)), &mut |_, _| {}, &AtomicBool::new(false)).unwrap();
    assert_eq!(r.len(), 1, "le materiel sans entrelace ne s'applique pas a une cible 1080i");
}
