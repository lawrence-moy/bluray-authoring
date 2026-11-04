//! Widget « Image de la video » : JSON, renumerotation des videos, verifications.
use bdauthor_core::design::*;

fn frame_of(d: &PageDesign, i: usize) -> Option<VideoFrame> {
    match &d.items[i].kind { Kind::Image { frame, .. } => *frame, _ => None }
}

#[test]
fn json_roundtrip_and_file_images_unchanged() {
    let mut d = PageDesign::default();
    d.items.push(Item::video_frame(100, 100, 480, 270, Some(1), 12.5));
    d.items.push(Item { x: 0, y: 0, w: 10, h: 10, kind: Kind::Image { path: "logo.png".into(), link: None, frame: None }, look: Default::default(), layer: 0, sub: Default::default(), id: 0, nav: Default::default(), name: String::new() });
    let v = d.to_value();
    let j = v.to_pretty();
    assert!(j.contains("\"video\": 2") && j.contains("\"instant\": 12.5") && j.contains("\"fichier\": \"logo.png\""), "{j}");
    let back = PageDesign::from_value(&v).unwrap();
    assert_eq!(frame_of(&back, 0), Some(VideoFrame { video: Some(1), at: 12.5, crop: None }));
    assert_eq!(frame_of(&back, 1), None);
    // video a choisir : pas de champ « video »
    let d2 = PageDesign { items: vec![Item::video_frame(0, 0, 100, 100, None, 0.0)], ..Default::default() };
    let back = PageDesign::from_value(&d2.to_value()).unwrap();
    assert_eq!(frame_of(&back, 0), Some(VideoFrame { video: None, at: 0.0, crop: None }));
}

#[test]
fn renumbering_and_validation() {
    let mut ds = Designs::default();
    ds.free.push(PageDesign { items: vec![Item::video_frame(0, 0, 400, 300, Some(2), 3.0), Item::video_frame(500, 0, 400, 300, Some(0), 1.0)], ..Default::default() });
    ds.swap_videos(0, 2);
    assert_eq!(frame_of(&ds.free[0], 0).unwrap().video, Some(0));
    assert_eq!(frame_of(&ds.free[0], 1).unwrap().video, Some(2));
    ds.remove_video(0);
    assert_eq!(frame_of(&ds.free[0], 0).unwrap().video, None);
    assert_eq!(frame_of(&ds.free[0], 1).unwrap().video, Some(1));
    let probs = ds.free[0].validate(0, 1, 1);
    assert!(probs.iter().any(|p| !p.error), "video a choisir : avertissement {probs:?}");
    assert!(probs.iter().any(|p| p.error), "video 2 inexistante : erreur {probs:?}");
}

#[test]
fn crop_json_and_pixels() {
    let mut d = PageDesign { items: vec![Item::video_frame(0, 0, 400, 400, Some(0), 2.0)], ..Default::default() };
    if let Kind::Image { frame: Some(f), .. } = &mut d.items[0].kind { f.crop = Some([0.25, 0.1, 0.5, 0.8]); }
    let j = d.to_value().to_pretty();
    assert!(j.contains("recadrage"), "{j}");
    let back = PageDesign::from_value(&d.to_value()).unwrap();
    let f = frame_of(&back, 0).unwrap();
    assert_eq!(f.crop, Some([0.25, 0.1, 0.5, 0.8]));
    assert_eq!(f.crop_px(1920, 1080), (480, 108, 960, 864));
    // sans recadrage : zone 16/9 centree (comme ffmpeg) ; image 4/3 -> bandes haut et bas retirees
    let n = VideoFrame { video: Some(0), at: 0.0, crop: None };
    assert_eq!(n.crop_px(1920, 1080), (0, 0, 1920, 1080));
    assert_eq!(n.crop_px(1440, 1080), (0, 135, 1440, 810));
    assert_eq!(n.crop_px(2560, 1080), (320, 0, 1920, 1080));
    // recadrage hors de l'image : borne
    let b = VideoFrame { video: Some(0), at: 0.0, crop: Some([0.9, 0.9, 0.5, 0.5]) };
    let (x, y, w, h) = b.crop_px(100, 100);
    assert!(x + w <= 100 && y + h <= 100 && w >= 1 && h >= 1);
}

#[test]
fn crop_handles() {
    use bdauthor_core::editor::{crop_drag, crop_full};
    let near = |a: [f64; 4], b: [f64; 4]| a.iter().zip(b.iter()).all(|(x, y)| (x - y).abs() < 1e-9);
    let o = [0.2, 0.2, 0.4, 0.4];
    // deplacement borne a l'image
    assert!(near(crop_drag(0, o, (0.5, 0.5), (0.9, 0.5), None), [0.6, 0.2, 0.4, 0.4]));
    assert!(near(crop_drag(0, o, (0.5, 0.5), (1.0, 0.5), None), [0.6, 0.2, 0.4, 0.4]));
    // angle bas droit libre ; avec rapport 1 (fractions) : carre
    assert!(near(crop_drag(4, o, (0.6, 0.6), (0.8, 0.7), None), [0.2, 0.2, 0.6, 0.5]));
    assert!(near(crop_drag(4, o, (0.6, 0.6), (0.8, 0.7), Some(1.0)), [0.2, 0.2, 0.6, 0.6]));
    // angle haut gauche au-dela de l'angle oppose : le cadre se retourne autour de l'ancre
    let r = crop_drag(1, o, (0.2, 0.2), (0.9, 0.9), None);
    assert!(near(r, [0.6, 0.6, 0.3, 0.3]), "{r:?}");
    // cote droit : seule la largeur change ; avec rapport, la hauteur suit autour du centre
    assert!(near(crop_drag(6, o, (0.6, 0.4), (0.7, 0.9), None), [0.2, 0.2, 0.5, 0.4]));
    let r = crop_drag(6, o, (0.6, 0.4), (0.8, 0.4), Some(2.0));
    assert!(near(r, [0.2, 0.25, 0.6, 0.3]), "{r:?}");
    // nouveau cadre trace, rapport limite par le bord
    let r = crop_drag(9, o, (0.5, 0.5), (1.0, 0.6), Some(4.0));
    assert!(near(r, [0.5, 0.5, 0.5, 0.125]), "{r:?}");
    assert!(near(crop_full(None), [0.0, 0.0, 1.0, 1.0]));
    assert!(near(crop_full(Some(2.0)), [0.0, 0.25, 1.0, 0.5]));
    assert!(near(crop_full(Some(0.5)), [0.25, 0.0, 0.5, 1.0]));
}
