"""Disque multi-titres a partir d'un fichier de projet JSON.

    {
      "titre": "Mes videos",
      "langue": "fra",
      "videos": [ {"fichier": "balade.mp4", "titre": "Balade"}, {"fichier": "fete.mp4"} ],
      "encodage": {"crf": 18, "preset": "medium"},
      "menu": {"fond": "fond.png", "fond_secondes": 5, "taille": "video", "boucle": false, "retour": "chapitres", "par_page": 8, "cache": true}
    }

Par video : "chapitres": [0, 30, {"t": 95, "titre": "Le pont"}] (secondes) ou "chapitres_auto_s": 60 ; 2 a 99 chapitres -> repères
dans la playlist (touches suivant/precedent) ET menu de chapitres (le bouton de la video ouvre ce menu : « Tout lire », « Retour »).
Les reperes sont alignes sur l'image d'acces (EP_map) precedente. `menu.retour` : « chapitres » (defaut) = apres une video a chapitres on
revient a son menu de chapitres ; « principal » = au menu principal.
Chemins relatifs au fichier de projet. `boucle` : false (defaut) = le menu s'arrete sur la derniere image du fond (la selection ne revient pas
a la premiere vignette) ; true = le fond est rejoue en boucle (la selection revient a la premiere vignette a chaque tour). 1 a 99 videos. Pagination : au-dela de 12 elements (videos ou chapitres) le menu est coupe en pages equilibrees de `par_page` (8 par defaut ; 1 a 12), avec boutons « Precedent » / « Suivant » ; chaque page = clip IG + playlist + titre propres ; apres une video on revient a la page du menu qui la contient. Structure produite : videos = clips/playlists 0..N-1 (titres 1..N),
fond de menu = clip N (image fixe, sans audio), playlist de menu N (fond + menu IG, boucle), clip IG 80002.
Usage : python -m bdauthor.project projet.json dossier_sortie
"""
import json, math, os, sys, tempfile
from PIL import Image, ImageDraw
from .disc import write_bdmv
from .encode import make_film_m2ts, make_still_m2ts, extract_thumbnail
from .menu import MenuButton, build_menu_clip, required_ics_pts, _font
from .mobj import Command
from .nav import (make_index, make_movie_objects_paged, make_playlist, plan_navigation,
                  MOVE_RI_INSN, JUMP_TITLE_INSN)
from .tsclip import scan_clip, make_clip_clpi
from .check import check_bdmv

FPS_STR = {23.976: "24000/1001", 24.0: "24", 25.0: "25", 29.97: "30000/1001", 50.0: "50", 59.94: "60000/1001"}
MAX_PER_PAGE = 12     # elements par page de menu (au-dela : pagination)
MAX_VIDEOS = 99
MAX_CHAPTERS = 99


# (colonnes, largeur vignette, hauteur vignette, hauteur etiquette, ecart horizontal, ecart vertical), en pixels d'un plan 1920x1080
LAYOUTS = [(3, 480, 270, 56, 60, 40), (4, 360, 202, 48, 40, 30), (4, 280, 157, 40, 40, 24), (4, 260, 146, 38, 40, 24)]


def layout(n, size, reserve=0):
    """Grille de boutons pour n vignettes sur un plan de `size`, dans la zone libre entre le titre (y = 250) et le bas
    (marge 60, plus `reserve` pixels pour des boutons additionnels). Prend la premiere grille qui tient.
    Retourne [(x, y, largeur_bouton, hauteur_vignette, hauteur_totale)] en pixels du plan `size`."""
    if n > MAX_PER_PAGE or n < 1:
        raise ValueError(f"{n} elements sur une page : 1 a {MAX_PER_PAGE}")
    s = size[0] / 1920
    avail = 1080 - 250 - 60 - reserve
    for cols0, tw, th, lab, gx, gy in LAYOUTS:
        cols = min(n, cols0)
        rows = -(-n // cols)
        W, H = tw, th + lab
        total_h = rows * H + (rows - 1) * gy
        if total_h <= avail:
            break
    else:
        raise ValueError(f"{n} elements ne tiennent pas dans le menu")
    total_w = cols * W + (cols - 1) * gx
    x0, y0 = (1920 - total_w) // 2, 250 + (avail - total_h) // 2
    out = []
    for i in range(n):
        r, c = divmod(i, cols)
        if r == rows - 1 and n % cols:                    # derniere ligne incomplete : centree
            k = n - (rows - 1) * cols
            xr = (1920 - (k * W + (k - 1) * gx)) // 2 + c * (W + gx)
        else:
            xr = x0 + c * (W + gx)
        out.append((round(xr * s), round((y0 + r * (H + gy)) * s), round(W * s), round(th * s), round(H * s)))
    return out


def _fit_text(draw, text, width, size):
    f = _font(size)
    while draw.textlength(text, font=f) > width - 12 and size > 12:
        size -= 2
        f = _font(size)
    if draw.textlength(text, font=f) > width - 12:
        while len(text) > 1 and draw.textlength(text + "...", font=f) > width - 12:
            text = text[:-1]
        text += "..."
    return text, f


def button_images(thumb_path, label, w, h_total, h_thumb):
    """(normal, selectionne) en RGBA opaque ; l'etat active reutilise le selectionne."""
    thumb = Image.open(thumb_path).convert("RGB").resize((w, h_thumb), Image.LANCZOS)
    out = []
    for selected in (False, True):
        im = Image.new("RGBA", (w, h_total), (24, 32, 58, 255) if not selected else (60, 100, 190, 255))
        im.paste(thumb, (0, 0))
        d = ImageDraw.Draw(im)
        txt, f = _fit_text(d, label, w, max(14, round((h_total - h_thumb) * 0.55)))
        box = d.textbbox((0, 0), txt, font=f)
        d.text(((w - box[2] - box[0]) // 2, h_thumb + (h_total - h_thumb - box[3] - box[1]) // 2), txt, fill=(255, 255, 255, 255), font=f)
        bw = max(3, w // 60) if selected else 1
        col = (255, 255, 255, 255) if selected else (70, 85, 120, 255)
        for k in range(bw):
            d.rectangle((k, k, w - 1 - k, h_total - 1 - k), outline=col)
        out.append(im)
    return out


def text_button_images(label, w, h):
    """(normal, selectionne) pour un bouton uniquement textuel (« Tout lire », « Retour »)."""
    out = []
    for selected in (False, True):
        im = Image.new("RGBA", (w, h), (24, 32, 58, 255) if not selected else (60, 100, 190, 255))
        d = ImageDraw.Draw(im)
        txt, f = _fit_text(d, label, w, max(14, round(h * 0.5)))
        box = d.textbbox((0, 0), txt, font=f)
        d.text(((w - box[2] - box[0]) // 2, (h - box[3] - box[1]) // 2), txt, fill=(255, 255, 255, 255), font=f)
        bw = max(3, w // 60) if selected else 1
        col = (255, 255, 255, 255) if selected else (70, 85, 120, 255)
        for k in range(bw):
            d.rectangle((k, k, w - 1 - k, h - 1 - k), outline=col)
        out.append(im)
    return out


def parse_chapters(v, sc):
    """Reperes d'une video : [(temps 45 kHz, nom, secondes depuis le debut)], [] si moins de 2 chapitres.
    Chaque repere (sauf le premier, au debut du clip) est cale sur l'image d'acces qui precede l'instant demande."""
    dur = (sc.end90 - sc.start90) / 90000
    spec, auto = v.get("chapitres"), v.get("chapitres_auto_s")
    if not spec and auto:
        spec = [k * auto for k in range(int(dur // auto) + 1) if k * auto < dur - auto / 2]
    if not spec:
        return []
    items = [(float(c["t"]), c.get("titre")) if isinstance(c, dict) else (float(c), None) for c in spec]
    if all(t > 0 for t, _ in items):
        items.insert(0, (0.0, None))
    eps = sorted(p for p, _, _ in sc.eps)
    out = {}
    for t, name in sorted(items):
        if t >= dur:
            continue
        if t == 0:
            m45 = sc.start90 // 2
        else:
            target = sc.start90 + round(t * 90000)
            m45 = max([p for p in eps if p <= target] or [eps[0]]) // 2
        out.setdefault(m45, name)
    chapters = [(m, out[m] or f"Chapitre {j + 1}", (m * 2 - sc.start90) / 90000) for j, m in enumerate(sorted(out))]
    if len(chapters) < 2:
        return []
    if len(chapters) > MAX_CHAPTERS:
        raise ValueError(f"{len(chapters)} chapitres : {MAX_CHAPTERS} au maximum par video")
    return chapters


def default_background(title, size):
    w, h = size
    im = Image.new("RGB", size)
    d = ImageDraw.Draw(im)
    for y in range(h):
        t = y / (h - 1)
        d.line([(0, y), (w, y)], fill=(round(18 + 20 * t), round(26 + 30 * t), round(60 + 50 * t)))
    f = _font(round(84 * w / 1920))
    txt, f = _fit_text(d, title, w - 200, round(84 * w / 1920))
    box = d.textbbox((0, 0), txt, font=f)
    d.text(((w - box[2] - box[0]) // 2, round(110 * w / 1920)), txt, fill=(255, 255, 255), font=f)
    return im


def paginate(count, par_page=None):
    """Repartit `count` elements en pages equilibrees. Une seule page si count <= par_page (12 par defaut) ; sinon pages de `par_page` (8 par defaut)."""
    if par_page is not None and not 1 <= par_page <= MAX_PER_PAGE:
        raise ValueError(f"menu.par_page : 1 a {MAX_PER_PAGE}")
    if count <= (par_page or MAX_PER_PAGE):
        return [list(range(count))]
    pages = -(-count // (par_page or 8))
    base, extra = divmod(count, pages)
    out, k = [], 0
    for q in range(pages):
        size = base + (1 if q < extra else 0)
        out.append(list(range(k, k + size)))
        k += size
    return out


def cached_film(path, lang, enc, cache_dir, log=print):
    """Encodage d'une video avec cache disque (cle : chemin, taille, date, langue, reglages) : une video deja encodee n'est pas refaite,
    et un build interrompu reprend ou il s'est arrete."""
    if not cache_dir:
        return make_film_m2ts(path, lang=lang, **enc)
    import hashlib, pickle
    stt = os.stat(path)
    key = hashlib.sha1(repr((os.path.abspath(path), stt.st_size, int(stt.st_mtime), lang, sorted(enc.items()), "v1")).encode()).hexdigest()[:20]
    f = os.path.join(cache_dir, key + ".pkl")
    if os.path.isfile(f):
        try:
            return pickle.load(open(f, "rb"))
        except Exception:
            pass
    res = make_film_m2ts(path, lang=lang, **enc)
    os.makedirs(cache_dir, exist_ok=True)
    pickle.dump(res, open(f + ".tmp", "wb"))
    os.replace(f + ".tmp", f)
    return res


def build_project(project_path, out_dir, log=print):
    base = os.path.dirname(os.path.abspath(project_path))
    proj = json.load(open(project_path, encoding="utf-8"))
    vids = proj["videos"]
    n = len(vids)
    if not 1 <= n <= MAX_VIDEOS:
        raise ValueError(f"1 a {MAX_VIDEOS} videos attendues, {n} donnees")
    lang = proj.get("langue", "und").encode("ascii")
    enc = proj.get("encodage", {})
    menu_cfg = proj.get("menu", {})
    par_page = menu_cfg.get("par_page")
    title = proj.get("titre", "Menu")
    P = lambda f: f if os.path.isabs(f) else os.path.join(base, f)
    vtitle = lambda v: v.get("titre") or os.path.splitext(os.path.basename(v["fichier"]))[0]
    files, scans, stats_all = {}, [], []
    cache_dir = os.path.join(base, ".cache_bdauthor") if menu_cfg.get("cache", True) else None
    with tempfile.TemporaryDirectory() as tmp:
        # 1. videos
        for i, v in enumerate(vids):
            log(f"[{i + 1}/{n}] encodage de {v['fichier']} ...")
            m2ts, st = cached_film(P(v["fichier"]), lang, enc, cache_dir, log)
            sc = scan_clip(m2ts)
            scans.append(sc)
            stats_all.append(st)
            files[f"STREAM/{i:05d}.m2ts"] = m2ts
            files[f"CLIPINF/{i:05d}.clpi"] = make_clip_clpi(m2ts, lang=lang)
        chapters = [parse_chapters(v, sc) for v, sc in zip(vids, scans)]
        with_ch = [i for i, c in enumerate(chapters) if c]
        main_pg = paginate(n, par_page)
        ch_pg = {i: paginate(len(chapters[i]), par_page) for i in with_ch}
        video_page = {i: p for p, idxs in enumerate(main_pg) for i in idxs}
        main_titles, ch_titles, total_titles = plan_navigation(n, len(main_pg), {i: len(g) for i, g in ch_pg.items()})
        first = scans[0]
        fmt = (first.width, first.height, first.fps)
        for i, sc in enumerate(scans[1:], 1):
            if (sc.width, sc.height, sc.fps) != fmt:
                log(f"avertissement : la video {i + 1} est en {sc.width}x{sc.height} {sc.fps} i/s, la premiere en {fmt[0]}x{fmt[1]} {fmt[2]} i/s "
                    f"(un changement de format peut provoquer un temps mort a la lecture)")
        size = (first.width, first.height)
        menu_size = (1920, 1080) if menu_cfg.get("taille") == "1080" else size
        sc_ = menu_size[0] / 1920
        bh, by = round(80 * sc_), round((1080 - 60 - 80) * sc_)

        def nav_buttons(specs, bw):
            """Boutons texte de la barre du bas : [(libelle, x sur un plan 1920, titre cible)]."""
            out = []
            for label, x, target in specs:
                nrm, sel = text_button_images(label, bw, bh)
                out.append(MenuButton("", target, round(x * sc_), by, images=(nrm, sel, sel)))
            return out

        def add_menu_page(buttons, pix, ig_num, pl_num, bg_num, bgscan, label):
            """Clip IG + playlist (fond bg_num + menu ig_num) d'une page de menu. Retourne (ics_pts, m2ts, ods, pds)."""
            pts = required_ics_pts(pix, menu_size)
            m2ts_, clpi_, _, pds_, ods_ = build_menu_clip(buttons, ui_model=0, ics_pts=pts, video_size=menu_size)
            if len(m2ts_) > 6 * 1024 * 1024:
                raise ValueError(f"{label} trop lourd ({len(m2ts_) / 1e6:.1f} Mo) : reduire le nombre d'elements par page (menu.par_page)")
            files[f"STREAM/{ig_num:05d}.m2ts"], files[f"CLIPINF/{ig_num:05d}.clpi"] = m2ts_, clpi_
            files[f"PLAYLIST/{pl_num:05d}.mpls"] = make_playlist(
                clip=f"{bg_num:05d}", in_time=bgscan.start90 // 2, out_time=bgscan.end90 // 2, video=(*bgscan.video, bgscan.video_code),
                audio=None, with_menu=True, menu_clip=f"{ig_num:05d}", menu_in=pts // 2, menu_out=pts // 2 + 1501,
                still_mode=0 if menu_cfg.get("boucle") else 2)     # 2 = arret infini sur la derniere image : le menu ne redemarre pas
            return pts, m2ts_, ods_, pds_

        # 2. fond de menu (clip image fixe, sans audio)
        bg_png = P(menu_cfg["fond"]) if menu_cfg.get("fond") else os.path.join(tmp, "fond.png")
        if not menu_cfg.get("fond"):
            default_background(title, size).save(bg_png)
        log("encodage du fond de menu ...")
        bg_m2ts, bg_stats = make_still_m2ts(bg_png, menu_cfg.get("fond_secondes", 5), size, fps=FPS_STR[first.fps],
                                            crf=enc.get("crf", 18), preset=enc.get("preset", "medium"))
        bg_sc = scan_clip(bg_m2ts)
        files[f"STREAM/{n:05d}.m2ts"] = bg_m2ts
        files[f"CLIPINF/{n:05d}.clpi"] = make_clip_clpi(bg_m2ts)
        # 3. menu principal : une page par groupe de videos (page 1 = clip IG 80002, playlist N)
        next_pl, next_ig, next_bg = n + 1, 80003, n + 1
        main_pls = [n] + [n + p for p in range(1, len(main_pg))]
        next_pl = n + len(main_pg)
        first_page = None
        for p, idxs in enumerate(main_pg):
            multi = len(main_pg) > 1
            cells = layout(len(idxs), menu_size, reserve=140 if multi else 0)
            buttons = []
            for i, (x, y, w, th, h) in zip(idxs, cells):
                v = vids[i]
                thumb = os.path.join(tmp, f"v{i}.png")
                extract_thumbnail(P(v["fichier"]), thumb, (w, th), at=v.get("vignette_s"))
                normal, selected = button_images(thumb, vtitle(v), w, h, th)
                go = ch_titles[i]["pages"][0] if i in ch_titles else i + 1     # video a chapitres : le bouton ouvre le menu de chapitres
                buttons.append(MenuButton("", go, x, y, images=(normal, selected, selected)))
            pix = [2 * c[2] * c[4] for c in cells]
            if multi:
                bw = round(360 * sc_)
                nav = ([("< Précédent", 560, main_titles[p - 1])] if p > 0 else []) + \
                      ([("Suivant >", 1000, main_titles[p + 1])] if p < len(main_pg) - 1 else [])
                buttons += nav_buttons(nav, bw)
                pix += [2 * bw * bh] * len(nav)
            ig_num = 80002 if p == 0 else next_ig
            if p > 0:
                next_ig += 1
            res = add_menu_page(buttons, pix, ig_num, main_pls[p], n, bg_sc, f"menu principal (page {p + 1})")
            if p == 0:
                first_page = res
        ics_pts, menu_m2ts, ods, pds = first_page
        # 4. menus de chapitres (un fond par video si pas de fond commun ; une page par groupe de chapitres)
        ch_pls = {}
        for k, i in enumerate(with_ch):
            bg_num = n
            csc = bg_sc
            if not menu_cfg.get("fond"):
                bg_num, next_bg = next_bg, next_bg + 1
                cbg = os.path.join(tmp, f"fond_ch{k}.png")
                default_background(f"{vtitle(vids[i])} - chapitres", size).save(cbg)
                cm2ts, _ = make_still_m2ts(cbg, menu_cfg.get("fond_secondes", 5), size, fps=FPS_STR[first.fps],
                                           crf=enc.get("crf", 18), preset=enc.get("preset", "medium"))
                csc = scan_clip(cm2ts)
                files[f"STREAM/{bg_num:05d}.m2ts"], files[f"CLIPINF/{bg_num:05d}.clpi"] = cm2ts, make_clip_clpi(cm2ts)
            log(f"menu de chapitres de la video {i + 1} ({len(chapters[i])} chapitres, {len(ch_pg[i])} page(s)) ...")
            ch_pls[i] = []
            npg = len(ch_pg[i])
            retour_t = main_titles[video_page[i]]
            for q, idxs in enumerate(ch_pg[i]):
                pl_num, ig_num = next_pl, next_ig
                next_pl, next_ig = next_pl + 1, next_ig + 1
                ch_pls[i].append(pl_num)
                ccells = layout(len(idxs), menu_size, reserve=140)
                cbuttons = []
                for j, (x, y, w, th, h) in zip(idxs, ccells):
                    m45, name, t_rel = chapters[i][j]
                    thumb = os.path.join(tmp, f"c{i}_{j}.png")
                    extract_thumbnail(P(vids[i]["fichier"]), thumb, (w, th), at=t_rel)
                    nrm, sel = button_images(thumb, f"{name} ({round(t_rel) // 60}:{round(t_rel) % 60:02d})", w, h, th)
                    cbuttons.append(MenuButton("", 0, x, y, images=(nrm, sel, sel),
                                               commands=[Command(MOVE_RI_INSN, 0, j), Command(JUMP_TITLE_INSN, ch_titles[i]["play"], 0)]))
                if npg == 1:
                    bw = round(360 * sc_)
                    nav = [("Tout lire", 560, i + 1), ("Retour", 1000, retour_t)]
                else:
                    bw = round(320 * sc_)
                    nav = ([("< Précédent", 260, ch_titles[i]["pages"][q - 1])] if q > 0 else []) + \
                          [("Tout lire", 620, i + 1), ("Retour", 980, retour_t)] + \
                          ([("Suivant >", 1340, ch_titles[i]["pages"][q + 1])] if q < npg - 1 else [])
                cbuttons += nav_buttons(nav, bw)
                pix = [2 * c[2] * c[4] for c in ccells] + [2 * bw * bh] * len(nav)
                add_menu_page(cbuttons, pix, ig_num, pl_num, bg_num, csc, f"menu de chapitres {i + 1} (page {q + 1})")
        # 5. playlists des videos et navigation
        for i, sc in enumerate(scans):
            pid_a, type_a, code_a = sc.audio_codes[0]
            files[f"PLAYLIST/{i:05d}.mpls"] = make_playlist(
                clip=f"{i:05d}", in_time=sc.start90 // 2, out_time=sc.end90 // 2, video=(*sc.video, sc.video_code),
                audio=(pid_a, type_a, code_a, lang), with_menu=False,
                marks=[c[0] for c in chapters[i]] or None)
        files["MovieObject.bdmv"] = make_movie_objects_paged(n, main_pls, main_titles, ch_pls, ch_titles, video_page,
                                                             retour=menu_cfg.get("retour", "chapitres"))
        files["index.bdmv"] = make_index(0, 0, tuple(range(1, total_titles + 1)))
    write_bdmv(out_dir, files)
    report = check_bdmv(os.path.join(out_dir, "BDMV"))
    info = {"videos": n, "pages_menu": len(main_pg), "chapitres": {i + 1: len(c) for i, c in enumerate(chapters) if c},
            "pages_chapitres": {i + 1: len(g) for i, g in ch_pg.items()}, "format": fmt, "menu": menu_size, "ics_pts_s": ics_pts / 90000,
            "octets_menu": len(menu_m2ts), "objets_graphiques": len(ods), "palette": len(pds.entries),
            "octets_total": sum(len(v) for k, v in files.items() if k.startswith("STREAM")),
            "duree_s": [round((sc.end90 - sc.start90) / 90000, 2) for sc in scans], "verification": report}
    return info


if __name__ == "__main__":
    info = build_project(sys.argv[1], sys.argv[2])
    rep = info.pop("verification")
    print({k: v for k, v in info.items()})
    for kind, msg in rep.items:
        if kind != "ok":
            print(f"[{kind}] {msg}")
    print("verification :", "aucune erreur" if rep.ok else "ERREURS")
