"""Usage : python -m tests.build_pagination video_source.mp4 [nombre_de_videos=14]

Disque paginé : N videos courtes (une avec ~17 chapitres => 3 pages de chapitres). Verifie la coherence (check.py) et l'accessibilite de tous les titres.
"""
import json, os, subprocess, sys, tempfile
from bdauthor.project import build_project
src, N = sys.argv[1], int(sys.argv[2]) if len(sys.argv) > 2 else 14
tmp = tempfile.mkdtemp(); b = os.path.join(tmp, "b.mp4")
subprocess.run(["ffmpeg", "-y", "-v", "error", "-ss", "1", "-t", "3", "-i", src, "-c:v", "libx264", "-preset", "ultrafast", "-crf", "14", "-c:a", "aac", b], check=True)
vids = [{"fichier": b, "titre": f"Video {k + 1}"} for k in range(N)]
vids[2] = {"fichier": src, "titre": "Longue", "chapitres_auto_s": 0.5}
proj = os.path.join(tmp, "p.json")
json.dump({"titre": "Pagination", "langue": "fra", "videos": vids, "encodage": {"crf": 18, "preset": "ultrafast"}, "menu": {"fond_secondes": 3}}, open(proj, "w"))
info = build_project(proj, "out/pagination", log=lambda *a: print(" ", *a))
rep = info.pop("verification")
print({k: v for k, v in info.items() if k != "duree_s"}); print("verificateur :", "aucune erreur" if rep.ok else "ERREURS")
