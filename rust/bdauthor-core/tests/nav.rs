//! Les fichiers de navigation construits par Rust doivent etre identiques octet pour octet a ceux construits par Python (`nav_*.bin`),
//! dont le disque de reference multiAVCHD (playlist par defaut == 00000.mpls) et le disque de test paginé.
use bdauthor_core::nav::*;
use std::collections::BTreeMap;
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

#[test]
fn plan_et_objets() {
    let cp: BTreeMap<usize, usize> = [(2, 3), (9, 1)].into();
    let plan = plan_navigation(14, 2, &cp);
    assert_eq!(plan.main_titles, vec![0, 15]);
    assert_eq!(plan.chapters[&2], ChapterTitles { pages: vec![16, 17, 18], play: 19 });
    assert_eq!(plan.chapters[&9], ChapterTitles { pages: vec![20], play: 21 });
    assert_eq!(plan.total_titles, 21);

    let cp2: BTreeMap<usize, usize> = [(0, 1), (2, 2)].into();
    let p2 = plan_navigation(3, 1, &cp2);
    assert_eq!((p2.main_titles.clone(), p2.chapters[&0].play, p2.chapters[&2].pages.clone(), p2.total_titles), (vec![0], 5, vec![6, 7], 8));

    let ch_pls: BTreeMap<usize, Vec<u16>> = [(2, vec![16, 17, 18]), (9, vec![19])].into();
    let video_page: Vec<usize> = (0..14).map(|i| if i < 7 { 0 } else { 1 }).collect();
    let objs = make_movie_objects_paged(14, &[14, 15], &plan.main_titles, &ch_pls, &plan.chapters, &video_page, true);
    assert_eq!(objs, d("pg_MovieObject.bdmv"));
    let objs = make_movie_objects_paged(14, &[14, 15], &plan.main_titles, &ch_pls, &plan.chapters, &video_page, false);
    assert_eq!(objs, d("nav_objets_principal.bin"));
}
