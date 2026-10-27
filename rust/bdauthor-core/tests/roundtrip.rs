//! Aller-retour octet pour octet sur les fichiers de reference (disque multiAVCHD d'origine) et sur ceux generes par bdauthor,
//! et comparaison des descriptions textuelles avec celles produites par la version Python (`*.dump`).
use bdauthor_core::{clpi::Clpi, index::IndexBdmv, mobj::MovieObjectBdmv, mpls::Mpls};
use std::path::PathBuf;

fn data_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests").join("data")
}

#[test]
fn aller_retour_et_descriptions_identiques_a_python() {
    let mut n = 0;
    for e in std::fs::read_dir(data_dir()).unwrap() {
        let p = e.unwrap().path();
        let name = p.file_name().unwrap().to_string_lossy().to_string();
        if name.ends_with(".dump") || !data_dir().join(format!("{name}.dump")).exists() {
            continue;
        }
        let data = std::fs::read(&p).unwrap();
        let golden = std::fs::read_to_string(data_dir().join(format!("{name}.dump"))).unwrap();
        let (packed, dump) = if name.ends_with("index.bdmv") {
            let o = IndexBdmv::parse(&data).unwrap();
            (o.pack(), o.dump())
        } else if name.contains("MovieObject") {
            let o = MovieObjectBdmv::parse(&data).unwrap();
            (o.pack(), o.dump(None))
        } else if name.ends_with(".mpls") {
            let o = Mpls::parse(&data).unwrap();
            (o.pack(), o.dump())
        } else {
            let o = Clpi::parse(&data).unwrap();
            (o.pack(), o.dump())
        };
        assert!(packed == data, "{name} : la reecriture differe de l'original ({} / {} octets)", packed.len(), data.len());
        assert_eq!(dump, golden, "{name} : description differente de celle de Python");
        n += 1;
    }
    assert!(n >= 13, "fichiers de reference manquants ({n})");
}

#[test]
fn signatures_et_troncatures() {
    assert!(IndexBdmv::parse(b"XXXX0100").is_err());
    assert!(MovieObjectBdmv::parse(b"MOBJ0100").is_err());
    let data = std::fs::read(data_dir().join("ref_00000.mpls")).unwrap();
    for cut in [0, 7, 40, 100, data.len() - 1] {
        assert!(Mpls::parse(&data[..cut]).is_err(), "coupe a {cut} octets acceptee");
    }
    let clpi = std::fs::read(data_dir().join("ref_00000.clpi")).unwrap();
    for cut in [3, 39, 200, clpi.len() - 2] {
        assert!(Clpi::parse(&clpi[..cut]).is_err(), "clpi coupe a {cut} octets accepte");
    }
}
