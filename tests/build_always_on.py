"""Genere un menu 'toujours affiche' (ui_model=0) : seul 80002.m2ts change par rapport au menu pop-up.

Usage : python -m tests.build_always_on
"""
import zipfile
from bdauthor.menu import MenuButton, build_menu_clip
from bdauthor.m2ts import reassemble, split_segments
from bdauthor.ig import Ics

buttons = [MenuButton("Lire le film", 1, 720, 420), MenuButton("Menu principal", 0, 720, 560)]
popup, clpi_popup, *_ = build_menu_clip(buttons, ui_model=1)
always, clpi_always, *_ = build_menu_clip(buttons, ui_model=0)
ics = Ics.parse(next(s for e in reassemble(always, 0x1400) for s in split_segments(e.data) if s.name == "ICS").payload)
print("ui_model du fichier genere :", ics.ui_model, "| meme taille :", len(always) == len(popup),
      "| .clpi identique au menu pop-up :", clpi_always == clpi_popup)
print("octets differents :", sum(a != b for a, b in zip(always, popup)))
with zipfile.ZipFile("out/test_menu_affiche_d_office.zip", "w", zipfile.ZIP_DEFLATED) as z:
    z.writestr("BDMV/STREAM/80002.m2ts", always)
print("zip : out/test_menu_affiche_d_office.zip")
