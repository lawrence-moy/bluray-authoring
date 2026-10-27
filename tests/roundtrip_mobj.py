"""Usage : python -m tests.roundtrip_mobj MovieObject.bdmv [num_objet ...]"""
import sys
from bdauthor.mobj import MovieObjectBdmv

data = open(sys.argv[1], "rb").read()
m = MovieObjectBdmv.parse(data)
only = {int(x) for x in sys.argv[2:]} or None
print(m.dump(only))
out = m.pack()
print("\naller-retour identique :", out == data, f"({len(out)} / {len(data)} octets)")
