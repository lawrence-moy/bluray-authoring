"""Usage : python -m tests.build_project video_source.mp4 [nb_extraits=6]

Decoupe la video en extraits courts, construit un disque multi-titres (out/projet_test/), puis verifie :
verificateur de references croisees, decodage complet de chaque clip par ffmpeg, navigation, apercu du menu.
"""
import json, os, subprocess, sys, tempfile
from PIL import Image
from bdauthor.project import build_project, layout
from bdauthor.mpls import Mpls
from bdauthor.mobj import MovieObjectBdmv
from bdauthor.index import IndexBdmv
from bdauthor.m2ts import reassemble, split_segments
from bdauthor.ig import Ics, Pds, Ods

src = sys.argv[1]
n = int(sys.argv[2]) if len(sys.argv) > 2 else 6
tmp = tempfile.mkdtemp()
vids = []
for k in range(n):
    f = os.path.join(tmp, f"extrait{k}.mp4")
    subprocess.run(["ffmpeg", "-y", "-v", "error", "-ss", str(k % 6), "-t", "2.4", "-i", src, "-c:v", "libx264", "-preset", "ultrafast",
                    "-crf", "14", "-c:a", "aac", f], check=True)
    vids.append({"fichier": f, "titre": ["Départ", "Sur la route", "Le long du fleuve", "Traversée du village", "Petite pause", "Arrivée",
                                         "Panorama", "Retour", "Dernier arrêt", "Soirée", "Nuit", "Matin"][k]})
proj = os.path.join(tmp, "projet.json")
json.dump({"titre": "Mes vidéos de caméra", "langue": "fra", "videos": vids, "encodage": {"crf": 18, "preset": "veryfast"},
           "menu": {"fond_secondes": 4}}, open(proj, "w"), ensure_ascii=False)
out = f"out/projet_test_{n}"
info = build_project(proj, out, log=lambda *a: None)
rep = info.pop("verification")
print({k: v for k, v in info.items() if k != "duree_s"})
print("verificateur :", "aucune erreur" if rep.ok else [m for k, m in rep.items if k == "ERREUR"], "|", sum(k == "avertissement" for k, _ in rep.items), "avertissement(s)")

root = out + "/BDMV"
print("--- decodage ffmpeg de chaque clip")
for f in sorted(os.listdir(root + "/STREAM")):
    p = f"{root}/STREAM/{f}"
    err = subprocess.run(["ffmpeg", "-v", "error", "-i", p, "-f", "null", "-"], capture_output=True, text=True).stderr.strip()
    nb = subprocess.run(["ffprobe", "-v", "error", "-count_frames", "-select_streams", "v:0", "-show_entries", "stream=nb_read_frames,width,height",
                         "-of", "csv=p=0", p], capture_output=True, text=True).stdout.strip()
    print(f"  {f}: images,largeur,hauteur = {nb} ; erreurs : {err or 'aucune'}")
print("--- navigation")
idx = IndexBdmv.parse(open(root + "/index.bdmv", "rb").read())
print(" ", idx.dump().splitlines()[3:6], "titres ->", [t.id_ref for t in idx.titles])
mo = MovieObjectBdmv.parse(open(root + "/MovieObject.bdmv", "rb").read())
print(mo.dump(only={0, 1, n}))
print("--- playlist de menu")
print(Mpls.parse(open(f"{root}/PLAYLIST/{n:05d}.mpls", "rb").read()).dump())

# apercu : image de fond decodee + objets IG decodes
bgpng = os.path.join(tmp, "bg.png")
subprocess.run(["ffmpeg", "-y", "-v", "error", "-i", f"{root}/STREAM/{n:05d}.m2ts", "-frames:v", "1", bgpng], check=True)
bg = Image.open(bgpng).convert("RGBA")
menu = open(root + "/STREAM/80002.m2ts", "rb").read()
segs = [s for e in reassemble(menu, 0x1400) for s in split_segments(e.data)]
ics = Ics.parse(next(s for s in segs if s.name == "ICS").payload)
pds = Pds.parse(next(s for s in segs if s.name == "PDS").payload)
ods = {o.object_id: o for o in (Ods.parse(s.payload) for s in segs if s.name == "ODS")}
print(f"menu : ICS {ics.video_width}x{ics.video_height}, {len(ods)} objets, palette {len(pds.entries)} entrees")
for g in ics.pages[0].bogs:
    b = g.buttons[0]
    sel = b.button_id == ics.pages[0].default_selected
    o = ods[b.selected[1] if sel else b.normal[0]]
    im = Image.new("RGBA", (o.width, o.height)); im.putdata([pds.rgba(v) for row in o.pixels() for v in row])
    bg.alpha_composite(im, (b.x, b.y))
bg.convert("RGB").save(f"out/apercu_projet_{n}.png")
print("voisins :", [(g.buttons[0].button_id, (g.buttons[0].upper, g.buttons[0].lower, g.buttons[0].left, g.buttons[0].right)) for g in ics.pages[0].bogs])
try:
    layout(13, (1280, 720))
except ValueError as e:
    print("13 videos refusees :", e)
