"""Quantification d'images RGBA vers une palette Blu-ray commune (Y, Cr, Cb, alpha).

Corrige les defauts constates sur l'ancien rle_menu.py : palette unique pour tous les etats d'un bouton,
transparence conservee (par paliers), conversion BT.709 en plage limitee (16-235), ordre Y/Cr/Cb.

Principe : les pixels (presque) transparents prennent l'indice 0 ; les autres sont quantifies en `rgb_colors`
couleurs (Pillow, mediane coupee, sur l'ensemble des images) puis associes au palier d'alpha le plus proche.
Une entree de palette = (couleur, alpha) => au plus rgb_colors * len(alpha_levels) + 1 entrees.
"""
from PIL import Image

def pixels(im):
    """Liste des pixels d'une image PIL (getdata est deprecie dans les versions recentes de Pillow)."""
    return list(im.get_flattened_data()) if hasattr(im, "get_flattened_data") else list(im.getdata())


ALPHA_LEVELS = (255, 192, 128, 64)
ALPHA_MIN = 16            # en dessous : transparent
MAX_ENTRIES = 254         # la reference utilise 255 entrees au plus (indices 0-254)


def rgb_to_ycrcb(r, g, b):
    """BT.709, plage limitee (inverse de Pds.rgba). Retourne (Y, Cr, Cb)."""
    y = 16 + 0.1826 * r + 0.6142 * g + 0.0620 * b
    cb = 128 - 0.1006 * r - 0.3386 * g + 0.4392 * b
    cr = 128 + 0.4392 * r - 0.3989 * g - 0.0403 * b
    return tuple(max(0, min(255, round(v))) for v in (y, cr, cb))


def quantize_images(images, rgb_colors=63, alpha_levels=ALPHA_LEVELS, alpha_min=ALPHA_MIN, opaque_ok=True):
    """images : liste d'images PIL. Retourne (entrees {indice: (Y, Cr, Cb, alpha)}, [lignes d'indices par image])."""
    if rgb_colors * len(alpha_levels) + 1 > MAX_ENTRIES:
        raise ValueError("trop de couleurs x paliers d'alpha pour une palette Blu-ray")
    data = [pixels(im.convert("RGBA")) for im in images]
    if opaque_ok and all(a < alpha_min or a >= 250 for px in data for (_, _, _, a) in px):
        return _quantize_opaque(images, data, alpha_min)
    visible = [(r, g, b) for px in data for (r, g, b, a) in px if a >= alpha_min]
    idx_iter, palette = iter(()), []
    if visible:
        n = len(visible)
        w = min(n, 1024)
        h = -(-n // w)
        mosaic = Image.new("RGB", (w, h))
        mosaic.putdata(visible + [visible[-1]] * (w * h - n))
        q = mosaic.quantize(colors=rgb_colors, method=0, dither=0)   # 0 = mediane coupee, sans diffusion
        idx_iter, palette = iter(pixels(q)[:n]), q.getpalette()
    used, rows_per_image = {}, []
    for im, px in zip(images, data):
        flat = []
        for (r, g, b, a) in px:
            if a < alpha_min:
                flat.append(0)
                continue
            key = (next(idx_iter), min(range(len(alpha_levels)), key=lambda k: abs(alpha_levels[k] - a)))
            if key not in used:
                used[key] = len(used) + 1
            flat.append(used[key])
        rows_per_image.append([flat[i * im.width:(i + 1) * im.width] for i in range(im.height)])
    entries = {0: (16, 128, 128, 0)}
    for (ri, ai), pi in used.items():
        entries[pi] = rgb_to_ycrcb(*palette[3 * ri:3 * ri + 3]) + (alpha_levels[ai],)
    return entries, rows_per_image


def _quantize_opaque(images, data, alpha_min):
    """Images sans transparence partielle (vignettes) : 253 couleurs communes et tramage de Floyd-Steinberg,
    plus de paliers d'alpha. Indice 0 = transparent, indices 1..n = couleurs opaques."""
    visible = [(r, g, b) for px in data for (r, g, b, a) in px if a >= alpha_min]
    entries = {0: (16, 128, 128, 0)}
    if not visible:
        return entries, [[[0] * im.width for _ in range(im.height)] for im in images]
    n = len(visible)
    w = min(n, 1024)
    h = -(-n // w)
    mosaic = Image.new("RGB", (w, h))
    mosaic.putdata(visible + [visible[-1]] * (w * h - n))
    q = mosaic.quantize(colors=MAX_ENTRIES - 1, method=0, dither=0)
    pal = q.getpalette()
    ncol = len(set(pixels(q)[:n]))
    ncol = max(ncol, max(pixels(q)[:n]) + 1)
    pal = pal[:3 * ncol]
    padded = pal + pal[:3] * (256 - ncol)               # doublons de l'entree 0 : jamais choisis au-dela de ncol
    ref = Image.new("P", (1, 1))
    ref.putpalette(padded)
    for k in range(ncol):
        entries[k + 1] = rgb_to_ycrcb(*pal[3 * k:3 * k + 3]) + (255,)
    rows_per_image = []
    for im, px in zip(images, data):
        idx = pixels(im.convert("RGB").quantize(palette=ref, dither=1))
        flat = [0 if a < alpha_min else (v + 1 if v < ncol else 1) for v, (_, _, _, a) in zip(idx, px)]
        rows_per_image.append([flat[i * im.width:(i + 1) * im.width] for i in range(im.height)])
    return entries, rows_per_image
