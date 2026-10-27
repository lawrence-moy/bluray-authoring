//! Construction des fichiers de navigation : index.bdmv, MovieObject.bdmv, playlists — equivalent de `nav.py`.
use crate::index::IndexBdmv;
use crate::index::IndexObject;
use crate::mobj::*;
use crate::mpls::*;
use crate::reader::Put;
use std::collections::BTreeMap;

/// index.bdmv : First Playback et Top Menu pointent vers les objets donnes ; `title_objs[k]` = objet du titre k+1.
pub fn make_index(first_playback_obj: u16, top_menu_obj: u16, title_objs: &[u16], user_data: &[u8]) -> Vec<u8> {
    let mut ud = user_data.to_vec();
    ud.resize(32, 0);
    ud.truncate(32);
    IndexBdmv {
        version: "0100".into(), app_flags: 0, video_format: 0, frame_rate: 0, user_data: ud,
        first_playback: IndexObject::hdmv(0, 1, first_playback_obj),
        top_menu: IndexObject::hdmv(0, 1, top_menu_obj),
        titles: title_objs.iter().map(|&o| IndexObject::hdmv(0, 0, o)).collect(),
    }
    .pack()
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct VideoStn {
    pub pid: u16,
    pub coding: u8,
    pub fmt_rate: u8,
}
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AudioStn {
    pub pid: u16,
    pub coding: u8,
    pub fmt_rate: u8,
    pub lang: [u8; 3],
}

/// Parametres d'une playlist (valeurs par defaut = disque de reference multiAVCHD).
#[derive(Debug, Clone)]
pub struct PlaylistParams {
    pub clip: String,
    pub in_time: u32,
    pub out_time: u32,
    pub video: VideoStn,
    pub audio: Option<AudioStn>,
    pub menu_clip: String,
    pub menu_in: u32,
    pub menu_out: u32,
    pub ig_pid: u16,
    pub ig_lang: [u8; 3],
    pub with_menu: bool,
    pub still_mode: u8, // 0 aucun, 1 fini, 2 infini
    pub still_time: u16,
    pub marks: Option<Vec<u32>>,
}

impl Default for PlaylistParams {
    fn default() -> Self {
        PlaylistParams {
            clip: "00000".into(), in_time: 27_000_000, out_time: 27_435_435,
            video: VideoStn { pid: 0x1011, coding: 0x1B, fmt_rate: 0x57 },
            audio: Some(AudioStn { pid: 0x1100, coding: 0x81, fmt_rate: 0x31, lang: *b"und" }),
            menu_clip: "80002".into(), menu_in: 21478, menu_out: 22979, ig_pid: 0x1400, ig_lang: *b"eng",
            with_menu: true, still_mode: 0, still_time: 0, marks: None,
        }
    }
}

/// Table des flux (STN) du PlayItem.
fn stn(video: &VideoStn, audio: &Option<AudioStn>, ig: Option<(u16, [u8; 3])>) -> Vec<u8> {
    let mut v = vec![9, 1];
    v.p16(video.pid);
    v.zeros(6);
    v.extend_from_slice(&[5, video.coding, video.fmt_rate]);
    v.zeros(3);
    let mut a = Vec::new();
    if let Some(au) = audio {
        a.extend_from_slice(&[9, 1]);
        a.p16(au.pid);
        a.zeros(6);
        a.extend_from_slice(&[5, au.coding, au.fmt_rate]);
        a.extend_from_slice(&au.lang);
    }
    let mut g = Vec::new();
    if let Some((pid, lang)) = ig {
        g.extend_from_slice(&[9, 2, 0, 0]);
        g.p16(pid);
        g.zeros(4);
        g.extend_from_slice(&[5, 0x91]);
        g.extend_from_slice(&lang);
        g.p8(0);
    }
    let mut counts = vec![1, audio.is_some() as u8, 0, ig.is_some() as u8, 0, 0, 0];
    counts.zeros(5);
    // le champ de longueur de multiAVCHD ne couvre pas l'entree IG : reproduit a l'identique
    let length = 2 + counts.len() + v.len() + a.len();
    let mut out = Vec::new();
    out.p16(length as u16);
    out.p16(0);
    out.extend(counts);
    out.extend(v);
    out.extend(a);
    out.extend(g);
    out
}

/// Playlist : un PlayItem (le film ou le fond) + un SubPath de type 3 (menu IG) si `with_menu` + marques de chapitre.
pub fn make_playlist(p: &PlaylistParams) -> Vec<u8> {
    let mut tail = vec![0u8; 9];
    tail.p8(p.still_mode);
    tail.p16(p.still_time);
    tail.extend(stn(&p.video, &p.audio, if p.with_menu { Some((p.ig_pid, p.ig_lang)) } else { None }));
    let subpaths = if p.with_menu {
        vec![SubPath {
            subpath_type: 3, is_repeat: 0,
            items: vec![SubPlayItem { clip: p.menu_clip.clone(), codec: "M2TS".into(), connection_condition: 1, is_multi_clip: 0, stc_id: 0,
                                      in_time: p.menu_in, out_time: p.menu_out, sync_playitem_id: 0, sync_start_pts: 0 }],
        }]
    } else {
        vec![]
    };
    let times = p.marks.clone().unwrap_or_else(|| vec![p.in_time]);
    let mut app_info = vec![0u8, 1];
    app_info.zeros(12);
    Mpls {
        version: "0100".into(), app_info, playitems: vec![PlayItem::new(&p.clip, p.in_time, p.out_time, tail)], subpaths,
        marks: times.into_iter().map(Mark::chapter).collect(), extension: vec![],
    }
    .pack()
}

/// Titres d'une video a chapitres : pages de son menu de chapitres et titre « lecture depuis un repere ».
#[derive(Debug, Clone, PartialEq)]
pub struct ChapterTitles {
    pub pages: Vec<u16>,
    pub play: u16,
}

pub struct NavPlan {
    pub main_titles: Vec<u16>, // titre de chaque page du menu principal (la page 1 = Top Menu = 0)
    pub chapters: BTreeMap<usize, ChapterTitles>,
    pub total_titles: u16,
}

/// Numerotation des titres avec pagination : 1..N = videos ; puis pages 2.. du menu principal ; puis, par video a chapitres
/// (ordre croissant), ses pages de chapitres et un titre « lecture depuis un repere ». `chapter_pages` : {video: nombre de pages}.
pub fn plan_navigation(n_videos: usize, main_pages: usize, chapter_pages: &BTreeMap<usize, usize>) -> NavPlan {
    let mut t = n_videos as u16;
    let mut main_titles = vec![0u16];
    for _ in 1..main_pages {
        t += 1;
        main_titles.push(t);
    }
    let mut chapters = BTreeMap::new();
    for (&i, &np) in chapter_pages {
        let pages: Vec<u16> = (t + 1..=t + np as u16).collect();
        t += np as u16 + 1;
        chapters.insert(i, ChapterTitles { pages, play: t });
    }
    NavPlan { main_titles, chapters, total_titles: t }
}

/// MovieObject.bdmv avec menus pagines. L'objet de rang k est celui du titre k (objet 0 = Top Menu = page 1).
/// `retour` : « chapitres » = apres une video a chapitres on revient a son menu de chapitres ; sinon a la page du menu principal.
pub fn make_movie_objects_paged(
    n_videos: usize,
    main_pls: &[u16],
    main_titles: &[u16],
    ch_pls: &BTreeMap<usize, Vec<u16>>,
    ch_titles: &BTreeMap<usize, ChapterTitles>,
    video_page: &[usize],
    retour_chapitres: bool,
) -> Vec<u8> {
    let loop_obj = |pl: u16| {
        MovieObject::new(vec![
            Command::new(PLAY_PL_INSN, pl as u32, 0), Command::new(NOP_INSN, 0, 0), Command::new(NOP_INSN, 0, 0),
            Command::new(NOP_INSN, 0, 0), Command::new(GOTO_INSN, 0, 0),
        ])
    };
    let back = |i: usize| -> u32 {
        match ch_titles.get(&i) {
            Some(c) if retour_chapitres => c.pages[0] as u32,
            _ => main_titles[video_page[i]] as u32,
        }
    };
    let mut objs = vec![loop_obj(main_pls[0])];
    for i in 0..n_videos {
        objs.push(MovieObject::new(vec![Command::new(PLAY_PL_INSN, i as u32, 0), Command::new(JUMP_TITLE_INSN, back(i), 0)]));
    }
    for &pl in &main_pls[1..] {
        objs.push(loop_obj(pl));
    }
    for (&i, pls) in ch_pls {
        for &pl in pls {
            objs.push(loop_obj(pl));
        }
        objs.push(MovieObject::new(vec![
            Command::new(MOVE_RR_INSN, 10, 0), Command::new(MOVE_RI_INSN, 0, 0),
            Command::new(PLAY_PL_PM_INSN, i as u32, 10), Command::new(JUMP_TITLE_INSN, back(i), 0),
        ]));
    }
    MovieObjectBdmv { version: "0100".into(), objects: objs }.pack()
}
