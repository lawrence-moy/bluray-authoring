//! Les fichiers de navigation construits par Rust doivent etre identiques octet pour octet a ceux construits par Python (`nav_*.bin`),
//! dont le disque de reference multiAVCHD (playlist par defaut == 00000.mpls) et le disque de test paginé.
use bdauthor_core::nav::*;
use std::path::PathBuf;

fn d(name: &str) -> Vec<u8> {
    std::fs::read(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/data").join(name)).unwrap()
}

#[test]
fn index() {
    let titres: Vec<u16> = (1..=21).collect();
    assert_eq!(make_index(0, 0, &titres, b"bdauthor"), d("nav_index.bin"));
    assert_eq!(make_index(0, 0, &titres, b"bdauthor"), d("pg_index.bdmv"));
}

#[test]
fn playlists() {
    assert_eq!(make_playlist(&PlaylistParams::default()), d("nav_defaut.bin"));
    assert_eq!(make_playlist(&PlaylistParams::default()), d("ref_00000.mpls"), "la playlist par defaut doit egaler celle de multiAVCHD");

    let film = PlaylistParams {
        clip: "00003".into(), audio: Some(AudioStn { pid: 0x1100, coding: 0x81, fmt_rate: 0x31, lang: *b"fra" }), with_menu: false,
        marks: Some(vec![27_000_000, 27_100_000]), ..Default::default()
    };
    assert_eq!(make_playlist(&film), d("nav_film.bin"));

    let menu = PlaylistParams {
        clip: "00012".into(), out_time: 27_200_000, audio: None, menu_clip: "80003".into(), still_mode: 2, ..Default::default()
    };
    assert_eq!(make_playlist(&menu), d("nav_menu.bin"));

    let fini = PlaylistParams {
        still_mode: 1, still_time: 7, audio: Some(AudioStn { pid: 0x1100, coding: 0x82, fmt_rate: 0x41, lang: *b"eng" }), ..Default::default()
    };
    assert_eq!(make_playlist(&fini), d("nav_still_fini.bin"));
}

