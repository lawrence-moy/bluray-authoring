use bdauthor_core::barcode::*;

/// Decode un Code 128 (retrouve les symboles par leurs largeurs) : verifie la coherence de la table et de la somme de controle.
fn decode128(modules: &[bool]) -> Vec<u32> {
    // largeurs des elements
    let mut w: Vec<usize> = Vec::new();
    let mut i = 0;
    while i < modules.len() { let s = i; while i < modules.len() && modules[i] == modules[s] { i += 1; } w.push(i - s); }
    assert_eq!(w.len() % 6, 1, "6 elements par symbole + 7 pour l'arret");
    let mut out = Vec::new();
    for ch in w[..w.len() - 7].chunks(6) {
        let key: String = ch.iter().map(|n| char::from(b'0' + *n as u8)).collect();
        let v = (0..106u32).find(|v| code128_values_table(*v) == key).expect("symbole inconnu");
        out.push(v);
    }
    out
}

fn code128_values_table(v: u32) -> String { C128[v as usize].to_string() }

#[test]
fn code128_values_and_checksum() {
    // exemple classique : P J J 1 2 3 C en jeu B -> depart B, valeurs ASCII - 32, controle (104 + sum(k * v)) mod 103
    let v = code128_values("PJJ123C").unwrap();
    assert_eq!(v[0], 104);
    assert_eq!(&v[1..8], &[48, 42, 42, 17, 18, 19, 35]);
    assert_eq!(*v.last().unwrap(), 55);
    // chiffres : jeu C
    assert_eq!(code128_values("123456").unwrap(), vec![105, 12, 34, 56, (105 + 12 + 2 * 34 + 3 * 56) % 103]);
    // melange : lettre puis chiffres puis lettre
    let v = code128_values("A1234b").unwrap();
    assert_eq!(v[..2], [104, 33]);
    assert_eq!(v[2], 99, "passage en jeu C");
    assert_eq!(v[3..5], [12, 34]);
    assert_eq!(v[5], 100, "retour en jeu B");
    assert_eq!(v[6], (b'b' - 32) as u32);
    // serie impaire de chiffres : le premier en jeu B
    let v = code128_values("12345").unwrap();
    assert_eq!(v[..3], [104, 17, 99]);
    assert_eq!(v[3..5], [23, 45]);
    // rejets
    assert!(code128_values("").is_err());
    assert!(code128_values("é").is_err());
}

#[test]
fn code128_modules_decode_back_to_the_symbols() {
    for text in ["PJJ123C", "BD-2026", "Hello, world!", "0123456789", "A1234b", "12345", "1", "~ ~"] {
        let bc = code128(text).unwrap();
        // chaque symbole : 11 modules ; arret : 13
        assert_eq!(bc.modules.len(), (code128_values(text).unwrap().len()) * 11 + 13, "{text}");
        assert!(bc.modules[0] && *bc.modules.last().unwrap(), "commence et finit par une barre");
        assert_eq!(decode128(&bc.modules), code128_values(text).unwrap(), "{text}");
    }
}

#[test]
fn code128_table_is_consistent() {
    // 107 symboles distincts, de 11 modules (13 pour l'arret) ; le nombre de modules noirs de chaque symbole est pair
    let mut seen = std::collections::HashSet::new();
    assert_eq!(C128[106].bytes().map(|c| (c - b'0') as u32).sum::<u32>(), 13);
    for v in 0..106u32 {
        let k = code128_values_table(v);
        assert_eq!(k.len(), 6, "{v}");
        assert_eq!(k.bytes().map(|c| (c - b'0') as u32).sum::<u32>(), 11, "symbole {v}");
        let bars: u32 = k.bytes().step_by(2).map(|c| (c - b'0') as u32).sum();
        assert_eq!(bars % 2, 0, "parite du symbole {v}");
        assert!(seen.insert(k), "symbole {v} en double");
    }
}

#[test]
fn ean13_known_codes() {
    // controle calcule quand il manque
    let a = encode(BarcodeKind::Ean13, "590123412345").unwrap();
    let b = encode(BarcodeKind::Ean13, "5901234123457").unwrap();
    assert_eq!(a, b);
    assert_eq!(a.text, "5 901234 123457");
    // 3 + 6*7 + 5 + 6*7 + 3 = 95 modules, gardes 101 / 01010 / 101
    assert_eq!(a.modules.len(), 95);
    let s: String = a.modules.iter().map(|m| if *m { '1' } else { '0' }).collect();
    assert_eq!(&s[..3], "101");
    assert_eq!(&s[45..50], "01010");
    assert_eq!(&s[92..], "101");
    // premier chiffre 5 : parite LGGLLG ; le premier chiffre de gauche (9) est en L : 0001011
    assert_eq!(&s[3..10], "0001011");
    // le deuxieme (0) est en G : R(0) = 1110010 -> a l'envers 0100111
    assert_eq!(&s[10..17], "0100111");
    // chiffres de droite en R : 7 -> R = complement de 0111011 = 1000100
    assert_eq!(&s[85..92], "1000100");
    assert!(encode(BarcodeKind::Ean13, "4006381333931").is_ok());
    // mauvais controle, mauvaise longueur, lettres
    assert!(encode(BarcodeKind::Ean13, "5901234123450").unwrap_err().contains('7'));
    assert!(encode(BarcodeKind::Ean13, "12345").is_err());
    assert!(encode(BarcodeKind::Ean13, "59012341234a").is_err());
    // espaces et tirets ignores
    assert_eq!(encode(BarcodeKind::Ean13, "5 901234-123457").unwrap(), b);
}

#[test]
fn ean8_and_upca() {
    let e = encode(BarcodeKind::Ean8, "9638507").unwrap();
    assert_eq!(e, encode(BarcodeKind::Ean8, "96385074").unwrap());
    assert_eq!(e.modules.len(), 67);
    assert_eq!(e.text, "9638 5074");
    assert!(encode(BarcodeKind::Ean8, "96385075").is_err());
    let u = encode(BarcodeKind::UpcA, "03600029145").unwrap();
    assert_eq!(u, encode(BarcodeKind::UpcA, "036000291452").unwrap());
    assert_eq!(u.modules.len(), 95);
    assert_eq!(u.text, "0 36000 29145 2");
    // UPC-A = EAN-13 precede d'un 0
    assert_eq!(u.modules, encode(BarcodeKind::Ean13, "0036000291452").unwrap().modules);
}

#[test]
fn samples_are_valid_and_runs_are_found() {
    for k in BarcodeKind::ALL { assert!(encode(k, k.sample()).is_ok(), "{}", k.label()); assert_eq!(BarcodeKind::from_name(k.name()), Some(k)); }
    let runs = bar_runs(&[true, true, false, true, false, false, true]);
    assert_eq!(runs, vec![(0, 2), (3, 1), (6, 1)]);
}
