use bdauthor_core::subtitle::*;

#[test]
fn webvtt() {
    let t = "WEBVTT - titre\n\nNOTE un commentaire\n\nSTYLE\n::cue { color: red }\n\nintro\n00:01.000 --> 00:03.500 align:start position:10%\n<v Paul>Bonjour &lrm;<i>tout le monde</i>\nsur deux lignes\n\n01:00:02.000 --> 01:00:04.000\nFin\n";
    let c = parse_file_text("vtt", t);
    assert_eq!(c.len(), 2);
    assert_eq!((c[0].start_ms, c[0].end_ms), (1000, 3500));
    assert_eq!(plain_text(&c[0].text), "Bonjour tout le monde\nsur deux lignes");
    assert_eq!(c[1].start_ms, 3_602_000);
}

#[test]
fn microdvd_with_fps_line() {
    let t = "{1}{1}25\n{25}{75}Bonjour|deux lignes\n{100}{150}{y:i}Italique\n";
    let c = parse_file_text("sub", t);
    assert_eq!(c.len(), 2);
    assert_eq!((c[0].start_ms, c[0].end_ms), (1000, 3000));
    assert_eq!(c[0].text, "Bonjour\ndeux lignes");
    assert_eq!(plain_text(&c[1].text), "Italique");
    assert_eq!(parse_runs(&c[1].text)[0][0].italic, true);
    // sans ligne de cadence : 23,976 i/s
    let d = parse_microdvd("{24}{48}x", 0.0);
    assert_eq!(d[0].start_ms, 1001);
}

#[test]
fn subviewer_and_sbv() {
    let sv = "[INFORMATION]\n[TITLE]x\n[END INFORMATION]\n[SUBTITLE]\n[COLF]&HFFFFFF,[STYLE]no,[SIZE]18,[FONT]Arial\n00:00:01.50,00:00:04.00\nLigne un[br]Ligne deux\n\n00:00:05.00,00:00:06.00\nSuite\n";
    let c = parse_file_text("sub", sv);
    assert_eq!(c.len(), 2);
    assert_eq!((c[0].start_ms, c[0].end_ms, c[0].text.as_str()), (1500, 4000, "Ligne un\nLigne deux"));
    let sbv = "0:00:01.000,0:00:03.000\nPremier\n\n0:00:04.000,0:00:05.000\nSecond\n";
    let c = parse_file_text("sbv", sbv);
    assert_eq!(c.len(), 2);
    assert_eq!(c[1].start_ms, 4000);
}

#[test]
fn sami() {
    let t = "<SAMI><HEAD><TITLE>x</TITLE></HEAD><BODY>\n<SYNC Start=1000><P Class=FRCC>Bonjour<br>monde\n<SYNC Start=3000><P Class=FRCC>&nbsp;\n<SYNC Start=4000><P Class=FRCC>Suite\n<SYNC Start=4500><P Class=ENCC>English\n<SYNC Start=6000><P Class=FRCC>&nbsp;\n</BODY></SAMI>";
    let c = parse_file_text("smi", t);
    assert_eq!(c.len(), 2);
    assert_eq!((c[0].start_ms, c[0].end_ms, c[0].text.as_str()), (1000, 3000, "Bonjour\nmonde"));
    assert_eq!(c[1].text, "Suite");
}

#[test]
fn srt_still_fine_and_wrong_extension() {
    let t = "1\n00:00:01,000 --> 00:00:02,000\nUn\n\n2\n00:00:03,000 --> 00:00:04,000\nDeux\n";
    assert_eq!(parse_file_text("txt", t).len(), 2);
    assert_eq!(parse_file_text("srt", t).len(), 2);
}

#[test]
fn track_fps_json_and_parse() {
    let mut t = SubTrackCfg::from_file("a.sub", "fra");
    assert!(!t.to_value().to_pretty().contains("cadence_ips"));
    t.fps = Some(25.0);
    let back = SubTrackCfg::from_value(&t.to_value()).unwrap();
    assert_eq!(back.fps, Some(25.0));
    let c = parse_file_text_fps("sub", "{25}{50}x", Some(25.0));
    assert_eq!((c[0].start_ms, c[0].end_ms), (1000, 2000));
}

#[test]
fn plausible_video_fps() {
    assert_eq!(plausible_fps(25.0), Some(25.0));
    assert!(plausible_fps(23.976023).is_some());
    assert!(plausible_fps(1000.0).is_none());
    assert!(plausible_fps(0.0).is_none());
    assert!(plausible_fps(26.5).is_none());
}
