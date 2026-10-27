"""Usage : python -m tests.inspect_ig 80002.m2ts [pid_hex=1400]

Inventaire des paquets/PES/segments IG + verification que chaque segment
(ICS, PDS, ODS) se decode puis se reecrit a l'identique.
"""
import sys
from collections import Counter
from bdauthor.m2ts import iter_packets, reassemble, split_segments
from bdauthor.ig import Ics, Pds, Ods, rle_encode, rle_decode

data = open(sys.argv[1], "rb").read()
pid = int(sys.argv[2], 16) if len(sys.argv) > 2 else 0x1400
print("PID :", dict((hex(k), v) for k, v in Counter(p.pid for p in iter_packets(data)).items()))
print("PCR :", [(p.index, hex(p.pid), p.pcr) for p in iter_packets(data) if p.pcr is not None])

segs = []
for e in reassemble(data, pid):
    segs += split_segments(e.data)
print("segments :", dict(Counter(s.name for s in segs)))

ok = True
for s in segs:
    cls = {"ICS": Ics, "PDS": Pds, "ODS": Ods}.get(s.name)
    if cls:
        same = cls.parse(s.payload).pack() == s.payload
        ok &= same
        if not same:
            print("aller-retour different pour", s.name)
for s in segs:
    if s.name == "ODS":
        o = Ods.parse(s.payload)
        px = o.pixels()
        ok &= rle_decode(rle_encode(px), o.width, o.height) == px
print("tous les segments : aller-retour identique :", ok)

ics = Ics.parse(next(s for s in segs if s.name == "ICS").payload)
print(f"ICS : {ics.video_width}x{ics.video_height}, stream_model={ics.stream_model}, "
      f"ui_model={ics.ui_model}, {len(ics.pages)} pages")
for p in ics.pages:
    n = sum(len(g.buttons) for g in p.bogs)
    print(f"  page {p.page_id}: {n} boutons, palette {p.palette_id_ref}")
