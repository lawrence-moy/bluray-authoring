//! `bdiso <dossier du disque> <image.iso> [nom du volume]` : cree l'image UDF 2.50 d'un dossier contenant BDMV, puis la verifie.
//! `bdiso --lire <image.iso>` : controle la structure d'une image et liste son contenu.
use bdauthor_core::iso::*;
use std::path::Path;
use std::sync::atomic::AtomicBool;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let stop = AtomicBool::new(false);
    let res = match args.as_slice() {
        [flag, iso] if flag == "--lire" => read_iso(Path::new(iso)).map(|t| {
            println!("volume « {} » · UDF {:x} · {} secteurs · miroir : {}", t.label, t.udf_rev, t.sectors, if t.mirror { if t.mirror_ok { "oui, identique" } else { "DIFFERENT" } } else { "non" });
            for e in &t.entries {
                if e.dir { println!("  {}/", e.path); } else { println!("  {}  ({} octets, secteur {})", e.path, e.size, e.extents.first().map_or(0, |x| x.0)); }
            }
        }),
        [src, iso, rest @ ..] if rest.len() <= 1 => {
            let label = rest.first().cloned().unwrap_or_else(|| Path::new(src).file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default());
            let opt = IsoOptions { label, ..IsoOptions::default() };
            let mut last = -1i64;
            write_iso(Path::new(src), Path::new(iso), &opt, &stop, &mut |p| {
                let pc = (p * 100.0) as i64;
                if pc / 10 != last / 10 { eprint!("{pc}% "); last = pc; }
            })
            .and_then(|r| {
                eprintln!();
                println!("image ecrite : {} ({:.1} Mo, {} fichiers, {} dossiers)", iso, r.bytes as f64 / 1e6, r.files, r.dirs);
                verify_iso(Path::new(iso), Path::new(src), &stop, &mut |_| {}).map(|n| println!("verification : {n} fichiers identiques"))
            })
        }
        _ => {
            eprintln!("usage : bdiso <dossier du disque> <image.iso> [nom du volume]\n        bdiso --lire <image.iso>");
            std::process::exit(2);
        }
    };
    if let Err(e) = res {
        eprintln!("ERREUR : {e}");
        std::process::exit(1);
    }
}
