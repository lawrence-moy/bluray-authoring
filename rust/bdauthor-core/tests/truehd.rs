//! Piste TrueHD recopiee : lecture du TS de ffmpeg (unites TrueHD + coeur AC-3 encode a part), entrelacement, muxage M2TS, relecture.
//! Le TS `enc_truehd.ts` (ffmpeg, 1,5 s, 2 canaux) porte la piste TrueHD puis un coeur AC-3, comme le produit `make_film_m2ts_tracks`.
use bdauthor_core::film::*;
use bdauthor_core::m2ts::reassemble;
use bdauthor_core::truehd;
use bdauthor_core::tsclip::*;
use std::path::PathBuf;

fn data(name: &str) -> Vec<u8> {
    std::fs::read(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/data").join(name)).unwrap()
}

/// Toutes les unites TrueHD (octets) d'une piste, dans l'ordre.
fn all_units(enc_audio: &[bdauthor_core::vmux::AudioFrame]) -> Vec<Vec<u8>> {
    let mut rate = 0;
    let mut out = Vec::new();
    for f in enc_audio {
        for u in truehd::units(&f.ext, &mut rate).unwrap() { out.push(u.data.to_vec()); }
    }
    out
}

#[test]
fn truehd_et_coeur_ac3_sont_entrelaces_puis_relus() {
    let mut enc = load_encoded(&data("enc_truehd.ts")).unwrap();
    // lecture brute : piste TrueHD (une unite par trame, sans coeur) puis coeur AC-3
    assert_eq!(enc.codecs, vec![0x83, 0x81]);
    assert!(enc.audio.iter().all(|f| f.data.is_empty() && !f.ext.is_empty()));
    let units_before = all_units(&enc.audio);
    assert!(units_before.len() > 1000, "{} unites", units_before.len());
    let cores = enc.extra_audio[0].clone();

    attach_truehd_cores(&mut enc, &[true]).unwrap();
    assert_eq!((enc.audio_tracks(), enc.codecs.clone(), enc.lpcm.len()), (1, vec![0x83], 1));
    // une trame par trame AC-3, le meme coeur, aucune unite perdue ni deplacee
    assert_eq!(enc.audio.len(), cores.len());
    for (f, c) in enc.audio.iter().zip(&cores) {
        assert_eq!((f.pts, &f.data), (c.pts, &c.data));
        assert!(f.ext.len() <= truehd::MAX_PES_BYTES);
    }
    assert_eq!(all_units(&enc.audio), units_before);
    // chaque trame porte environ 1536 echantillons (38 a 39 unites de 40)
    let mid = &enc.audio[enc.audio.len() / 2];
    let n = truehd::units(&mid.ext, &mut 48_000).unwrap().len();
    assert!((37..=40).contains(&n), "{n} unites dans une trame");

    // muxage puis relecture du clip
    let (m2ts, _) = mux_encoded_tracks(&enc, &[*b"fra"], &[]).unwrap();
    // BDAUTHOR_TEST_OUT : garde le clip pour l'examiner (ffmpeg, lecteur)
    if let Ok(p) = std::env::var("BDAUTHOR_TEST_OUT") { std::fs::write(p, &m2ts).unwrap(); }
    let back = load_encoded_m2ts(&m2ts).unwrap();
    assert_eq!((back.audio_tracks(), back.codecs.clone()), (1, vec![0x83]));
    assert_eq!(back.audio.len(), enc.audio.len());
    for (a, b) in back.audio.iter().zip(&enc.audio) {
        assert_eq!((&a.data, &a.ext), (&b.data, &b.ext));
    }

    // paquets : AC-3 (0x76) puis TrueHD (0x72) en alternance, dans la piste 0x1100
    let pes = reassemble(&m2ts, 0x1100).unwrap();
    let ids: Vec<Option<u8>> = pes.iter().map(|e| bdauthor_core::dts::pes_extension_id(&e.header)).collect();
    assert_eq!(&ids[..4], &[Some(0x76), Some(0x72), Some(0x76), Some(0x72)]);
    assert_eq!(ids.len(), enc.audio.len() * 2);
    assert_eq!(pes[0].pts, pes[1].pts);

    // le clip est decrit comme du TrueHD
    let clip = make_clip_clpi(&m2ts, *b"fra", 0, None, "0100").unwrap();
    assert!(!clip.is_empty());
    let scan = scan_clip(&m2ts).unwrap();
    assert_eq!(scan.audio, vec![(0x1100, 0x83)]);
}

#[test]
fn un_coeur_manquant_ou_un_nombre_de_pistes_inattendu_est_refuse() {
    let mut enc = load_encoded(&data("enc_truehd.ts")).unwrap();
    // deux pistes lues, mais on en annonce deux de TrueHD : il manque un coeur
    assert!(attach_truehd_cores(&mut enc, &[true, true]).is_err());
    let mut enc = load_encoded(&data("enc_truehd.ts")).unwrap();
    assert!(attach_truehd_cores(&mut enc, &[false, false]).is_ok());
    assert_eq!(enc.codecs, vec![0x83, 0x81]);
}
