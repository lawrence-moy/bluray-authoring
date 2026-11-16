//! Pistes DTS / DTS-HD recopiees : trames, profils, paquets PES etendus et un clip complet (ffmpeg requis, sinon ignore).
use bdauthor_core::audio::*;
use bdauthor_core::dts;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::AtomicBool;

/// Sous-trame d'extension DTS-HD synthetique de `n` octets (avec la synchro XLL si `lossless`).
fn ext(n: usize, lossless: bool) -> Vec<u8> {
    let hs = 16usize;
    let v: u32 = ((((hs - 1) as u32) << 16) | (n as u32 - 1)) << 5;
    let mut e = vec![0x64, 0x58, 0x20, 0x25, 0x00];
    e.extend(v.to_be_bytes());
    e.resize(hs, 0);
    if lossless { e.extend([0x41, 0xA2, 0x95, 0x47]); }
    e.resize(n, 0);
    e
}

/// Coeur DTS synthetique : `amode` (disposition), 48 kHz, 512 echantillons, `len` octets.
fn core(amode: u8, len: usize) -> Vec<u8> {
    // FTYPE 1, SHORT 31, CPF 0, NBLKS 15, FSIZE len-1, AMODE, SFREQ 13, le reste a 0
    let mut b: u64 = 1 << 47;
    b |= 31 << 42;
    b |= 15 << 34;
    b |= ((len as u64 - 1) & 0x3FFF) << 20;
    b |= (amode as u64) << 14;
    b |= 13 << 10;
    let mut f = vec![0x7F, 0xFE, 0x80, 0x01];
    f.extend(&b.to_be_bytes()[2..]);
    f.resize(len, 0);
    f
}

#[test]
fn dts_frames_are_split_and_typed() {
    let c = core(9, 2012);
    let h = dts::Core::parse(&c).unwrap();
    assert_eq!((h.len, h.samples, h.amode, h.rate(), h.ticks()), (2012, 512, 9, Some(48000), 960));
    // coeur seul, coeur + extension
    let (a, b) = dts::split(&c).unwrap();
    assert_eq!((a.len(), b.len()), (2012, 0));
    let mut f = c.clone();
    f.extend(ext(400, true));
    let (a, b) = dts::split(&f).unwrap();
    assert_eq!((a.len(), b.len()), (2012, 400));
    // donnees parasites apres le coeur, trame tronquee
    let mut junk = c.clone();
    junk.extend([1, 2, 3, 4, 5]);
    assert!(dts::split(&junk).is_err());
    assert!(dts::split(&c[..1000]).is_err());
    // profils : MA si l'extension porte la partie sans perte, HR sinon, DTS simple sans extension
    let (ma, hr) = (ext(400, true), ext(400, false));
    assert_eq!(dts::stream_type([&[][..], &[][..]]), dts::STREAM_DTS);
    assert_eq!(dts::stream_type([hr.as_slice()]), dts::STREAM_DTS_HD_HRA);
    assert_eq!(dts::stream_type([hr.as_slice(), ma.as_slice()]), dts::STREAM_DTS_HD_MA);
    // attributs : 5.1 -> 0x61, stereo -> 0x31, mono -> 0x11 ; descripteur « HDMV »
    assert_eq!(dts::attrs(&c).unwrap(), (6, 1));
    assert_eq!(dts::attrs(&core(2, 1024)).unwrap(), (3, 1));
    assert_eq!(dts::attrs(&core(0, 1024)).unwrap(), (1, 1));
    assert_eq!(dts::registration_descriptor(0x86, &c).unwrap(), vec![0x05, 0x08, b'H', b'D', b'M', b'V', 0xFF, 0x86, 0x61, 0xFF]);
}

#[test]
fn extended_pes_carries_its_id() {
    use bdauthor_core::vmux::build_pes_ext;
    let p = build_pes_ext(dts::PES_EXT_HD, 900_000, &[1, 2, 3]).unwrap();
    // en-tete identique a celui de ffmpeg (m2ts) : 0xFD, PTS + extension, longueur 8
    assert_eq!(&p[..9], &[0, 0, 1, 0xFD, 0, 14, 0x84, 0x81, 8]);
    assert_eq!(&p[14..17], &[0x01, 0x81, 0x72]);
    assert_eq!(&p[17..], &[1, 2, 3]);
    let pes = bdauthor_core::m2ts::parse_pes(0x1100, 0, &p).unwrap();
    assert_eq!(pes.pts, Some(900_000));
    assert_eq!(dts::pes_extension_id(&pes.header), Some(0x72));
    // un PES ordinaire (0xBD) n'a pas d'identifiant d'extension
    let plain = bdauthor_core::vmux::build_pes(0xBD, 0, None, &[0]).unwrap();
    assert_eq!(dts::pes_extension_id(&bdauthor_core::m2ts::parse_pes(1, 0, &plain).unwrap().header), None);
}

#[test]
fn dts_copy_format_options() {
    let mut t = AudioTrackCfg::embedded(0, "eng");
    t.format = AudioFormat::DtsCopy;
    assert!(t.to_value().to_pretty().contains("\"format\": \"dts_copy\""));
    assert_eq!(AudioTrackCfg::from_value(&t.to_value()).unwrap(), t);
    assert_eq!(bdauthor_core::ffmpeg::audio_codec_args("192k", &[t.clone()]).join(" "), "-ar 48000 -c:a:0 copy");
    assert!(AudioFormat::DtsCopy.is_heavy() && !AudioFormat::DtsCopy.is_lpcm());
    // debit : celui de la source, sinon une estimation prudente
    assert_eq!(t.kbps(6, 1509, 192), 1509);
    assert_eq!(t.kbps(6, 0, 192), DTS_HD_KBPS);
}

fn workdir(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("bdauthor_test_{name}_{}", std::process::id()));
    std::fs::create_dir_all(&d).unwrap();
    d
}

/// Source MKV : video et une piste DTS « HD » (coeur DTS encode par ffmpeg, extension synthetique a chaque trame).
fn hd_source(ffmpeg: &Path, dir: &Path) -> PathBuf {
    let core_file = dir.join("core.dts");
    let ok = Command::new(ffmpeg)
        .args(["-y", "-v", "error", "-f", "lavfi", "-i", "sine=frequency=440:duration=2:sample_rate=48000,aformat=channel_layouts=5.1(side)", "-c:a", "dca", "-strict", "-2", "-b:a", "1509k"])
        .arg(&core_file).status().unwrap().success();
    assert!(ok, "coeur DTS");
    let d = std::fs::read(&core_file).unwrap();
    let (mut out, mut i) = (Vec::new(), 0);
    while i < d.len() {
        let n = dts::Core::parse(&d[i..]).unwrap().len;
        out.extend(&d[i..i + n]);
        out.extend(ext(400, true));
        i += n;
    }
    let hd = dir.join("hd.dts");
    std::fs::write(&hd, out).unwrap();
    let src = dir.join("src.mkv");
    let ok = Command::new(ffmpeg)
        .args(["-y", "-v", "error", "-f", "lavfi", "-i", "testsrc=size=640x360:rate=25:duration=2", "-i"]).arg(&hd)
        .args(["-map", "0:v", "-map", "1:a", "-c:v", "libx264", "-pix_fmt", "yuv420p", "-c:a", "copy"])
        .arg(&src).status().unwrap().success();
    assert!(ok, "source");
    src
}

#[test]
fn dts_hd_copy_makes_a_valid_clip() {
    use bdauthor_core::ffmpeg::{EncodeOpts, Tools};
    use bdauthor_core::film::{load_encoded_m2ts, make_film_m2ts_tracks};
    use bdauthor_core::tsclip::{make_clip_clpi_tracks, scan_clip};
    let Ok(tools) = Tools::locate() else { eprintln!("ffmpeg absent : test ignore"); return };
    let dir = workdir("dts");
    let src = hd_source(&tools.ffmpeg, &dir);
    let mut t0 = AudioTrackCfg::embedded(0, "eng");
    t0.format = AudioFormat::DtsCopy;
    let t1 = AudioTrackCfg::embedded(0, "fra");
    let tracks = vec![t0, t1];
    let langs = [*b"eng", *b"fra"];
    let opts = EncodeOpts { preset: "ultrafast".into(), ..Default::default() };
    let (m2ts, stats) = make_film_m2ts_tracks(&tools, &src, &langs, &tracks, &opts, Some(&dir), &mut |_| {}, &AtomicBool::new(false)).unwrap();
    assert!(stats.trames_audio > 100, "{}", stats.trames_audio);
    assert_eq!(m2ts.len() % 192, 0);

    // clip : DTS-HD MA (0x86) 5.1 puis AC-3 stereo
    let sc = scan_clip(&m2ts).unwrap();
    assert_eq!(sc.audio.iter().map(|a| a.1).collect::<Vec<_>>(), vec![0x86, 0x81]);
    assert_eq!(sc.audio_codes.iter().map(|a| a.2).collect::<Vec<_>>(), vec![0x61, 0x31]);
    assert!(make_clip_clpi_tracks(&m2ts, &langs, b'0', None, "0200", &[]).unwrap().len() > 100);

    // chaque trame : un PES coeur (0x71) puis un PES extension (0x72) de meme PTS
    let pes = bdauthor_core::m2ts::reassemble(&m2ts, sc.audio[0].0).unwrap();
    assert!(pes.len() > 100);
    for w in pes.chunks(2).filter(|w| w.len() == 2) {
        assert_eq!((dts::pes_extension_id(&w[0].header), dts::pes_extension_id(&w[1].header)), (Some(0x71), Some(0x72)));
        assert_eq!(w[0].pts, w[1].pts);
        assert_eq!((w[0].data.len(), w[1].data.len()), (2012, 400));
    }

    // relecture : la piste DTS retrouve ses trames entieres, espacees de 512 echantillons
    let enc = load_encoded_m2ts(&m2ts).unwrap();
    assert_eq!(enc.codecs, vec![0x86, 0x81]);
    assert_eq!(enc.lpcm, vec![false, false]);
    assert!(enc.audio.iter().all(|f| f.data.len() == 2012 && f.ext.len() == 400));
    for w in enc.audio.windows(2) { assert_eq!(w[1].pts - w[0].pts, 960); }
    let _ = std::fs::remove_dir_all(dir);
}
