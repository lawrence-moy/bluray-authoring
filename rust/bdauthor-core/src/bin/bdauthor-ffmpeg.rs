//! Outil de test du pilotage ffmpeg.
//!   bdauthor-ffmpeg probe  <source>
//!   bdauthor-ffmpeg encode <source> <sortie.ts> [crf] [preset]
//!   bdauthor-ffmpeg film   <source> <sortie.m2ts> [crf] [preset] [langue]   (ecrit aussi <sortie>.clpi)
//!   bdauthor-ffmpeg still  <image.png> <sortie.ts> <secondes> <largeur> <hauteur> [fps]
//!   bdauthor-ffmpeg thumb  <source> <sortie.png> <largeur> <hauteur> [secondes]
use bdauthor_core::ffmpeg::*;
use std::path::Path;
use std::sync::atomic::AtomicBool;

fn run() -> Result<()> {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let tools = Tools::locate()?;
    let cancel = AtomicBool::new(false);
    match a.first().map(String::as_str) {
        Some("probe") if a.len() >= 2 => {
            let i = probe(&tools, Path::new(&a[1]))?;
            let t = pick_target(i.width, i.height, i.fps());
            println!("{i:#?}\ncible : {}x{} {} ({})", t.width, t.height, t.fps, t.label);
        }
        Some("encode") if a.len() >= 3 => {
            let src = Path::new(&a[1]);
            let info = probe(&tools, src)?;
            let mut o = EncodeOpts::default();
            if let Some(c) = a.get(3) { o.crf = c.parse().unwrap_or(18); }
            if let Some(p) = a.get(4) { o.preset = p.clone(); }
            let mut last = -1i32;
            let t = encode_to_ts(&tools, src, Path::new(&a[2]), &info, &o, &mut |p| {
                let pc = (p * 100.0) as i32;
                if pc / 10 != last / 10 { eprintln!("  {pc} %"); last = pc; }
            }, &cancel)?;
            println!("termine : {}", t.label);
        }
        Some("film") if a.len() >= 3 => {
            let mut o = EncodeOpts::default();
            if let Some(c) = a.get(3) { o.crf = c.parse().unwrap_or(18); }
            if let Some(p) = a.get(4) { o.preset = p.clone(); }
            let lang: [u8; 3] = a.get(5).and_then(|l| l.as_bytes().try_into().ok()).unwrap_or(*b"und");
            let mut last = -1i32;
            let (m2ts, st) = bdauthor_core::film::make_film_m2ts(&tools, Path::new(&a[1]), lang, &o, None, &mut |p| {
                let pc = (p * 100.0) as i32;
                if pc / 10 != last / 10 { eprintln!("  {pc} %"); last = pc; }
            }, &cancel)?;
            let clpi = bdauthor_core::tsclip::make_clip_clpi(&m2ts, lang, 0, None, "0100")?;
            std::fs::write(&a[2], &m2ts)?;
            std::fs::write(Path::new(&a[2]).with_extension("clpi"), clpi)?;
            println!("termine : {} images, {} trames audio, {} paquets, avance {:.3} s", st.images, st.trames_audio, st.mux.paquets, st.mux.avance_s);
        }
        Some("still") if a.len() >= 6 => {
            let size = (a[4].parse().unwrap(), a[5].parse().unwrap());
            let fps = a.get(6).map(String::as_str).unwrap_or("60000/1001");
            make_still_ts(&tools, Path::new(&a[1]), Path::new(&a[2]), a[3].parse().unwrap(), size, fps, &EncodeOpts::default(), &cancel)?;
            println!("termine");
        }
        Some("thumb") if a.len() >= 5 => {
            let src = Path::new(&a[1]);
            let info = probe(&tools, src)?;
            extract_thumbnail(&tools, src, Path::new(&a[2]), (a[3].parse().unwrap(), a[4].parse().unwrap()),
                              a.get(5).and_then(|x| x.parse().ok()), info.duration_s)?;
            println!("termine");
        }
        _ => eprintln!("usage : voir l'en-tete de src/bin/bdauthor-ffmpeg.rs"),
    }
    Ok(())
}

fn main() {
    if let Err(e) = run() {
        eprintln!("erreur : {e}");
        std::process::exit(1);
    }
}
