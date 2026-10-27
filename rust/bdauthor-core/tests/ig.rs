use bdauthor_core::ig::*;
use bdauthor_core::m2ts::{reassemble, split_segments};

#[test]
fn rle_roundtrip() {
    let mut rows = vec![vec![0u8; 300], vec![5u8; 70], vec![1, 1, 2, 2, 2, 0, 0, 7, 7, 7, 7]];
    rows.push((0..40u8).collect());
    rows.push(vec![9u8; 20000 % 16000]);
    // largeurs differentes : decode exige la meme largeur, on teste ligne par ligne
    for r in &rows {
        let enc = rle_encode(&[r.clone()]);
        let dec = rle_decode(&enc, r.len(), 1).unwrap();
        assert_eq!(dec[0], *r);
    }
}

#[test]
fn reference_ig_roundtrip() {
    let data = std::fs::read(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/data/ref_80002.m2ts")).unwrap();
    let (mut ics, mut pds, mut ods) = (0, 0, 0);
    for pes in reassemble(&data, 0x1400).unwrap() {
        for s in split_segments(&pes.data).unwrap() {
            match s.seg_type {
                SEG_ICS => { assert_eq!(Ics::parse(&s.payload).unwrap().pack(), s.payload); ics += 1; }
                SEG_PDS => { assert_eq!(Pds::parse(&s.payload).unwrap().pack(), s.payload); pds += 1; }
                SEG_ODS => {
                    let o = Ods::parse(&s.payload).unwrap();
                    assert_eq!(o.pack(), s.payload);
                    let px = o.pixels().unwrap();
                    // Le codeur de reference code les series de 3 en litteral (notre codeur : seuil 2) : on compare les pixels.
                    assert_eq!(rle_decode(&rle_encode(&px), o.width as usize, o.height as usize).unwrap(), px);
                    ods += 1;
                }
                _ => {}
            }
        }
    }
    assert!(ics > 0 && pds > 0 && ods > 0, "{ics} {pds} {ods}");
}
