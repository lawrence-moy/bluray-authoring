"""Usage : python -m tests.build_disc_complet 00000.m2ts 00000.clpi_ref

Construit out/disque_complet/ depuis le film et verifie les fichiers contre ceux deja valides
(playlists et navigation du disque d'accueil, .clpi de reference).
"""
import sys, zipfile, os
from bdauthor.disc import build_disc
from bdauthor.nav import make_playlist, make_index, make_movie_objects_accueil
from bdauthor.tsclip import make_clip_clpi
from bdauthor.clpi import Clpi

m2ts = open(sys.argv[1], "rb").read()
files, sc = build_disc(m2ts, "out/disque_complet")
print("playlist menu == playlist validee dans VLC   :", files["PLAYLIST/00001.mpls"] == make_playlist())
print("playlist film == playlist du disque d'accueil :", files["PLAYLIST/00000.mpls"] == make_playlist(with_menu=False))
print("index / MovieObject == disque d'accueil       :", files["index.bdmv"] == make_index(),
      files["MovieObject.bdmv"] == make_movie_objects_accueil())
if len(sys.argv) > 2:
    ref = open(sys.argv[2], "rb").read()
    print("00000.clpi (remplissage zero) identique a multiAVCHD :", files["CLIPINF/00000.clpi"] == ref,
          "| avec son remplissage '0' :", make_clip_clpi(m2ts, pad=b"0") == ref)
print("fichiers :")
for k, v in files.items():
    print(f"  BDMV/{k:22s}{len(v):10d}")
with zipfile.ZipFile("out/disque_complet.zip", "w", zipfile.ZIP_DEFLATED) as z:
    for dp, dn, fn in os.walk("out/disque_complet"):
        for f in fn:
            full = os.path.join(dp, f)
            z.write(full, os.path.relpath(full, "out/disque_complet"))
        if not fn and not dn:
            z.write(dp, os.path.relpath(dp, "out/disque_complet") + "/")
print("zip :", os.path.getsize("out/disque_complet.zip"), "octets")
