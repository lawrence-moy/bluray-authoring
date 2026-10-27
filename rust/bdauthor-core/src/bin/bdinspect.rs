//! Affiche le contenu d'un fichier de navigation Blu-ray : index.bdmv, MovieObject.bdmv, *.mpls, *.clpi.
//!   bdinspect <fichier> [--verifier]     (--verifier : controle aussi que la reecriture est identique a l'octet pres)
use bdauthor_core::{clpi::Clpi, index::IndexBdmv, mobj::MovieObjectBdmv, mpls::Mpls};

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let Some(path) = args.first() else {
        eprintln!("usage : bdinspect <index.bdmv|MovieObject.bdmv|*.mpls|*.clpi> [--verifier]");
        std::process::exit(2);
    };
    let data = match std::fs::read(path) {
        Ok(d) => d,
        Err(e) => { eprintln!("lecture impossible : {e}"); std::process::exit(1) }
    };
    let r: Result<(String, Vec<u8>), String> = match data.get(..4) {
        Some(b"INDX") => IndexBdmv::parse(&data).map(|o| (o.dump(), o.pack())).map_err(|e| e.to_string()),
        Some(b"MOBJ") => MovieObjectBdmv::parse(&data).map(|o| (o.dump(None), o.pack())).map_err(|e| e.to_string()),
        Some(b"MPLS") => Mpls::parse(&data).map(|o| (o.dump(), o.pack())).map_err(|e| e.to_string()),
        Some(b"HDMV") => Clpi::parse(&data).map(|o| (o.dump(), o.pack())).map_err(|e| e.to_string()),
        _ => Err("format non reconnu".into()),
    };
    match r {
        Ok((text, packed)) => {
            println!("{text}");
            if args.iter().any(|a| a == "--verifier") {
                println!("\naller-retour identique : {}", packed == data);
            }
        }
        Err(e) => { eprintln!("erreur : {e}"); std::process::exit(1) }
    }
}
