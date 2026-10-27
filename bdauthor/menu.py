"""Construction d'un menu IG : boutons -> ICS + PDS + ODS -> clip .m2ts + .clpi.

Deux modes de rendu :
 - texte (par defaut) : rectangles arrondis + libelle, palette fixe de 5 entrees ;
 - images PNG : `MenuButton(..., images=(normal, selecte, active))`, palette commune calculee (png_palette.py).
Les valeurs des champs reprennent celles observees sur le disque de reference (menu pop-up ou toujours affiche,
ICS 1920x1080, son 0xFF, etc.). La navigation entre boutons est calculee d'apres leur position (auto_neighbors).
"""
import os
from dataclasses import dataclass
from .ig import (Ics, Page, Bog, Button, EffectSequence, Pds, Ods, rle_encode)
from .m2ts import Segment
from .mobj import Command
from .mux import ig_pes_specs, mux_ig_clip
from .clpi import Clpi, ClipInfo, SequenceInfo, AtcSeq, StcSeq, ProgramInfo, Program, Stream
from .png_palette import quantize_images, rgb_to_ycrcb, pixels

# mot d'instruction JUMP_TITLE releve dans MovieObject.bdmv de reference (operande immediat dans `dst`)
JUMP_TITLE_INSN = 0x21810000

PALETTE_RGB = {1: (30, 60, 140), 2: (90, 150, 230), 3: (240, 150, 40), 4: (255, 255, 255)}  # 0 = transparent


@dataclass
class MenuButton:
    label: str
    title: int            # JUMP_TITLE vers ce numero (0 = menu principal, 1.. = titres)
    x: int
    y: int
    w: int = 480          # ignores si `images` est donne (taille de l'image « normal »)
    h: int = 96
    images: tuple = None  # (normal, selectionne, active) : chemins PNG ou images PIL ; 1 ou 2 elements acceptes
    neighbors: tuple = None   # (haut, bas, gauche, droite) en numeros de bouton ; sinon calcule
    commands: list = None     # liste de Command a executer au clic ; sinon JUMP_TITLE `title`


def _font(size):
    from PIL import ImageFont
    for name in ("DejaVuSans-Bold.ttf", "DejaVuSans.ttf", "arialbd.ttf", "arial.ttf", "segoeuib.ttf", "calibrib.ttf"):
        try:
            return ImageFont.truetype(name, size)
        except OSError:
            pass
    return ImageFont.load_default(size)


def render_states(btn: MenuButton):
    """Mode texte : trois images indexees (normal, selectionne, active) sous forme de lignes d'indices."""
    from PIL import Image, ImageDraw
    out = []
    for fill in (1, 2, 3):
        im = Image.new("P", (btn.w, btn.h), 0)
        d = ImageDraw.Draw(im)
        d.rounded_rectangle((0, 0, btn.w - 1, btn.h - 1), 16, fill=fill)
        f = _font(40)
        box = d.textbbox((0, 0), btn.label, font=f)
        d.text(((btn.w - box[2] - box[0]) // 2, (btn.h - box[3] - box[1]) // 2), btn.label, fill=4, font=f)
        px = pixels(im)
        out.append([px[i * btn.w:(i + 1) * btn.w] for i in range(btn.h)])
    return out


def _png_states(btn: MenuButton):
    """Mode PNG : [normal, selectionne, active] en images PIL RGBA."""
    from PIL import Image
    cache, imgs = {}, []
    for i in btn.images:                       # une seule conversion par source distincte : les etats identiques partagent l'objet
        is_path = isinstance(i, (str, bytes)) or hasattr(i, "__fspath__")
        key = os.fspath(i) if is_path else id(i)
        if key not in cache:
            cache[key] = (Image.open(i) if is_path else i).convert("RGBA")
        imgs.append(cache[key])
    while len(imgs) < 3:
        imgs.append(imgs[-1])
    return imgs[:3]


def auto_neighbors(buttons):
    """(haut, bas, gauche, droite) de chaque bouton, d'apres la position des centres ; soi-meme si aucun voisin."""
    cx = [b.x + b.w / 2 for b in buttons]
    cy = [b.y + b.h / 2 for b in buttons]

    def pick(i, ok, score):
        c = [j for j in range(len(buttons)) if j != i and ok(j)]
        return min(c, key=lambda j: (score(j), j)) if c else i
    res = []
    for i in range(len(buttons)):
        res.append((
            pick(i, lambda j: cy[j] < cy[i], lambda j: (cy[i] - cy[j]) + 2 * abs(cx[j] - cx[i])),
            pick(i, lambda j: cy[j] > cy[i], lambda j: (cy[j] - cy[i]) + 2 * abs(cx[j] - cx[i])),
            pick(i, lambda j: cx[j] < cx[i], lambda j: (cx[i] - cx[j]) + 2 * abs(cy[j] - cy[i])),
            pick(i, lambda j: cx[j] > cx[i], lambda j: (cx[j] - cx[i]) + 2 * abs(cy[j] - cy[i]))))
    return res


def build_menu(buttons, ics_pts=42956, video_size=(1920, 1080), ui_model=1, rgb_colors=63):
    """Retourne (liste PesSpec, Ics, Pds, liste d'ODS) pour un menu a une page."""
    if any(b.images for b in buttons):
        flat, owner = [], []                     # images uniques ; pour chaque bouton, l'indice de ses 3 etats dans `flat`
        for b in buttons:
            imgs = _png_states(b) if b.images else _text_rgba(b)
            ids = []
            for im in imgs:
                for k, other in enumerate(flat):
                    if other is im:
                        ids.append(k)
                        break
                else:
                    flat.append(im)
                    ids.append(len(flat) - 1)
            owner.append(ids)
        entries, rows = quantize_images(flat, rgb_colors=rgb_colors)
        pds = Pds(0, 0, entries)
        states = [[(rows[k], flat[k].width, flat[k].height, k) for k in ids] for ids in owner]
        for n, b in enumerate(buttons):
            b.w, b.h = flat[owner[n][0]].width, flat[owner[n][0]].height
    else:
        pds = Pds(0, 0, {0: (16, 128, 128, 0)})
        for i, rgb in PALETTE_RGB.items():
            pds.entries[i] = rgb_to_ycrcb(*rgb) + (255,)
        states = [[(rows, b.w, b.h, (n, k)) for k, rows in enumerate(render_states(b))] for n, b in enumerate(buttons)]
    for n, b in enumerate(buttons):
        if b.x < 0 or b.y < 0 or b.x + b.w > video_size[0] or b.y + b.h > video_size[1]:
            raise ValueError(f"le bouton {n} ({b.x},{b.y},{b.w}x{b.h}) sort de l'ecran {video_size}")
    ods, bogs, made = [], [], {}
    nbs = auto_neighbors(buttons)
    for n, (b, st) in enumerate(zip(buttons, states)):
        oid = []
        for rows, w, h, key in st:
            if key not in made:                   # un objet par image distincte (etats identiques partages)
                made[key] = len(ods)
                ods.append(Ods(len(ods), 0, 0xC0, w, h, rle_encode(rows)))
            oid.append(made[key])
        base, sel, act = oid
        up, down, left, right = b.neighbors or nbs[n]
        btn = Button(button_id=n, numeric_select_value=0xFFFF, auto_action_flags=0, x=b.x, y=b.y,
                     upper=up, lower=down, left=left, right=right,
                     normal=(base, base, 0), selected=(0xFF, sel, sel, 0),
                     activated=(0xFF, act, act),
                     commands=list(b.commands) if b.commands else [Command(JUMP_TITLE_INSN, b.title, 0)])
        bogs.append(Bog(n, [btn]))
    page = Page(page_id=0, version=0, uo_mask=bytes(8), in_effects=EffectSequence(), out_effects=EffectSequence(),
                frame_rate_code=0, default_selected=0, default_activated=0xFFFF, palette_id_ref=0, bogs=bogs)
    ics = Ics(video_width=video_size[0], video_height=video_size[1], frame_rate_byte=0x10,
              composition_number=0, composition_state=0x80, sequence_descriptor=0xC0,
              stream_model=1, ui_model=ui_model, pages=[page])
    specs = ig_pes_specs(Segment(0x18, ics.pack()).pack(), Segment(0x14, pds.pack()).pack(),
                         [(Segment(0x15, o.pack()).pack(), o.width, o.height) for o in ods],
                         ics_pts, video_size)
    return specs, ics, pds, ods


def _text_rgba(btn: MenuButton):
    """Bouton texte en RGBA (pour melanger boutons texte et boutons PNG dans un meme menu)."""
    from PIL import Image, ImageDraw
    out = []
    for fill in (1, 2, 3):
        im = Image.new("RGBA", (btn.w, btn.h), (0, 0, 0, 0))
        d = ImageDraw.Draw(im)
        d.rounded_rectangle((0, 0, btn.w - 1, btn.h - 1), 16, fill=PALETTE_RGB[fill] + (255,))
        f = _font(40)
        box = d.textbbox((0, 0), btn.label, font=f)
        d.text(((btn.w - box[2] - box[0]) // 2, (btn.h - box[3] - box[1]) // 2), btn.label,
               fill=PALETTE_RGB[4] + (255,), font=f)
        out.append(im)
    return out


def make_clpi(num_source_packets, ics_pts, rate=5388536, ig_pid=0x1400, pcr_pid=0x1001, pmt_pid=0x0100,
              lang=b"eng", end_extra=1501) -> bytes:
    """Fichier .clpi d'un clip de menu IG (sans EP_map). Horloge 45 kHz."""
    start = ics_pts // 2
    c = Clpi(version=b"0100",
             clip_info=ClipInfo(1, 5, 0, rate, num_source_packets),
             sequence=SequenceInfo([AtcSeq(0, 0, [StcSeq(pcr_pid, 0, start, start + end_extra)])]),
             program=ProgramInfo([Program(0, pmt_pid, 0, [Stream(ig_pid, 0x91, lang + bytes(17))])]))
    return c.pack()


def build_menu_clip(buttons, **kw):
    specs, ics, pds, ods = build_menu(buttons, **kw)
    m2ts = mux_ig_clip(specs)
    return m2ts, make_clpi(len(m2ts) // 192, kw.get("ics_pts", 42956)), ics, pds, ods


def required_ics_pts(n_objects_pixels, video_size, minimum=42956, margin=9000):
    """Plus petit PTS d'ICS (pair, 90 kHz) laissant le temps de decoder la liste d'objets (nombre de pixels de chacun)."""
    import math
    plane = math.ceil(video_size[0] * video_size[1] * 90000 / 8_000_000)
    need = plane + sum(math.ceil(px * 90000 / 8_000_000) for px in n_objects_pixels) + margin
    return max(minimum, need + (need % 2))
