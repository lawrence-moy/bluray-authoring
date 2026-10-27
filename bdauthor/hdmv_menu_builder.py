"""Remplace l'ancien hdmv_menu_builder.py (meme appel) en s'appuyant sur les modules valides : menu.py, png_palette.py,
ig.py et mux.py. Les anciens rle_menu.py et igs_muxer.py ne sont plus utilises et peuvent etre supprimes.

Les temps du menu doivent rester coherents avec la playlist : ics_pts (90 kHz) = 2 x entree du SubPlayItem
(par defaut 42956 <-> 21478, voir nav.make_playlist).
"""
from .menu import MenuButton, build_menu_clip


def build_custom_menu_clip(normal_png, selected_png, activated_png, out_m2ts_path, out_clpi_path,
                           x=720, y=420, title=1, ui_model=0, ics_pts=42956, video_size=(1920, 1080)):
    """Un bouton a trois etats PNG -> clip de menu (.m2ts) + son .clpi. `title` : titre lance par le bouton."""
    btn = MenuButton(label="", title=title, x=x, y=y, images=(normal_png, selected_png, activated_png))
    m2ts, clpi, *_ = build_menu_clip([btn], ui_model=ui_model, ics_pts=ics_pts, video_size=video_size)
    with open(out_m2ts_path, "wb") as f:
        f.write(m2ts)
    with open(out_clpi_path, "wb") as f:
        f.write(clpi)
    print(f"Menu M2TS et CLPI generes : {out_m2ts_path}, {out_clpi_path}")
    return m2ts, clpi
