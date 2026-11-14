use bdauthor_core::coherence::{check, Dest, Place, Rule, Severity};
use bdauthor_core::design::*;
use bdauthor_core::model::ProjectModel;

fn model(n: usize) -> ProjectModel {
    let vids: Vec<String> = (0..n).map(|i| format!(r#"{{"file": "v{i}.mp4", "title": "Video {i}"}}"#)).collect();
    ProjectModel::from_json(&format!(r#"{{ "title": "T", "language": "fra", "videos": [{}] }}"#, vids.join(","))).unwrap()
}

fn goto(t: Target, x: i32, y: i32, name: &str) -> Item {
    let mut it = Item::nav(NavAction::Goto, x, y, 300, 80);
    if let Kind::Nav { target, .. } = &mut it.kind { *target = Some(t); }
    it.name = name.into();
    it
}

fn page(name: &str, items: Vec<Item>) -> PageDesign { PageDesign { items, name: name.into(), ..Default::default() } }

fn has(r: &bdauthor_core::coherence::Report, rule: Rule, sev: Severity, place: Place) -> bool {
    r.findings.iter().any(|f| f.rule == rule && f.severity == sev && f.place == place)
}

#[test]
fn unreachable_pages_orphan_videos_returns_and_duplicates() {
    let mut m = model(3);
    let mut back = Item::video_slot(100, 600, 400, 260, 225, 0, None, "");
    if let Kind::Slot { ret, .. } = &mut back.kind { *ret = SlotReturn::Page(9); }
    m.designs.free = vec![
        page("Home", vec![Item::video_slot(100, 200, 400, 260, 225, 0, None, ""), back, goto(Target::Main(1), 1200, 900, "Languages")]),
        page("Languages", vec![goto(Target::Main(0), 800, 900, "Back")]),
        page("Bonus", vec![Item::video_slot(100, 200, 400, 260, 225, 2, None, "")]),
    ];
    let r = check(&m, &|_| None);
    assert!(!r.generated);
    assert!(has(&r, Rule::Reach, Severity::Warning, Place::Page(2)), "{:#?}", r.findings);
    assert!(!has(&r, Rule::Reach, Severity::Warning, Place::Page(1)));
    assert!(has(&r, Rule::Orphan, Severity::Warning, Place::Video(1)));
    let v2 = r.findings.iter().find(|f| f.place == Place::Video(2)).unwrap();
    assert!(v2.msg.contains("cannot be reached") && v2.msg.contains("Bonus"), "{}", v2.msg);
    assert!(has(&r, Rule::Return, Severity::Warning, Place::Item(0, 1)));
    assert!(has(&r, Rule::Duplicates, Severity::Info, Place::Item(0, 0)));
    // carte : accueil -> video 1 et page 2 ; la page Bonus est marquee inaccessible
    assert_eq!(r.home, 0);
    assert!(r.pages[0].exits.iter().any(|e| e.dest == Dest::Video(0)) && r.pages[0].exits.iter().any(|e| e.dest == Dest::Page(1)));
    assert!(!r.pages[2].reachable && r.pages[1].reachable);
    assert_eq!(r.videos[0].from, vec![0]);
    assert_eq!(r.videos[0].returns, vec![0]);
    // tri : du plus grave au moins grave
    assert!(r.findings.windows(2).all(|w| w[0].severity <= w[1].severity));
}

#[test]
fn automatic_launch_loops() {
    let mut m = model(1);
    let mut a = page("A", vec![goto(Target::Main(1), 100, 900, "to B"), goto(Target::Video(0), 600, 900, "play")]);
    let mut b = page("B", vec![goto(Target::Main(0), 100, 900, "to A")]);
    let ida = a.ensure_id(0);
    let idb = b.ensure_id(0);
    a.idle = Some(Idle { seconds: 30, item: ida });
    b.idle = Some(Idle { seconds: 20, item: idb });
    let mut c = page("C", vec![Item::video_slot(100, 200, 400, 260, 225, 0, None, ""), goto(Target::Main(0), 800, 900, "home")]);
    let idc = c.ensure_id(0);
    c.idle = Some(Idle { seconds: 10, item: idc });
    m.designs.free = vec![a, b, c];
    let r = check(&m, &|_| None);
    let loops: Vec<_> = r.findings.iter().filter(|f| f.rule == Rule::AutoLoop).collect();
    assert_eq!(loops.len(), 2, "{loops:#?}");
    let w = loops.iter().find(|f| f.severity == Severity::Warning).unwrap();
    assert!(w.msg.contains("A (auto 30 s) → B (auto 20 s) → A"), "{}", w.msg);
    let i = loops.iter().find(|f| f.severity == Severity::Info).unwrap();
    assert!(i.msg.contains("C (auto 10 s, plays a video) → C"), "{}", i.msg);
    assert_eq!(r.pages[0].exits.iter().find(|e| e.dest == Dest::Page(1)).unwrap().auto_s, Some(30));
}

#[test]
fn arrow_navigation_reachability_and_one_way_links() {
    let mut m = model(1);
    let mut d = page("P", vec![goto(Target::Video(0), 100, 500, "A"), goto(Target::Main(0), 600, 500, "B"), goto(Target::Main(0), 1100, 500, "C")]);
    let idb = d.ensure_id(1);
    for k in 0..4 { d.items[0].nav.set(k, NavLink::Stay); d.items[1].nav.set(k, NavLink::Stay); }
    d.items[0].nav.set(3, NavLink::Item(idb));
    m.designs.free = vec![d];
    let r = check(&m, &|_| None);
    assert!(has(&r, Rule::Arrows, Severity::Warning, Place::Item(0, 2)), "{:#?}", r.findings);
    assert!(!has(&r, Rule::Arrows, Severity::Warning, Place::Item(0, 1)));
    let one = r.findings.iter().find(|f| f.rule == Rule::Arrows && f.severity == Severity::Info).unwrap();
    assert!(one.msg.contains("« A » ▸ right → « B »") && one.msg.contains("does not move"), "{}", one.msg);
}

#[test]
fn chapters_and_languages() {
    let mut m = model(2);
    m.languages = vec![
        bdauthor_core::subtitle::Language { id: 1, name: "Français".into(), code: "fra".into() },
        bdauthor_core::subtitle::Language { id: 2, name: "Español".into(), code: "spa".into() },
    ];
    let mut t = bdauthor_core::audio::AudioTrackCfg::embedded(0, "spa");
    t.lang_id = 2;
    m.videos[0].audio.push(t);
    let mut ch = Item::video_slot(100, 200, 400, 260, 225, 1, Some(500.0), "");
    if let Kind::Slot { chapter, .. } = &mut ch.kind { *chapter = Some(42); }
    m.designs.free = vec![page("P", vec![
        goto(Target::Audio(2, None), 100, 900, "Spanish"),
        goto(Target::Audio(1, None), 600, 900, "French"),
        ch,
        Item::video_slot(600, 200, 400, 260, 225, 0, None, ""),
    ])];
    let r = check(&m, &|v| Some(if v == 1 { 300.0 } else { 1000.0 }));
    let es = r.findings.iter().find(|f| f.place == Place::Item(0, 0) && f.rule == Rule::Languages).expect("espagnol");
    assert!(es.msg.contains("video 2 without an audio track"), "{}", es.msg);
    // video 1 a une piste espagnole seulement ; la video 2 (sans pistes choisies) est dans la langue du disque
    let fr = r.findings.iter().find(|f| f.place == Place::Item(0, 1) && f.rule == Rule::Languages).expect("francais");
    assert!(fr.msg.contains("video 1 without"), "{}", fr.msg);
    assert!(has(&r, Rule::Chapters, Severity::Error, Place::Item(0, 2)), "{:#?}", r.findings);
    assert!(has(&r, Rule::Chapters, Severity::Info, Place::Item(0, 2)));
    assert_eq!(r.count(Severity::Error), 1);
}

#[test]
fn no_page_means_generated_menu() {
    let r = check(&model(2), &|_| None);
    assert!(r.generated && r.findings.len() == 1 && r.videos.len() == 2);
}
