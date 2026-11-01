use bdauthor_core::ig::*;
use bdauthor_core::m2ts::{reassemble, split_segments};
use bdauthor_core::menu::*;
use bdauthor_core::palette::RgbaImage;
use std::sync::Arc;

fn hex(b: &[u8]) -> String { b.iter().map(|x| format!("{x:02x}")).collect() }

fn solid(w: u32, h: u32, c: [u8; 4]) -> Arc<RgbaImage> {
    let mut im = RgbaImage::new(w, h);
    for y in 0..h { for x in 0..w { im.set(x, y, c); } }
    Arc::new(im)
}

#[test]
fn neighbors_match_python() {
    let r = [(100, 100, 300, 100), (600, 100, 300, 100), (100, 400, 300, 100), (600, 420, 300, 100), (350, 700, 300, 100)];
    assert_eq!(auto_neighbors(&r), vec![(0, 2, 0, 1), (1, 3, 0, 1), (0, 4, 2, 3), (1, 4, 2, 3), (3, 4, 2, 3)]);
}

#[test]
fn required_pts_match_python() {
    assert_eq!(required_ics_pts(&[480 * 96 * 3, 100000], (1920, 1080), 42956, 9000), 42956);
    assert_eq!(required_ics_pts(&[10], (1920, 1080), 0, 1), 23330);
}

#[test]
fn clpi_matches_python() {
    let want = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/data/menu_clpi.hex")).unwrap();
    assert_eq!(hex(&make_clpi(123, 42956, 5_388_536, *b"eng", 1501)), want.trim());
}

#[test]
fn menu_clip_roundtrip() {
    // 3 boutons : un opaque a 3 etats, un semi-transparent, un partageant ses images avec le premier
    let n = solid(300, 80, [30, 60, 140, 255]);
    let s = solid(300, 80, [90, 150, 230, 255]);
    let a = solid(300, 80, [240, 150, 40, 255]);
    let mut ghost = RgbaImage::new(200, 60);
    for y in 0..60 { for x in 0..200 { ghost.set(x, y, [255, 255, 255, if x < 100 { 128 } else { 0 }]); } }
    let ghost = Arc::new(ghost);
    let buttons = vec![
        MenuButton::new("un", 1, 100, 100, vec![n.clone(), s.clone(), a.clone()]),
        MenuButton::new("deux", 2, 100, 300, vec![ghost.clone()]),
        MenuButton::new("trois", 3, 600, 100, vec![n.clone(), s.clone(), a.clone()]),
    ];
    let clip = build_menu_clip(&buttons, &MenuOptions::default()).unwrap();
    assert_eq!(clip.m2ts.len() % 192, 0);
    let segs: Vec<_> = reassemble(&clip.m2ts, 0x1400).unwrap().iter().flat_map(|p| split_segments(&p.data).unwrap()).collect();
    let ics = Ics::parse(&segs.iter().find(|s| s.seg_type == SEG_ICS).unwrap().payload).unwrap();
    let pds = Pds::parse(&segs.iter().find(|s| s.seg_type == SEG_PDS).unwrap().payload).unwrap();
    let ods: Vec<Ods> = segs.iter().filter(|s| s.seg_type == SEG_ODS).map(|s| Ods::parse(&s.payload).unwrap()).collect();
    assert_eq!(ics, clip.menu.ics);
    assert_eq!(ods.len(), 4, "3 images partagees + 1 fantome");
    assert_eq!(ics.pages[0].bogs.len(), 3);
    // boutons 0 et 2 partagent les memes objets
    let b = |i: usize| ics.pages[0].bogs[i].buttons[0].clone();
    assert_eq!(b(0).normal, b(2).normal);
    assert_eq!(b(1).normal, b(1).selected_as_normal());
    // les pixels decodes retrouvent les couleurs (a la quantification pres)
    let px = ods[0].pixels().unwrap();
    let c = pds.rgba(px[0][0]);
    assert!((c[0] as i32 - 30).abs() < 6 && (c[1] as i32 - 60).abs() < 6 && (c[2] as i32 - 140).abs() < 6 && c[3] == 255, "{c:?}");
    let g = ods[3].pixels().unwrap();
    assert_eq!(pds.rgba(g[0][150])[3], 0);
    assert_eq!(pds.rgba(g[0][10])[3], 128);
}

trait Sel { fn selected_as_normal(&self) -> (u16, u16, u8); }
impl Sel for Button { fn selected_as_normal(&self) -> (u16, u16, u8) { self.normal } }

#[test]
fn out_of_screen_rejected() {
    let buttons = vec![MenuButton::new("x", 1, 1800, 100, vec![solid(300, 80, [1, 2, 3, 255])])];
    assert!(build_menu(&buttons, &MenuOptions::default()).is_err());
}

#[test]
fn manual_neighbors_reach_the_disc() {
    let n = solid(300, 80, [30, 60, 140, 255]);
    let s = solid(300, 80, [90, 150, 230, 255]);
    let a = solid(300, 80, [240, 150, 40, 255]);
    let mk = |t, x, y| MenuButton::new("b", t, x, y, vec![n.clone(), s.clone(), a.clone()]);
    let mut buttons = vec![mk(1, 100, 100), mk(2, 500, 100), mk(3, 900, 100)];
    buttons[0].nav_set[3] = Some(2); // droite : directement le 3e
    buttons[1].nav_set[2] = Some(1); // gauche : reste sur place
    let clip = build_menu_clip(&buttons, &MenuOptions::default()).unwrap();
    let b = |i: usize| clip.menu.ics.pages[0].bogs[i].buttons[0].clone();
    assert_eq!(b(0).right, 2);
    assert_eq!(b(1).left, 1);
    assert_eq!(b(2).left, 1, "non modifie : automatique");
}
