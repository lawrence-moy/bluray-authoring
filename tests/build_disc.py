"""Usage : python -m tests.build_disc [00000.mpls_de_reference]

Genere dans out/bdmv_overlay/ les fichiers a superposer sur le disque de reference
(meme film 00000.m2ts/00000.clpi), plus out/bdmv_overlay.zip.
"""
import os, shutil, sys, zipfile
from bdauthor.nav import make_index, make_movie_objects, make_playlist
from bdauthor.menu import MenuButton, build_menu_clip
from bdauthor.index import IndexBdmv
from bdauthor.mobj import MovieObjectBdmv
from bdauthor.mpls import Mpls
from bdauthor.clpi import Clpi

if len(sys.argv) > 1:   # validation : playlist generee avec les parametres du disque de reference == fichier d'origine
    ref = open(sys.argv[1], "rb").read()
    print("mpls genere = mpls de reference :", make_playlist() == ref)

m2ts, clpi, *_ = build_menu_clip([MenuButton("Lire le film", 1, 720, 420), MenuButton("Menu principal", 0, 720, 560)])
files = {
    "index.bdmv": make_index(),
    "MovieObject.bdmv": make_movie_objects(),
    "PLAYLIST/00000.mpls": make_playlist(),
    "CLIPINF/80002.clpi": clpi,
    "STREAM/80002.m2ts": m2ts,
}
root = "out/bdmv_overlay"
shutil.rmtree(root, ignore_errors=True)
backup = {k: v for k, v in files.items() if not k.startswith("STREAM")}
for rel, data in list(files.items()) + [("BACKUP/" + k, v) for k, v in backup.items()]:
    path = os.path.join(root, "BDMV", rel)
    os.makedirs(os.path.dirname(path), exist_ok=True)
    open(path, "wb").write(data)
with zipfile.ZipFile("out/bdmv_overlay.zip", "w", zipfile.ZIP_DEFLATED) as z:
    for dp, _, fs in os.walk(root):
        for f in fs:
            full = os.path.join(dp, f)
            z.write(full, os.path.relpath(full, root))

print("fichiers generes :")
for rel, data in files.items():
    print(f"  BDMV/{rel:22s} {len(data):6d} octets")
print("relecture et aller-retour :")
print("  index   ", IndexBdmv.parse(files["index.bdmv"]).pack() == files["index.bdmv"])
print("  mobj    ", MovieObjectBdmv.parse(files["MovieObject.bdmv"]).pack() == files["MovieObject.bdmv"])
print("  mpls    ", Mpls.parse(files["PLAYLIST/00000.mpls"]).pack() == files["PLAYLIST/00000.mpls"])
print("  clpi    ", Clpi.parse(files["CLIPINF/80002.clpi"]).pack() == files["CLIPINF/80002.clpi"])
print(IndexBdmv.parse(files["index.bdmv"]).dump())
print(MovieObjectBdmv.parse(files["MovieObject.bdmv"]).dump())
print(Mpls.parse(files["PLAYLIST/00000.mpls"]).dump())
