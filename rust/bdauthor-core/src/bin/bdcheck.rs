//! `bdcheck <dossier BDMV>` : verifie les references croisees d'un disque.
fn main() {
    let root = std::env::args().nth(1).unwrap_or_else(|| { eprintln!("usage : bdcheck <dossier BDMV>"); std::process::exit(2) });
    let rep = bdauthor_core::check::check_bdmv(std::path::Path::new(&root));
    print!("{}", rep.render());
    std::process::exit(if rep.ok() { 0 } else { 1 });
}
