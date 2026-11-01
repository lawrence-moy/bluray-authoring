use bdauthor_core::design::{Item, Kind};
use bdauthor_core::editor::{align_items, ItemAlign};

fn it(x: i32, y: i32, w: i32, h: i32) -> Item {
    Item { x, y, w, h, kind: Kind::Slot { thumb_h: h - 40, video: None, link: None, start: None, chapter: None, label: String::new(), thumb_at: None, ret: Default::default() }, look: Default::default(), layer: 0, sub: Default::default(), id: 0, nav: Default::default() }
}

#[test]
fn single_item_centres_on_page() {
    let mut v = vec![it(10, 10, 200, 100)];
    assert!(align_items(&mut v, &[0], ItemAlign::CenterH));
    assert_eq!(v[0].x, 860);
    assert!(align_items(&mut v, &[0], ItemAlign::CenterV));
    assert_eq!(v[0].y, 490);
    assert!(!align_items(&mut v, &[0], ItemAlign::CenterV));
}

#[test]
fn group_aligns_to_bounding_box() {
    let mut v = vec![it(100, 50, 200, 100), it(500, 300, 100, 60), it(0, 0, 10, 10)];
    align_items(&mut v, &[0, 1], ItemAlign::Left);
    assert_eq!((v[0].x, v[1].x), (100, 100));
    align_items(&mut v, &[0, 1], ItemAlign::Bottom);
    assert_eq!(v[0].y + v[0].h, v[1].y + v[1].h);
    assert_eq!((v[2].x, v[2].y), (0, 0)); // non selectionne : intact
    align_items(&mut v, &[0, 1], ItemAlign::Right);
    assert_eq!(v[0].x + v[0].w, v[1].x + v[1].w);
}

#[test]
fn distribute_and_page_centre() {
    let mut v = vec![it(0, 0, 100, 100), it(150, 0, 100, 100), it(700, 0, 100, 100)];
    align_items(&mut v, &[0, 1, 2], ItemAlign::DistributeH);
    assert_eq!((v[0].x, v[1].x, v[2].x), (0, 350, 700));
    align_items(&mut v, &[0, 1, 2], ItemAlign::PageH);
    assert_eq!(v[0].x, (1920 - 800) / 2);
    assert_eq!(v[2].x - v[0].x, 700);
    align_items(&mut v, &[0, 1, 2], ItemAlign::PageV);
    assert_eq!(v[1].y, 490);
}

#[test]
fn stays_inside_page_and_ignores_bad_indexes() {
    let mut v = vec![it(1800, 0, 400, 100)];
    align_items(&mut v, &[0, 9], ItemAlign::Right);
    assert!(v[0].x >= 0);
    assert!(!align_items(&mut v, &[], ItemAlign::Left));
}
