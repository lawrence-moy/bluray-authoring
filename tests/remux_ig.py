"""Usage : python -m tests.remux_ig 80002.m2ts

1) recalcule les PTS/DTS a partir des segments et de la regle de duree ;
2) re-multiplexe et compare octet par octet au fichier d'origine.
"""
import sys
from bdauthor.m2ts import iter_packets, reassemble, split_segments
from bdauthor.ig import Ods
from bdauthor.mux import ig_pes_specs, mux_ig_clip

ref = open(sys.argv[1], "rb").read()
pes = reassemble(ref, 0x1400)
segs = [split_segments(e.data)[0] for e in pes]
ics = next(s for s in segs if s.name == "ICS")
pds = next(s for s in segs if s.name == "PDS")
ods = []
for s in segs:
    if s.name == "ODS":
        o = Ods.parse(s.payload)
        ods.append((s.pack(), o.width, o.height))

specs = ig_pes_specs(ics.pack(), pds.pack(), ods, ics_pts=pes[0].pts)
ok = [(s.pts, s.dts) for s in specs] == [(e.pts, e.dts) for e in pes]
print("horodatages recalcules identiques :", ok, f"({len(specs)} PES)")
if not ok:
    for i, (s, e) in enumerate(zip(specs, pes)):
        if (s.pts, s.dts) != (e.pts, e.dts):
            print("  1er ecart au PES", i, (s.pts, s.dts), (e.pts, e.dts)); break

pcr_idx = [p.index for p in iter_packets(ref) if p.pcr is not None][1:]
for label, extra in (("PCR aux positions du disque de reference", pcr_idx), ("politique par defaut (PCR toutes les 90 ms)", None)):
    out = mux_ig_clip(specs, pcr_extra_at=extra)
    same = out == ref
    diff = sum(a != b for a, b in zip(out, ref)) + abs(len(out) - len(ref))
    print(f"{label} : {len(out)} octets, identique : {same}, octets differents : {diff}")
    if not same and len(out) == len(ref):
        i = next(i for i, (a, b) in enumerate(zip(out, ref)) if a != b)
        print(f"  premiere difference : paquet {i // 192}, octet {i % 192}")
