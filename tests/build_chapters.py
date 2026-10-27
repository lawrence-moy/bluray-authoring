"""Usage : python -m tests.build_chapters video_source.mp4

Disque de 3 videos : A (complete, 4 chapitres), B (extrait, sans chapitres), C (extrait, chapitres automatiques).
Construit out/projet_chapitres/ et verifie : references croisees (dont commandes des boutons et reperes), navigation, apercus.
"""
import json, os, subprocess, sys, tempfile
from PIL import Image
from bdauthor.project import build_project
from bdauthor.mpls import Mpls
from bdauthor.mobj import MovieObjectBdmv
from bdauthor.index import IndexBdmv
from bdauthor.clpi import Clpi
from bdauthor.m2ts import reassemble, split_segments
from bdauthor.ig import Ics, Pds, Ods

src = sys.argv[1]
tmp = tempfile.mkdtemp()
b, c = os.path.join(tmp, "b.mp4"), os.path.join(tmp, "c.mp4")
subprocess.run(["ffmpeg", "-y", "-v", "error", "-ss", "1", "-t", "3", "-i", src, "-c:v", "libx264", "-preset", "ultrafast", "-crf", "14", "-c:a", "aac", b], check=True)
subprocess.run(["ffmpeg", "-y", "-v", "error", "-ss", "2", "-t", "4", "-i", src, "-c:v", "libx264", "-preset", "ultrafast", "-crf", "14", "-c:a", "aac", c], check=True)
proj = os.path.join(tmp, "projet.json")
json.dump({"titre": "Test des chapitres", "langue": "fra",
           "videos": [{"fichier": src, "titre": "Video complete", "chapitres": [0, 2.1, {"t": 4.3, "titre": "Le milieu"}, 6.4]},
                      {"fichier": b, "titre": "Sans chapitres"},
                      {"fichier": c, "titre": "Chapitres auto", "chapitres_auto_s": 1.2}],
           "encodage": {"crf": 18, "preset": "veryfast"}, "menu": {"fond_secondes": 4}}, open(proj, "w"), ensure_ascii=False)
out = "out/projet_chapitres"
info = build_project(proj, out, log=lambda *a: print(" ", *a))
rep = info.pop("verification")
print({k: v for k, v in info.items() if k not in ("duree_s",)})
for k, m in rep.items:
    if k != "ok" or "menu" in m:
        print(f"[{k}] {m}")
print("verificateur :", "aucune erreur" if rep.ok else "ERREURS")
root = out + "/BDMV"
idx = IndexBdmv.parse(open(root + "/index.bdmv", "rb").read())
print("titres :", len(idx.titles), "->", [t.id_ref for t in idx.titles])
print("--- reperes des playlists video")
for i in range(3):
    m = Mpls.parse(open(f"{root}/PLAYLIST/{i:05d}.mpls", "rb").read())
    print(f"  playlist {i}: in {m.playitems[0].in_time} out {m.playitems[0].out_time} | marques (s depuis le debut) :",
          [round((k.time - m.playitems[0].in_time) / 45000, 2) for k in m.marks])
print("--- navigation")
print(MovieObjectBdmv.parse(open(root + "/MovieObject.bdmv", "rb").read()).dump(only={0, 4, 5, 6}))
print("--- menus : playlists")
for pl in (3, 4, 5):
    f = f"{root}/PLAYLIST/{pl:05d}.mpls"
    if os.path.exists(f):
        m = Mpls.parse(open(f, "rb").read())
        print(f"  playlist {pl}: clip {m.playitems[0].clip}, menu IG clip {m.subpaths[0].items[0].clip}, still_mode {m.playitems[0].tail[9]}")
# apercu du menu de chapitres (video 1)
def preview(bg_clip, ig_clip, name):
    bgpng = os.path.join(tmp, "bg.png")
    subprocess.run(["ffmpeg", "-y", "-v", "error", "-i", f"{root}/STREAM/{bg_clip}.m2ts", "-frames:v", "1", bgpng], check=True)
    bg = Image.open(bgpng).convert("RGBA")
    segs = [s for e in reassemble(open(f"{root}/STREAM/{ig_clip}.m2ts", "rb").read(), 0x1400) for s in split_segments(e.data)]
    ics = Ics.parse(next(s for s in segs if s.name == "ICS").payload)
    pds = Pds.parse(next(s for s in segs if s.name == "PDS").payload)
    ods = {o.object_id: o for o in (Ods.parse(s.payload) for s in segs if s.name == "ODS")}
    for g in ics.pages[0].bogs:
        bt = g.buttons[0]
        o = ods[bt.selected[1] if bt.button_id == ics.pages[0].default_selected else bt.normal[0]]
        im = Image.new("RGBA", (o.width, o.height)); im.putdata([pds.rgba(v) for row in o.pixels() for v in row])
        bg.alpha_composite(im, (bt.x, bt.y))
    bg.convert("RGB").save(name)
    return [(g.buttons[0].button_id, [c.text() for c in g.buttons[0].commands]) for g in ics.pages[0].bogs]
for pl, ig, bg in ((3, "80003", "00004"), (4, "80004", "00005")):
    if os.path.exists(f"{root}/STREAM/{ig}.m2ts"):
        cmds = preview(bg, ig, f"out/apercu_chapitres_{pl}.png")
        print(f"  menu {ig} : boutons et commandes :", cmds)
