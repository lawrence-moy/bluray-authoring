//! Ecriture d'un dossier BDMV : fichiers + copies BACKUP + dossiers vides du disque de reference (equivalent de `disc.write_bdmv`).
use std::collections::BTreeMap;
use std::path::Path;

/// `files` : {chemin relatif a BDMV: octets}. Les copies BACKUP couvrent tout sauf STREAM.
pub fn write_bdmv(out_dir: &Path, files: &BTreeMap<String, Vec<u8>>) -> std::io::Result<()> {
    let root = out_dir.join("BDMV");
    // AUXDATA (sons des menus) n'a pas de copie de secours
    let backups = files.iter().filter(|(k, _)| !k.starts_with("STREAM") && !k.starts_with("AUXDATA")).map(|(k, v)| (format!("BACKUP/{k}"), v));
    for (rel, data) in files.iter().map(|(k, v)| (k.clone(), v)).chain(backups) {
        let path = root.join(&rel);
        if let Some(d) = path.parent() {
            std::fs::create_dir_all(d)?;
        }
        std::fs::write(path, data)?;
    }
    for d in ["AUXDATA", "BDJO", "JAR", "META"] {
        std::fs::create_dir_all(root.join(d))?;
    }
    std::fs::create_dir_all(out_dir.join("CERTIFICATE"))
}
