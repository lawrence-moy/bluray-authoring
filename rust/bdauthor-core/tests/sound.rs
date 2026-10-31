//! Sons des menus : fichier sound.bdmv, style, identifiants des boutons.
use bdauthor_core::menu::{build_menu, MenuButton, MenuOptions};
use bdauthor_core::sound::{make_sound_bdmv, parse_sound_bdmv, Sound};
use bdauthor_core::style::ButtonStyle;

#[test]
fn sound_file_layout() {
    let a = Sound { channels: 2, samples: vec![1, 2, 3, 4, 5, 6, 7, 8] };
    let b = Sound { channels: 1, samples: vec![9, 10] };
    let d = make_sound_bdmv(&[a.clone(), b.clone()]);
    assert_eq!(&d[0..8], b"BCLK0200");
    // en-tete 40 octets + longueur (4) + reserve + nombre + 2 entrees de 10 octets
    let data_start = u32::from_be_bytes([d[8], d[9], d[10], d[11]]) as usize;
    assert_eq!(data_start, 40 + 4 + 2 + 20);
    assert_eq!(d.len(), data_start + 10);
    let idx = parse_sound_bdmv(&d).unwrap();
    assert_eq!(idx, vec![(2, 48_000, 16, data_start, 8), (1, 48_000, 16, data_start + 8, 2)]);
    assert_eq!(&d[idx[1].3..idx[1].3 + 2], &[9, 10]);
    assert!((a.seconds() - 8.0 / (4.0 * 48_000.0)).abs() < 1e-12);
}

#[test]
fn style_sounds_json_and_button_ids() {
    let st = ButtonStyle { sound_select: "sons/clic.wav".into(), sound_activate: "sons/ok.wav".into(), ..ButtonStyle::default() };
    let back = ButtonStyle::from_value(&st.to_value()).unwrap();
    assert_eq!((back.sound_select.as_str(), back.sound_activate.as_str()), ("sons/clic.wav", "sons/ok.wav"));
    // sans son : pas de champ dans le JSON
    assert!(ButtonStyle::default().to_value().get("son_selection").is_none() && ButtonStyle::default().to_value().get("son_activation").is_none());
    // identifiants ecrits dans les boutons du menu
    let im = std::sync::Arc::new(bdauthor_core::palette::RgbaImage::new(20, 10));
    let mut b = MenuButton::new("", 1, 0, 0, vec![im.clone(), im.clone(), im]);
    b.sound_select = 0;
    b.sound_activate = 1;
    let m = build_menu(&[b], &MenuOptions::default()).unwrap();
    let btn = &m.ics.pages[0].bogs[0].buttons[0];
    assert_eq!((btn.selected.0, btn.activated.0), (0, 1));
}
