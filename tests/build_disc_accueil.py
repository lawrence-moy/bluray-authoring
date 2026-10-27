"""Disque avec menu d'accueil : le disque demarre sur la playlist de menu (video de fond en boucle + boutons),
« Lire le film » lance le film (playlist sans menu) puis retour au menu.

Usage : python -m tests.build_disc_accueil
Produit out/bdmv_overlay_accueil.zip, a superposer a une COPIE du disque d'origine.
"""
import os, zipfile
from bdauthor.nav import make_index, make_movie_objects_accueil, make_playlist
from bdauthor.menu import MenuButton, build_menu_clip
from bdauthor.index import IndexBdmv
from bdauthor.mobj import MovieObjectBdmv
from bdauthor.mpls import Mpls
from bdauthor.clpi import Clpi

buttons = [MenuButton("Lire le film", 1, 720, 420), MenuButton("Menu principal", 0, 720, 560)]
m2ts, clpi, *_ = build_menu_clip(buttons, ui_model=0)
files = {
    "index.bdmv": make_index(),
    "MovieObject.bdmv": make_movie_objects_accueil(),
    "PLAYLIST/00000.mpls": make_playlist(with_menu=False),   # le film
    "PLAYLIST/00001.mpls": make_playlist(with_menu=True),    # le menu (fond = le film, en attendant un clip dedie)
    "CLIPINF/80002.clpi": clpi,
    "STREAM/80002.m2ts": m2ts,
}
os.makedirs("out", exist_ok=True)
with zipfile.ZipFile("out/bdmv_overlay_accueil.zip", "w", zipfile.ZIP_DEFLATED) as z:
    for rel, data in files.items():
        z.writestr(f"BDMV/{rel}", data)
        if not rel.startswith("STREAM"):
            z.writestr(f"BDMV/BACKUP/{rel}", data)

print("relecture / aller-retour :")
print("  index", IndexBdmv.parse(files["index.bdmv"]).pack() == files["index.bdmv"],
      "| mobj", MovieObjectBdmv.parse(files["MovieObject.bdmv"]).pack() == files["MovieObject.bdmv"],
      "| mpls", all(Mpls.parse(files[k]).pack() == files[k] for k in files if k.endswith(".mpls")),
      "| clpi", Clpi.parse(files["CLIPINF/80002.clpi"]).pack() == files["CLIPINF/80002.clpi"])
print("  00001.mpls == playlist de menu deja validee dans VLC :", files["PLAYLIST/00001.mpls"] == make_playlist())
print(IndexBdmv.parse(files["index.bdmv"]).dump().splitlines()[3:6])
print(MovieObjectBdmv.parse(files["MovieObject.bdmv"]).dump())
print("playlist 00000 :", Mpls.parse(files["PLAYLIST/00000.mpls"]).dump().splitlines()[1:])
print("playlist 00001 :", Mpls.parse(files["PLAYLIST/00001.mpls"]).dump().splitlines()[1:])
print("zip : out/bdmv_overlay_accueil.zip")
