//! Pistes audio LPCM : en-tetes et trames, options ffmpeg, format dans le projet, et un clip complet (ffmpeg requis, sinon ignore).
use bdauthor_core::audio::*;
use bdauthor_core::lpcm;
use std::path::PathBuf;
use std::process::Command;
use std::sync::atomic::AtomicBool;

/// Trame LPCM synthetique : en-tete de 4 octets puis `samples` echantillons de silence.
fn frame(ch_code: u8, freq: u8, bits_code: u8, bytes_per_sample: usize, samples: usize) -> Vec<u8> {
    let payload = bytes_per_sample * samples;
    let mut f = vec![(payload >> 8) as u8, payload as u8, (ch_code << 4) | freq, bits_code << 6];
    f.extend(std::iter::repeat(0).take(payload));
    f
}

#[test]
fn lpcm_frames_are_split_and_timed() {
    // 5.1 en 24 bits : 6 canaux x 3 octets, 360 echantillons = 7,5 ms = 675 ticks de 90 kHz
    let f51 = frame(9, 1, 3, 18, 360);
    let h = lpcm::Header::parse(&f51).unwrap();
    assert_eq!((h.channels(), h.rate(), h.bits, h.samples(), h.ticks()), (6, 48_000, 24, 360, 675));
    // stereo 16 bits : 240 echantillons = 5 ms = 450 ticks
    let fst = frame(3, 1, 1, 4, 240);
    assert_eq!(lpcm::Header::parse(&fst).unwrap().ticks(), 450);
    // 5.0 : 5 canaux rangees sur 6 places
    assert_eq!(lpcm::Header::parse(&frame(8, 1, 3, 18, 360)).unwrap().sample_bytes(), 18);
    // plusieurs trames dans un PES
    let mut pes = f51.clone();
    pes.extend(&fst);
    let fr = lpcm::frames(&pes).unwrap();
    assert_eq!(fr.len(), 2);
    assert_eq!((fr[0].0.len(), fr[0].1.ticks(), fr[1].0.len(), fr[1].1.ticks()), (f51.len(), 675, fst.len(), 450));
    // 96 kHz : 360 echantillons = 3,75 ms = 337,5 ticks ; les trames se datent d'apres le total d'echantillons
    let f96 = frame(9, 4, 3, 18, 360);
    let h96 = lpcm::Header::parse(&f96).unwrap();
    assert_eq!((h96.rate(), h96.samples(), h96.ticks()), (96_000, 360, 337));
    assert_eq!(lpcm::frames(&[f96.clone(), f96.clone()].concat()).unwrap().len(), 2);
    assert_eq!((lpcm::pts_after(1000, 360, 96_000), lpcm::pts_after(1000, 720, 96_000), lpcm::pts_after(0, 96_000 * 3600, 96_000)), (1338, 1675, 90_000 * 3600));
    assert_eq!(lpcm::pts_after(0, 240, 48_000), 450);
    assert_eq!(lpcm::attrs(&f96).unwrap(), (6, 4));
    // trame tronquee, frequences differentes dans un paquet, frequence inconnue, en-tete invalide
    assert!(lpcm::frames(&f51[..100]).is_err());
    assert!(lpcm::frames(&[f51.clone(), f96.clone()].concat()).is_err());
    assert!(lpcm::frames(&frame(9, 2, 3, 18, 360)).is_err());
    assert!(lpcm::Header::parse(&[0, 4, 0x31, 0]).is_err());
    // attributs du clip : stereo 0x31, 5.1 0x61, mono 0x11 ; descripteur « HDMV »
    assert_eq!(lpcm::attrs(&fst).unwrap(), (3, 1));
    assert_eq!(lpcm::attrs(&f51).unwrap(), (6, 1));
    assert_eq!(lpcm::attrs(&frame(1, 1, 1, 4, 240)).unwrap(), (1, 1));
    assert_eq!(lpcm::registration_descriptor(&f51).unwrap(), vec![0x05, 0x08, b'H', b'D', b'M', b'V', 0xFF, 0x80, 0x61, 0xFF]);
}

#[test]
fn audio_format_is_saved_only_when_not_ac3() {
    let mut t = AudioTrackCfg::embedded(0, "fra");
    assert!(!t.to_value().to_pretty().contains("format"));
    assert_eq!(AudioTrackCfg::from_value(&t.to_value()).unwrap().format, AudioFormat::Ac3);
    t.format = AudioFormat::Lpcm24;
    let json = t.to_value().to_pretty();
    assert!(json.contains("\"format\": \"lpcm24\""), "{json}");
    assert_eq!(AudioTrackCfg::from_value(&t.to_value()).unwrap(), t);
    // format inconnu : refuse
    let bad = bdauthor_core::json::parse(r#"{"track": 0, "format": "mp3"}"#).unwrap();
    assert!(AudioTrackCfg::from_value(&bad).is_err());
    // debits : AC-3 au debit du projet, LPCM selon les canaux (5.1 24 bits : 6912 kb/s, stereo 16 bits : 1536 kb/s, 5.0 comme 5.1)
    assert_eq!(AudioFormat::Ac3.kbps(6, 192), 192);
    assert_eq!(AudioFormat::Lpcm24.kbps(6, 192), 6912);
    assert_eq!(AudioFormat::Lpcm16.kbps(2, 192), 1536);
    assert_eq!(AudioFormat::Lpcm24.kbps(5, 192), 6912);
    // 96 kHz : enregistre seulement s'il est choisi, debit double ; refuse hors LPCM
    let mut t = AudioTrackCfg::embedded(0, "fra");
    t.format = AudioFormat::Lpcm24;
    t.sample_rate = 96_000;
    let json = t.to_value().to_pretty();
    assert!(json.contains("\"sample_rate_hz\": 96000"), "{json}");
    assert_eq!(AudioTrackCfg::from_value(&t.to_value()).unwrap(), t);
    assert_eq!((t.rate(), t.kbps(6, 0, 192), t.kbps(2, 0, 192)), (96_000, 13_824, 4_608));
    assert!(t.problems().is_empty());
    let mut ac3 = t.clone();
    ac3.format = AudioFormat::Ac3;
    assert_eq!(ac3.rate(), 48_000);
    assert!(!ac3.problems().is_empty());
    // 48 kHz ecrit a la main : equivaut a l'absence
    let v = bdauthor_core::json::parse(r#"{"track": 0, "format": "lpcm16", "sample_rate_hz": 48000}"#).unwrap();
    assert_eq!(AudioTrackCfg::from_value(&v).unwrap().sample_rate, 0);
}

#[test]
fn ffmpeg_audio_options_per_track() {
    use bdauthor_core::ffmpeg::audio_codec_args;
    let ac3 = AudioTrackCfg::embedded(0, "fra");
    // sans LPCM : exactement les options d'avant
    assert_eq!(audio_codec_args("192k", &[]).join(" "), "-c:a ac3 -b:a 192k -ar 48000 -ac 2");
    assert_eq!(audio_codec_args("192k", &[ac3.clone(), ac3.clone()]).join(" "), "-c:a ac3 -b:a 192k -ar 48000 -ac 2");
    // avec LPCM : un codec par piste
    let mut l16 = ac3.clone();
    l16.format = AudioFormat::Lpcm16;
    let mut l24 = ac3.clone();
    l24.format = AudioFormat::Lpcm24;
    let a = audio_codec_args("192k", &[l24, ac3, l16]).join(" ");
    assert_eq!(a, "-ar 48000 -c:a:0 pcm_bluray -sample_fmt:a:0 s32 -c:a:1 ac3 -b:a:1 192k -ac:a:1 2 -c:a:2 pcm_bluray -sample_fmt:a:2 s16");
    // LPCM 96 kHz : frequence propre a la piste
    let mut l96 = AudioTrackCfg::embedded(0, "fra");
    l96.format = AudioFormat::Lpcm24;
    l96.sample_rate = 96_000;
    assert_eq!(audio_codec_args("192k", &[l96, AudioTrackCfg::embedded(0, "eng")]).join(" "),
               "-ar 48000 -c:a:0 pcm_bluray -sample_fmt:a:0 s32 -ar:a:0 96000 -c:a:1 ac3 -b:a:1 192k -ac:a:1 2");
}

fn workdir(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("bdauthor_test_{name}_{}", std::process::id()));
    std::fs::create_dir_all(&d).unwrap();
    d
}

#[test]
fn lpcm_tracks_make_a_valid_clip() {
    use bdauthor_core::ffmpeg::{EncodeOpts, Tools};
    use bdauthor_core::film::{load_encoded_m2ts, make_film_m2ts_tracks};
    use bdauthor_core::tsclip::{make_clip_clpi_tracks, scan_clip};
    let Ok(tools) = Tools::locate() else { eprintln!("ffmpeg absent : test ignore"); return };
    let dir = workdir("lpcm");
    // source : video, une piste 5.1 et une piste stereo (flac)
    let src = dir.join("src.mkv");
    let ok = Command::new(&tools.ffmpeg)
        .args(["-y", "-v", "error", "-f", "lavfi", "-i", "testsrc=size=640x360:rate=25:duration=2", "-f", "lavfi", "-i", "sine=frequency=440:duration=2:sample_rate=48000,aformat=channel_layouts=5.1(side)",
               "-f", "lavfi", "-i", "sine=frequency=880:duration=2:sample_rate=48000,aformat=channel_layouts=stereo", "-map", "0:v", "-map", "1:a", "-map", "2:a", "-c:v", "libx264", "-pix_fmt", "yuv420p", "-c:a", "flac"])
        .arg(&src).status().unwrap().success();
    assert!(ok, "source");
    let mut t0 = AudioTrackCfg::embedded(0, "eng");
    t0.format = AudioFormat::Lpcm24;
    let t1 = AudioTrackCfg::embedded(1, "fra");
    let mut t2 = AudioTrackCfg::embedded(1, "deu");
    t2.format = AudioFormat::Lpcm16;
    let tracks = vec![t0, t1, t2];
    let langs = [*b"eng", *b"fra", *b"deu"];
    let opts = EncodeOpts { preset: "ultrafast".into(), ..Default::default() };
    let (m2ts, stats) = make_film_m2ts_tracks(&tools, &src, &langs, &tracks, &opts, Some(&dir), &mut |_| {}, &AtomicBool::new(false)).unwrap();
    assert!(stats.trames_audio > 100, "{}", stats.trames_audio);
    assert_eq!(m2ts.len() % 192, 0);

    // le clip : trois flux audio, de types 0x80, 0x81 et 0x80 ; attributs 5.1 / stereo / stereo
    let sc = scan_clip(&m2ts).unwrap();
    assert_eq!(sc.audio.iter().map(|a| a.1).collect::<Vec<_>>(), vec![0x80, 0x81, 0x80]);
    assert_eq!(sc.audio_codes.iter().map(|a| a.2).collect::<Vec<_>>(), vec![0x61, 0x31, 0x31]);
    assert_eq!(sc.audio_codes.iter().map(|a| a.0).collect::<Vec<_>>(), vec![0x1100, 0x1101, 0x1102]);
    // CLPI : produit sans erreur, avec les trois flux
    let clpi = make_clip_clpi_tracks(&m2ts, &langs, b'0', None, "0200", &[]).unwrap();
    assert!(clpi.len() > 100);

    // relecture du clip : memes pistes, les trames LPCM se suivent sans trou et couvrent environ 2 s
    let enc = load_encoded_m2ts(&m2ts).unwrap();
    assert_eq!(enc.lpcm, vec![true, false, true]);
    for (k, tr) in std::iter::once(&enc.audio).chain(enc.extra_audio.iter()).enumerate() {
        assert!(!tr.is_empty(), "piste {k}");
    }
    for tr in [&enc.audio, &enc.extra_audio[1]] {
        let mut total = 0u64;
        for w in tr.windows(2) {
            let ticks = lpcm::Header::parse(&w[0].data).unwrap().ticks();
            assert_eq!(w[1].pts - w[0].pts, ticks, "trames contigues");
            total += ticks;
        }
        let secs = total as f64 / 90_000.0;
        assert!((1.5..2.5).contains(&secs), "duree {secs}");
    }
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn lpcm_96khz_track_makes_a_valid_clip() {
    use bdauthor_core::ffmpeg::{EncodeOpts, Tools};
    use bdauthor_core::film::{load_encoded_m2ts, make_film_m2ts_tracks};
    use bdauthor_core::tsclip::{make_clip_clpi_tracks, scan_clip};
    let Ok(tools) = Tools::locate() else { eprintln!("ffmpeg absent : test ignore"); return };
    let dir = workdir("lpcm96");
    // source : video et une piste 5.1 a 96 kHz (flac)
    let src = dir.join("src.mkv");
    let ok = Command::new(&tools.ffmpeg)
        .args(["-y", "-v", "error", "-f", "lavfi", "-i", "testsrc=size=640x360:rate=25:duration=3", "-f", "lavfi", "-i", "sine=frequency=440:duration=3:sample_rate=96000,aformat=channel_layouts=5.1(side)",
               "-map", "0:v", "-map", "1:a", "-c:v", "libx264", "-pix_fmt", "yuv420p", "-c:a", "flac"])
        .arg(&src).status().unwrap().success();
    assert!(ok, "source");
    let mut t0 = AudioTrackCfg::embedded(0, "eng");
    t0.format = AudioFormat::Lpcm24;
    t0.sample_rate = 96_000;
    let t1 = AudioTrackCfg::embedded(0, "fra");
    let tracks = vec![t0, t1];
    let langs = [*b"eng", *b"fra"];
    let opts = EncodeOpts { preset: "ultrafast".into(), ..Default::default() };
    let (m2ts, _) = make_film_m2ts_tracks(&tools, &src, &langs, &tracks, &opts, Some(&dir), &mut |_| {}, &AtomicBool::new(false)).unwrap();
    // clip : LPCM 5.1 a 96 kHz (attributs 0x64) puis AC-3 stereo a 48 kHz
    let sc = scan_clip(&m2ts).unwrap();
    assert_eq!(sc.audio.iter().map(|a| a.1).collect::<Vec<_>>(), vec![0x80, 0x81]);
    assert_eq!(sc.audio_codes.iter().map(|a| a.2).collect::<Vec<_>>(), vec![0x64, 0x31]);
    assert!(make_clip_clpi_tracks(&m2ts, &langs, b'0', None, "0200", &[]).unwrap().len() > 100);
    // relecture : trames a 96 kHz datees sans derive (a un tick pres du compte des echantillons), environ 3 s
    let enc = load_encoded_m2ts(&m2ts).unwrap();
    let a = &enc.audio;
    assert!(a.len() > 100);
    let mut n = 0u64;
    for f in a {
        let h = lpcm::Header::parse(&f.data).unwrap();
        assert_eq!(h.rate(), 96_000);
        let want = lpcm::pts_after(a[0].pts, n, 96_000);
        assert!(f.pts.abs_diff(want) <= 1, "{} {}", f.pts, want);
        n += h.samples() as u64;
    }
    let secs = n as f64 / 96_000.0;
    assert!((2.5..3.5).contains(&secs), "duree {secs}");
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn lpcm_channel_limit_options() {
    use bdauthor_core::audio::LpcmChannels;
    let mut t = AudioTrackCfg::embedded(0, "fra");
    t.format = AudioFormat::Lpcm24;
    t.lpcm_channels = LpcmChannels::Max51;
    assert!(t.to_value().to_pretty().contains("\"lpcm_channels\": \"5.1\""));
    assert_eq!(AudioTrackCfg::from_value(&t.to_value()).unwrap(), t);
    // debit : 7.1 plafonne a 5.1, une source 5.1 ou stereo inchangee
    assert_eq!((t.kbps(8, 0, 192), t.kbps(6, 0, 192), t.kbps(2, 0, 192)), (6912, 6912, 2304));
    t.lpcm_channels = LpcmChannels::Stereo;
    // une source mono reste mono (sur Blu-ray, un nombre impair de canaux occupe la place du nombre pair suivant)
    assert_eq!((t.kbps(8, 0, 192), t.kbps(1, 0, 192)), (2304, 2304));
    assert!(t.problems().is_empty());
    assert_eq!(bdauthor_core::ffmpeg::audio_codec_args("192k", &[t.clone()]).join(" "), "-ar 48000 -c:a:0 pcm_bluray -sample_fmt:a:0 s32 -filter:a:0 aresample=ochl=stereo:lfe_mix_level=0.7071:rematrix_maxval=2");
    t.lpcm_channels = LpcmChannels::StereoDpl2;
    assert!(bdauthor_core::ffmpeg::audio_codec_args("192k", &[t.clone()]).join(" ").contains("matrix_encoding=dplii"));
    // hors LPCM : refuse
    let mut a = t.clone();
    a.format = AudioFormat::Ac3;
    assert!(!a.problems().is_empty());
    assert!(!bdauthor_core::ffmpeg::audio_codec_args("192k", &[a]).join(" ").contains("-filter"));
    // ancien projet sans le champ : comme la source
    let v = bdauthor_core::json::parse(r#"{"track": 0, "format": "lpcm16"}"#).unwrap();
    assert_eq!(AudioTrackCfg::from_value(&v).unwrap().lpcm_channels, LpcmChannels::Source);
}

#[test]
fn lpcm_71_source_is_mixed_down() {
    use bdauthor_core::audio::LpcmChannels;
    use bdauthor_core::ffmpeg::{EncodeOpts, Tools};
    use bdauthor_core::film::{load_encoded_m2ts, make_film_m2ts_tracks};
    use bdauthor_core::tsclip::scan_clip;
    let Ok(tools) = Tools::locate() else { eprintln!("ffmpeg absent : test ignore"); return };
    let dir = workdir("lpcm71");
    let src = dir.join("src.mkv");
    let ok = Command::new(&tools.ffmpeg)
        .args(["-y", "-v", "error", "-f", "lavfi", "-i", "testsrc=size=640x360:rate=25:duration=2", "-f", "lavfi", "-i", "sine=frequency=440:duration=2:sample_rate=48000,aformat=channel_layouts=7.1",
               "-map", "0:v", "-map", "1:a", "-c:v", "libx264", "-pix_fmt", "yuv420p", "-c:a", "flac"])
        .arg(&src).status().unwrap().success();
    assert!(ok, "source");
    let opts = EncodeOpts { preset: "ultrafast".into(), ..Default::default() };
    let mut want = Vec::new();
    for (lim, ch, attr) in [(LpcmChannels::Source, 8, 0x61), (LpcmChannels::Max51, 6, 0x61), (LpcmChannels::Stereo, 2, 0x31), (LpcmChannels::StereoDpl2, 2, 0x31)] {
        let mut t = AudioTrackCfg::embedded(0, "fra");
        t.format = AudioFormat::Lpcm24;
        t.lpcm_channels = lim;
        let (m2ts, _) = make_film_m2ts_tracks(&tools, &src, &[*b"fra"], &[t], &opts, Some(&dir), &mut |_| {}, &AtomicBool::new(false)).unwrap();
        let sc = scan_clip(&m2ts).unwrap();
        let enc = load_encoded_m2ts(&m2ts).unwrap();
        let h = lpcm::Header::parse(&enc.audio[0].data).unwrap();
        want.push((lim, h.channels(), sc.audio_codes[0].2));
        assert_eq!((h.channels(), sc.audio_codes[0].2), (ch, attr), "{lim:?}");
    }
    eprintln!("{want:?}");
    let _ = std::fs::remove_dir_all(dir);
}
