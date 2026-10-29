//! Construction complete d'un disque avec un vrai ffmpeg (ignore si ffmpeg est absent) et un moteur de dessin minimal.
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
            let (x, y, w, h) = match d { Decor::Text { x, y, w, h, .. } | Decor::Image { x, y, w, h, .. } => (*x, *y, *w, *h) };
            for yy in y.max(0)..(y + h).min(size.1 as i32) { for xx in x.max(0)..(x + w).min(size.0 as i32) { im.set(xx as u32, yy as u32, [255, 255, 255, 255]); } }
            let _ = k;
        }
        Ok(im)
    }
}

#[test]
fn layout_and_pagination() {
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
      "encodage": {"preset": "ultrafast"}, "menu": {"par_page": 2, "fond_secondes": 2} }"#;
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
    assert_eq!(info.main_pages, 2);
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
    let Ok(tools) = Tools::locate() else { return };
    let dir = std::env::temp_dir().join(format!("bdauthor_design_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    for (i, f) in ["a", "b", "c"].iter().enumerate() {
        assert!(Command::new(&tools.ffmpeg).args(["-y", "-v", "error", "-f", "lavfi", "-i", "testsrc=size=640x360:rate=25:duration=4",
            "-f", "lavfi", "-i", &format!("sine=frequency={}:duration=4", 300 + 200 * i), "-c:v", "libx264", "-pix_fmt", "yuv420p", "-c:a", "aac", "-shortest"])
            .arg(dir.join(format!("{f}.mp4"))).status().unwrap().success());
    }
    // page 1 du menu principal personnalisee (2 vignettes + titre + logo + 'Suivant' deplace), page 2 automatique ;
    // menu de chapitres de la video 1 : page 1 personnalisee avec une vignette de trop (doit retomber sur la grille automatique)
    let proj = r##"{ "titre": "Test", "langue": "fra",
      "videos": [ {"fichier": "a.mp4", "chapitres": [0, 2.5, 3.2]}, {"fichier": "b.mp4"}, {"fichier": "c.mp4"} ],
      "encodage": {"preset": "ultrafast"}, "menu": {"par_page": 2, "fond_secondes": 2},
      "mise_en_page": { "principal": { "1": { "elements": [
          {"type":"texte","texte":"Mon titre","x":100,"y":60,"w":1700,"h":120,"taille":90,"couleur":"#ffcc00","alignement":"gauche"},
          {"type":"image","fichier":"b.mp4","x":1500,"y":800,"w":300,"h":200},
          {"type":"vignette","vignette_h":300,"x":200,"y":300,"w":560,"h":356,"element":2},
          {"type":"vignette","vignette_h":300,"x":1000,"y":300,"w":560,"h":356},
          {"type":"image","fichier":"b.mp4","x":1400,"y":500,"w":300,"h":200},
          {"type":"navigation","action":"suivant","texte":"Page 2","x":800,"y":900,"w":300,"h":90},
          {"type":"vignette","vignette_h":150,"x":100,"y":800,"w":250,"h":200,"video":3,"cible":{"menu":"principal","page":3}} ] } },
        "libres": [ { "elements": [
          {"type":"texte","texte":"Mes favoris","x":100,"y":60,"w":1700,"h":120,"taille":80,"couleur":"#ffffff","alignement":"centre"},
          {"type":"vignette","vignette_h":300,"x":200,"y":300,"w":560,"h":356,"element":3},
          {"type":"vignette","vignette_h":300,"x":1000,"y":300,"w":560,"h":356,"element":1},
          {"type":"navigation","action":"precedent","texte":"Retour","x":800,"y":900,"w":300,"h":90},
          {"type":"navigation","action":"aller_a","texte":"Chapitres de a","cible":{"menu":"chapitres","video":1,"page":1},"x":1200,"y":900,"w":400,"h":90},
          {"type":"texte","texte":"Lire c","x":100,"y":700,"w":400,"h":90,"taille":60,"couleur":"#ffffff","alignement":"centre","cible":{"lire":"video","video":3}},
          {"type":"image","fichier":"b.mp4","x":1600,"y":60,"w":200,"h":120,"cible":{"menu":"principal","page":1}},
          {"type":"navigation","action":"aller_a","texte":"Lire b","cible":{"lire":"video","video":2},"x":1500,"y":700,"w":300,"h":90},
          {"type":"vignette","vignette_h":100,"x":600,"y":700,"w":200,"h":130,"video":null},
          {"type":"vignette","vignette_h":100,"x":850,"y":700,"w":200,"h":130,"video":3} ] } ],
        "chapitres": { "1": { "1": { "elements": [
          {"type":"vignette","vignette_h":200,"x":100,"y":300,"w":400,"h":250},
          {"type":"vignette","vignette_h":200,"x":600,"y":300,"w":400,"h":250},
          {"type":"vignette","vignette_h":200,"x":1100,"y":300,"w":400,"h":250} ] } } } } }"##;
    std::fs::write(dir.join("projet.json"), proj).unwrap();
    // la page de chapitres invalide bloque la construction, avec l'erreur dans le journal
    let mut logs = Vec::new();
    let r = {
        let (mut log, mut prog) = (|m: &str| logs.push(m.to_string()), |_f: f64| {});
        let cancel = AtomicBool::new(false);
        let mut env = BuildEnv { tools: &tools, renderer: &Flat, cancel: &cancel, log: &mut log, progress: &mut prog };
        build_project(&dir.join("projet.json"), &dir.join("out"), &mut env)
    };
    let e = r.expect_err("mise en page invalide : la construction doit s'arreter").to_string();
    assert!(e.contains("menu de chapitres 1 (page 1)"), "{e}");
    assert!(logs.iter().any(|l| l.starts_with("ERREUR de mise en page") && l.contains("chapitres")), "{logs:?}");
    // sans la page fautive, la construction aboutit
    let proj = proj.replace(r#""chapitres": { "1": { "1": { "elements": ["#, r#""chapitres_ignores": { "1": { "1": { "elements": ["#);
    std::fs::write(dir.join("projet.json"), &proj).unwrap();
    OVERLAYS.store(0, std::sync::atomic::Ordering::SeqCst);
    let info = {
        let (mut log, mut prog) = (|_m: &str| {}, |_f: f64| {});
        let cancel = AtomicBool::new(false);
        let mut env = BuildEnv { tools: &tools, renderer: &Flat, cancel: &cancel, log: &mut log, progress: &mut prog };
        build_project(&dir.join("projet.json"), &dir.join("out"), &mut env).unwrap()
    };
    assert!(info.report.ok(), "{}", info.report.render());
    assert_eq!(info.main_pages, 3, "2 pages automatiques + 1 page libre");
    assert_eq!(OVERLAYS.load(std::sync::atomic::Ordering::SeqCst), 1, "seule la 2e vignette est recouverte par l'image placee devant elle");
    // page libre : 3 vignettes (dont la video 3 deux fois ; celle sans video est absente), « Precedent », 2 boutons « Aller a »,
    // un texte et une image cliquables
    let rep = info.report.render();
    assert!(rep.contains(": 8 bouton(s)"), "{rep}");
    // page 1 : 2 vignettes d'elements, « Suivant », une vignette de la video 3 menant a la page libre
    assert!(rep.contains(": 4 bouton(s)"), "{rep}");
    // la page 1 personnalisee a son propre fond : un clip de fond de plus que sans personnalisation
    let bgs = std::fs::read_dir(dir.join("out/BDMV/STREAM")).unwrap().filter(|e| { let n = e.as_ref().unwrap().file_name().into_string().unwrap(); n.starts_with("0000") || n.starts_with("0001") }).count();
    assert!(bgs >= 5, "{bgs}");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn project_with_subtitles() {
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
    let Ok(tools) = Tools::locate() else { return };
    let dir = std::env::temp_dir().join(format!("bdauthor_style_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    for f in ["a", "b"] {
        assert!(Command::new(&tools.ffmpeg).args(["-y", "-v", "error", "-f", "lavfi", "-i", "testsrc=size=640x360:rate=25:duration=3",
            "-f", "lavfi", "-i", "sine=duration=3", "-c:v", "libx264", "-pix_fmt", "yuv420p", "-c:a", "aac", "-shortest"])
            .arg(dir.join(format!("{f}.mp4"))).status().unwrap().success());
    }
    // halo + agrandissement + etat active ; une vignette collee au bord gauche (son halo doit etre rogne) ; un bouton dessine
    let proj = r##"{ "titre": "Style", "langue": "fra", "videos": [ {"fichier": "a.mp4"}, {"fichier": "b.mp4"} ],
      "encodage": {"preset": "ultrafast"}, "menu": {"fond_secondes": 2},
      "mise_en_page": { "style_boutons": {"halo": 24, "agrandir": 10, "etat_active": true},
        "principal": { "1": { "elements": [
          {"type":"vignette","vignette_h":300,"x":0,"y":300,"w":560,"h":356},
          {"type":"vignette","vignette_h":300,"x":1000,"y":300,"w":560,"h":356,"style":{"halo":0,"agrandir":0}},
          {"type":"navigation","action":"aller_a","texte":"B","cible":{"lire":"video","video":2},"x":800,"y":900,"w":300,"h":90,
           "images_etats":{"normal":"btn.png"}} ] } } } }"##;
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
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn home_intro_and_play_all() {
    let Ok(tools) = Tools::locate() else { return };
    let dir = std::env::temp_dir().join(format!("bdauthor_home_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    for (f, d) in [("a", 3), ("b", 3), ("intro", 2)] {
        assert!(Command::new(&tools.ffmpeg).args(["-y", "-v", "error", "-f", "lavfi", "-i", &format!("testsrc=size=640x360:rate=25:duration={d}"),
            "-f", "lavfi", "-i", &format!("sine=duration={d}"), "-c:v", "libx264", "-pix_fmt", "yuv420p", "-c:a", "aac", "-shortest"])
            .arg(dir.join(format!("{f}.mp4"))).status().unwrap().success());
    }
    // accueil = page libre (page 2 du menu principal) ; « Tout lire » sur la page 1 ; sequence d'ouverture
    let proj = r##"{ "titre": "Accueil", "langue": "fra", "videos": [ {"fichier": "a.mp4", "chapitres": [0, 1.5]}, {"fichier": "b.mp4"} ],
      "encodage": {"preset": "ultrafast"}, "menu": {"fond_secondes": 2, "intro": "intro.mp4"},
      "mise_en_page": { "accueil": {"menu": "principal", "page": 2},
        "principal": { "1": { "elements": [
          {"type":"vignette","vignette_h":300,"x":200,"y":300,"w":560,"h":356},
          {"type":"vignette","vignette_h":300,"x":1000,"y":300,"w":560,"h":356},
          {"type":"navigation","action":"tout_lire","texte":"Tout lire","x":800,"y":900,"w":300,"h":90} ] } },
        "libres": [ { "elements": [
          {"type":"navigation","action":"aller_a","texte":"Videos","cible":{"menu":"principal","page":1},"x":800,"y":500,"w":300,"h":90} ] } ] } }"##;
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
    // titres : 2 videos, page 2 du menu principal (= page libre), chapitres de a (1 page + lecture depuis un repere) = 5 ;
    // puis la page 1 (titre 6) et « Tout lire » (titre 7)
    assert_eq!(idx.titles.len(), 7);
    assert!(text(0).contains("JUMP_TITLE 3"), "Top Menu -> accueil (titre de la page libre) : {}", text(0));
    assert!(text(6).starts_with("PLAY_PL"), "titre 6 = page 1 du menu : {}", text(6));
    assert!(text(7).starts_with("PLAY_PL") && text(7).contains("JUMP_TITLE 0"), "{}", text(7));
    // ouverture : objet dedie, lecture de l'intro puis accueil
    let fp = idx.first_playback.id_ref as usize;
    assert_eq!(fp, mo.objects.len() - 1);
    assert!(text(fp).starts_with("PLAY_PL") && text(fp).ends_with("JUMP_TITLE 0"), "{}", text(fp));
    // la playlist « Tout lire » enchaine les deux videos, avec une marque au debut de chacune et les chapitres de a
    let pl_all = mo.objects[7].commands[0].dst;
    let m = bdauthor_core::mpls::Mpls::parse(&std::fs::read(root.join(format!("PLAYLIST/{pl_all:05}.mpls"))).unwrap()).unwrap();
    assert_eq!(m.playitems.iter().map(|p| p.clip.as_str()).collect::<Vec<_>>(), vec!["00000", "00001"]);
    assert_eq!(m.marks.iter().map(|k| k.playitem_ref).collect::<Vec<_>>(), vec![0, 0, 1]);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn animated_menu_background_with_music() {
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
      "encodage": {"preset": "ultrafast"}, "menu": {"fond_video": "fond.mp4", "musique": "musique.mp3", "par_page": 1},
      "mise_en_page": { "principal": { "2": { "elements": [
          {"type":"texte","texte":"Page 2","x":100,"y":60,"w":800,"h":120,"taille":80,"couleur":"#ffcc00","alignement":"gauche"},
          {"type":"vignette","vignette_h":300,"x":600,"y":300,"w":560,"h":356},
          {"type":"navigation","action":"precedent","texte":"<","x":800,"y":900,"w":300,"h":90} ] } } } }"##;
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
    // pages du menu principal : playlists 2 et 3 (apres les 2 videos) ; fond avec son, en boucle (pas d'arret sur image)
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
      "encodage": {"preset": "ultrafast"}, "menu": {"musique": "musique.mp3", "par_page": 1, "fond_secondes": 2},
      "mise_en_page": { "principal": {
        "1": { "fond_page": {"video": "fond.mp4", "musique": null}, "elements": [
          {"type":"vignette","vignette_h":300,"x":600,"y":300,"w":560,"h":356},
          {"type":"navigation","action":"suivant","texte":">","x":800,"y":900,"w":300,"h":90} ] },
        "2": { "fond_page": {"image": "vert.png"}, "elements": [
          {"type":"vignette","vignette_h":300,"x":600,"y":300,"w":560,"h":356},
          {"type":"navigation","action":"precedent","texte":"<","x":400,"y":900,"w":300,"h":90},
          {"type":"navigation","action":"suivant","texte":">","x":1200,"y":900,"w":300,"h":90} ] } } } }"##;
    std::fs::write(dir.join("projet.json"), proj).unwrap();
    let info = {
        let (mut log, mut prog) = (|_m: &str| {}, |_f: f64| {});
        let cancel = AtomicBool::new(false);
        let mut env = BuildEnv { tools: &tools, renderer: &Flat, cancel: &cancel, log: &mut log, progress: &mut prog };
        build_project(&dir.join("projet.json"), &dir.join("out"), &mut env).unwrap()
    };
    assert!(info.report.ok(), "{}", info.report.render());
    let root = dir.join("out/BDMV");
    // playlists des pages du menu principal : 3, 4, 5
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
    let Ok(tools) = Tools::locate() else { return };
    let dir = std::env::temp_dir().join(format!("bdauthor_badlayout_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    assert!(Command::new(&tools.ffmpeg).args(["-y", "-v", "error", "-f", "lavfi", "-i", "testsrc=size=640x360:rate=25:duration=1", "-c:v", "libx264", "-pix_fmt", "yuv420p"])
        .arg(dir.join("a.mp4")).status().unwrap().success());
    // page 1 : 2 vignettes pour 1 seule video -> une vignette sans rien a montrer
    let proj = r##"{ "videos": [ {"fichier": "a.mp4"} ], "encodage": {"preset": "ultrafast"},
      "mise_en_page": { "principal": { "1": { "elements": [
          {"type":"vignette","vignette_h":300,"x":200,"y":300,"w":560,"h":356},
          {"type":"vignette","vignette_h":300,"x":1000,"y":300,"w":560,"h":356} ] } } } }"##;
    std::fs::write(dir.join("projet.json"), proj).unwrap();
    let mut logs = Vec::new();
    let r = {
        let (mut log, mut prog) = (|m: &str| logs.push(m.to_string()), |_f: f64| {});
        let cancel = AtomicBool::new(false);
        let mut env = BuildEnv { tools: &tools, renderer: &Flat, cancel: &cancel, log: &mut log, progress: &mut prog };
        build_project(&dir.join("projet.json"), &dir.join("out"), &mut env)
    };
    let e = r.expect_err("doit echouer").to_string();
    assert!(e.contains("menu principal (page 1)") && e.contains("sans élément"), "{e}");
    assert!(!logs.iter().any(|l| l.contains("encodage")), "aucun encodage avant l'erreur : {logs:?}");
    assert!(!dir.join(".cache_bdauthor").exists());
    let _ = std::fs::remove_dir_all(&dir);
}
