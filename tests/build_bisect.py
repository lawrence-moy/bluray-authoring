"""Usage : python -m tests.build_bisect index.bdmv_original

Variantes pour localiser ce que libbluray refuse. Chaque zip se superpose a une COPIE du disque d'origine.
  test1_index_seul   : seul index.bdmv change (First Playback et Top Menu -> objet 1 d'origine, qui lance la playlist 0)
  test4_sans_menu    : notre index + notre MovieObject + playlist sans SubPath ni flux IG (film seul)
"""
import os, sys, zipfile
from bdauthor.index import IndexBdmv
from bdauthor.mpls import Mpls
from bdauthor.nav import make_index, make_movie_objects, make_playlist

orig = open(sys.argv[1], "rb").read()
idx = IndexBdmv.parse(orig)
idx.first_playback.id_ref = 1
idx.top_menu.id_ref = 1
t1 = idx.pack()
assert IndexBdmv.parse(t1).pack() == t1 and len(t1) == len(orig)

pl = make_playlist(with_menu=False)
Mpls.parse(pl)
variants = {
    "test1_index_seul": {"index.bdmv": t1},
    "test4_sans_menu": {"index.bdmv": make_index(), "MovieObject.bdmv": make_movie_objects(), "PLAYLIST/00000.mpls": pl},
}
os.makedirs("out", exist_ok=True)
for name, files in variants.items():
    with zipfile.ZipFile(f"out/{name}.zip", "w", zipfile.ZIP_DEFLATED) as z:
        for rel, data in files.items():
            z.writestr(f"BDMV/{rel}", data)
            z.writestr(f"BDMV/BACKUP/{rel}", data)
    print(name, {k: len(v) for k, v in files.items()})
print("playlist sans menu :", Mpls.parse(pl).dump())
print("index test1 :", IndexBdmv.parse(t1).dump().splitlines()[3:5])
