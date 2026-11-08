//! Construction complete d'un disque avec un vrai ffmpeg (ignore si ffmpeg est absent) et un moteur de dessin minimal.
mod common;
use bdauthor_core::ffmpeg::Tools;
use bdauthor_core::palette::RgbaImage;
use bdauthor_core::project::*;
use bdauthor_core::style::ButtonStyle;
use std::path::PathBuf;
use std::process::Command;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;

static OVERLAYS: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
struct Flat;
fn fill(w: u32, h: u32, c: [u8; 3]) -> Arc<RgbaImage> {
    let mut im = RgbaImage::new(w, h);
    for y in 0..h { for x in 0..w { im.set(x, y, [c[0], c[1], c[2], 255]); } }
    Arc::new(im)
}
impl Renderer for Flat {
    fn thumb_button(&self, thumb: &RgbaImage, _label: &str, w: u32, h_total: u32, h_thumb: u32, st: &ButtonStyle) -> [Arc<RgbaImage>; 3] {
        let mk = |bg: [u8; 3]| {
            let mut im = RgbaImage::new(w, h_total);
            for y in 0..h_total { for x in 0..w { im.set(x, y, if y < h_thumb { thumb.pixel(x, y) } else { [bg[0], bg[1], bg[2], 255] }); } }
            Arc::new(im)
        };
        let s = mk(st.sel_bg);
        [mk(st.normal_bg), s.clone(), if st.activated { mk(st.act_bg) } else { s }]
    }
    fn text_button(&self, _label: &str, w: u32, h: u32, st: &ButtonStyle) -> [Arc<RgbaImage>; 3] {
        let (m, _) = st.margins(w as i32, h as i32);
        let (w, h) = (w + 2 * m as u32, h + 2 * st.margins(w as i32, h as i32).1 as u32);
        let s = fill(w, h, st.sel_bg);
        [fill(w, h, st.normal_bg), s.clone(), if st.activated { fill(w, h, st.act_bg) } else { s }]
    }
    fn background(&self, _t: &str, size: (u32, u32)) -> RgbaImage { (*fill(size.0, size.1, [20, 30, 80])).clone() }
    fn overlay(&self, states: [Arc<RgbaImage>; 3], decor: &[Decor]) -> [Arc<RgbaImage>; 3] {
        assert!(!decor.is_empty());
        OVERLAYS.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        states
    }
    fn subtitle(&self, text: &str, style: &bdauthor_core::subtitle::SubStyle, max_w: u32) -> Option<RgbaImage> {
        let (w, h) = ((text.chars().count() as u32 * 12 + 40).min(max_w), style.size as u32);
        let mut im = RgbaImage::new(w, h);
        for y in 0..h { for x in 0..w { im.set(x, y, [255, 0, 255, 255]); } }
        Some(im)
    }
    fn skin_button(&self, skin: &[Option<std::path::PathBuf>; 3], w: u32, h: u32, st: &ButtonStyle) -> Result<[Arc<RgbaImage>; 3], String> {
        assert!(skin[0].as_ref().map_or(false, |p| p.is_absolute()), "chemin complet attendu");
        let (mx, my) = st.margins(w as i32, h as i32);
        let (w, h) = (w + 2 * mx as u32, h + 2 * my as u32);
        Ok([fill(w, h, [200, 0, 0]), fill(w, h, [0, 200, 0]), fill(w, h, [0, 0, 200])])
    }
    fn compose_background(&self, _base: Option<&std::path::Path>, size: (u32, u32), decor: &[Decor]) -> Result<RgbaImage, String> {
        let mut im = (*fill(size.0, size.1, [40, 20, 80])).clone();
        // un carre blanc par element pose, pour que le fond change reellement
        for (k, d) in decor.iter().enumerate() {
            let (x, y, w, h) = match d { Decor::Text { x, y, w, h, .. } | Decor::Image { x, y, w, h, .. } | Decor::Shape { x, y, w, h, .. } => (*x, *y, *w, *h) };
            for yy in y.max(0)..(y + h).min(size.1 as i32) { for xx in x.max(0)..(x + w).min(size.0 as i32) { im.set(xx as u32, yy as u32, [255, 255, 255, 255]); } }
            let _ = k;
        }
        Ok(im)
    }
}

#[test]
fn layout_and_pagination() {
    common::fr();
    assert_eq!(paginate(12, None).unwrap().len(), 1);
    let p = paginate(13, None).unwrap();
    assert_eq!(p.iter().map(|g| g.len()).collect::<Vec<_>>(), vec![7, 6]);
    let p = paginate(20, Some(5)).unwrap();
    assert_eq!(p.iter().map(|g| g.len()).collect::<Vec<_>>(), vec![5, 5, 5, 5]);
    assert!(paginate(3, Some(13)).is_err());
    // valeurs de reference Python (project.layout)
    let t = |c: &[Cell]| c.iter().map(|c| (c.x, c.y, c.w, c.th, c.h)).collect::<Vec<_>>();
    assert_eq!(t(&layout(1, (1920, 1080), 0).unwrap()), vec![(720, 472, 480, 270, 326)]);
    assert_eq!(t(&layout(5, (1280, 720), 140).unwrap()), vec![(120, 200, 240, 135, 167), (387, 200, 240, 135, 167), (653, 200, 240, 135, 167), (920, 200, 240, 135, 167), (520, 387, 240, 135, 167)]);
    assert_eq!(t(&layout(7, (1280, 720), 140).unwrap()), vec![(120, 200, 240, 135, 167), (387, 200, 240, 135, 167), (653, 200, 240, 135, 167), (920, 200, 240, 135, 167), (253, 387, 240, 135, 167), (520, 387, 240, 135, 167), (787, 387, 240, 135, 167)]);
    let l12 = layout(12, (1920, 1080), 0).unwrap();
    assert_eq!((l12[0].x, l12[0].y, l12[11].x, l12[11].y, l12[0].w, l12[0].h), (340, 315, 1300, 757, 280, 197));
    assert!(layout(13, (1920, 1080), 0).is_err());
}

#[test]
fn full_project() {
    common::fr();
    let Ok(tools) = Tools::locate() else { eprintln!("ffmpeg absent : test ignore"); return };
    let dir: PathBuf = std::env::var_os("BDAUTHOR_TEST_OUT").map(PathBuf::from).unwrap_or_else(|| std::env::temp_dir().join(format!("bdauthor_proj_{}", std::process::id())));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("videos")).unwrap();
    for (i, f) in ["a", "b", "c"].iter().enumerate() {
        let ok = Command::new(&tools.ffmpeg).args(["-y", "-v", "error", "-f", "lavfi", "-i", &format!("testsrc=size=640x360:rate=25:duration=4"),
            "-f", "lavfi", "-i", &format!("sine=frequency={}:duration=4", 300 + 200 * i), "-c:v", "libx264", "-pix_fmt", "yuv420p", "-c:a", "aac", "-shortest"])
            .arg(dir.join(format!("videos/{f}.mp4"))).status().unwrap().success();
        assert!(ok);
    }
    let proj = r#"{ "titre": "Test", "langue": "fra",
      "videos": [ {"fichier": "videos/a.mp4", "titre": "Alpha", "chapitres": [0, 2.5, {"t": 3, "titre": "Fin"}]}, {"fichier": "videos/b.mp4"}, {"fichier": "videos/c.mp4", "titre": "Gamma"} ],
      "encodage": {"preset": "ultrafast"}, "menu": {"fond_secondes": 2} }"#;
    std::fs::write(dir.join("projet.json"), proj).unwrap();
    let mut logs = Vec::new();
    let mut last = 0.0;
    let info = {
        let (mut log, mut prog) = (|m: &str| logs.push(m.to_string()), |f: f64| last = f);
        let cancel = AtomicBool::new(false);
        let mut env = BuildEnv { tools: &tools, renderer: &Flat, cancel: &cancel, log: &mut log, progress: &mut prog };
        build_project(&dir.join("projet.json"), &dir.join("out"), &mut env).unwrap()
    };
    assert_eq!(last, 1.0);
    println!("{info:#?}");
    assert!(info.report.ok(), "{}", info.report.render());
    // projet sans page : pages de depart = 1 page pour les videos + 1 page de chapitres de la video 1
    assert_eq!(info.main_pages, 2);
    assert!(logs.iter().any(|l| l.contains("page(s) de depart")), "{logs:?}");
    assert!(info.chapters.contains_key(&1));
    // second passage : tout vient du cache
    let mut logs2 = Vec::new();
    {
        let (mut log, mut prog) = (|m: &str| logs2.push(m.to_string()), |_f: f64| {});
        let cancel = AtomicBool::new(false);
        let mut env = BuildEnv { tools: &tools, renderer: &Flat, cancel: &cancel, log: &mut log, progress: &mut prog };
        build_project(&dir.join("projet.json"), &dir.join("out2"), &mut env).unwrap();
    }
    assert_eq!(logs2.iter().filter(|l| l.contains("cache")).count(), 3, "{logs2:?}");
    assert_eq!(std::fs::read(dir.join("out/BDMV/MovieObject.bdmv")).unwrap(), std::fs::read(dir.join("out2/BDMV/MovieObject.bdmv")).unwrap());
    if std::env::var_os("BDAUTHOR_TEST_OUT").is_none() { let _ = std::fs::remove_dir_all(&dir); }
}

#[test]
fn custom_layout_project() {
    common::fr();
    let Ok(tools) = Tools::locate() else { return };
    let dir = std::env::temp_dir().join(format!("bdauthor_design_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    for (i, f) in ["a", "b", "c"].iter().enumerate() {
        assert!(Command::new(&tools.ffmpeg).args(["-y", "-v", "error", "-f", "lavfi", "-i", "testsrc=size=640x360:rate=25:duration=4",
            "-f", "lavfi", "-i", &format!("sine=frequency={}:duration=4", 300 + 200 * i), "-c:v", "libx264", "-pix_fmt", "yuv420p", "-c:a", "aac", "-shortest"])
            .arg(dir.join(format!("{f}.mp4"))).status().unwrap().success());
    }
    // page 1 : titre, logo, 2 vignettes, une image devant la 2e, « Suivant » deplace, petite vignette de la video 3 menant a la page 3 ;
    // page 2 : video 3 ; page 3 : favoris et liens ; page 4 : chapitres de la video 1
    let proj = r##"{ "titre": "Test", "langue": "fra",
      "videos": [ {"fichier": "a.mp4", "chapitres": [0, 2.5, 3.2]}, {"fichier": "b.mp4"}, {"fichier": "c.mp4"} ],
      "encodage": {"preset": "ultrafast"}, "menu": {"fond_secondes": 2},
      "mise_en_page": { "pages": [
        { "elements": [
          {"type":"texte","texte":"Mon titre","x":100,"y":60,"w":1700,"h":120,"taille":90,"couleur":"#ffcc00","alignement":"gauche","fond_couleur":"#000000","fond_opacite":50,"fond_arrondi":12,"contour":3,"contour_couleur":"#202020"},
          {"type":"image","video":2,"instant":1.5,"x":1500,"y":200,"w":320,"h":180},
          {"type":"image","video":1,"instant":0.5,"recadrage":[0.1,0.1,0.5,0.5],"x":1500,"y":420,"w":200,"h":200},
          {"type":"image","fichier":"b.mp4","x":1500,"y":800,"w":300,"h":200},
          {"type":"vignette","vignette_h":300,"x":200,"y":300,"w":560,"h":356,"video":2},
          {"type":"vignette","vignette_h":300,"x":1000,"y":300,"w":560,"h":356,"video":1},
          {"type":"image","fichier":"b.mp4","x":1400,"y":500,"w":300,"h":200},
          {"type":"navigation","action":"suivant","texte":"Page 2","x":800,"y":900,"w":300,"h":90},
          {"type":"vignette","vignette_h":150,"x":100,"y":800,"w":250,"h":200,"video":3,"cible":{"page":3}} ] },
        { "elements": [
          {"type":"vignette","vignette_h":300,"x":600,"y":300,"w":560,"h":356,"video":3},
          {"type":"navigation","action":"precedent","texte":"<","x":400,"y":900,"w":300,"h":90},
          {"type":"navigation","action":"suivant","texte":">","x":1200,"y":900,"w":300,"h":90} ] },
        { "elements": [
          {"type":"texte","texte":"Mes favoris","x":100,"y":60,"w":1700,"h":120,"taille":80,"couleur":"#ffffff","alignement":"centre"},
          {"type":"vignette","vignette_h":300,"x":200,"y":300,"w":560,"h":356,"video":3},
          {"type":"vignette","vignette_h":300,"x":1000,"y":300,"w":560,"h":356,"video":1},
          {"type":"navigation","action":"precedent","texte":"Retour","x":800,"y":900,"w":300,"h":90},
          {"type":"navigation","action":"aller_a","texte":"Chapitres de a","cible":{"page":4},"x":1200,"y":900,"w":400,"h":90},
          {"type":"texte","texte":"Lire c","x":100,"y":700,"w":400,"h":90,"taille":60,"couleur":"#ffffff","alignement":"centre","cible":{"lire":"video","video":3}},
          {"type":"image","fichier":"b.mp4","x":1600,"y":60,"w":200,"h":120,"cible":{"page":1}},
          {"type":"navigation","action":"aller_a","texte":"Lire b","cible":{"lire":"video","video":2},"x":1500,"y":700,"w":300,"h":90},
          {"type":"vignette","vignette_h":100,"x":600,"y":700,"w":200,"h":130,"video":null},
          {"type":"vignette","vignette_h":100,"x":850,"y":700,"w":200,"h":130,"video":3} ] },
        { "elements": [
          {"type":"vignette","vignette_h":200,"x":100,"y":300,"w":400,"h":250,"video":1,"depart_s":0},
          {"type":"vignette","vignette_h":200,"x":600,"y":300,"w":400,"h":250,"video":1,"depart_s":2.5},
          {"type":"vignette","vignette_h":200,"x":1100,"y":300,"w":400,"h":250,"video":9,"depart_s":3.2},
          {"type":"navigation","action":"aller_a","texte":"Retour","cible":{"page":1},"x":800,"y":900,"w":300,"h":90} ] } ] } }"##;
    std::fs::write(dir.join("projet.json"), proj).unwrap();
    // la page de chapitres invalide (video 9) bloque la construction avant tout encodage, avec l'erreur dans le journal
    let mut logs = Vec::new();
    let r = {
        let (mut log, mut prog) = (|m: &str| logs.push(m.to_string()), |_f: f64| {});
        let cancel = AtomicBool::new(false);
        let mut env = BuildEnv { tools: &tools, renderer: &Flat, cancel: &cancel, log: &mut log, progress: &mut prog };
        build_project(&dir.join("projet.json"), &dir.join("out"), &mut env)
    };
    let e = r.expect_err("mise en page invalide : la construction doit s'arreter").to_string();
    assert!(e.contains("page 4") && e.contains("n'existe pas"), "{e}");
    assert!(logs.iter().any(|l| l.starts_with("ERREUR de mise en page")), "{logs:?}");
    assert!(!logs.iter().any(|l| l.contains("encodage")), "aucun encodage avant l'erreur : {logs:?}");
    // corrigee, la construction aboutit
    let proj = proj.replace(r#""video":9"#, r#""video":1"#);
    std::fs::write(dir.join("projet.json"), &proj).unwrap();
    OVERLAYS.store(0, std::sync::atomic::Ordering::SeqCst);
    let info = {
        let (mut log, mut prog) = (|_m: &str| {}, |_f: f64| {});
        let cancel = AtomicBool::new(false);
        let mut env = BuildEnv { tools: &tools, renderer: &Flat, cancel: &cancel, log: &mut log, progress: &mut prog };
        build_project(&dir.join("projet.json"), &dir.join("out"), &mut env).unwrap()
    };
    assert!(info.report.ok(), "{}", info.report.render());
    assert_eq!(info.main_pages, 4);
    assert_eq!(OVERLAYS.load(std::sync::atomic::Ordering::SeqCst), 1, "seule la 2e vignette est recouverte par l'image placee devant elle");
    // page 3 : 3 vignettes (dont la video 3 deux fois ; celle sans video est absente), « Precedent », 2 boutons « Aller a »,
    // un texte et une image cliquables
    let rep = info.report.render();
    assert!(rep.contains(": 8 bouton(s)"), "{rep}");
    // page 1 : 2 vignettes, « Suivant », une vignette de la video 3 menant a la page 3
    assert!(rep.contains(": 4 bouton(s)"), "{rep}");
    // les pages a decor (titres, images) ont leur propre fond
    let bgs = std::fs::read_dir(dir.join("out/BDMV/STREAM")).unwrap().filter(|e| { let n = e.as_ref().unwrap().file_name().into_string().unwrap(); n.starts_with("0000") || n.starts_with("0001") }).count();
    assert!(bgs >= 5, "{bgs}");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn project_with_subtitles() {
    common::fr();
    let Ok(tools) = Tools::locate() else { eprintln!("ffmpeg absent : test ignore"); return };
    let dir = std::env::temp_dir().join(format!("bdauthor_projsub_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("a_fr.srt"), "1\r\n00:00:00,500 --> 00:00:01,500\r\nBonjour\r\n\r\n2\r\n00:00:02,000 --> 00:00:03,000\r\nSalut\r\n").unwrap();
    std::fs::write(dir.join("a_en.srt"), "1\n00:00:01,000 --> 00:00:02,000\nHello\n").unwrap();
    let run = |args: &[&str]| assert!(Command::new(&tools.ffmpeg).args(["-y", "-v", "error"]).args(args).status().unwrap().success());
    // a.mp4 avec une piste de sous-titres texte integree (mov_text)
    run(&["-f", "lavfi", "-i", "testsrc=size=640x360:rate=25:duration=4", "-f", "lavfi", "-i", "sine=duration=4", "-i", dir.join("a_en.srt").to_str().unwrap(),
          "-map", "0", "-map", "1", "-map", "2", "-c:v", "libx264", "-pix_fmt", "yuv420p", "-c:a", "aac", "-c:s", "mov_text", "-metadata:s:s:0", "language=eng", "-shortest", dir.join("a.mp4").to_str().unwrap()]);
    run(&["-f", "lavfi", "-i", "testsrc=size=640x360:rate=25:duration=3", "-f", "lavfi", "-i", "sine=duration=3", "-c:v", "libx264", "-pix_fmt", "yuv420p", "-c:a", "aac", "-shortest", dir.join("b.mp4").to_str().unwrap()]);
    let streams = bdauthor_core::ffmpeg::list_subtitle_streams(&tools, &dir.join("a.mp4")).unwrap();
    assert_eq!(streams.len(), 1);
    assert!(streams[0].is_text() && streams[0].lang == "eng", "{streams:?}");
    let proj = r#"{ "titre": "Test", "langue": "fra",
      "videos": [ {"fichier": "a.mp4", "soustitres": [ {"fichier": "a_fr.srt", "langue": "fra", "style": {"taille": 70}}, {"piste": 0, "langue": "eng", "decalage_ms": 250, "style": {"position": "haut"}} ]},
                  {"fichier": "b.mp4"} ],
      "encodage": {"preset": "ultrafast"}, "menu": {"fond_secondes": 2} }"#;
    std::fs::write(dir.join("projet.json"), proj).unwrap();
    let build = |out: &str| {
        let mut logs = Vec::new();
        let info = {
            let (mut log, mut prog) = (|m: &str| logs.push(m.to_string()), |_f: f64| {});
            let cancel = AtomicBool::new(false);
            let mut env = BuildEnv { tools: &tools, renderer: &Flat, cancel: &cancel, log: &mut log, progress: &mut prog };
            build_project(&dir.join("projet.json"), &dir.join(out), &mut env).unwrap()
        };
        (info, logs)
    };
    let (info, logs) = build("out");
    assert!(info.report.ok(), "{}", info.report.render());
    let rep = info.report.render();
    assert!(rep.contains("sous-titres 00000 PID 0x1200") && rep.contains("sous-titres 00000 PID 0x1201"), "{rep}");
    assert!(!rep.contains("sous-titres 00001"), "{rep}");
    assert!(logs.iter().any(|l| l.contains("2 piste(s) de sous-titres")), "{logs:?}");
    // le clip declare les deux flux PG, la playlist aussi
    let clpi = bdauthor_core::clpi::Clpi::parse(&std::fs::read(dir.join("out/BDMV/CLIPINF/00000.clpi")).unwrap()).unwrap();
    let pg: Vec<(u16, String)> = clpi.program.programs[0].streams.iter().filter(|s| s.coding_type == 0x90).map(|s| (s.pid, String::from_utf8_lossy(&s.attrs[..3]).into_owned())).collect();
    assert_eq!(pg, vec![(0x1200, "fra".to_string()), (0x1201, "eng".to_string())]);
    let clpi1 = bdauthor_core::clpi::Clpi::parse(&std::fs::read(dir.join("out/BDMV/CLIPINF/00001.clpi")).unwrap()).unwrap();
    assert!(clpi1.program.programs[0].streams.iter().all(|s| s.coding_type != 0x90));
    let mpls = std::fs::read(dir.join("out/BDMV/PLAYLIST/00000.mpls")).unwrap();
    let needle = [9u8, 1, 0x12, 0x00, 0, 0, 0, 0, 0, 0, 5, 0x90, b'f', b'r', b'a', 0];
    assert!(mpls.windows(needle.len()).any(|w| w == needle), "entree PG absente de la playlist");
    // ffprobe voit 2 flux de sous-titres dans le clip
    let pr = Command::new(&tools.ffprobe).args(["-v", "error", "-select_streams", "s", "-show_entries", "stream=codec_name,id", "-of", "csv=p=0"]).arg(dir.join("out/BDMV/STREAM/00000.m2ts")).output().unwrap();
    let ids: std::collections::BTreeSet<String> = String::from_utf8_lossy(&pr.stdout).lines().filter(|l| l.contains("hdmv_pgs")).map(String::from).collect();
    assert_eq!(ids.len(), 2, "{ids:?}");
    // second passage : encodage en cache, sous-titres refaits
    let (_, logs2) = build("out2");
    assert!(logs2.iter().any(|l| l.contains("cache")) && logs2.iter().any(|l| l.contains("2 piste(s) de sous-titres")), "{logs2:?}");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn styled_buttons_project() {
    common::fr();
    let Ok(tools) = Tools::locate() else { return };
    let dir = std::env::temp_dir().join(format!("bdauthor_style_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    for f in ["a", "b"] {
        assert!(Command::new(&tools.ffmpeg).args(["-y", "-v", "error", "-f", "lavfi", "-i", "testsrc=size=640x360:rate=25:duration=3",
            "-f", "lavfi", "-i", "sine=duration=3", "-c:v", "libx264", "-pix_fmt", "yuv420p", "-c:a", "aac", "-shortest"])
            .arg(dir.join(format!("{f}.mp4"))).status().unwrap().success());
    }
    // sons des menus : clic (selection, style du projet) et ok (activation, style propre de la vignette 2)
    for (f, hz) in [("clic.wav", 880), ("ok.wav", 440)] {
        assert!(Command::new(&tools.ffmpeg).args(["-y", "-v", "error", "-f", "lavfi", "-i", &format!("sine=frequency={hz}:duration=0.2")]).arg(dir.join(f)).status().unwrap().success());
    }
    // halo + agrandissement + etat active ; une vignette collee au bord gauche (son halo doit etre rogne) ; un bouton dessine
    let proj = r##"{ "titre": "Style", "langue": "fra", "videos": [ {"fichier": "a.mp4"}, {"fichier": "b.mp4"} ],
      "encodage": {"preset": "ultrafast"}, "menu": {"fond_secondes": 2},
      "mise_en_page": { "style_boutons": {"halo": 24, "agrandir": 10, "etat_active": true, "halo_anime": true, "son_selection": "clic.wav"},
        "pages": [ { "elements": [
          {"type":"vignette","vignette_h":300,"x":0,"y":300,"w":560,"h":356,"video":1},
          {"type":"vignette","vignette_h":300,"x":1000,"y":300,"w":560,"h":356,"video":2,"style":{"halo":0,"agrandir":0},"sons":{"activation":"ok.wav"}},
          {"type":"navigation","action":"aller_a","texte":"B","cible":{"lire":"video","video":2},"x":800,"y":900,"w":300,"h":90,
           "images_etats":{"normal":"btn.png","animation_normal":["n1.png","n2.png"],"animation_selection":["f1.png","f2.png","f3.png"],"animation_une_fois":true}} ] } ] } }"##;
    std::fs::write(dir.join("projet.json"), proj).unwrap();
    let mut logs = Vec::new();
    let info = {
        let (mut log, mut prog) = (|m: &str| logs.push(m.to_string()), |_f: f64| {});
        let cancel = AtomicBool::new(false);
        let mut env = BuildEnv { tools: &tools, renderer: &Flat, cancel: &cancel, log: &mut log, progress: &mut prog };
        build_project(&dir.join("projet.json"), &dir.join("out"), &mut env).unwrap()
    };
    assert!(info.report.ok(), "{}", info.report.render());
    // relecture du menu : positions et tailles des boutons
    let data = std::fs::read(dir.join("out/BDMV/STREAM/80002.m2ts")).unwrap();
    let mut segs = Vec::new();
    for e in bdauthor_core::m2ts::reassemble(&data, 0x1400).unwrap() { segs.extend(bdauthor_core::m2ts::split_segments(&e.data).unwrap()); }
    let ics = bdauthor_core::ig::Ics::parse(&segs.iter().find(|s| s.name() == "ICS").unwrap().payload).unwrap();
    let ods: Vec<bdauthor_core::ig::Ods> = segs.iter().filter(|s| s.name() == "ODS").map(|s| bdauthor_core::ig::Ods::parse(&s.payload).unwrap()).collect();
    let size = |id: u16| { let o = ods.iter().find(|o| o.object_id == id).unwrap(); (o.width as i32, o.height as i32) };
    let btns: Vec<&bdauthor_core::ig::Button> = ics.pages[0].bogs.iter().flat_map(|g| g.buttons.iter()).collect();
    assert_eq!(btns.len(), 3);
    // video encodee en 720p : echelle 2/3. Vignette 1 : 373x237, halo 16 + 5 % -> marges (16+19, 16+12) ; rognee a gauche
    let b0 = btns.iter().min_by_key(|b| b.x).unwrap();
    assert_eq!(b0.x, 0);
    let (w0, h0) = size(b0.normal.0);
    assert_eq!((w0, h0), (373 + 35, 237 + 2 * 28));
    assert_eq!(b0.y as i32, 200 - 28);
    // etat active distinct (style du projet) : trois images ; la vignette 2 a son propre style, sans etat active : deux images
    let distinct = |b: &bdauthor_core::ig::Button| [b.normal.0, b.selected.1, b.activated.1].iter().collect::<std::collections::BTreeSet<_>>().len();
    let mut d: Vec<usize> = btns.iter().map(|b| distinct(b)).collect();
    d.sort();
    assert_eq!(d, vec![2, 3, 3]);
    // vignette 2 : style propre sans effet -> taille exacte, position exacte
    let b1 = btns.iter().find(|b| b.x == 667).expect("vignette 2 a x = 1000 x 2/3");
    assert_eq!(size(b1.normal.0), (373, 237));
    // halo anime (style du projet) : 8 images de meme taille, en boucle ; la vignette 2 (sans halo) n'est pas animee
    assert_eq!((b0.selected.2 - b0.selected.1, b0.selected.3), (7, 0x80));
    for id in b0.selected.1..=b0.selected.2 { assert_eq!(size(id), (w0, h0)); }
    assert_eq!((b1.selected.1, b1.selected.3), (b1.selected.2, 0));
    assert!(ics.pages[0].frame_rate_code > 0);
    // bouton dessine avec animation personnalisee (3 images, une fois) : prioritaire sur le halo anime du style du projet
    let bb = btns.iter().find(|b| b.x > 400 && b.y > 500).expect("bouton B");
    assert_eq!((bb.selected.2 - bb.selected.1, bb.selected.3), (2, 0));
    // etat normal anime (2 images, en boucle) ; les autres boutons ont un etat normal fixe
    assert_eq!((bb.normal.1 - bb.normal.0, bb.normal.2), (1, 0x80));
    assert_eq!((b1.normal.0, b1.normal.2), (b1.normal.1, 0));
    // sons : sound.bdmv (2 sons stereo 48 kHz d'environ 0,2 s), pas de copie de secours ; identifiants dans les boutons
    let snd = std::fs::read(dir.join("out/BDMV/AUXDATA/sound.bdmv")).expect("sound.bdmv");
    let idx = bdauthor_core::sound::parse_sound_bdmv(&snd).unwrap();
    assert_eq!(idx.len(), 2);
    for (ch, rate, bits, _, len) in &idx { assert_eq!((*ch, *rate, *bits), (2, 48_000, 16)); assert!((*len as i64 - 38_400).abs() < 2_000, "{len}"); }
    assert!(!dir.join("out/BDMV/BACKUP/AUXDATA").exists());
    // b0 (style du projet) : clic a la selection, pas de son d'activation ; b1 (style propre, sans son de selection) : ok a l'activation
    assert_eq!((b0.selected.0, b0.activated.0), (0, 0xFF));
    assert_eq!((b1.selected.0, b1.activated.0), (0xFF, 1));
    assert!(logs.iter().any(|l| l.contains("sons des menus : 2 son(s)")), "{logs:?}");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn home_intro_and_play_all() {
    common::fr();
    let Ok(tools) = Tools::locate() else { return };
    let dir = std::env::temp_dir().join(format!("bdauthor_home_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    for (f, d) in [("a", 3), ("b", 3), ("intro", 2)] {
        assert!(Command::new(&tools.ffmpeg).args(["-y", "-v", "error", "-f", "lavfi", "-i", &format!("testsrc=size=640x360:rate=25:duration={d}"),
            "-f", "lavfi", "-i", &format!("sine=duration={d}"), "-c:v", "libx264", "-pix_fmt", "yuv420p", "-c:a", "aac", "-shortest"])
            .arg(dir.join(format!("{f}.mp4"))).status().unwrap().success());
    }
    // accueil = page 2 ; « Tout lire » sur la page 1 ; sequence d'ouverture
    let proj = r##"{ "titre": "Accueil", "langue": "fra", "videos": [ {"fichier": "a.mp4", "chapitres": [0, 1.5]}, {"fichier": "b.mp4"} ],
      "encodage": {"preset": "ultrafast"}, "menu": {"fond_secondes": 2, "intro": "intro.mp4"},
      "mise_en_page": { "accueil": {"page": 2}, "pages": [
        { "elements": [
          {"type":"vignette","vignette_h":300,"x":200,"y":300,"w":560,"h":356,"video":1},
          {"type":"vignette","vignette_h":300,"x":1000,"y":300,"w":560,"h":356,"video":2},
          {"type":"navigation","action":"tout_lire","texte":"Tout lire","x":800,"y":900,"w":300,"h":90} ] },
        { "elements": [
          {"type":"navigation","action":"aller_a","texte":"Videos","cible":{"page":1},"x":800,"y":500,"w":300,"h":90} ] } ] } }"##;
    std::fs::write(dir.join("projet.json"), proj).unwrap();
    let info = {
        let (mut log, mut prog) = (|_m: &str| {}, |_f: f64| {});
        let cancel = AtomicBool::new(false);
        let mut env = BuildEnv { tools: &tools, renderer: &Flat, cancel: &cancel, log: &mut log, progress: &mut prog };
        build_project(&dir.join("projet.json"), &dir.join("out"), &mut env).unwrap()
    };
    assert!(info.report.ok(), "{}", info.report.render());
    let root = dir.join("out/BDMV");
    let mo = bdauthor_core::mobj::MovieObjectBdmv::parse(&std::fs::read(root.join("MovieObject.bdmv")).unwrap()).unwrap();
    let idx = bdauthor_core::index::IndexBdmv::parse(&std::fs::read(root.join("index.bdmv")).unwrap()).unwrap();
    let text = |o: usize| mo.objects[o].commands.iter().map(|c| c.text()).collect::<Vec<_>>().join(" ; ");
    // pages 1 et 2 (accueil) -> titres 3 et 4 ; « Tout lire » (titre 5)
    assert_eq!(idx.titles.len(), 5);
    assert!(text(0).contains("JUMP_TITLE 4"), "Top Menu -> accueil (titre de la page 2) : {}", text(0));
    assert!(text(3).starts_with("PLAY_PL"), "titre 3 = page 1 du menu : {}", text(3));
    // « Tout lire » revient a la page d'ou il a ete lance (registre de retour)
    assert!(text(5).starts_with("PLAY_PL") && text(5).contains("JUMP_TITLE r1"), "{}", text(5));
    // ouverture : objet dedie, lecture de l'intro puis accueil
    let fp = idx.first_playback.id_ref as usize;
    assert_eq!(fp, mo.objects.len() - 1);
    assert!(text(fp).starts_with("PLAY_PL") && text(fp).ends_with("JUMP_TITLE 0"), "{}", text(fp));
    // la playlist « Tout lire » enchaine les deux videos, avec une marque au debut de chacune et les chapitres de a
    let pl_all = mo.objects[5].commands[0].dst;
    let m = bdauthor_core::mpls::Mpls::parse(&std::fs::read(root.join(format!("PLAYLIST/{pl_all:05}.mpls"))).unwrap()).unwrap();
    assert_eq!(m.playitems.iter().map(|p| p.clip.as_str()).collect::<Vec<_>>(), vec!["00000", "00001"]);
    assert_eq!(m.marks.iter().map(|k| k.playitem_ref).collect::<Vec<_>>(), vec![0, 0, 1]);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn animated_menu_background_with_music() {
    common::fr();
    let Ok(tools) = Tools::locate() else { return };
    let dir = std::env::temp_dir().join(format!("bdauthor_animbg_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let run = |args: &[&str]| assert!(Command::new(&tools.ffmpeg).args(["-y", "-v", "error"]).args(args).status().unwrap().success());
    for f in ["a", "b"] {
        run(&["-f", "lavfi", "-i", "testsrc=size=640x360:rate=25:duration=2", "-f", "lavfi", "-i", "sine=duration=2", "-c:v", "libx264", "-pix_fmt", "yuv420p", "-c:a", "aac", "-shortest", dir.join(format!("{f}.mp4")).to_str().unwrap()]);
    }
    run(&["-f", "lavfi", "-i", "mandelbrot=size=320x180:rate=25", "-t", "3", "-c:v", "libx264", "-pix_fmt", "yuv420p", dir.join("fond.mp4").to_str().unwrap()]);
    run(&["-f", "lavfi", "-i", "sine=frequency=440:duration=5", "-c:a", "libmp3lame", dir.join("musique.mp3").to_str().unwrap()]);
    let proj = r##"{ "titre": "Anime", "langue": "fra", "videos": [ {"fichier": "a.mp4", "chapitres": [0, 1]}, {"fichier": "b.mp4"} ],
      "encodage": {"preset": "ultrafast"}, "menu": {"fond_video": "fond.mp4", "musique": "musique.mp3"},
      "mise_en_page": { "pages": [
        { "elements": [
          {"type":"vignette","vignette_h":300,"x":600,"y":300,"w":560,"h":356,"video":1},
          {"type":"navigation","action":"suivant","texte":">","x":800,"y":900,"w":300,"h":90} ] },
        { "elements": [
          {"type":"texte","texte":"Page 2","x":100,"y":60,"w":800,"h":120,"taille":80,"couleur":"#ffcc00","alignement":"gauche"},
          {"type":"vignette","vignette_h":300,"x":600,"y":300,"w":560,"h":356,"video":2},
          {"type":"navigation","action":"precedent","texte":"<","x":800,"y":900,"w":300,"h":90} ] } ] } }"##;
    std::fs::write(dir.join("projet.json"), proj).unwrap();
    let build = |out: &str| {
        let mut logs = Vec::new();
        let info = {
            let (mut log, mut prog) = (|m: &str| logs.push(m.to_string()), |_f: f64| {});
            let cancel = AtomicBool::new(false);
            let mut env = BuildEnv { tools: &tools, renderer: &Flat, cancel: &cancel, log: &mut log, progress: &mut prog };
            build_project(&dir.join("projet.json"), &dir.join(out), &mut env).unwrap()
        };
        (info, logs)
    };
    let t0 = std::time::Instant::now();
    let (info, _) = build("out");
    let first = t0.elapsed();
    assert!(info.report.ok(), "{}", info.report.render());
    let root = dir.join("out/BDMV");
    // pages du menu : playlists 2 et 3 (apres les 2 videos) ; fond avec son, en boucle (pas d'arret sur image)
    for pl in [2, 3] {
        let m = bdauthor_core::mpls::Mpls::parse(&std::fs::read(root.join(format!("PLAYLIST/{pl:05}.mpls"))).unwrap()).unwrap();
        let it = &m.playitems[0];
        let sc = bdauthor_core::tsclip::scan_clip(&std::fs::read(root.join(format!("STREAM/{}.m2ts", it.clip))).unwrap()).unwrap();
        assert!(!sc.audio_codes.is_empty(), "fond de la playlist {pl} sans son");
        let dur = (sc.end90 - sc.start90) as f64 / 90000.0;
        assert!((2.5..3.5).contains(&dur), "duree du fond = celle de la video ({dur} s)");
        let mpls = std::fs::read(root.join(format!("PLAYLIST/{pl:05}.mpls"))).unwrap();
        // entree audio (AC3, 0x81) dans la table des flux
        assert!(mpls.windows(3).any(|w| w == [5, 0x81, sc.audio_codes[0].2]), "flux audio absent de la playlist {pl}");
        let _ = it;
    }
    // la page 2 a son propre fond (texte pose sur la video), la page 1 le fond commun
    let clips: Vec<String> = [2, 3].iter().map(|pl| bdauthor_core::mpls::Mpls::parse(&std::fs::read(root.join(format!("PLAYLIST/{pl:05}.mpls"))).unwrap()).unwrap().playitems[0].clip.clone()).collect();
    assert_ne!(clips[0], clips[1]);
    // second passage : fonds repris du cache
    let t1 = std::time::Instant::now();
    let (info2, _) = build("out2");
    assert!(info2.report.ok());
    assert!(dir.join(".cache_bdauthor").read_dir().unwrap().filter_map(|e| e.ok()).any(|e| e.file_name().to_string_lossy().starts_with("fond_")));
    assert!(t1.elapsed() < first, "le cache doit accelerer ({:?} contre {:?})", t1.elapsed(), first);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn per_page_backgrounds() {
    common::fr();
    let Ok(tools) = Tools::locate() else { return };
    let dir = std::env::temp_dir().join(format!("bdauthor_pagebg_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let run = |args: &[&str]| assert!(Command::new(&tools.ffmpeg).args(["-y", "-v", "error"]).args(args).status().unwrap().success());
    for f in ["a", "b", "c"] {
        run(&["-f", "lavfi", "-i", "testsrc=size=640x360:rate=25:duration=2", "-f", "lavfi", "-i", "sine=duration=2", "-c:v", "libx264", "-pix_fmt", "yuv420p", "-c:a", "aac", "-shortest", dir.join(format!("{f}.mp4")).to_str().unwrap()]);
    }
    run(&["-f", "lavfi", "-i", "mandelbrot=size=320x180:rate=25", "-t", "3", "-c:v", "libx264", "-pix_fmt", "yuv420p", dir.join("fond.mp4").to_str().unwrap()]);
    run(&["-f", "lavfi", "-i", "sine=frequency=440:duration=4", "-c:a", "libmp3lame", dir.join("musique.mp3").to_str().unwrap()]);
    run(&["-f", "lavfi", "-i", "color=c=green:size=320x180", "-frames:v", "1", dir.join("vert.png").to_str().unwrap()]);
    // musique du projet ; page 1 : video propre, sans musique ; page 2 : image propre + musique du projet ; page 3 : comme le projet
    let proj = r##"{ "titre": "Fonds", "langue": "fra", "videos": [ {"fichier": "a.mp4"}, {"fichier": "b.mp4"}, {"fichier": "c.mp4"} ],
      "encodage": {"preset": "ultrafast"}, "menu": {"musique": "musique.mp3", "fond_secondes": 2},
      "mise_en_page": { "pages": [
        { "fond_page": {"video": "fond.mp4", "musique": null}, "elements": [
          {"type":"vignette","vignette_h":300,"x":600,"y":300,"w":560,"h":356,"video":1},
          {"type":"navigation","action":"suivant","texte":">","x":800,"y":900,"w":300,"h":90} ] },
        { "fond_page": {"image": "vert.png"}, "elements": [
          {"type":"vignette","vignette_h":300,"x":600,"y":300,"w":560,"h":356,"video":2},
          {"type":"navigation","action":"precedent","texte":"<","x":400,"y":900,"w":300,"h":90},
          {"type":"navigation","action":"suivant","texte":">","x":1200,"y":900,"w":300,"h":90} ] },
        { "elements": [
          {"type":"vignette","vignette_h":300,"x":600,"y":300,"w":560,"h":356,"video":3},
          {"type":"navigation","action":"precedent","texte":"<","x":800,"y":900,"w":300,"h":90} ] } ] } }"##;
    std::fs::write(dir.join("projet.json"), proj).unwrap();
    let info = {
        let (mut log, mut prog) = (|_m: &str| {}, |_f: f64| {});
        let cancel = AtomicBool::new(false);
        let mut env = BuildEnv { tools: &tools, renderer: &Flat, cancel: &cancel, log: &mut log, progress: &mut prog };
        build_project(&dir.join("projet.json"), &dir.join("out"), &mut env).unwrap()
    };
    assert!(info.report.ok(), "{}", info.report.render());
    let root = dir.join("out/BDMV");
    // playlists des pages du menu : 3, 4, 5
    let page = |pl: u32| {
        let m = bdauthor_core::mpls::Mpls::parse(&std::fs::read(root.join(format!("PLAYLIST/{pl:05}.mpls"))).unwrap()).unwrap();
        let clip = m.playitems[0].clip.clone();
        let sc = bdauthor_core::tsclip::scan_clip(&std::fs::read(root.join(format!("STREAM/{clip}.m2ts"))).unwrap()).unwrap();
        (clip, !sc.audio_codes.is_empty(), ((sc.end90 - sc.start90) as f64 / 90000.0).round() as i64)
    };
    let (c1, a1, d1) = page(3);
    let (c2, a2, d2) = page(4);
    let (c3, a3, d3) = page(5);
    assert_eq!((a1, d1), (false, 3), "page 1 : video propre (3 s), sans musique");
    assert_eq!((a2, d2), (true, 4), "page 2 : image propre + musique du projet (4 s)");
    assert_eq!((a3, d3), (true, 4), "page 3 : fond commun + musique du projet");
    assert!(c1 != c2 && c2 != c3 && c1 != c3);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn invalid_main_layout_stops_before_encoding() {
    common::fr();
    let Ok(tools) = Tools::locate() else { return };
    let dir = std::env::temp_dir().join(format!("bdauthor_badlayout_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    assert!(Command::new(&tools.ffmpeg).args(["-y", "-v", "error", "-f", "lavfi", "-i", "testsrc=size=640x360:rate=25:duration=1", "-c:v", "libx264", "-pix_fmt", "yuv420p"])
        .arg(dir.join("a.mp4")).status().unwrap().success());
    // page 1 : une vignette de la video 2 alors que le projet n'en a qu'une
    let proj = r##"{ "videos": [ {"fichier": "a.mp4"} ], "encodage": {"preset": "ultrafast"},
      "mise_en_page": { "pages": [ { "elements": [
          {"type":"vignette","vignette_h":300,"x":200,"y":300,"w":560,"h":356,"video":1},
          {"type":"vignette","vignette_h":300,"x":1000,"y":300,"w":560,"h":356,"video":2} ] } ] } }"##;
    std::fs::write(dir.join("projet.json"), proj).unwrap();
    let mut logs = Vec::new();
    let r = {
        let (mut log, mut prog) = (|m: &str| logs.push(m.to_string()), |_f: f64| {});
        let cancel = AtomicBool::new(false);
        let mut env = BuildEnv { tools: &tools, renderer: &Flat, cancel: &cancel, log: &mut log, progress: &mut prog };
        build_project(&dir.join("projet.json"), &dir.join("out"), &mut env)
    };
    let e = r.expect_err("doit echouer").to_string();
    assert!(e.contains("page 1") && e.contains("la vidéo 2 n'existe pas"), "{e}");
    assert!(!logs.iter().any(|l| l.contains("encodage")), "aucun encodage avant l'erreur : {logs:?}");
    assert!(!dir.join(".cache_bdauthor").exists());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn generic_pages_with_chapter_thumbnails() {
    common::fr();
    let Ok(tools) = Tools::locate() else { return };
    let dir = std::env::temp_dir().join(format!("bdauthor_generic_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    for f in ["a", "b"] {
        assert!(Command::new(&tools.ffmpeg).args(["-y", "-v", "error", "-f", "lavfi", "-i", "testsrc=size=640x360:rate=25:duration=6",
            "-f", "lavfi", "-i", "sine=duration=6", "-c:v", "libx264", "-pix_fmt", "yuv420p", "-c:a", "aac", "-shortest"])
            .arg(dir.join(format!("{f}.mp4"))).status().unwrap().success());
    }
    // page 1 : vignette de la video a, vignette de chapitre de a a 3 s (retour a l'accueil), « Suivant » ;
    // page 2 (accueil) : vignette de chapitre de b a 2 s (retour en page 1), « Precedent »
    let proj = r##"{ "titre": "Generique", "langue": "fra", "videos": [ {"fichier": "a.mp4"}, {"fichier": "b.mp4"} ],
      "langues": [ {"id":1,"nom":"Français","code":"fra"}, {"id":2,"nom":"Anglais","code":"eng"} ],
      "encodage": {"preset": "ultrafast"}, "menu": {"fond_secondes": 2},
      "mise_en_page": { "accueil": {"page": 2}, "pages": [
        { "nom": "Videos", "elements": [
          {"type":"vignette","vignette_h":300,"x":200,"y":300,"w":560,"h":356,"video":1,"soustitres":"aucun"},
          {"type":"vignette","vignette_h":300,"x":1000,"y":300,"w":560,"h":356,"video":1,"depart_s":3,"libelle":"Milieu","retour":"accueil"},
          {"type":"navigation","action":"suivant","texte":"Suite","x":800,"y":900,"w":300,"h":90} ] },
        { "nom": "Chapitres", "elements": [
          {"type":"vignette","vignette_h":300,"x":200,"y":300,"w":560,"h":356,"video":2,"depart_s":2,"retour":{"page":1},"soustitres":{"langue":2}},
          {"type":"navigation","action":"precedent","texte":"Retour","x":800,"y":900,"w":300,"h":90},
          {"type":"texte","texte":"Sous-titres FR","x":1300,"y":900,"w":400,"h":90,"taille":40,"cible":{"rester":true},"soustitres":{"langue":1}} ] } ] } }"##;
    std::fs::write(dir.join("projet.json"), proj).unwrap();
    let info = {
        let (mut log, mut prog) = (|_m: &str| {}, |_f: f64| {});
        let cancel = AtomicBool::new(false);
        let mut env = BuildEnv { tools: &tools, renderer: &Flat, cancel: &cancel, log: &mut log, progress: &mut prog };
        build_project(&dir.join("projet.json"), &dir.join("out"), &mut env).unwrap()
    };
    assert!(info.report.ok(), "{}", info.report.render());
    assert_eq!(info.main_pages, 2);
    let root = dir.join("out/BDMV");
    let mo = bdauthor_core::mobj::MovieObjectBdmv::parse(&std::fs::read(root.join("MovieObject.bdmv")).unwrap()).unwrap();
    let text = |o: usize| mo.objects[o].commands.iter().map(|c| c.text()).collect::<Vec<_>>().join(" ; ");
    // titres : videos 1-2, pages 3-4, lecture depuis un repere de a (5) et de b (6)
    assert!(text(0).contains("JUMP_TITLE 4"), "Top Menu -> page 2 : {}", text(0));
    assert_eq!(text(1), "PLAY_PL 0 ; JUMP_TITLE r1");
    for o in [5, 6] { assert!(text(o).contains("PLAY_PL_PM") && text(o).ends_with("JUMP_TITLE r1"), "{}", text(o)); }
    // reperes : debut + depart de la vignette, cales sur une image d'acces au plus a 1 s avant
    for (v, t) in [(0usize, 3.0f64), (1, 2.0)] {
        let m = bdauthor_core::mpls::Mpls::parse(&std::fs::read(root.join(format!("PLAYLIST/{v:05}.mpls"))).unwrap()).unwrap();
        assert_eq!(m.marks.len(), 2, "video {v}");
        let rel = (m.marks[1].time - m.marks[0].time) as f64 / 45000.0;
        assert!(rel <= t + 1e-6 && rel > t - 1.01, "video {v} : repere a {rel} s");
    }
    // boutons de la page 1 : video a (retour page 1 = titre 3) ; chapitre de a (retour accueil = titre 0, repere 1, titre 5)
    let ig_cmds = |clip: u32| -> Vec<String> {
        let data = std::fs::read(root.join(format!("STREAM/{clip}.m2ts"))).unwrap();
        let mut segs = Vec::new();
        for e in bdauthor_core::m2ts::reassemble(&data, 0x1400).unwrap() { segs.extend(bdauthor_core::m2ts::split_segments(&e.data).unwrap()); }
        let ics = bdauthor_core::ig::Ics::parse(&segs.iter().find(|s| s.name() == "ICS").unwrap().payload).unwrap();
        let mut b: Vec<&bdauthor_core::ig::Button> = ics.pages[0].bogs.iter().flat_map(|g| g.buttons.iter()).collect();
        b.sort_by_key(|b| (b.y, b.x));
        b.iter().map(|b| b.commands.iter().map(|c| c.text()).collect::<Vec<_>>().join(" ; ")).collect()
    };
    let p1 = ig_cmds(80002);
    assert_eq!(p1.len(), 3, "{p1:?}");
    assert!(p1[0].contains("r1, 3") && p1[0].contains("JUMP_TITLE 1"), "{p1:?}");
    // sous-titres choisis par la vignette : SET_STREAM avant la lecture (masques ici, piste 2 affichee en page 2)
    assert!(p1[0].starts_with("SET_STREAM"), "{p1:?}");
    assert!(!p1[1].contains("SET_STREAM"), "{p1:?}");
    assert!(p1[1].contains("r1, 0") && p1[1].contains("r0, 1") && p1[1].contains("JUMP_TITLE 5"), "{p1:?}");
    let p2 = ig_cmds(80003);
    assert!(p2[0].contains("r1, 3") && p2[0].contains("r0, 1") && p2[0].contains("JUMP_TITLE 6"), "{p2:?}");
    assert!(p2[0].starts_with("SET_STREAM"), "{p2:?}");
    // texte « Sous-titres FR » : reglage seul, sans navigation
    let only = p2.iter().find(|c| c.starts_with("SET_STREAM") && !c.contains("JUMP")).expect("bouton de reglage seul");
    assert!(only.contains("NOP"), "{p2:?}");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn language_list_fixes_track_numbers() {
    common::fr();
    let Ok(tools) = Tools::locate() else { eprintln!("ffmpeg absent : test ignore"); return };
    let dir = std::env::temp_dir().join(format!("bdauthor_langs_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("fr.srt"), "1\n00:00:00,500 --> 00:00:01,500\nBonjour\n").unwrap();
    std::fs::write(dir.join("en.srt"), "1\n00:00:01,000 --> 00:00:02,000\nHello\n").unwrap();
    for f in ["a", "b"] {
        assert!(Command::new(&tools.ffmpeg).args(["-y", "-v", "error", "-f", "lavfi", "-i", "testsrc=size=640x360:rate=25:duration=4",
            "-f", "lavfi", "-i", "sine=duration=4", "-c:v", "libx264", "-pix_fmt", "yuv420p", "-c:a", "aac", "-shortest"])
            .arg(dir.join(format!("{f}.mp4"))).status().unwrap().success());
    }
    // langues : 1 = fra (piste 0), 2 = eng (piste 1). La video a les range dans l'ordre inverse ; la video b n'a que l'anglais.
    let proj = r##"{ "titre": "Langues", "langue": "fra",
      "langues": [ {"id":1,"nom":"Francais","code":"fra"}, {"id":2,"nom":"Anglais","code":"eng"} ],
      "videos": [ {"fichier": "a.mp4", "soustitres": [ {"fichier":"en.srt","langue":"eng","langue_id":2}, {"fichier":"fr.srt","langue":"fra","langue_id":1} ]},
                  {"fichier": "b.mp4", "soustitres": [ {"fichier":"en.srt","langue":"eng","langue_id":2} ]} ],
      "encodage": {"preset": "ultrafast"}, "menu": {"fond_secondes": 2},
      "mise_en_page": { "pages": [ { "nom": "Menu", "elements": [
          {"type":"vignette","vignette_h":300,"x":200,"y":300,"w":560,"h":356,"video":1},
          {"type":"texte","texte":"English","x":1300,"y":900,"w":400,"h":90,"taille":40,"cible":{"rester":true,"ensuite":2},"soustitres":{"langue":2}},
          {"type":"texte","texte":"Francais","x":300,"y":900,"w":400,"h":90,"taille":40,"cible":{"rester":true},"soustitres":{"langue":1}} ] },
        { "nom": "Fin", "elements": [ {"type":"texte","texte":"Retour","x":100,"y":100,"w":300,"h":90,"taille":40,"cible":{"page":1}} ] } ] } }"##;
    std::fs::write(dir.join("projet.json"), proj).unwrap();
    let info = {
        let (mut log, mut prog) = (|_m: &str| {}, |_f: f64| {});
        let cancel = AtomicBool::new(false);
        let mut env = BuildEnv { tools: &tools, renderer: &Flat, cancel: &cancel, log: &mut log, progress: &mut prog };
        build_project(&dir.join("projet.json"), &dir.join("out"), &mut env).unwrap()
    };
    assert!(info.report.ok(), "{}", info.report.render());
    let root = dir.join("out/BDMV");
    // chaque video declare les memes pistes : 0x1200 = francais, 0x1201 = anglais
    for v in 0..2 {
        let clpi = bdauthor_core::clpi::Clpi::parse(&std::fs::read(root.join(format!("CLIPINF/{v:05}.clpi"))).unwrap()).unwrap();
        let pg: Vec<(u16, String)> = clpi.program.programs[0].streams.iter().filter(|s| s.coding_type == 0x90).map(|s| (s.pid, String::from_utf8_lossy(&s.attrs[..3]).into_owned())).collect();
        assert_eq!(pg, vec![(0x1200, "fra".to_string()), (0x1201, "eng".to_string())], "video {v}");
    }
    // video a : la piste 0x1200 (fra) a bien les repliques francaises ; video b : 0x1200 est vide
    let count = |clip: &str, pid: u16| -> usize {
        let data = std::fs::read(root.join(format!("STREAM/{clip}.m2ts"))).unwrap();
        bdauthor_core::m2ts::reassemble(&data, pid).map(|v| v.len()).unwrap_or(0)
    };
    assert!(count("00000", 0x1200) > 0 && count("00000", 0x1201) > 0);
    // video b : la piste francaise existe (pour garder les numeros) mais n'affiche rien
    assert!(count("00001", 0x1200) > 0 && count("00001", 0x1200) < count("00001", 0x1201));
    let rep = info.report.render();
    assert!(rep.contains("sous-titres 00001 PID 0x1200 : 1 ensemble(s) d'affichage, 0 sous-titre(s) affiche(s)"), "{rep}");
    assert!(rep.contains("sous-titres 00000 PID 0x1200 : 2 ensemble(s) d'affichage, 1 sous-titre(s) affiche(s)"), "{rep}");
    // boutons : « English » = piste 2 (rang 1), « Francais » = piste 1 (rang 0)
    let data = std::fs::read(root.join("STREAM/80002.m2ts")).unwrap();
    let mut segs = Vec::new();
    for e in bdauthor_core::m2ts::reassemble(&data, 0x1400).unwrap() { segs.extend(bdauthor_core::m2ts::split_segments(&e.data).unwrap()); }
    let ics = bdauthor_core::ig::Ics::parse(&segs.iter().find(|s| s.name() == "ICS").unwrap().payload).unwrap();
    let mut b: Vec<&bdauthor_core::ig::Button> = ics.pages[0].bogs.iter().flat_map(|g| g.buttons.iter()).collect();
    b.sort_by_key(|b| (b.y, b.x));
    let cmds: Vec<String> = b.iter().map(|b| b.commands.iter().map(|c| c.text()).collect::<Vec<_>>().join(" ; ")).collect();
    assert_eq!(cmds.len(), 3, "{cmds:?}");
    assert!(cmds[1].starts_with(&bdauthor_core::mobj::set_pg_stream(Some(0)).text()), "Francais : {cmds:?}");
    assert!(cmds[2].starts_with(&bdauthor_core::mobj::set_pg_stream(Some(1)).text()), "English : {cmds:?}");
    // « English » sort ensuite vers la page « Fin » (titre 4 = 2 videos + 2e page) ; « Francais » reste sur la page
    assert_eq!(b[2].commands.len(), 2, "{cmds:?}");
    assert_eq!((b[2].commands[1].insn, b[2].commands[1].dst), (bdauthor_core::mobj::JUMP_TITLE_INSN, 4), "{cmds:?}");
    assert!(!cmds[1].contains("JUMP_TITLE"), "{cmds:?}");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn several_audio_tracks_per_video() {
    common::fr();
    let Ok(tools) = Tools::locate() else { eprintln!("ffmpeg absent : test ignore"); return };
    let dir = std::env::temp_dir().join(format!("bdauthor_audio_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let run = |args: &[&str]| assert!(Command::new(&tools.ffmpeg).args(["-y", "-v", "error"]).args(args).status().unwrap().success());
    // a.mp4 : image 4 s, piste audio 0 en francais (4 s), piste audio 1 en anglais (2 s) ; b.mp4 : une piste ; doub.wav : doublage francais de b
    run(&["-f", "lavfi", "-i", "testsrc=size=640x360:rate=25:duration=4", "-f", "lavfi", "-i", "sine=frequency=440:duration=4", "-f", "lavfi", "-i", "sine=frequency=880:duration=2",
          "-map", "0", "-map", "1", "-map", "2", "-c:v", "libx264", "-pix_fmt", "yuv420p", "-c:a", "aac", "-metadata:s:a:0", "language=fra", "-metadata:s:a:1", "language=eng", dir.join("a.mp4").to_str().unwrap()]);
    run(&["-f", "lavfi", "-i", "testsrc=size=640x360:rate=25:duration=3", "-f", "lavfi", "-i", "sine=duration=3", "-c:v", "libx264", "-pix_fmt", "yuv420p", "-c:a", "aac", "-shortest", dir.join("b.mp4").to_str().unwrap()]);
    run(&["-f", "lavfi", "-i", "sine=frequency=300:duration=3", dir.join("doub.wav").to_str().unwrap()]);
    let streams = bdauthor_core::ffmpeg::list_audio_streams(&tools, &dir.join("a.mp4")).unwrap();
    assert_eq!(streams.iter().map(|s| (s.index, s.lang.as_str())).collect::<Vec<_>>(), vec![(0, "fra"), (1, "eng")]);
    // langues : 1 = fra (piste 1), 2 = eng (piste 2). La video a les declare dans l'ordre inverse.
    let proj = r##"{ "titre": "Audio", "langue": "fra",
      "langues": [ {"id":1,"nom":"Francais","code":"fra"}, {"id":2,"nom":"Anglais","code":"eng"} ],
      "videos": [ {"fichier": "a.mp4", "pistes_audio": [ {"piste":1,"langue":"eng","langue_id":2}, {"piste":0,"langue":"fra","langue_id":1} ]},
                  {"fichier": "b.mp4", "pistes_audio": [ {"fichier":"doub.wav","langue":"fra","langue_id":1} ]},
                  {"fichier": "a.mp4"},
                  {"fichier": "a.mp4", "pistes_audio": [ {"piste":1,"langue":"eng","langue_id":2} ]} ],
      "mise_en_page": {"pages": [ {"nom": "P", "elements": [
          {"type":"texte","texte":"English","x":100,"y":100,"w":400,"h":90,"taille":40,"cible":{"audio_langue":2}},
          {"type":"vignette","vignette_h":300,"x":200,"y":300,"w":560,"h":356,"video":1} ] } ] },
      "encodage": {"preset": "ultrafast"}, "menu": {"fond_secondes": 2} }"##;
    std::fs::write(dir.join("projet.json"), proj).unwrap();
    let build = |out: &str| {
        let mut logs = Vec::new();
        let info = {
            let (mut log, mut prog) = (|m: &str| logs.push(m.to_string()), |_f: f64| {});
            let cancel = AtomicBool::new(false);
            let mut env = BuildEnv { tools: &tools, renderer: &Flat, cancel: &cancel, log: &mut log, progress: &mut prog };
            build_project(&dir.join("projet.json"), &dir.join(out), &mut env).unwrap()
        };
        (info, logs)
    };
    let (info, _) = build("out");
    assert!(info.report.ok(), "{}", info.report.render());
    let root = dir.join("out/BDMV");
    let audio = |v: usize| -> Vec<(u16, String)> {
        let clpi = bdauthor_core::clpi::Clpi::parse(&std::fs::read(root.join(format!("CLIPINF/{v:05}.clpi"))).unwrap()).unwrap();
        clpi.program.programs[0].streams.iter().filter(|s| s.coding_type == 0x81).map(|s| (s.pid, String::from_utf8_lossy(&s.attrs[1..4]).into_owned())).collect()
    };
    // video a : l'ordre choisi dans la video est garde : anglais (piste 1) puis francais (piste 2)
    assert_eq!(audio(0), vec![(0x1100, "eng".to_string()), (0x1101, "fra".to_string())]);
    // video b : doublage francais seul, meme numero que dans la video a ; video c (sans reglage) : sa premiere piste, en francais
    assert_eq!(audio(1), vec![(0x1100, "fra".to_string())]);
    assert_eq!(audio(2), vec![(0x1100, "fra".to_string())]);
    // video d : anglais seul, en piste 1 alors qu'il est en piste 2 dans la video a (chaque video a ses propres langues)
    assert_eq!(audio(3), vec![(0x1100, "eng".to_string())]);
    // debut des videos : piste de la langue retenue par le bouton (r2 = identifiant de langue) ; une seule piste : rien a choisir
    let mo = bdauthor_core::mobj::MovieObjectBdmv::parse(&std::fs::read(root.join("MovieObject.bdmv")).unwrap()).unwrap();
    let text = |o: usize| mo.objects[o].commands.iter().map(|c| c.text()).collect::<Vec<_>>().join(" ; ");
    let (s1, s2) = (bdauthor_core::mobj::set_audio_stream(0).text(), bdauthor_core::mobj::set_audio_stream(1).text());
    assert_eq!(text(1), format!("CMP_EQ r2, 2 ; {s1} ; CMP_EQ r2, 1 ; {s2} ; PLAY_PL 0 ; JUMP_TITLE r1"));
    assert_eq!(text(4), "PLAY_PL 3 ; JUMP_TITLE r1");
    // le bouton « English » retient la langue 2 (et ne fixe plus de numero de piste)
    let mut cmds = Vec::new();
    for e in std::fs::read_dir(root.join("STREAM")).unwrap() {
        let path = e.unwrap().path();
        let Ok(es) = bdauthor_core::m2ts::reassemble(&std::fs::read(&path).unwrap(), 0x1400) else { continue };
        let mut segs = Vec::new();
        for e in es { segs.extend(bdauthor_core::m2ts::split_segments(&e.data).unwrap()); }
        let Some(ics) = segs.iter().find(|s| s.name() == "ICS") else { continue };
        let ics = bdauthor_core::ig::Ics::parse(&ics.payload).unwrap();
        for b in ics.pages.iter().flat_map(|p| p.bogs.iter()).flat_map(|g| g.buttons.iter()) {
            cmds.push(b.commands.iter().map(|c| c.text()).collect::<Vec<_>>().join(" ; "));
        }
    }
    assert!(cmds.iter().any(|c| c.starts_with("MOVE r2, 2") && !c.contains("SET_STREAM")), "{cmds:?}");
    // le francais de a dure 4 s, l'anglais 2 s : l'ordre des flux suit bien les langues
    let count = |clip: &str, pid: u16| -> usize {
        let data = std::fs::read(root.join(format!("STREAM/{clip}.m2ts"))).unwrap();
        bdauthor_core::m2ts::reassemble(&data, pid).map(|v| v.len()).unwrap_or(0)
    };
    assert!(count("00000", 0x1101) > count("00000", 0x1100) && count("00000", 0x1100) > 0, "{} {}", count("00000", 0x1100), count("00000", 0x1101));
    assert_eq!(count("00001", 0x1101), 0);
    // playlist de a : deux entrees audio (eng puis fra)
    let mpls = std::fs::read(root.join("PLAYLIST/00000.mpls")).unwrap();
    let entry = |pid: u8, l: &[u8; 3]| { let mut n = vec![9u8, 1, 0x11, pid, 0, 0, 0, 0, 0, 0, 5, 0x81]; n.push(0); n.extend_from_slice(l); n };
    let find = |pid: u8, l: &[u8; 3]| { let n = entry(pid, l); mpls.windows(n.len()).position(|w| w[..12] == n[..12] && w[13..] == n[13..]) };
    let (en, fr) = (find(0x00, b"eng"), find(0x01, b"fra"));
    assert!(fr.is_some() && en.is_some() && en < fr, "{fr:?} {en:?}");
    // ffprobe voit 2 flux audio dans le clip de a
    let pr = Command::new(&tools.ffprobe).args(["-v", "error", "-select_streams", "a", "-show_entries", "stream=codec_name,id", "-of", "csv=p=0"]).arg(root.join("STREAM/00000.m2ts")).output().unwrap();
    let ids: std::collections::BTreeSet<String> = String::from_utf8_lossy(&pr.stdout).lines().filter(|l| l.contains("ac3")).map(String::from).collect();
    assert_eq!(ids.len(), 2, "{ids:?}");
    let _ = std::fs::remove_dir_all(&dir);
}

/// Construit le projet `projet.json` de `dir` ; rend le compte rendu de verification, le journal et le dossier de sortie.
fn build_in(tools: &Tools, dir: &std::path::Path, out: &str) -> (BuildInfo, Vec<String>) {
    let mut logs = Vec::new();
    let info = {
        let (mut log, mut prog) = (|m: &str| logs.push(m.to_string()), |_f: f64| {});
        let cancel = AtomicBool::new(false);
        let mut env = BuildEnv { tools, renderer: &Flat, cancel: &cancel, log: &mut log, progress: &mut prog };
        build_project(&dir.join("projet.json"), &dir.join(out), &mut env).unwrap()
    };
    (info, logs)
}

#[test]
fn conforming_video_is_copied_not_reencoded() {
    common::fr();
    let Ok(tools) = Tools::locate() else { eprintln!("ffmpeg absent : test ignore"); return };
    let dir = std::env::temp_dir().join(format!("bdauthor_copy_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let run = |args: &[&str]| assert!(Command::new(&tools.ffmpeg).args(["-y", "-v", "error"]).args(args).status().unwrap().success());
    // a.mp4 : deja un flux Blu-ray (720p23,976, High 4.1, GOP ferme) ; b.mp4 : 640x360 a 25 i/s, pas admis tel quel
    run(&["-f", "lavfi", "-i", "testsrc2=size=1280x720:rate=24000/1001:duration=4", "-f", "lavfi", "-i", "sine=duration=4", "-c:v", "libx264", "-profile:v", "high", "-level", "4.1",
          "-pix_fmt", "yuv420p", "-preset", "ultrafast", "-x264-params", "keyint=24:min-keyint=24:open-gop=0:bframes=2:scenecut=0", "-c:a", "aac", "-shortest", dir.join("a.mp4").to_str().unwrap()]);
    run(&["-f", "lavfi", "-i", "testsrc=size=640x360:rate=25:duration=3", "-f", "lavfi", "-i", "sine=duration=3", "-c:v", "libx264", "-pix_fmt", "yuv420p", "-c:a", "aac", "-shortest", dir.join("b.mp4").to_str().unwrap()]);
    let info = bdauthor_core::ffmpeg::probe(&tools, &dir.join("a.mp4")).unwrap();
    assert!(matches!(info.profile.as_str(), "High" | "Main") && info.level == 41 && info.cfr, "{info:?}");
    let cancel = AtomicBool::new(false);
    let r = bdauthor_core::ffmpeg::check_copy(&tools, &dir.join("a.mp4"), &info, 30000, &cancel).unwrap().expect("a.mp4 devrait etre conforme");
    assert!(r.avg_kbps > 0.0 && r.max_key_gap_s <= 1.1, "{r:?}");
    let bi = bdauthor_core::ffmpeg::probe(&tools, &dir.join("b.mp4")).unwrap();
    assert!(bdauthor_core::ffmpeg::check_copy(&tools, &dir.join("b.mp4"), &bi, 30000, &cancel).unwrap().is_err());

    let proj = r##"{ "titre": "Copie", "langue": "fra",
      "videos": [ {"fichier": "a.mp4"}, {"fichier": "b.mp4"} ],
      "encodage": {"preset": "ultrafast", "copie_si_conforme": true}, "menu": {"fond_secondes": 2} }"##;
    std::fs::write(dir.join("projet.json"), proj).unwrap();
    let (info, logs) = build_in(&tools, &dir, "out");
    assert!(info.report.ok(), "{}", info.report.render());
    assert_eq!(logs.iter().filter(|l| l.contains("conforme :")).count(), 1, "{logs:?}");
    assert_eq!(logs.iter().filter(|l| l.contains("reencodage necessaire")).count(), 1, "{logs:?}");
    assert_eq!(logs.iter().filter(|l| l.contains("flux video copie")).count(), 1, "{logs:?}");
    // le clip de la video copiee garde les images de la source (96 images a 23,976 i/s pendant 4 s)
    let clip = std::fs::read(dir.join("out/BDMV/STREAM/00000.m2ts")).unwrap();
    let frames = bdauthor_core::film::load_encoded_m2ts(&clip).unwrap().video.len();
    assert!((95..=97).contains(&frames), "{frames}");
    let sc = bdauthor_core::tsclip::scan_clip(&clip).unwrap();
    assert!(sc.eps.len() >= 4, "points d'acces : {}", sc.eps.len());
}

#[test]
fn target_bitrate_encoding_follows_the_plan() {
    common::fr();
    let Ok(tools) = Tools::locate() else { eprintln!("ffmpeg absent : test ignore"); return };
    let dir = std::env::temp_dir().join(format!("bdauthor_fill_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let run = |args: &[&str]| assert!(Command::new(&tools.ffmpeg).args(["-y", "-v", "error"]).args(args).status().unwrap().success());
    run(&["-f", "lavfi", "-i", "testsrc2=size=640x360:rate=25:duration=4", "-f", "lavfi", "-i", "sine=duration=4", "-c:v", "libx264", "-pix_fmt", "yuv420p", "-c:a", "aac", "-shortest", dir.join("a.mp4").to_str().unwrap()]);
    // un plafond de 2000 kb/s (vbv-maxrate) : le debit vise est 90 % de ce plafond, bien en dessous de ce qu'un BD-25 permettrait
    let proj = r##"{ "titre": "Debit", "langue": "fra", "videos": [ {"fichier": "a.mp4"} ],
      "encodage": {"preset": "ultrafast", "remplir_disque": true, "maxrate": 2000}, "menu": {"fond_secondes": 2} }"##;
    std::fs::write(dir.join("projet.json"), proj).unwrap();
    let (info, logs) = build_in(&tools, &dir, "out");
    assert!(info.report.ok(), "{}", info.report.render());
    assert!(logs.iter().any(|l| l.contains("debit video vise : 1800 kb/s") && l.contains("plafonne")), "{logs:?}");
    let clip = std::fs::read(dir.join("out/BDMV/STREAM/00000.m2ts")).unwrap();
    // 4 s a ~1800 kb/s de video + 192 kb/s d'audio, multiplexes : de l'ordre de 1 Mo, loin d'un encodage en CRF 18 de cette mire
    assert!(clip.len() > 300_000 && clip.len() < 1_600_000, "{} octets", clip.len());
    // sans le plan, le CRF est utilise (autre taille, et pas de ligne de debit)
    std::fs::write(dir.join("projet.json"), proj.replace("\"remplir_disque\": true, ", "")).unwrap();
    let (_, logs2) = build_in(&tools, &dir, "out2");
    assert!(!logs2.iter().any(|l| l.contains("debit video vise")), "{logs2:?}");
}
