from bdauthor.menu import MenuButton, build_menu_clip
b = [MenuButton("", 1, 720, 420,
                images=("examples/bouton_lire_normal2.png", "examples/bouton_lire_selected2.png", "examples/bouton_lire_activated2.png"))]
m2ts, clpi, *_ = build_menu_clip(b, ui_model=0)
open("80002.m2ts", "wb").write(m2ts)
open("80002.clpi", "wb").write(clpi)