"""Usage : python -m tests.film_mux_check 00000.m2ts 00000.clpi

Passe le film de reference dans notre multiplexeur (vmux.py) et verifie : PES intacts, descripteurs de la PMT reconstruits,
PCR / PSI / ATS coherents, .clpi equivalent (memes points d'acces, memes temps).
"""
import sys, collections
from bdauthor.m2ts import iter_packets, reassemble
from bdauthor.vmux import mux_film
from bdauthor.encode import ac3_descriptor, avc_descriptor
from bdauthor.tsclip import nal_units, scan_clip, make_clip_clpi
from bdauthor.clpi import Clpi

ref = open(sys.argv[1], "rb").read()
v = reassemble(ref, 0x1011); a = reassemble(ref, 0x1100)
video = [(e.pts, e.dts, e.data) for e in v]
audio = [(e.pts, e.data) for e in a]
sps = next(e.data[o:o + n] for e in v for t, o, n in nal_units(e.data) if t == 7)
vdesc, adesc = avc_descriptor(sps), ac3_descriptor(audio[0][1])
print("descripteur AVC  :", vdesc.hex(), "| reference : 28044d4029bf  ->", vdesc.hex() == "28044d4029bf")
print("descripteur AC-3 :", adesc.hex(), "| reference : 810408200400  ->", adesc.hex() == "810408200400")
streams = [(0x1B, 0x1011, vdesc), (0x81, 0x1100, bytes.fromhex("050441432d33") + adesc + bytes.fromhex("0a04") + b"und\x00")]
out, st = mux_film(video, audio, streams)
print("statistiques :", {k: (round(x, 3) if isinstance(x, float) else x) for k, x in st.items()})

pk = list(iter_packets(out))
print("multiple de 32 paquets :", len(pk) % 32 == 0, "| ATS croissants :", all(b.ats >= a_.ats for a_, b in zip(pk, pk[1:]) if b.ats > 1000 and a_.ats > 1000 or True))
v2, a2 = reassemble(out, 0x1011), reassemble(out, 0x1100)
same_v = [(e.pts, e.dts, e.data) for e in v2] == video
same_a = [(e.pts, e.data) for e in a2] == audio
print(f"PES video identiques : {same_v} ({len(v2)}) | trames audio identiques : {same_a} ({len(a2)})")
cnt = collections.Counter(p.pid for p in pk)
print("paquets par PID :", {hex(k): n for k, n in sorted(cnt.items())})
pcr = [p for p in pk if p.pcr is not None]
print("PCR : %d, ecart max %.1f ms (reference : 100,1 ms), PCR == ATS complet : %s" % (
    len(pcr), max(b.pcr - a_.pcr for a_, b in zip(pcr, pcr[1:])) / 27000, all((p.pcr & 0x3FFFFFFF) == p.ats for p in pcr)))
# continuite des compteurs
bad = 0
last = {}
for p in pk:
    if not p.payload or p.pid == 0x1fff: continue
    if p.pid in last and p.cc != (last[p.pid] + 1) & 15: bad += 1
    last[p.pid] = p.cc
print("sauts de compteur de continuite :", bad)
# lead
import statistics
lead = []
for e in v2:
    first = pk[e.first_packet]; 
    base = [p for p in pcr][0]; 
lead_v = [((e.dts if e.dts is not None else e.pts) * 300 - (pk[e.first_packet].ats + ((pcr[0].pcr >> 30) << 30))) / 27e6 for e in v2]
print("avance video (DTS - arrivee 1er paquet) : min %.3f med %.3f max %.3f s (reference 0,419 / 0,483 / 0,534)" %
      (min(lead_v), statistics.median(lead_v), max(lead_v)))
# clpi
sc = scan_clip(out)
refc = Clpi.parse(open(sys.argv[2], "rb").read())
mine = Clpi.parse(make_clip_clpi(out, pad=b"0"))
pts = lambda c: [(x, ) for x in c.cpi.streams[0].entries()]
ep_ref = [p for p, _ in refc.cpi.streams[0].entries()]
ep_new = [p for p, _ in mine.cpi.streams[0].entries()]
print("EP_map : memes PTS :", ep_ref == ep_new, "| memes I_end_offset :", [f.end_offset for f in refc.cpi.streams[0].fine] == [f.end_offset for f in mine.cpi.streams[0].fine],
      "| meme sequence :", refc.sequence == mine.sequence, "| memes flux :", [(s.pid, s.coding_type, s.attrs) for s in refc.program.programs[0].streams] == [(s.pid, s.coding_type, s.attrs) for s in mine.program.programs[0].streams])
print("paquets : reference", len(ref) // 192, "| notre mux", len(out) // 192, "| debit clpi", refc.clip_info.ts_recording_rate, mine.clip_info.ts_recording_rate)
