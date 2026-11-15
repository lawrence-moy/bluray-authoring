use bdauthor_core::model::ProjectModel;
use bdauthor_core::occupancy::{crf_kbps, estimate, VideoFacts};

fn model(json: &str) -> ProjectModel { ProjectModel::from_json(json).unwrap() }

#[test]
fn crf_rate_halves_every_six_points() {
    assert!((crf_kbps(18, 1080) - 9000.0).abs() < 1.0);
    assert!((crf_kbps(24, 1080) - 4500.0).abs() < 1.0);
    assert!((crf_kbps(18, 720) - 4000.0).abs() < 1.0);
}

#[test]
fn estimate_adds_videos_audio_subs_and_menus() {
    let m = model(r#"{ "title": "T", "language": "fra", "videos": [ {"file": "a.mp4"}, {"file": "b.mp4", "audio_tracks": [
        {"track":0,"language":"fra","language_id":1}, {"track":1,"language":"eng","language_id":2,"format":"lpcm24"} ]} ],
        "encoding": {"crf": 18} }"#);
    let hour = |i: usize| VideoFacts { duration: Some(3600.0), height: Some(1080), source_kbps: if i == 0 { Some(2000) } else { None } };
    let u = estimate(&m, &hour);
    assert_eq!(u.disc, "BD-25");
    assert_eq!(u.unknown, 0);
    let part = |k: usize| u.parts[k].bytes;
    // video : a.mp4 plafonnee pres de sa source (2600 kb/s), b.mp4 au CRF (9000 kb/s)
    assert_eq!(part(0), ((2600.0 + 9000.0) * 1000.0 / 8.0 * 3600.0) as u64);
    // audio : AC-3 192 (a), AC-3 192 + LPCM 24 bits 5.1 (b)
    assert_eq!(part(1), ((192 + 192 + 6 * 24 * 48) as u64) * 1000 / 8 * 3600);
    assert_eq!(u.total, u.parts.iter().map(|p| p.bytes).sum::<u64>());
    assert!(u.ratio() > 0.3 && !u.over_capacity());
    // duree inconnue : non comptee
    let u2 = estimate(&m, &|_| VideoFacts::default());
    assert_eq!((u2.unknown, u2.parts[0].bytes, u2.parts[1].bytes), (2, 0, 0));
}

#[test]
fn fill_disc_uses_the_room_left_under_the_margin() {
    let m = model(r#"{ "title": "T", "language": "fra", "videos": [ {"file": "a.mp4"} ], "encoding": {"fill_disc": true} }"#);
    let u = estimate(&m, &|_| VideoFacts { duration: Some(5400.0), height: Some(1080), source_kbps: None });
    assert!(u.fill);
    let r = u.total as f64 / u.usable() as f64;
    assert!((r - 1.0).abs() < 0.002, "{r}");
    assert!(!u.over_margin() || r < 1.002);
}
