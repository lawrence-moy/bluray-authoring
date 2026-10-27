"""Usage : python -m tests.gen_clpi 00000.m2ts [00000.clpi_de_reference]

Analyse le clip, genere son .clpi et le compare au .clpi de reference (octet pour octet,
avec le remplissage '0' de multiAVCHD, puis structure seule avec remplissage a zero).
"""
import sys
from bdauthor.tsclip import scan_clip, make_clip_clpi
from bdauthor.clpi import Clpi

data = open(sys.argv[1], "rb").read()
sc = scan_clip(data)
print(f"{sc.num_packets} paquets, PCR PID {sc.pcr_pid:#x}, PMT PID {sc.pmt_pid:#x}, video PID {sc.video[0]:#x}, "
      f"audio {[hex(p) for p, _ in sc.audio]}")
print(f"video {sc.width}x{sc.height} {'p' if sc.progressive else 'i'} {sc.fps} i/s ; "
      f"sequence {sc.start90 / 90000:.3f}s -> {sc.end90 / 90000:.3f}s ; {len(sc.eps)} points d'acces")
if len(sys.argv) > 2:
    ref = open(sys.argv[2], "rb").read()
    mine = make_clip_clpi(data, pad=b"0")
    print("clpi genere (remplissage '0' de multiAVCHD) identique a la reference :", mine == ref)
    if mine != ref:
        i = next((i for i, (a, b) in enumerate(zip(mine, ref)) if a != b), None)
        print("  tailles", len(mine), len(ref), "; premiere difference :", None if i is None else hex(i))
    clean = make_clip_clpi(data)
    a, b = Clpi.parse(clean), Clpi.parse(ref)
    strip = lambda c: (c.clip_info, c.sequence, c.cpi, [(s.pid, s.coding_type, s.attrs[:4]) for s in c.program.programs[0].streams])
    print("meme structure avec remplissage a zero :", strip(a) == strip(b), "| taille", len(clean))
