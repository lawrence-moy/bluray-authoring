//! Image ISO UDF 2.50 : ecriture, relecture de la structure, verification contre le dossier source.
use bdauthor_core::iso::*;
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;

fn tmp(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("bdauthor_iso_{name}_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn put(root: &Path, rel: &str, data: &[u8]) {
    let p = root.join(rel);
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    std::fs::write(p, data).unwrap();
}

/// Contenu pseudo-aleatoire reproductible.
fn bytes(n: usize, seed: u32) -> Vec<u8> {
    let mut x = seed.wrapping_mul(2654435761).max(1);
    (0..n).map(|_| { x ^= x << 13; x ^= x >> 17; x ^= x << 5; x as u8 }).collect()
}

/// Dossier de disque minimal : navigation, 3 videos, copies de secours, dossiers vides, un fichier vide.
fn disc(name: &str) -> PathBuf {
    let d = tmp(name);
    let src = d.join("disque");
    put(&src, "BDMV/index.bdmv", &bytes(300, 1));
    put(&src, "BDMV/MovieObject.bdmv", &bytes(5000, 2));
    put(&src, "BDMV/PLAYLIST/00000.mpls", &bytes(700, 3));
    put(&src, "BDMV/CLIPINF/00000.clpi", &bytes(900, 4));
    put(&src, "BDMV/STREAM/00000.m2ts", &bytes(3 * 1024 * 1024 + 6144, 5));
    put(&src, "BDMV/STREAM/00001.m2ts", &bytes(6144 * 7, 6));
    put(&src, "BDMV/STREAM/00002.m2ts", &bytes(5000, 7));
    put(&src, "BDMV/AUXDATA/sound.bdmv", &[]);
    for f in ["index.bdmv", "MovieObject.bdmv", "PLAYLIST/00000.mpls", "CLIPINF/00000.clpi"] {
        put(&src, &format!("BDMV/BACKUP/{f}"), &std::fs::read(src.join("BDMV").join(f)).unwrap());
    }
    for e in ["BDMV/JAR", "BDMV/META", "BDMV/BDJO", "CERTIFICATE/BACKUP"] { std::fs::create_dir_all(src.join(e)).unwrap(); }
    // hors du disque : ignore
    put(&src, "journal.txt", b"pas dans l'image");
    src
}

fn opts(label: &str) -> IsoOptions { IsoOptions { label: label.into(), ..IsoOptions::default() } }

fn entry<'a>(t: &'a IsoTree, path: &str) -> &'a IsoEntry { t.entries.iter().find(|e| e.path == path).unwrap_or_else(|| panic!("{path} absent")) }

#[test]
fn write_read_verify() {
    let src = disc("base");
    let iso = src.parent().unwrap().join("disque.iso");
    let mut o = opts("MON DISQUE");
    o.owner = "Moi".into();
    o.organization = "Maison".into();
    o.contact = "moi@exemple.fr".into();
    let stop = AtomicBool::new(false);
    let mut last = 0.0;
    let rep = write_iso(&src, &iso, &o, &stop, &mut |p| { assert!(p >= last); last = p; }).unwrap();
    assert_eq!(last, 1.0);
    assert_eq!(rep.bytes, std::fs::metadata(&iso).unwrap().len());
    assert_eq!(rep.bytes, iso_size(&src, &o).unwrap(), "la taille annoncee doit etre exacte");
    assert_eq!(rep.bytes % 2048, 0);
    assert!(!src.parent().unwrap().join("disque.iso.part").exists());
    let t = read_iso(&iso).unwrap();
    assert_eq!((t.label.as_str(), t.owner.as_str(), t.organization.as_str(), t.contact.as_str()), ("MON DISQUE", "Moi", "Maison", "moi@exemple.fr"));
    assert_eq!(t.udf_rev, 0x0250);
    assert!(t.mirror && t.mirror_ok, "copie miroir distincte et identique");
    // arborescence : dossiers vides compris, fichiers hors BDMV/CERTIFICATE exclus
    assert!(entry(&t, "BDMV/JAR").dir);
    assert!(entry(&t, "CERTIFICATE/BACKUP").dir);
    assert!(t.entries.iter().all(|e| e.path != "journal.txt"));
    assert_eq!(entry(&t, "BDMV/AUXDATA/sound.bdmv").size, 0);
    assert_eq!(entry(&t, "BDMV/STREAM/00000.m2ts").size, 3 * 1024 * 1024 + 6144);
    assert_eq!(rep.files, t.entries.iter().filter(|e| !e.dir).count());
    assert_eq!(rep.dirs, t.entries.iter().filter(|e| e.dir).count() + 1);
    // videos alignees sur 64 Ko (32 secteurs) ; ordre : navigation, videos, copies de secours
    for v in ["00000", "00001", "00002"] {
        assert_eq!(entry(&t, &format!("BDMV/STREAM/{v}.m2ts")).extents[0].0 % 32, 0, "{v} aligne");
    }
    let at = |p: &str| entry(&t, p).extents[0].0;
    assert!(at("BDMV/index.bdmv") < at("BDMV/STREAM/00000.m2ts"));
    assert!(at("BDMV/STREAM/00000.m2ts") < at("BDMV/STREAM/00001.m2ts"));
    assert!(at("BDMV/STREAM/00002.m2ts") < at("BDMV/BACKUP/index.bdmv"));
    // verification complete
    let n = verify_iso(&iso, &src, &stop, &mut |_| {}).unwrap();
    assert_eq!(n, rep.files);
}

#[test]
fn corruption_detected() {
    let src = disc("corrompu");
    let iso = src.parent().unwrap().join("disque.iso");
    let stop = AtomicBool::new(false);
    write_iso(&src, &iso, &opts("X"), &stop, &mut |_| {}).unwrap();
    let t = read_iso(&iso).unwrap();
    let pos = entry(&t, "BDMV/STREAM/00001.m2ts").extents[0].0 * 2048 + 100;
    let mut data = std::fs::read(&iso).unwrap();
    data[pos as usize] ^= 0xFF;
    std::fs::write(&iso, &data).unwrap();
    let e = verify_iso(&iso, &src, &stop, &mut |_| {}).unwrap_err().to_string();
    assert!(e.contains("00001.m2ts"), "{e}");
    // descripteur abime : la structure est refusee
    data[pos as usize] ^= 0xFF;
    data[32 * 2048 + 30] ^= 0x01;
    std::fs::write(&iso, &data).unwrap();
    // la sequence principale est abimee : la lecture la refuse
    assert!(read_iso(&iso).is_err());
    // source modifiee apres coup : difference signalee
    data[32 * 2048 + 30] ^= 0x01;
    std::fs::write(&iso, &data).unwrap();
    put(&src, "BDMV/PLAYLIST/00001.mpls", b"nouveau");
    assert!(verify_iso(&iso, &src, &stop, &mut |_| {}).unwrap_err().to_string().contains("arborescence"));
}

#[test]
fn options() {
    let src = disc("options");
    let dir = src.parent().unwrap();
    let stop = AtomicBool::new(false);
    // date fixe : image identique d'une construction a l'autre
    let mut o = opts("Été 2026 — vacances");
    o.fixed_date = true;
    write_iso(&src, &dir.join("a.iso"), &o, &stop, &mut |_| {}).unwrap();
    write_iso(&src, &dir.join("b.iso"), &o, &stop, &mut |_| {}).unwrap();
    assert!(std::fs::read(dir.join("a.iso")).unwrap() == std::fs::read(dir.join("b.iso")).unwrap());
    // le tiret long impose l'UTF-16 : 15 caracteres au plus
    assert_eq!(read_iso(&dir.join("a.iso")).unwrap().label, "Été 2026 — vaca");
    assert_eq!(fit_label("Été 2026 — vacances"), "Été 2026 — vaca");
    assert_eq!(fit_label("Été 2026 - vacances"), "Été 2026 - vacances");
    // sans miroir, sans alignement, ordre de l'arborescence, UDF 2.60
    let o2 = IsoOptions { label: "ドラマ".into(), udf: UdfRev::V260, align: false, play_order: false, mirror: false, ..IsoOptions::default() };
    write_iso(&src, &dir.join("c.iso"), &o2, &stop, &mut |_| {}).unwrap();
    let t = read_iso(&dir.join("c.iso")).unwrap();
    assert_eq!(t.label, "ドラマ");
    assert_eq!(t.udf_rev, 0x0260);
    assert!(!t.mirror && !t.mirror_ok);
    // ordre alphabetique de l'arborescence : BACKUP avant STREAM ; les videos s'enchainent sans trou
    let at = |p: &str| entry(&t, p).extents[0].0;
    assert!(at("BDMV/BACKUP/index.bdmv") < at("BDMV/STREAM/00000.m2ts"));
    assert_eq!(at("BDMV/STREAM/00001.m2ts"), at("BDMV/STREAM/00000.m2ts") + (3 * 1024 * 1024 + 6144) / 2048);
    verify_iso(&dir.join("c.iso"), &src, &stop, &mut |_| {}).unwrap();
    assert!(std::fs::metadata(dir.join("c.iso")).unwrap().len() < std::fs::metadata(dir.join("a.iso")).unwrap().len());
    // nom trop long : tronque (30 caracteres latins)
    let long = "A".repeat(40);
    write_iso(&src, &dir.join("d.iso"), &opts(&long), &stop, &mut |_| {}).unwrap();
    assert_eq!(read_iso(&dir.join("d.iso")).unwrap().label, "A".repeat(MAX_LABEL));
}

#[test]
fn big_files_split_and_cancel() {
    let src = disc("etendues");
    let dir = src.parent().unwrap();
    let stop = AtomicBool::new(false);
    // etendues de 64 Ko au plus : la grosse video est decoupee en plusieurs morceaux contigus
    write_iso_ext(&src, &dir.join("e.iso"), &opts("E"), 65536, &stop, &mut |_| {}).unwrap();
    let t = read_iso(&dir.join("e.iso")).unwrap();
    let v = entry(&t, "BDMV/STREAM/00000.m2ts");
    assert_eq!(v.extents.len(), (3 * 1024 * 1024 + 6144usize).div_ceil(65536));
    assert!(v.extents.windows(2).all(|w| w[0].0 + w[0].1 / 2048 == w[1].0));
    verify_iso(&dir.join("e.iso"), &src, &stop, &mut |_| {}).unwrap();
    // annulation : pas de fichier laisse
    let stop = AtomicBool::new(true);
    assert!(write_iso(&src, &dir.join("f.iso"), &opts("F"), &stop, &mut |_| {}).is_err());
    assert!(!dir.join("f.iso").exists() && !dir.join("f.iso.part").exists());
    // pas de BDMV : erreur claire
    assert!(write_iso(dir, &dir.join("g.iso"), &opts("G"), &AtomicBool::new(false), &mut |_| {}).unwrap_err().to_string().contains("BDMV"));
}
