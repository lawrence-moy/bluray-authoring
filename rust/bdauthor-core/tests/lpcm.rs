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
    assert_eq!((fr[0].0.len(), fr[0].1, fr[1].0.len(), fr[1].1), (f51.len(), 675, fst.len(), 450));
    // trame tronquee, 96 kHz, en-tete invalide
    assert!(lpcm::frames(&f51[..100]).is_err());
    assert!(lpcm::frames(&frame(9, 4, 3, 18, 360)).is_err());
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
