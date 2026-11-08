//! Debit video qui remplit le disque.
use bdauthor_core::fill::*;

const BD25: u64 = 25_025_314_816;

fn input() -> FillInput {
    // 2 h de video, 2 pistes audio de 192 kb/s, rien de recopie
    FillInput { capacity: BD25, margin_pct: 95, reserve: menu_reserve_bytes(1), audio_bytes: audio_bytes(192, 2, 7200.0), copied_bytes: 0, encode_seconds: 7200.0, cap_kbps: 27000 }
}

#[test]
fn bitrate_fills_the_disc() {
    let i = input();
    let p = fill_bitrate(&i).unwrap();
    // refait le calcul a la main : la video encodee + l'audio + le menu, multiplexes, font bien ~95 % de la capacite
    let video = p.kbps as f64 * 1000.0 / 8.0 * 7200.0;
    let total = (video + i.audio_bytes as f64) * MUX_OVERHEAD + i.reserve as f64;
    let ratio = total / BD25 as f64;
    assert!((0.945..=0.951).contains(&ratio), "{ratio}");
    assert!(p.kbps > 20_000 && p.kbps <= 27_000, "{}", p.kbps);
}

#[test]
fn short_content_is_capped() {
    let p = fill_bitrate(&FillInput { encode_seconds: 600.0, audio_bytes: audio_bytes(192, 1, 600.0), ..input() }).unwrap();
    assert_eq!(p.kbps, 27_000);
    assert!(p.room_kbps > 100_000.0);
    assert!(describe(&p, "BD-25").contains("capped"));
}

#[test]
fn copied_videos_take_their_place() {
    let base = fill_bitrate(&input()).unwrap().kbps;
    // 4 Gio recopies : il reste moins de place pour le reste, a duree egale
    let less = fill_bitrate(&FillInput { copied_bytes: 4 << 30, ..input() }).unwrap().kbps;
    assert!(less < base, "{less} {base}");
    // moitie moins de contenu a encoder : plus de debit (jusqu'au plafond)
    let more = fill_bitrate(&FillInput { encode_seconds: 3600.0, ..input() }).unwrap().kbps;
    assert!(more > base && more <= 27_000);
    // tout est recopie : rien a encoder
    assert_eq!(fill_bitrate(&FillInput { encode_seconds: 0.0, ..input() }).unwrap().kbps, 27_000);
}

#[test]
fn too_much_content_is_refused() {
    let e = fill_bitrate(&FillInput { encode_seconds: 7200.0 * 50.0, ..input() }).unwrap_err();
    assert!(e.contains("does not fit"), "{e}");
    assert!(fill_bitrate(&FillInput { copied_bytes: BD25, ..input() }).is_err());
}

#[test]
fn audio_and_rate_helpers() {
    assert_eq!((parse_kbps("192k"), parse_kbps("448K"), parse_kbps("640000"), parse_kbps("?")), (192, 448, 640, 192));
    assert_eq!(audio_bytes(192, 1, 100.0), 2_400_000);
    assert_eq!(audio_bytes(192, 0, 100.0), 2_400_000); // au moins une piste (la piste par defaut)
    assert!(menu_reserve_bytes(12) > menu_reserve_bytes(1));
    // marge a 0 : 97 %, jamais 100
    let a = fill_bitrate(&FillInput { margin_pct: 0, ..input() }).unwrap();
    let b = fill_bitrate(&FillInput { margin_pct: 97, ..input() }).unwrap();
    assert_eq!(a.kbps, b.kbps);
}
