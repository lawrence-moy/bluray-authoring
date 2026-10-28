//! Sous-titres PGS de bout en bout avec un vrai ffmpeg : encodage, remuxage avec sous-titres, relecture par ffmpeg (decodeur PGS).
use bdauthor_core::ffmpeg::*;
use bdauthor_core::film::*;
use bdauthor_core::palette::RgbaImage;
use bdauthor_core::project::Renderer;
use bdauthor_core::subtitle::*;
use bdauthor_core::tsclip::*;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;

struct Flat;
impl Renderer for Flat {
    fn thumb_button(&self, _: &RgbaImage, _: &str, _: u32, _: u32, _: u32, _: &bdauthor_core::style::ButtonStyle) -> [Arc<RgbaImage>; 3] { unimplemented!() }
    fn text_button(&self, _: &str, _: u32, _: u32, _: &bdauthor_core::style::ButtonStyle) -> [Arc<RgbaImage>; 3] { unimplemented!() }
    fn background(&self, _: &str, _: (u32, u32)) -> RgbaImage { unimplemented!() }
    fn compose_background(&self, _: Option<&Path>, _: (u32, u32), _: &[bdauthor_core::project::Decor]) -> std::result::Result<RgbaImage, String> { unimplemented!() }
    fn subtitle(&self, text: &str, style: &SubStyle, max_w: u32) -> Option<RgbaImage> {
        let w = (text.chars().count() as u32 * 12 + 40).min(max_w);
        let h = style.size as u32;
        let mut im = RgbaImage::new(w, h);
        for y in 0..h { for x in 0..w { im.set(x, y, [255, 0, 255, 255]); } } // magenta : absent de la mire
        Some(im)
    }
}

fn workdir(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("bdauthor_test_{name}_{}", std::process::id()));
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn frame(ffmpeg: &Path, file: &Path, t: f64, with_sub: bool) -> Vec<u8> {
    let mut c = Command::new(ffmpeg);
    c.args(["-v", "error", "-i"]).arg(file);
    if with_sub {
        c.args(["-filter_complex", "[0:v:0][0:s:0]overlay"]);
    } else {
        c.args(["-map", "0:v:0"]);
    }
    c.args(["-ss", &format!("{t}"), "-frames:v", "1", "-f", "rawvideo", "-pix_fmt", "rgb24", "-"]);
    let o = c.output().unwrap();
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    o.stdout
}

#[test]
fn sous_titres_decodes_par_ffmpeg() {
    let Ok(tools) = Tools::locate() else { eprintln!("ffmpeg absent : test ignore"); return };
    let dir = workdir("subs");
    let src = dir.join("src.mp4");
    assert!(Command::new(&tools.ffmpeg)
        .args(["-y", "-v", "error", "-f", "lavfi", "-i", "testsrc=size=640x360:rate=24:duration=5", "-f", "lavfi", "-i", "sine=duration=5",
               "-c:v", "libx264", "-pix_fmt", "yuv420p", "-c:a", "aac", "-shortest"])
        .arg(&src).status().unwrap().success());
    let (base, _) = make_film_m2ts(&tools, &src, *b"fra", &EncodeOpts { preset: "ultrafast".into(), ..Default::default() }, Some(&dir), &mut |_| {}, &AtomicBool::new(false)).unwrap();
    // le remuxage sans sous-titres redonne exactement le meme clip
    let enc = load_encoded_m2ts(&base).unwrap();
    let (again, _) = mux_encoded(&enc, *b"fra").unwrap();
    assert!(again == base, "remuxage non identique");

    let cues = vec![
        Cue { start_ms: 1000, end_ms: 2500, text: "Bonjour".into() },
        Cue { start_ms: 3000, end_ms: 3600, text: "Deux".into() },
    ];
    let mut style = SubStyle::default();
    style.size = 90;
    let tracks = encode_sub_tracks(&enc, &[SubSpec { lang: *b"fra", cues: cues.clone(), style: style.clone() }, SubSpec { lang: *b"eng", cues: vec![], style }], &Flat).unwrap();
    assert_eq!(tracks.len(), 2);
    let (m2ts, stats) = mux_encoded_subs(&enc, *b"fra", &tracks[..1]).unwrap();
    assert!(m2ts.len() > base.len() && m2ts.len() % 192 == 0);
    assert!(stats.ecart_pcr_max_ms < 100.0);

    let sc = scan_clip(&m2ts).unwrap();
    assert_eq!(sc.pg, vec![0x1200]);
    assert_eq!((sc.start90, sc.end90), {
        let b = scan_clip(&base).unwrap();
        (b.start90, b.end90)
    });
    let clpi = make_clip_clpi_pg(&m2ts, *b"fra", 0, None, "0100", &[*b"fra"]).unwrap();
    let parsed = bdauthor_core::clpi::Clpi::parse(&clpi).unwrap();
    let st = &parsed.program.programs[0].streams;
    assert_eq!(st.len(), 3);
    assert_eq!((st[2].pid, st[2].coding_type), (0x1200, 0x90));
    assert_eq!(&st[2].attrs[..3], b"fra");

    // ffmpeg voit le flux et l'affiche aux bons instants
    let out = dir.join("film.m2ts");
    std::fs::write(&out, &m2ts).unwrap();
    let probe = Command::new(&tools.ffprobe).args(["-v", "error", "-show_entries", "stream=codec_name,codec_type:stream_tags=language", "-of", "csv=p=0"]).arg(&out).output().unwrap();
    let txt = String::from_utf8_lossy(&probe.stdout).to_string();
    assert!(txt.contains("hdmv_pgs_subtitle"), "{txt}");
    let (w, h) = (sc.width as usize, sc.height as usize);
    let diff = |t: f64| -> (usize, usize, usize) {
        let (a, b) = (frame(&tools.ffmpeg, &out, t, true), frame(&tools.ffmpeg, &out, t, false));
        assert_eq!(a.len(), w * h * 3);
        let ys: Vec<usize> = a.chunks_exact(3).zip(b.chunks_exact(3)).enumerate().filter(|(_, (p, q))| (0..3).map(|k| (p[k] as i32 - q[k] as i32).abs()).sum::<i32>() > 60).map(|(i, _)| i / w).collect();
        (ys.len(), ys.iter().copied().min().unwrap_or(0), ys.iter().copied().max().unwrap_or(0))
    };
    let (a, b, c, d) = (diff(1.5), diff(0.5), diff(2.8), diff(3.3));
    assert!(a.0 > 3000, "sous-titre absent a 1,5 s : {a:?}");
    assert_eq!(b.0, 0, "sous-titre visible a 0,5 s : {b:?}");
    assert_eq!(c.0, 0, "sous-titre visible a 2,8 s : {c:?}");
    assert!(d.0 > 1000 && d.0 < a.0, "second sous-titre : {d:?}, premier {a:?}");
    assert!(a.2 < h - 20 && a.1 > h * 2 / 3, "position verticale {}..{} sur {h}", a.1, a.2);
    let _ = std::fs::remove_dir_all(dir);
}
