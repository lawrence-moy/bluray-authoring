"""Verificateur de dossier BDMV : controle les references croisees entre fichiers (sans lecteur).

Usage : python -m bdauthor.check chemin/vers/BDMV

Controles : index -> objets ; commandes des objets (playlists, titres, objets, sauts) a operandes immediats ;
playlists -> clips (.clpi et .m2ts presents), bornes de temps dans la sequence du clip, flux declares (PID, types) ;
.clpi -> taille du .m2ts, EP_map ; .m2ts -> structure (paquets, PAT/PMT) ; copies BACKUP identiques.
Les commandes a operandes en registre ne sont pas suivies (signalees « indirect »).
"""
import os, sys
from .index import IndexBdmv
from .mobj import MovieObjectBdmv
from .mpls import Mpls
from .clpi import Clpi
from .m2ts import iter_packets, reassemble, split_segments
from .ig import Ics


class Report:
    def __init__(self):
        self.items = []

    def err(self, m): self.items.append(("ERREUR", m))
    def warn(self, m): self.items.append(("avertissement", m))
    def info(self, m): self.items.append(("ok", m))

    @property
    def ok(self):
        return not any(k == "ERREUR" for k, _ in self.items)


def _read(root, rel):
    p = os.path.join(root, rel)
    return open(p, "rb").read() if os.path.isfile(p) else None


def check_bdmv(root):
    r = Report()
    idx_b, mo_b = _read(root, "index.bdmv"), _read(root, "MovieObject.bdmv")
    if idx_b is None or mo_b is None:
        r.err("index.bdmv ou MovieObject.bdmv absent")
        return r
    index, mobj = IndexBdmv.parse(idx_b), MovieObjectBdmv.parse(mo_b)
    n_obj, n_titles = len(mobj.objects), len(index.titles)
    playlists = sorted(f[:-5] for f in os.listdir(os.path.join(root, "PLAYLIST")) if f.endswith(".mpls")) \
        if os.path.isdir(os.path.join(root, "PLAYLIST")) else []
    r.info(f"{n_titles} titre(s), {n_obj} objet(s), {len(playlists)} playlist(s)")

    # index -> objets
    for name, o in (("First Playback", index.first_playback), ("Top Menu", index.top_menu)):
        if o.id_ref >= n_obj:
            r.err(f"{name} -> objet {o.id_ref} inexistant")
    for i, t in enumerate(index.titles, 1):
        if t.id_ref >= n_obj:
            r.err(f"titre {i} -> objet {t.id_ref} inexistant")

    # objets -> playlists / titres / objets
    def check_cmds(where, cmds):
        for j, c in enumerate(cmds):
            name = c.text().split()[0]
            imm = c.imm_op1
            if name in ("PLAY_PL", "PLAY_PL_PI", "PLAY_PL_PM"):
                if imm:
                    if f"{c.dst:05d}" not in playlists:
                        r.err(f"{where} commande {j} {name} {c.dst} : {c.dst:05d}.mpls absente")
                else:
                    r.warn(f"{where} commande {j} {name} : playlist indirecte (registre), non suivie")
            elif name in ("JUMP_TITLE", "CALL_TITLE") and imm and c.dst > n_titles and c.dst != 0xFFFF:
                r.err(f"{where} commande {j} {name} {c.dst} : seulement {n_titles} titre(s)")
            elif name in ("JUMP_OBJECT", "CALL_OBJECT") and imm and c.dst >= n_obj:
                r.err(f"{where} commande {j} {name} {c.dst} : objet inexistant")
            elif name == "GOTO" and imm and c.dst >= len(cmds):
                r.err(f"{where} commande {j} GOTO {c.dst} : hors de l'objet")
    for i, o in enumerate(mobj.objects):
        check_cmds(f"objet {i}", o.commands)

    # clips
    clips = {}
    def load_clip(num):
        if num not in clips:
            clpi, m2ts = _read(root, f"CLIPINF/{num}.clpi"), os.path.join(root, f"STREAM/{num}.m2ts")
            if clpi is None:
                r.err(f"clip {num} : {num}.clpi absent")
                clips[num] = None
            elif not os.path.isfile(m2ts):
                r.err(f"clip {num} : {num}.m2ts absent")
                clips[num] = None
            else:
                clips[num] = (Clpi.parse(clpi), os.path.getsize(m2ts), m2ts)
        return clips[num]

    # playlists
    for pl in playlists:
        m = Mpls.parse(_read(root, f"PLAYLIST/{pl}.mpls"))
        for k, it in enumerate(m.playitems):
            c = load_clip(it.clip)
            if not c:
                continue
            clpi = c[0]
            seq = clpi.sequence.atc[0].stc[0]
            if not (seq.start_time <= it.in_time < it.out_time <= seq.end_time):
                r.err(f"playlist {pl} item {k} : temps {it.in_time}-{it.out_time} hors de la sequence du clip {it.clip} ({seq.start_time}-{seq.end_time})")
            pids = {s.pid: s.coding_type for s in clpi.program.programs[0].streams}
            stn = it.tail[12 + 2 + 2 + 12:]   # entrees de la table des flux
            pos = 0
            while pos + 2 < len(stn):
                ln = stn[pos]
                if ln == 0:
                    break
                typ = stn[pos + 1]
                pid = int.from_bytes(stn[pos + 2:pos + 4], "big") if typ == 1 else int.from_bytes(stn[pos + 4:pos + 6], "big")
                alen = stn[pos + 1 + ln]
                coding = stn[pos + 2 + ln]
                if typ == 1:
                    if pids.get(pid) != coding:
                        r.err(f"playlist {pl} item {k} : flux PID {pid:#x} type {coding:#x} absent du clip {it.clip}")
                pos += 2 + ln + alen
        for sp in m.subpaths:
            for si in sp.items:
                c = load_clip(si.clip)
                if c:
                    seq = c[0].sequence.atc[0].stc[0]
                    if not (seq.start_time <= si.in_time < si.out_time <= seq.end_time):
                        r.err(f"playlist {pl} sous-chemin : temps {si.in_time}-{si.out_time} hors de la sequence du clip {si.clip} ({seq.start_time}-{seq.end_time})")
        if not m.marks:
            r.warn(f"playlist {pl} : aucune marque de chapitre")
        elif m.playitems and clips.get(m.playitems[0].clip):
            it0, c0 = m.playitems[0], clips[m.playitems[0].clip][0]
            eps = [p for st in (c0.cpi.streams if c0.cpi else []) for p, _ in st.entries()]   # temps de l'EP_map : arrondis a 512 ticks (90 kHz)
            times = [k.time for k in m.marks]
            if times != sorted(times):
                r.err(f"playlist {pl} : marques de chapitre non croissantes")
            for k, mk in enumerate(m.marks):
                if not it0.in_time <= mk.time <= it0.out_time:
                    r.err(f"playlist {pl} marque {k} : temps {mk.time} hors du PlayItem ({it0.in_time}-{it0.out_time})")
                elif eps and mk.time != it0.in_time and not any(0 <= mk.time * 2 - e < 1024 for e in eps):
                    r.warn(f"playlist {pl} marque {k} : pas sur une image d'acces de l'EP_map")

    # clips -> structure
    for num, c in clips.items():
        if not c:
            continue
        clpi, size, path = c
        if size != clpi.clip_info.num_source_packets * 192:
            r.err(f"clip {num} : {size} octets mais le .clpi annonce {clpi.clip_info.num_source_packets} paquets")
        if size % 6144:
            r.warn(f"clip {num} : taille non multiple de 6144 octets")
        pids = set()
        last_ats, bad_sync = -1, 0
        data = open(path, "rb").read()
        for p in iter_packets(data):
            pids.add(p.pid)
        declared = {s.pid for s in clpi.program.programs[0].streams}
        if not {0, clpi.program.programs[0].pmt_pid, clpi.sequence.atc[0].stc[0].pcr_pid} <= pids:
            r.err(f"clip {num} : PAT, PMT ou PCR absents du flux")
        if not declared <= pids:
            r.err(f"clip {num} : flux declares {sorted(map(hex, declared - pids))} absents du .m2ts")
        if clpi.cpi:
            for st in clpi.cpi.streams:
                spns = [spn for _, spn in st.entries()]
                if spns != sorted(spns) or (spns and spns[-1] >= clpi.clip_info.num_source_packets):
                    r.err(f"clip {num} : EP_map incoherente (SPN non croissants ou hors du fichier)")
        r.info(f"clip {num} : {size // 192} paquets, {len(declared)} flux, EP_map {'oui' if clpi.cpi else 'non'}")

    # menus IG : commandes et voisins des boutons
    for num, c in clips.items():
        if not c:
            continue
        ig = [s_.pid for s_ in c[0].program.programs[0].streams if s_.coding_type == 0x91]
        if not ig:
            continue
        segs = [sg for e in reassemble(open(c[2], "rb").read(), ig[0]) for sg in split_segments(e.data)]
        ics_seg = next((sg for sg in segs if sg.name == "ICS"), None)
        if not ics_seg:
            r.err(f"clip {num} : flux IG sans ICS")
            continue
        ics = Ics.parse(ics_seg.payload)
        n_btn = 0
        for page in ics.pages:
            ids = {b.button_id for g in page.bogs for b in g.buttons}
            for g in page.bogs:
                for b in g.buttons:
                    n_btn += 1
                    check_cmds(f"menu {num} bouton {b.button_id}", b.commands)
                    for nb in (b.upper, b.lower, b.left, b.right):
                        if nb not in ids:
                            r.err(f"menu {num} bouton {b.button_id} : voisin {nb} inexistant")
            if page.default_selected not in ids and page.default_selected != 0xFFFF:
                r.err(f"menu {num} : bouton selectionne par defaut {page.default_selected} inexistant")
        r.info(f"menu {num} : {n_btn} bouton(s), commandes verifiees")

    # BACKUP
    for rel in ("index.bdmv", "MovieObject.bdmv"):
        if _read(root, rel) != _read(root, "BACKUP/" + rel):
            r.warn(f"BACKUP/{rel} absent ou different")
    for sub, ext in (("PLAYLIST", ".mpls"), ("CLIPINF", ".clpi")):
        d = os.path.join(root, sub)
        for f in sorted(os.listdir(d)) if os.path.isdir(d) else []:
            if f.endswith(ext) and _read(root, f"{sub}/{f}") != _read(root, f"BACKUP/{sub}/{f}"):
                r.warn(f"BACKUP/{sub}/{f} absent ou different")
    return r


def main(argv=None):
    root = (argv or sys.argv)[-1]
    rep = check_bdmv(root)
    for kind, msg in rep.items:
        print(f"[{kind}] {msg}")
    print("RESULTAT :", "aucune erreur" if rep.ok else "erreurs detectees")
    return 0 if rep.ok else 1


if __name__ == "__main__":
    sys.exit(main())
