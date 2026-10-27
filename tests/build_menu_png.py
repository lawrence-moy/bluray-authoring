"""Usage : python -m tests.build_menu_png

Dessine des boutons PNG (degrades, bords adoucis, 3 etats), construit le menu, relit le flux avec nos parseurs et
mesure l'ecart entre les images d'origine et celles que decodera un lecteur. Ecrit :
  examples/*.png  out/apercu_menu_png.png  out/menu_png_overlay.zip (a superposer a disque_complet)
"""
import os, zipfile
from PIL import Image, ImageDraw
from bdauthor.menu import MenuButton, build_menu_clip, auto_neighbors, _font
from bdauthor.hdmv_menu_builder import build_custom_menu_clip
from bdauthor.m2ts import reassemble, split_segments
from bdauthor.ig import Ics, Pds, Ods
from bdauthor.png_palette import pixels

W, H, S = 480, 96, 4
STATES = {  # (haut du degrade, bas du degrade, contour)
    "normal":   ((40, 70, 150), (20, 40, 100), (120, 150, 220)),
    "selected": ((110, 170, 245), (60, 110, 200), (255, 255, 255)),
    "activated": ((250, 170, 60), (220, 110, 20), (255, 240, 200)),
}


def draw_button(label, state):
    top, bot, edge = STATES[state]
    big = Image.new("RGBA", (W * S, H * S), (0, 0, 0, 0))
    grad = Image.new("RGBA", big.size)
    gd = ImageDraw.Draw(grad)
    for y in range(big.height):
        t = y / (big.height - 1)
        gd.line([(0, y), (big.width, y)], fill=tuple(round(a + (b - a) * t) for a, b in zip(top, bot)) + (255,))
    mask = Image.new("L", big.size, 0)
    ImageDraw.Draw(mask).rounded_rectangle((0, 0, big.width - 1, big.height - 1), 20 * S, fill=255)
    big.paste(grad, mask=mask)
    ImageDraw.Draw(big).rounded_rectangle((2 * S, 2 * S, big.width - 2 * S, big.height - 2 * S), 18 * S, outline=edge + (255,), width=3 * S)
    d = ImageDraw.Draw(big)
    f = _font(40 * S)
    box = d.textbbox((0, 0), label, font=f)
    d.text(((big.width - box[2] - box[0]) // 2, (big.height - box[3] - box[1]) // 2), label, fill=(255, 255, 255, 255), font=f)
    return big.resize((W, H), Image.LANCZOS)


os.makedirs("examples", exist_ok=True); os.makedirs("out", exist_ok=True)
labels = {"lire": "Lire le film", "menu": "Menu principal"}
for key, label in labels.items():
    for st in STATES:
        draw_button(label, st).save(f"examples/bouton_{key}_{st}.png")

buttons = [MenuButton("", 1, 720, 420, images=tuple(f"examples/bouton_lire_{s}.png" for s in STATES)),
           MenuButton("", 0, 720, 560, images=tuple(f"examples/bouton_menu_{s}.png" for s in STATES))]
m2ts, clpi, ics, pds, ods = build_menu_clip(buttons, ui_model=0)

# --- relecture complete par nos parseurs
segs = [s for e in reassemble(m2ts, 0x1400) for s in split_segments(e.data)]
ics2 = Ics.parse(next(s for s in segs if s.name == "ICS").payload)
pds2 = Pds.parse(next(s for s in segs if s.name == "PDS").payload)
ods2 = {o.object_id: o for o in (Ods.parse(s.payload) for s in segs if s.name == "ODS")}
print(f"m2ts {len(m2ts)} octets, {len(m2ts) // 192} paquets (multiple de 6144 : {len(m2ts) % 6144 == 0}) ; ICS {len(ics.pack())} octets ;"
      f" palette : {len(pds2.entries)} entrees ; ODS : {len(ods2)}")
print("relecture identique :", ics2.pack() == ics.pack(), pds2.pack() == pds.pack())

# --- fidelite : image decodee vs image d'origine
tot_rgb = tot_a = n_vis = bad_transp = 0
worst = 0
centres = []
for b_i, (key, _) in enumerate(labels.items()):
    for k, st in enumerate(STATES):
        src = Image.open(f"examples/bouton_{key}_{st}.png").convert("RGBA")
        o = ods2[3 * b_i + k]
        rows = o.pixels()
        assert (o.width, o.height) == src.size
        sp = pixels(src)
        for i, (r, g, b, a) in enumerate(sp):
            R, G, B, A = pds2.rgba(rows[i // W][i % W])
            if a >= 16:
                e = (abs(R - r) + abs(G - g) + abs(B - b)) / 3
                tot_rgb += e; tot_a += abs(A - a); n_vis += 1; worst = max(worst, e)
            elif A != 0:
                bad_transp += 1
        centres.append(pds2.rgba(rows[10][W // 2]))
print(f"pixels visibles : {n_vis} ; ecart moyen couleur {tot_rgb / n_vis:.2f}/255 (pire pixel {worst:.0f}) ; "
      f"ecart moyen alpha {tot_a / n_vis:.2f}/255 ; pixels transparents mal rendus : {bad_transp}")
print("etats distincts (pixel haut du bouton lire) :", len({centres[0], centres[1], centres[2]}) == 3, centres[:3])

# --- navigation
print("voisins (haut,bas,gauche,droite) :", [(b.button_id, (b.upper, b.lower, b.left, b.right)) for g in ics2.pages[0].bogs for b in g.buttons])
grid = [MenuButton("", 1, 100, 100, 200, 60), MenuButton("", 1, 500, 100, 200, 60),
        MenuButton("", 1, 100, 400, 200, 60), MenuButton("", 1, 500, 400, 200, 60)]
print("grille 2x2 (attendu 0:(0,2,0,1) 1:(1,3,0,1) 2:(0,2,2,3) 3:(1,3,2,3)) :", dict(enumerate(auto_neighbors(grid))))

# --- bouton hors ecran
try:
    build_menu_clip([MenuButton("", 1, 1700, 420, images=(f"examples/bouton_lire_normal.png",))])
except ValueError as e:
    print("bouton hors ecran refuse :", e)

# --- ancien appel (hdmv_menu_builder)
build_custom_menu_clip("examples/bouton_lire_normal.png", "examples/bouton_lire_selected.png", "examples/bouton_lire_activated.png",
                       "out/o.m2ts", "out/o.clpi")
os.remove("out/o.m2ts"); os.remove("out/o.clpi")

# --- apercu et paquet a superposer a disque_complet
bg = Image.new("RGBA", (1920, 1080), (50, 60, 80, 255))
for g in ics2.pages[0].bogs:
    b = g.buttons[0]
    sel = b.button_id == ics2.pages[0].default_selected
    o = ods2[b.selected[1] if sel else b.normal[0]]
    im = Image.new("RGBA", (o.width, o.height)); im.putdata([pds2.rgba(v) for row in o.pixels() for v in row])
    bg.alpha_composite(im, (b.x, b.y))
bg.convert("RGB").resize((960, 540)).save("out/apercu_menu_png.png")
with zipfile.ZipFile("out/menu_png_overlay.zip", "w", zipfile.ZIP_DEFLATED) as z:
    z.writestr("BDMV/STREAM/80002.m2ts", m2ts)
    z.writestr("BDMV/CLIPINF/80002.clpi", clpi)
    z.writestr("BDMV/BACKUP/CLIPINF/80002.clpi", clpi)
print("ecrit : out/apercu_menu_png.png, out/menu_png_overlay.zip")
