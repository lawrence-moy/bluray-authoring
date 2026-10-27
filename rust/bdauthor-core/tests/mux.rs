//! Muxage : les octets produits par Rust doivent etre identiques a ceux de la version Python (empreinte FNV-1a 64 bits + longueur,
//! `tests/data/mux_hashes.txt`). Le test du film de reference a besoin du fichier 00000.m2ts (variable BDAUTHOR_REF_M2TS) ; sans lui il est ignore.
use bdauthor_core::film::*;
use bdauthor_core::m2ts::*;
use bdauthor_core::mux::*;
use bdauthor_core::tsclip::*;
use bdauthor_core::vmux::*;
use std::path::PathBuf;

fn data(name: &str) -> Vec<u8> {
    std::fs::read(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/data").join(name)).unwrap()
}

fn fnv(b: &[u8]) -> u64 {
    b.iter().fold(0xcbf2_9ce4_8422_2325u64, |h, &x| (h ^ x as u64).wrapping_mul(0x0000_0100_0000_01b3))
}

/// name -> (longueur, empreinte, avance)
fn expected(name: &str) -> (usize, u64, f64) {
    let txt = String::from_utf8(data("mux_hashes.txt")).unwrap();
    let l = txt.lines().find(|l| l.split(' ').next() == Some(name)).unwrap_or_else(|| panic!("{name} absent de mux_hashes.txt"));
    let f: Vec<&str> = l.split(' ').collect();
    (f[1].parse().unwrap(), u64::from_str_radix(f[2], 16).unwrap(), f.get(3).and_then(|x| x.parse().ok()).unwrap_or(0.0))
}

fn check(name: &str, bytes: &[u8]) {
    let (len, h, _) = expected(name);
    assert_eq!((bytes.len(), fnv(bytes)), (len, h), "{name} : different de la sortie Python (rust {} octets, {:016x})", bytes.len(), fnv(bytes));
}

struct Lcg(u64);
impl Lcg {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        self.0 >> 33
    }
}

fn synth(big: Option<usize>) -> (Vec<VideoAu>, Vec<AudioFrame>) {
    let mut r = Lcg(12345);
    let mut video = Vec::new();
    for i in 0..40u64 {
        let dts = 54_000_000 + i * 1501;
        let pts = if i % 3 != 0 { dts + 3003 } else { dts + 1501 * 2 };
        let size = if i != 7 { [800usize, 5000, 30000, 120000][(r.next() % 4) as usize] } else { big.unwrap_or(3000) };
        let d: Vec<u8> = (0..size).map(|_| (r.next() & 0xFF) as u8).collect();
        video.push(VideoAu { pts, dts: Some(dts), data: d });
    }
    video[0].pts = video[0].dts.unwrap(); // pts == dts : chemin « PTS seul »
    let audio = (0..60u64).map(|k| AudioFrame { pts: 54_000_000 - 2880 + k * 2880, data: (0..768).map(|_| (r.next() & 0xFF) as u8).collect() }).collect();
    (video, audio)
}

fn synth_streams() -> Vec<PmtStream> {
    let mut a = hex("050441432d33810408200400");
    a.extend(hex("0a04"));
    a.extend_from_slice(b"fra\0");
    vec![PmtStream { stream_type: 0x1B, pid: 0x1011, desc: hex("28044d4029bf") }, PmtStream { stream_type: 0x81, pid: 0x1100, desc: a }]
}

fn hex(s: &str) -> Vec<u8> {
    (0..s.len() / 2).map(|i| u8::from_str_radix(&s[2 * i..2 * i + 2], 16).unwrap()).collect()
}

#[test]
fn film_synthetique() {
    let (v, a) = synth(None);
    let (m, st) = mux_film(&v, &a, &synth_streams(), &MuxParams::default()).unwrap();
    check("synth", &m);
    assert!((st.avance_s - expected("synth").2).abs() < 1e-6);
    assert_eq!(m.len() % (192 * 32), 0);
}

#[test]
fn film_synthetique_avance_augmentee() {
    // une image de 2,5 Mo n'arrive pas en 0,483 s : l'avance doit etre augmentee automatiquement
    let (v, a) = synth(Some(2_500_000));
    let (m, st) = mux_film(&v, &a, &synth_streams(), &MuxParams::default()).unwrap();
    check("synth_grand", &m);
    assert!(st.avance_s > 0.483);
}

#[test]
fn menu_de_reference() {
    let ig = data("ref_80002.m2ts");
    let specs: Vec<PesSpec> = reassemble(&ig, 0x1400).unwrap().iter().map(|e| PesSpec { pts: e.pts.unwrap(), dts: e.dts, data: e.data.clone() }).collect();
    let extra: Vec<usize> = iter_packets(&ig).unwrap().map(|p| p.unwrap()).filter(|p| p.pcr.is_some()).map(|p| p.index).skip(1).collect();
    assert!(mux_ig_clip(&specs, Some(&extra)).unwrap() == ig, "le rejeu du menu de reference differe de l'original");
    check("ig_defaut", &mux_ig_clip(&specs, None).unwrap());

    // PTS/DTS recalcules d'apres les segments et la regle de duree de decodage
    let segs: Vec<Segment> = reassemble(&ig, 0x1400).unwrap().iter().map(|e| split_segments(&e.data).unwrap().remove(0)).collect();
    let ics = segs.iter().find(|s| s.name() == "ICS").unwrap().pack();
    let pds = segs.iter().find(|s| s.name() == "PDS").unwrap().pack();
    let ods: Vec<(Vec<u8>, u64, u64)> = segs.iter().filter(|s| s.name() == "ODS").map(|s| {
        let p = &s.payload; // id(2) version(1) sequence(1) longueur(3) largeur(2) hauteur(2)
        (s.pack(), u16::from_be_bytes([p[7], p[8]]) as u64, u16::from_be_bytes([p[9], p[10]]) as u64)
    }).collect();
    let calc = ig_pes_specs(&ics, &pds, &ods, specs[0].pts, (1920, 1080));
    assert_eq!(calc.iter().map(|s| (s.pts, s.dts)).collect::<Vec<_>>(), specs.iter().map(|s| (s.pts, s.dts)).collect::<Vec<_>>());
}

#[test]
fn flux_ffmpeg_vers_m2ts_et_clpi() {
    let enc = load_encoded(&data("enc_court.ts")).unwrap();
    let (m, _) = mux_encoded(&enc, *b"fra").unwrap();
    check("enc_court_m2ts", &m);
    check("enc_court_clpi", &make_clip_clpi(&m, *b"fra", 0, None, "0100").unwrap());
}

#[test]
fn film_de_reference() {
    let Ok(path) = std::env::var("BDAUTHOR_REF_M2TS") else {
        eprintln!("BDAUTHOR_REF_M2TS non defini : test ignore");
        return;
    };
    let reff = std::fs::read(path).unwrap();
    // le .clpi genere a partir du film de reference est celui de multiAVCHD
    let c = make_clip_clpi(&reff, *b"und", b'0', None, "0100").unwrap();
    assert!(c == data("ref_00000.clpi"), "le .clpi regenere differe de 00000.clpi");
    check("ref_clpi_sur_ref", &c);
    // remuxage
    let (v, a) = (reassemble(&reff, 0x1011).unwrap(), reassemble(&reff, 0x1100).unwrap());
    let sps = v.iter().find_map(|e| nal_units(&e.data).iter().find(|n| n.0 == 7).map(|&(_, o, n)| e.data[o..o + n].to_vec())).unwrap();
    let audio: Vec<AudioFrame> = a.iter().map(|e| AudioFrame { pts: e.pts.unwrap(), data: e.data.clone() }).collect();
    let video: Vec<VideoAu> = v.iter().map(|e| VideoAu { pts: e.pts.unwrap(), dts: e.dts, data: e.data.clone() }).collect();
    let mut ad = hex("050441432d33");
    ad.extend(ac3_descriptor(&audio[0].data).unwrap());
    ad.extend(hex("0a04"));
    ad.extend_from_slice(b"und\0");
    let streams = vec![PmtStream { stream_type: 0x1B, pid: 0x1011, desc: avc_descriptor(&sps) }, PmtStream { stream_type: 0x81, pid: 0x1100, desc: ad }];
    let (m, _) = mux_film(&video, &audio, &streams, &MuxParams::default()).unwrap();
    check("ref_film", &m);
    check("ref_film_clpi", &make_clip_clpi(&m, *b"und", b'0', None, "0100").unwrap());
}
