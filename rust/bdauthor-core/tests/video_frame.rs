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
    assert_eq!(frame_of(&back, 0), Some(VideoFrame { video: Some(1), at: 12.5 }));
    assert_eq!(frame_of(&back, 1), None);
    // video a choisir : pas de champ « video »
    let d2 = PageDesign { items: vec![Item::video_frame(0, 0, 100, 100, None, 0.0)], ..Default::default() };
    let back = PageDesign::from_value(&d2.to_value()).unwrap();
    assert_eq!(frame_of(&back, 0), Some(VideoFrame { video: None, at: 0.0 }));
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
