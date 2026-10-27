"""Fichiers de navigation d'un disque minimal : index.bdmv, MovieObject.bdmv, playlist .mpls.

Valeurs reprises du disque de reference (multiAVCHD) : drapeaux d'objets, mots d'instruction,
structure de la table des flux. Seule la playlist peut etre verifiee a l'identique (voir tests/build_disc.py).
"""
import struct
from .index import IndexBdmv, IndexObject, OBJ_HDMV
from .mobj import MovieObjectBdmv, MovieObject, Command
from .mpls import Mpls, PlayItem, SubPlayItem, SubPath, Mark

PLAY_PL_INSN = 0x22800000      # releves dans MovieObject.bdmv de reference
JUMP_TITLE_INSN = 0x21810000
JUMP_OBJECT_INSN = 0x21800000
GOTO_INSN = 0x20810000          # operande = indice de commande dans l'objet
NOP_INSN = 0x00000000
MOVE_RR_INSN = 0x50000001       # MOVE rX, rY      (releve dans l'objet 1 de la reference)
MOVE_RI_INSN = 0x50400001       # MOVE rX, valeur  (operande 2 immediat)
PLAY_PL_PM_INSN = 0x42820000    # PLAY_PL_PM playlist(immediat), repere(registre)


def make_index(first_playback_obj=0, top_menu_obj=0, title_objs=(1,), user_data=b"bdauthor"):
    return IndexBdmv(
        version=b"0100", app_flags=0, video_format=0, frame_rate=0, user_data=user_data.ljust(32, b"\0"),
        first_playback=IndexObject(OBJ_HDMV, 0, 1, first_playback_obj),
        top_menu=IndexObject(OBJ_HDMV, 0, 1, top_menu_obj),
        titles=[IndexObject(OBJ_HDMV, 0, 0, o) for o in title_objs]).pack()


def make_movie_objects(play_playlist=0):
    """Objet 0 (First Playback et Top Menu) et objet 1 (titre 1) : lancent la meme playlist."""
    def obj():
        return MovieObject(1, 0, 0, [Command(PLAY_PL_INSN, play_playlist, 0)])
    return MovieObjectBdmv(version=b"0100", objects=[obj(), obj()]).pack()


def _stn(video, audio, ig=None):
    """Table des flux. video=(pid, coding, fmt_rate) audio=(pid, coding, fmt_rate, lang) ig=(pid, lang)."""
    v = bytes([9, 1]) + struct.pack(">H", video[0]) + bytes(6) + bytes([5, video[1], video[2]]) + bytes(3)
    a = (bytes([9, 1]) + struct.pack(">H", audio[0]) + bytes(6) + bytes([5, audio[1], audio[2]]) + audio[3]) if audio else b""
    g = (bytes([9, 2, 0, 0]) + struct.pack(">H", ig[0]) + bytes(4) + bytes([5, 0x91]) + ig[1] + bytes(1)) if ig else b""
    counts = bytes([1, 1 if audio else 0, 0, 1 if ig else 0, 0, 0, 0]) + bytes(5)
    # le champ de longueur de multiAVCHD ne couvre pas l'entree IG (voir NOTES.md) : reproduit a l'identique
    length = 2 + len(counts) + len(v) + len(a)
    return struct.pack(">HH", length, 0) + counts + v + a + g


def make_playlist(clip="00000", in_time=27_000_000, out_time=27_435_435,
                  video=(0x1011, 0x1B, 0x57), audio=(0x1100, 0x81, 0x31, b"und"),
                  menu_clip="80002", menu_in=21478, menu_out=22979, ig_pid=0x1400, ig_lang=b"eng",
                  with_menu=True, still_mode=0, still_time=0, marks=None):
    """Playlist : un PlayItem (le film) + un SubPath de type 3 (menu IG) + un chapitre."""
    # queue du PlayItem : masque UO (8 o), drapeau d'acces aleatoire (1 o), still_mode (1 o : 0 aucun, 1 fini, 2 infini), still_time (2 o)
    tail = bytes(9) + bytes([still_mode]) + still_time.to_bytes(2, "big") + _stn(video, audio, (ig_pid, ig_lang) if with_menu else None)
    return Mpls(version=b"0100", app_info=bytes([0, 1]) + bytes(12),
                playitems=[PlayItem(clip, "M2TS", 0, 1, 0, in_time, out_time, tail)],
                subpaths=([SubPath(3, 0, [SubPlayItem(menu_clip, "M2TS", 1, 0, 0, menu_in, menu_out, 0, 0)])]
                          if with_menu else []),
                marks=[Mark(1, 0, t, 0xFFFF, 0) for t in (marks or [in_time])]).pack()


def make_movie_objects_accueil(menu_playlist=1, film_playlist=0):
    """Navigation avec menu d'accueil.

    Objet 0 (First Playback et Top Menu) : joue en boucle la playlist de menu (motif de l'objet 259 de multiAVCHD :
    PLAY_PL, NOP x3, GOTO au debut). Objet 1 (titre 1) : joue le film puis retourne au Top Menu.
    """
    menu = MovieObject(1, 0, 0, [Command(PLAY_PL_INSN, menu_playlist, 0), Command(NOP_INSN, 0, 0),
                                 Command(NOP_INSN, 0, 0), Command(NOP_INSN, 0, 0), Command(GOTO_INSN, 0, 0)])
    film = MovieObject(1, 0, 0, [Command(PLAY_PL_INSN, film_playlist, 0), Command(JUMP_TITLE_INSN, 0, 0)])
    return MovieObjectBdmv(version=b"0100", objects=[menu, film]).pack()


def make_movie_objects_multi(n_titles, menu_playlist):
    """Navigation a N titres. Objet 0 (First Playback et Top Menu) : playlist de menu en boucle ;
    objet i (titre i, 1 <= i <= N) : playlist i-1 puis retour au Top Menu."""
    menu = MovieObject(1, 0, 0, [Command(PLAY_PL_INSN, menu_playlist, 0), Command(NOP_INSN, 0, 0),
                                 Command(NOP_INSN, 0, 0), Command(NOP_INSN, 0, 0), Command(GOTO_INSN, 0, 0)])
    titles = [MovieObject(1, 0, 0, [Command(PLAY_PL_INSN, i, 0), Command(JUMP_TITLE_INSN, 0, 0)])
              for i in range(n_titles)]
    return MovieObjectBdmv(version=b"0100", objects=[menu] + titles).pack()


def plan_titles(n_videos, chapter_videos):
    """Numerotation des titres : 1..N = videos ; puis, pour chaque video a chapitres (dans l'ordre),
    un titre « menu de chapitres » et un titre « lecture depuis un repere ». Retourne ({i: (titre_menu, titre_lecture)}, total)."""
    plan, t = {}, n_videos
    for i in chapter_videos:
        plan[i] = (t + 1, t + 2)
        t += 2
    return plan, t


def make_movie_objects_chapters(n_videos, main_menu_pl, chapter_menu_pl, retour="chapitres"):
    """Navigation avec menus de chapitres. chapter_menu_pl : {indice de video: numero de playlist du menu de chapitres}.
    Objet 0 = menu principal (boucle) ; objet i = video i-1 puis retour au menu principal ;
    objets de chapitres : menu (playlist du menu en boucle) et lecture depuis le repere memorise dans r0
    (motif de l'objet 1 de la reference : MOVE r10, r0 ; MOVE r0, 0 ; PLAY_PL_PM playlist, r10).
    retour : « chapitres » (defaut) = a la fin d'une video a chapitres (lue en entier ou depuis un chapitre), on revient a son menu
    de chapitres ; « principal » = on revient au menu principal. Les videos sans chapitres reviennent toujours au menu principal."""
    plan, total = plan_titles(n_videos, sorted(chapter_menu_pl))
    loop = lambda pl: MovieObject(1, 0, 0, [Command(PLAY_PL_INSN, pl, 0), Command(NOP_INSN, 0, 0), Command(NOP_INSN, 0, 0),
                                            Command(NOP_INSN, 0, 0), Command(GOTO_INSN, 0, 0)])
    objs = [loop(main_menu_pl)]
    back = lambda i: plan[i][0] if (retour == "chapitres" and i in plan) else 0     # titre de retour apres la video i
    objs += [MovieObject(1, 0, 0, [Command(PLAY_PL_INSN, i, 0), Command(JUMP_TITLE_INSN, back(i), 0)]) for i in range(n_videos)]
    for i in sorted(chapter_menu_pl):
        objs.append(loop(chapter_menu_pl[i]))
        objs.append(MovieObject(1, 0, 0, [Command(MOVE_RR_INSN, 10, 0), Command(MOVE_RI_INSN, 0, 0),
                                          Command(PLAY_PL_PM_INSN, i, 10), Command(JUMP_TITLE_INSN, back(i), 0)]))
    return MovieObjectBdmv(version=b"0100", objects=objs).pack(), plan, total


def plan_navigation(n_videos, main_pages, chapter_pages):
    """Numerotation des titres avec pagination. Titres 1..N = videos ; puis les pages 2.. du menu principal ; puis, pour chaque video a chapitres
    (par ordre croissant), ses pages de menu de chapitres et un titre « lecture depuis un repere ».
    chapter_pages : {indice de video: nombre de pages}. La page 1 du menu principal est le Top Menu (titre 0).
    Retourne (titres des pages du menu principal, {i: {"pages": [titres], "play": titre}}, nombre total de titres)."""
    t = n_videos
    main_titles = [0]
    for _ in range(1, main_pages):
        t += 1
        main_titles.append(t)
    ch = {}
    for i in sorted(chapter_pages):
        pages = list(range(t + 1, t + chapter_pages[i] + 1))
        t += chapter_pages[i]
        t += 1
        ch[i] = {"pages": pages, "play": t}
    return main_titles, ch, t


def make_movie_objects_paged(n_videos, main_pls, main_titles, ch_pls, ch_titles, video_page, retour="chapitres"):
    """Navigation avec menus pagines. main_pls : playlists des pages du menu principal ; ch_pls : {i: [playlists des pages de chapitres]} ;
    video_page[i] : page du menu principal qui contient la video i (on y revient apres la video ou depuis son menu de chapitres).
    L'objet de rang k est celui du titre k (objet 0 = Top Menu = page 1 du menu principal)."""
    loop = lambda pl: MovieObject(1, 0, 0, [Command(PLAY_PL_INSN, pl, 0), Command(NOP_INSN, 0, 0), Command(NOP_INSN, 0, 0),
                                            Command(NOP_INSN, 0, 0), Command(GOTO_INSN, 0, 0)])
    back = lambda i: ch_titles[i]["pages"][0] if (retour == "chapitres" and i in ch_titles) else main_titles[video_page[i]]
    objs = [loop(main_pls[0])]
    objs += [MovieObject(1, 0, 0, [Command(PLAY_PL_INSN, i, 0), Command(JUMP_TITLE_INSN, back(i), 0)]) for i in range(n_videos)]
    objs += [loop(pl) for pl in main_pls[1:]]
    for i in sorted(ch_pls):
        objs += [loop(pl) for pl in ch_pls[i]]
        objs.append(MovieObject(1, 0, 0, [Command(MOVE_RR_INSN, 10, 0), Command(MOVE_RI_INSN, 0, 0),
                                          Command(PLAY_PL_PM_INSN, i, 10), Command(JUMP_TITLE_INSN, back(i), 0)]))
    return MovieObjectBdmv(version=b"0100", objects=objs).pack()
