//! Image ISO UDF 2.50 : ecriture, relecture de la structure, verification contre le dossier source.
mod common;
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
    common::fr();
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
    common::fr();
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
    common::fr();
    let src = disc("options");
    let dir = src.parent().unwrap();
    let stop = AtomicBool::new(false);
    // date fixe : image identique d'une construction a l'autre
    let mut o = opts("Été 2026 — vacances");
    o.date = IsoDate::Fixed;
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
    common::fr();
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

#[test]
fn project_settings() {
    common::fr();
    use bdauthor_core::json::parse;
    use bdauthor_core::model::ProjectModel;
    // par defaut : rien n'est ecrit dans le projet
    let mut m = ProjectModel::default();
    assert!(!m.to_json().contains("\"iso\""));
    assert_eq!(m.iso.label("Mes vidéos"), "Mes vidéos");
    // reglages modifies : seuls ceux-ci sont ecrits, et relus a l'identique
    m.iso.keep = IsoKeep::Both;
    m.iso.capacity = 1;
    m.iso.opts.label = "VACANCES".into();
    m.iso.opts.udf = UdfRev::V260;
    m.iso.opts.mirror = false;
    m.iso.opts.date = IsoDate::Custom(IsoStamp { year: 2026, month: 12, day: 25, hour: 20, minute: 30 });
    m.iso.margin = 90;
    m.iso.opts.compat = false;
    m.iso.verify = false;
    m.iso.file = "disques/vacances.iso".into();
    let j = m.to_json();
    let v = bdauthor_core::json::parse_project(&j).unwrap();
    let iso = v.get("iso").unwrap();
    assert_eq!(iso.get("conserver").and_then(|x| x.as_str()), Some("les_deux"));
    assert_eq!(iso.get("capacite").and_then(|x| x.as_str()), Some("BD-50"));
    assert!(iso.get("aligner").is_none(), "valeur par defaut non ecrite");
    let back = ProjectModel::from_json(&j).unwrap();
    assert_eq!(back.iso, m.iso);
    assert_eq!(back.iso.iso_path(Path::new("/p"), Path::new("/x/out")), Path::new("/p/disques/vacances.iso"));
    let d = IsoSettings::default();
    assert_eq!(d.iso_path(Path::new("/p"), Path::new("/x/out")), Path::new("/x/out.iso"));
    // valeurs inconnues refusees
    assert!(ProjectModel::from_json(r#"{"videos": [], "iso": {"capacite": "BD-30"}}"#).is_err());
    assert!(ProjectModel::from_json(r#"{"videos": [], "iso": {"conserver": "tout"}}"#).is_err());
}

#[test]
fn finish_after_build() {
    common::fr();
    let src = disc("fin");
    let dir = src.parent().unwrap().to_path_buf();
    let stop = AtomicBool::new(false);
    let mut logs: Vec<String> = Vec::new();
    // les deux : image verifiee, dossier garde ; le titre sert de nom de volume
    let s = IsoSettings { keep: IsoKeep::Both, ..IsoSettings::default() };
    let iso = s.iso_path(&dir, &src);
    assert_eq!(iso, dir.join("disque.iso"));
    let sum = finish_build(&src, &iso, &s, "Mon titre", &stop, &mut |l| logs.push(l.into()), &mut |_| {}).unwrap();
    assert!(sum.contains("vérifiée") && sum.contains("BD-25"), "{sum}");
    assert!(logs.iter().any(|l| l.contains("fichiers identiques")));
    assert_eq!(read_iso(&iso).unwrap().label, "Mon titre");
    assert!(src.join("BDMV").is_dir());
    // image seule, dans un dossier de construction temporaire supprime ensuite
    let s2 = IsoSettings { keep: IsoKeep::Iso, file: "sous/seule.iso".into(), verify: false, ..IsoSettings::default() };
    let iso2 = s2.iso_path(&dir, &src);
    let tmpd = temp_build_dir(&iso2).unwrap();
    assert_eq!(tmpd, dir.join("sous/seule.construction"));
    std::fs::create_dir_all(&tmpd).unwrap();
    for e in std::fs::read_dir(&src).unwrap() {
        let e = e.unwrap();
        let _ = std::process::Command::new("cp").arg("-r").arg(e.path()).arg(&tmpd).status();
    }
    let sum = finish_build(&tmpd, &iso2, &s2, "T", &stop, &mut |_| {}, &mut |_| {}).unwrap();
    assert!(!sum.contains("vérifiée"));
    assert!(iso2.is_file() && !tmpd.exists());
    verify_iso(&iso2, &src, &stop, &mut |_| {}).unwrap();
}

#[test]
fn compat_date_and_margin() {
    common::fr();
    let src = disc("compat");
    let dir = src.parent().unwrap().to_path_buf();
    let stop = AtomicBool::new(false);
    // compatibilite maximale (par defaut) : 3 ancres, sequence de reserve en fin de disque
    write_iso(&src, &dir.join("a.iso"), &opts("A"), &stop, &mut |_| {}).unwrap();
    let t = read_iso(&dir.join("a.iso")).unwrap();
    assert_eq!(t.anchors, 3);
    assert!(t.reserve_at as u64 > t.sectors - 300, "reserve en fin de disque : {} / {}", t.reserve_at, t.sectors);
    assert_eq!(std::fs::metadata(dir.join("a.iso")).unwrap().len(), iso_size(&src, &opts("A")).unwrap());
    verify_iso(&dir.join("a.iso"), &src, &stop, &mut |_| {}).unwrap();
    // sans : 2 ancres, reserve au debut, image plus petite
    let o = IsoOptions { compat: false, ..opts("B") };
    write_iso(&src, &dir.join("b.iso"), &o, &stop, &mut |_| {}).unwrap();
    let t2 = read_iso(&dir.join("b.iso")).unwrap();
    assert_eq!((t2.anchors, t2.reserve_at), (2, 48));
    assert_eq!(t.sectors - t2.sectors, 287);
    // reserve abimee : detectee
    let mut data = std::fs::read(dir.join("a.iso")).unwrap();
    data[t.reserve_at as usize * 2048 + 40] ^= 1;
    std::fs::write(dir.join("c.iso"), &data).unwrap();
    assert!(read_iso(&dir.join("c.iso")).unwrap_err().to_string().contains("reserve"));
    // date choisie : image reproductible, differente de la date fixe
    let d = IsoStamp { year: 2026, month: 12, day: 25, hour: 20, minute: 30 };
    let oc = IsoOptions { date: IsoDate::Custom(d), ..opts("D") };
    write_iso(&src, &dir.join("d1.iso"), &oc, &stop, &mut |_| {}).unwrap();
    write_iso(&src, &dir.join("d2.iso"), &oc, &stop, &mut |_| {}).unwrap();
    let (d1, d2) = (std::fs::read(dir.join("d1.iso")).unwrap(), std::fs::read(dir.join("d2.iso")).unwrap());
    assert!(d1 == d2);
    // horodatage du descripteur principal (secteur 32, octet 376) : annee 2026, mois 12, jour 25, 20 h 30
    assert_eq!(&d1[32 * 2048 + 378..32 * 2048 + 384], &[0xEA, 0x07, 12, 25, 20, 30]);
    assert_eq!(IsoStamp::parse("2026-12-25 20:30"), Some(d));
    assert_eq!(IsoStamp::parse("2026-02-29"), None, "2026 n'est pas bissextile");
    assert_eq!(IsoStamp::parse("2028-02-29").map(|s| s.day), Some(29));
    assert_eq!(IsoStamp { year: 2026, month: 2, day: 31, hour: 25, minute: 0 }.clamped().text(), "2026-02-28 23:00");
    // marge de securite : avertissement au-dela du pourcentage, rien si desactivee ; au-dela du disque : depassement
    let cap = CAPACITIES[0].1;
    assert!(capacity_warning(cap / 2, 0, 95).is_none());
    let w = capacity_warning(cap * 96 / 100, 0, 95).unwrap();
    assert!(w.contains("96,0 %") && w.contains("marge de sécurité (95 %)"), "{w}");
    assert!(capacity_warning(cap * 96 / 100, 0, 0).is_none());
    assert!(capacity_warning(cap * 96 / 100, 1, 95).is_none(), "rentre largement sur un BD-50");
    assert!(capacity_warning(cap + 1, 0, 0).unwrap().contains("dépasse la capacité"));
}
