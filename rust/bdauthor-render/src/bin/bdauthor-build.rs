//! `bdauthor-build projet.json dossier_sortie` : construit un dossier BDMV complet d'apres un projet JSON.
use bdauthor_core::ffmpeg::Tools;
use bdauthor_core::project::{build_project, BuildEnv};
use bdauthor_core::check::Kind;
use bdauthor_render::FontRenderer;
use std::path::Path;
use std::sync::atomic::AtomicBool;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.len() != 2 {
        eprintln!("usage : bdauthor-build projet.json dossier_sortie");
        std::process::exit(2);
    }
    let run = || -> Result<bool, String> {
        let tools = Tools::locate().map_err(|e| e.to_string())?;
        let renderer = FontRenderer::new()?;
        let cancel = AtomicBool::new(false);
        let mut last = -1i32;
        let mut log = |m: &str| println!("{m}");
        let mut prog = |f: f64| {
            let p = (f * 100.0) as i32;
            if p != last && p % 5 == 0 { println!("  {p} %"); }
            last = p;
        };
        let mut env = BuildEnv { tools: &tools, renderer: &renderer, cancel: &cancel, log: &mut log, progress: &mut prog };
        let info = build_project(Path::new(&args[0]), Path::new(&args[1]), &mut env).map_err(|e| e.to_string())?;
        println!("{} video(s), {} page(s) de menu, format {}x{} {} i/s, menu {}x{}, {:.1} Mo de flux",
                 info.videos, info.main_pages, info.format.0, info.format.1, info.format.2, info.menu_size.0, info.menu_size.1, info.total_stream_bytes as f64 / 1e6);
        for (k, m) in &info.report.items {
            if *k != Kind::Info { println!("[{}] {m}", if *k == Kind::Error { "ERREUR" } else { "avertissement" }); }
        }
        println!("verification : {}", if info.report.ok() { "aucune erreur" } else { "ERREURS" });
        Ok(info.report.ok())
    };
    match run() {
        Ok(true) => {}
        Ok(false) => std::process::exit(1),
        Err(e) => { eprintln!("erreur : {e}"); std::process::exit(1); }
    }
}
