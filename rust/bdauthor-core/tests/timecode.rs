use bdauthor_core::timecode::{format, parse};

#[test]
fn formats_hours_minutes_seconds_frames() {
    assert_eq!(format(0.0, 25.0), "00:00:00:00");
    assert_eq!(format(1088.8, 25.0), "00:18:08:20");
    assert_eq!(format(7504.0, 25.0), "02:05:04:00");
    assert_eq!(format(3600.0 + 61.5, 24.0), "01:01:01:12");
    // 23,976 i/s : la 24e image tombe sur la seconde suivante (reel, pas de drop frame)
    assert_eq!(format(24.0 / 23.976, 23.976), "00:00:01:00");
    assert_eq!(format(23.0 / 23.976, 23.976), "00:00:00:23");
    // cadence inconnue : 25 i/s
    assert_eq!(format(1.2, 0.0), "00:00:01:05");
}

#[test]
fn parses_timecodes_and_plain_seconds() {
    assert_eq!(parse("00:18:08:20", 25.0), Some(1088.8));
    assert_eq!(parse("1:01:01:12", 24.0), Some(3661.5));
    assert_eq!(parse("01:12", 24.0), Some(1.5));
    assert_eq!(parse("2:05:04:00", 25.0), Some(7504.0));
    assert_eq!(parse("90", 25.0), Some(90.0));
    assert_eq!(parse("90,5", 25.0), Some(90.5));
    assert_eq!(parse("1;02;03;04", 25.0), Some(3723.16));
    assert_eq!(parse("", 25.0), None);
    assert_eq!(parse("a:b", 25.0), None);
    assert_eq!(parse("1:2:3:4:5", 25.0), None);
}

#[test]
fn format_then_parse_is_stable() {
    for fps in [23.976, 24.0, 25.0, 29.97, 30.0, 50.0] {
        for n in [0u32, 1, 17, 1000, 123456] {
            let t = n as f64 / fps;
            let back = parse(&format(t, fps), fps).unwrap();
            assert!((back - t).abs() <= 0.5 / fps + 1e-9, "fps {fps} n {n}: {t} -> {back}");
        }
    }
}
