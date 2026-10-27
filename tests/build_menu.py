"""Usage : python -m tests.build_menu [80002.clpi_de_reference]

Genere un menu de 2 boutons dans out/ (80002.m2ts + 80002.clpi + apercu PNG)
et relit le resultat avec les parseurs.
"""
import os, sys
from PIL import Image
from bdauthor.menu import MenuButton, build_menu_clip, make_clpi
from bdauthor.m2ts import reassemble, split_segments, iter_packets
from bdauthor.ig import Ics, Pds, Ods
from bdauthor.clpi import Clpi

if len(sys.argv) > 1:   # validation : le .clpi genere avec les parametres du disque de reference doit etre identique
    ref = open(sys.argv[1], "rb").read()
    c = Clpi.parse(ref)
    mine = make_clpi(c.clip_info.num_source_packets, c.sequence.atc[0].stc[0].start_time * 2,
                     rate=c.clip_info.ts_recording_rate)
    print("clpi genere = clpi de reference :", mine == ref)

buttons = [MenuButton("Lire le film", 1, 720, 420), MenuButton("Menu principal", 0, 720, 560)]
m2ts, clpi, ics, pds, ods = build_menu_clip(buttons)
os.makedirs("out", exist_ok=True)
open("out/80002.m2ts", "wb").write(m2ts)
open("out/80002.clpi", "wb").write(clpi)

pk = list(iter_packets(m2ts))
print(f"m2ts : {len(m2ts)} octets, {len(pk)} paquets, PID IG {sum(p.pid == 0x1400 for p in pk)} paquets, "
      f"ICS {len(ics.pack())} octets, {len(ods)} ODS, aligne sur 6144 : {len(m2ts) % 6144 == 0}")
segs = []
for e in reassemble(m2ts, 0x1400):
    segs += split_segments(e.data)
ics2 = Ics.parse(next(s for s in segs if s.name == "ICS").payload)
pds2 = Pds.parse(next(s for s in segs if s.name == "PDS").payload)
ods2 = [Ods.parse(s.payload) for s in segs if s.name == "ODS"]
print("relecture : ICS identique", ics2.pack() == ics.pack(), "| images identiques",
      all(o.pixels() == q.pixels() for o, q in zip(ods2, ods)))
for g in ics2.pages[0].bogs:
    b = g.buttons[0]
    print(f"  bouton {b.button_id} ({b.x},{b.y}) voisins U{b.upper} D{b.lower} : {b.commands[0].text()}")
print("clpi relu :", Clpi.parse(clpi).dump().splitlines()[1])

bg = Image.new("RGBA", (1920, 1080), (50, 60, 80, 255))
for g in ics2.pages[0].bogs:
    b = g.buttons[0]
    sel = b.button_id == ics2.pages[0].default_selected
    o = next(x for x in ods2 if x.object_id == (b.selected[1] if sel else b.normal[0]))
    im = Image.new("RGBA", (o.width, o.height))
    im.putdata([pds2.rgba(v) for row in o.pixels() for v in row])
    bg.alpha_composite(im, (b.x, b.y))
bg.convert("RGB").resize((960, 540)).save("out/apercu_menu.png")
print("apercu : out/apercu_menu.png")
