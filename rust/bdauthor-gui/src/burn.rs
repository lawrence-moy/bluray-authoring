//! Gravure d'une image ISO : envoyee au graveur d'images disque de Windows (isoburn.exe), qui choisit le graveur, propose de
//! verifier le disque et grave les BD-R / BD-RE comme les DVD. Ailleurs que sous Windows, la gravure n'est pas proposee.
use crate::i18n::{tr, trf};
use std::path::{Path, PathBuf};

/// Graveur d'images disque de Windows (`%SystemRoot%\System32\isoburn.exe`), s'il est present.
pub fn burner() -> Option<PathBuf> {
    if !cfg!(windows) { return None; }
    let root = std::env::var_os("SystemRoot").map(PathBuf::from).unwrap_or_else(|| PathBuf::from(r"C:\Windows"));
    let p = root.join("System32").join("isoburn.exe");
    p.is_file().then_some(p)
}

/// La gravure est possible sur ce systeme.
pub fn available() -> bool { burner().is_some() }

/// Explication quand la gravure n'est pas possible.
pub fn unavailable_reason() -> &'static str {
    if cfg!(windows) {
        tr("the Windows disc image burner (isoburn.exe) was not found")
    } else {
        tr("burning goes through the Windows disc image burner; on this system, use your usual burning software")
    }
}

/// Ouvre le graveur de Windows sur `iso` : il demande le graveur a utiliser et lance la gravure. Ne bloque pas.
pub fn burn(iso: &Path) -> Result<(), String> {
    if !iso.is_file() { return Err(trf!("the image {} does not exist", iso.display())); }
    let Some(exe) = burner() else { return Err(unavailable_reason().to_string()) };
    std::process::Command::new(exe).arg(iso).spawn().map(|_| ()).map_err(|e| trf!("cannot start the disc image burner: {}", e))
}

/// Disque conseille pour une image de `bytes` octets (BD-25, BD-50, BD-100), ou `None` si elle est trop grosse.
pub fn disc_for(bytes: u64) -> Option<&'static str> {
    bdauthor_core::iso::CAPACITIES.iter().find(|(_, cap)| bytes <= *cap).map(|(n, _)| *n)
}
