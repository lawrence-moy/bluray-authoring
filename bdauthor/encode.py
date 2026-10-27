"""Encodage d'une video quelconque en flux Blu-ray (ffmpeg + libx264 + AC3) puis multiplexage M2TS (vmux.py).

Cibles gerees : 720p a 59,94 ou 50 i/s, 720p 23,976 / 24, 1080p 23,976 / 24. Autres sources : converties vers la cible la
plus proche (la cadence est changee par doublement ou suppression d'images, jamais par interpolation). Seul 720p59,94 a ete
verifie.
"""
import json, os, subprocess, tempfile
from .m2ts import iter_packets, reassemble
from .tsclip import parse_psi, nal_units, parse_sps
from .vmux import mux_film, AC3_WORDS_48K, AC3_FRAME_TICKS

START_PTS = 54_000_000          # 600 s, comme le film de reference


def probe(src):
    r = subprocess.run(["ffprobe", "-v", "error", "-print_format", "json", "-show_streams", "-show_format", src],
                       capture_output=True, text=True, check=True)
    return json.loads(r.stdout)


def pick_target(width, height, fps):
    """(largeur, hauteur, cadence ffmpeg, libelle) pour une source."""
    if height > 800 and fps < 26:
        return 1920, 1080, "24000/1001" if abs(fps - 23.976) < abs(fps - 24) else "24", "1080p"
    if fps >= 55:
        return 1280, 720, "60000/1001", "720p59.94"
    if fps >= 45:
        return 1280, 720, "50", "720p50"
    if fps >= 26:
        return 1280, 720, "60000/1001", "720p59.94 (cadence doublee)"
    return 1280, 720, "24000/1001" if abs(fps - 23.976) < abs(fps - 24) else "24", "720p24"


def _x264_args(crf, maxrate, keyint, preset):
    x264 = (f"bluray-compat=1:keyint={keyint}:open-gop=0:aud=1:nal-hrd=vbr:vbv-maxrate={maxrate}:vbv-bufsize=30000:"
            f"level=4.1:bframes=3")
    return ["-c:v", "libx264", "-profile:v", "high", "-preset", preset, "-crf", str(crf), "-x264-params", x264,
            "-color_primaries", "bt709", "-color_trc", "bt709", "-colorspace", "bt709", "-color_range", "tv"]


def encode_to_ts(src, dst, crf=18, maxrate=30000, keyint=30, audio_bitrate="192k", preset="medium", info=None):
    info = info or probe(src)
    v = next(s for s in info["streams"] if s["codec_type"] == "video")
    num, den = (int(x) for x in v["r_frame_rate"].split("/"))
    w, h, fps, label = pick_target(int(v["width"]), int(v["height"]), num / den)
    full = v.get("pix_fmt", "").startswith("yuvj") or v.get("color_range") == "pc"
    vf = (f"fps={fps},scale={w}:{h}:in_range={'pc' if full else 'tv'}:in_color_matrix=bt709:"
          f"out_range=tv:out_color_matrix=bt709,format=yuv420p")
    cmd = ["ffmpeg", "-y", "-v", "error", "-i", src, "-map", "0:v:0", "-map", "0:a:0", "-vf", vf,
           *_x264_args(crf, maxrate, keyint, preset),
           "-c:a", "ac3", "-b:a", audio_bitrate, "-ar", "48000", "-ac", "2",
           "-f", "mpegts", "-pes_payload_size", "0", dst]
    subprocess.run(cmd, check=True)
    return label


def _as_source_packets(path):
    raw = open(path, "rb").read()
    return b"".join(b"\x00\x00\x00\x00" + raw[i:i + 188] for i in range(0, len(raw) - len(raw) % 188, 188))


def ac3_frames(payload):
    """Decoupe un bloc de donnees AC3 en trames (sync 0B77, taille d'apres frmsizecod, 48 kHz)."""
    out, pos = [], 0
    while pos + 7 <= len(payload):
        if payload[pos:pos + 2] != b"\x0b\x77":
            raise ValueError("synchro AC3 absente")
        if payload[pos + 4] >> 6 != 0:
            raise ValueError("seul 48 kHz est gere")
        n = AC3_WORDS_48K[payload[pos + 4] & 0x3F] * 2
        out.append(payload[pos:pos + n])
        pos += n
    return out


def ac3_descriptor(frame):
    """Descripteur AC-3 de la PMT (tag 0x81) d'apres l'en-tete de la premiere trame."""
    fscod, frmsizecod, bsid = frame[4] >> 6, frame[4] & 0x3F, frame[5] >> 3
    bsmod, acmod = frame[5] & 7, frame[6] >> 5
    bit = 6 * 8 + 3
    bits = bin(int.from_bytes(frame[6:10], "big"))[2:].zfill(32)
    pos = 3
    dsurmod = 0
    if acmod & 1 and acmod != 1:
        pos += 2
    if acmod & 4:
        pos += 2
    if acmod == 2:
        dsurmod = int(bits[pos:pos + 2], 2)
        pos += 2
    lfeon = int(bits[pos])
    channels = acmod + (8 if lfeon else 0) if acmod != 0 else 0
    return bytes([0x81, 4, (fscod << 5) | bsid, ((frmsizecod >> 1) << 2) | dsurmod, (bsmod << 5) | (channels << 1), 0])


def avc_descriptor(sps_nal):
    return bytes([0x28, 4, sps_nal[1], sps_nal[2], sps_nal[3], 0xBF])


def load_encoded(ts_path):
    data = _as_source_packets(ts_path)
    _, _, streams = parse_psi(data)
    vpid = next(p for t, p in streams if t == 0x1B)
    apid = next((p for t, p in streams if t == 0x81), None)
    video, audio, sps = [], [], None
    for e in reassemble(data, vpid):
        video.append((e.pts, e.dts, e.data))
        if sps is None:
            for t, off, ln in nal_units(e.data):
                if t == 7:
                    sps = e.data[off:off + ln]
    for e in (reassemble(data, apid) if apid is not None else []):
        for k, fr in enumerate(ac3_frames(e.data)):
            audio.append((e.pts + k * AC3_FRAME_TICKS, fr))
    shift = START_PTS - min([p for p, _, _ in video] + [p for p, _ in audio])
    video = [(p + shift, None if d is None else d + shift, x) for p, d, x in video]
    video.sort(key=lambda a: (a[1] if a[1] is not None else a[0]))
    audio = [(p + shift, x) for p, x in audio]
    return video, audio, sps


def make_film_m2ts(src, lang=b"und", workdir=None, **opts):
    """Video quelconque -> contenu d'un .m2ts Blu-ray. Retourne (octets, informations)."""
    info = probe(src)
    with tempfile.TemporaryDirectory(dir=workdir) as tmp:
        ts = os.path.join(tmp, "enc.ts")
        label = encode_to_ts(src, ts, info=info, **opts)
        video, audio, sps = load_encoded(ts)
    m2ts, stats = _mux(video, audio, sps, lang)
    stats.update(cible=label, images=len(video), trames_audio=len(audio))
    return m2ts, stats


def _mux(video, audio, sps, lang):
    streams = [(0x1B, 0x1011, avc_descriptor(sps))]
    if audio:
        streams.append((0x81, 0x1100, bytes.fromhex("050441432d33") + ac3_descriptor(audio[0][1])
                        + bytes([0x0A, 4]) + lang + b"\x00"))
    return mux_film(video, audio, streams)


def make_still_m2ts(image, seconds, size, fps="60000/1001", crf=18, preset="medium", workdir=None):
    """Image fixe -> clip video seul (pas d'audio) de `seconds` secondes, au format `size` (largeur, hauteur)."""
    with tempfile.TemporaryDirectory(dir=workdir) as tmp:
        ts = os.path.join(tmp, "still.ts")
        vf = f"scale={size[0]}:{size[1]}:out_color_matrix=bt709:out_range=tv,format=yuv420p"
        cmd = ["ffmpeg", "-y", "-v", "error", "-loop", "1", "-framerate", fps, "-t", str(seconds), "-i", image,
               "-vf", vf, *_x264_args(crf, 30000, 30, preset), "-an", "-f", "mpegts", ts]
        subprocess.run(cmd, check=True)
        video, audio, sps = load_encoded(ts)
    m2ts, stats = _mux(video, audio, sps, b"und")
    stats.update(cible=f"{size[0]}x{size[1]} fixe", images=len(video), trames_audio=0)
    return m2ts, stats


def extract_thumbnail(src, dst_png, size, at=None):
    """Une image de la video, redimensionnee et rognee (sans deformation) a `size`."""
    dur = float(probe(src)["format"].get("duration", 0))
    t = at if at is not None else min(1.0, dur * 0.25)
    vf = f"scale={size[0]}:{size[1]}:force_original_aspect_ratio=increase,crop={size[0]}:{size[1]}"
    subprocess.run(["ffmpeg", "-y", "-v", "error", "-ss", f"{t:.3f}", "-i", src, "-frames:v", "1", "-vf", vf, dst_png], check=True)
