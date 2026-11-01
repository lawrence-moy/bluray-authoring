//! `bdauthor-build projet.json dossier_sortie` : construit un dossier BDMV complet d'apres un projet JSON, puis l'image ISO si le
//! projet la demande (cle « iso » ; le dossier est toujours garde ici).
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
        let model = bdauthor_core::model::ProjectModel::from_json(&std::fs::read_to_string(&args[0]).map_err(|e| e.to_string())?)?;
        if model.iso.enabled() {
            let mut s = model.iso.clone();
            s.keep = bdauthor_core::iso::IsoKeep::Both;
            let base = std::fs::canonicalize(&args[0]).ok().and_then(|p| p.parent().map(Path::to_path_buf)).unwrap_or_default();
            let iso = s.iso_path(&base, Path::new(&args[1]));
            let sum = bdauthor_core::iso::finish_build(Path::new(&args[1]), &iso, &s, &model.title, &cancel, &mut log, &mut prog).map_err(|e| e.to_string())?;
            println!("{sum}");
        }
        Ok(info.report.ok())
    };
    match run() {
        Ok(true) => {}
        Ok(false) => std::process::exit(1),
        Err(e) => { eprintln!("erreur : {e}"); std::process::exit(1); }
    }
}
