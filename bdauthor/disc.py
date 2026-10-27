"""Assemble un dossier BDMV complet a partir d'un .m2ts de film (H.264 + AC3).

Structure produite : 00000 = film, 80002 = clip de menu IG, 00000.mpls = film seul, 00001.mpls = menu
(le film sert de fond), navigation avec menu d'accueil (voir nav.make_movie_objects_accueil).

Usage : python -m bdauthor.disc film.(m2ts|mp4|mkv|...) dossier_sortie [--lang und] [--crf 18]
Une video qui n'est pas deja un .m2ts est d'abord encodee (ffmpeg, voir encode.py).
"""
import argparse, os
from .tsclip import scan_clip, make_clip_clpi
from .menu import MenuButton, build_menu_clip
from .nav import make_index, make_movie_objects_accueil, make_playlist


def write_bdmv(out_dir, files):
    """Ecrit `files` ({chemin relatif a BDMV: octets}) + copies BACKUP + dossiers vides du disque de reference."""
    root = os.path.join(out_dir, "BDMV")
    for rel, data in list(files.items()) + [("BACKUP/" + k, v) for k, v in files.items() if not k.startswith("STREAM")]:
        path = os.path.join(root, rel)
        os.makedirs(os.path.dirname(path), exist_ok=True)
        with open(path, "wb") as f:
            f.write(data)
    for d in ("AUXDATA", "BDJO", "JAR", "META"):
        os.makedirs(os.path.join(root, d), exist_ok=True)
    os.makedirs(os.path.join(out_dir, "CERTIFICATE"), exist_ok=True)


def build_disc(m2ts: bytes, out_dir: str, buttons=None, lang=b"und", ui_model=0, pad=b"\x00"):
    sc = scan_clip(m2ts)
    buttons = buttons or [MenuButton("Lire le film", 1, 720, 420), MenuButton("Menu principal", 0, 720, 560)]
    menu_m2ts, menu_clpi, *_ = build_menu_clip(buttons, ui_model=ui_model)
    start45, end45 = sc.start90 // 2, sc.end90 // 2
    pid_v, type_v = sc.video
    pid_a, type_a, code_a = sc.audio_codes[0]
    kw = dict(clip="00000", in_time=start45, out_time=end45,
              video=(pid_v, type_v, sc.video_code), audio=(pid_a, type_a, code_a, lang))
    files = {
        "index.bdmv": make_index(),
        "MovieObject.bdmv": make_movie_objects_accueil(),
        "PLAYLIST/00000.mpls": make_playlist(with_menu=False, **kw),
        "PLAYLIST/00001.mpls": make_playlist(with_menu=True, **kw),
        "CLIPINF/00000.clpi": make_clip_clpi(m2ts, lang=lang, pad=pad),
        "CLIPINF/80002.clpi": menu_clpi,
        "STREAM/00000.m2ts": m2ts,
        "STREAM/80002.m2ts": menu_m2ts,
    }
    write_bdmv(out_dir, files)
    return files, sc


if __name__ == "__main__":
    ap = argparse.ArgumentParser()
    ap.add_argument("source"); ap.add_argument("out_dir"); ap.add_argument("--lang", default="und")
    ap.add_argument("--crf", type=int, default=18, help="qualite x264 (plus petit = meilleure qualite)")
    a = ap.parse_args()
    lang = a.lang.encode("ascii")
    if a.source.lower().endswith(".m2ts"):
        m2ts = open(a.source, "rb").read()
    else:
        from .encode import make_film_m2ts
        m2ts, stats = make_film_m2ts(a.source, lang=lang, crf=a.crf)
        print(f"encodage : {stats['cible']}, {stats['images']} images, {stats['trames_audio']} trames audio, "
              f"avance {stats['avance_s']:.3f} s, PCR max {stats['ecart_pcr_max_ms']:.1f} ms")
    files, sc = build_disc(m2ts, a.out_dir, lang=lang)
    print(f"{sc.width}x{sc.height}{'p' if sc.progressive else 'i'} {sc.fps} i/s, {len(sc.eps)} points d'acces ; "
          f"{len(files)} fichiers ecrits dans {a.out_dir}/BDMV")
