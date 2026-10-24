#!/usr/bin/env python3
# Schneidet aus einem Einzelspieler-Spielstand die Szene fuer das Launcher-Hintergrundbild aus
# (s. crates/app/src/hero_scene.rs): Bloecke als Namen mit Eigenschaften, Biome samt ihren
# Farbwerten und das Himmelslicht, dazu die Kamera. Texturen stehen nicht darin -- die kommen
# beim Rendern aus der Installation des Spielers.
#
#   python3 -I scripts/hero_scene.py <welt> <client.jar> <x> <y> <z> <yaw> <pitch> <fov> <ausgabe>
#
# Ausgeschnitten wird lueckenlos jeder Chunk bis RADIUS Chunks um die Kamera, der vor ihr liegt
# (hoechstens HINTEN Chunks dahinter). Gezeichnet wird im Client nur bis RADIUS - 1 (s.
# Sichtweite im Format): so hat jeder gezeichnete Chunk Nachbarn, und Wasser bekommt am Rand der
# Szene keine Seitenwaende.
#
# Format (vor dem Deflate, Little-Endian):
#   b"CRSC", u32 Version 3
#   f64 x, y, z (Fuesse), f32 yaw, pitch, f32 Sichtfeld (Grad), i64 Tageszeit, u8 Sichtweite
#   u32 Anzahl Blockzustaende, je u16 Laenge + "name[k=v,...]"
#   u32 Anzahl Biome, je u16 Laenge + Name, f32 Temperatur, f32 Niederschlag, u32 Wasserfarbe,
#       u8 Niederschlag ja/nein
#   u32 Anzahl Spalten, je i32 cx, i32 cz, u8 Anzahl Sections, je:
#       i8 Section-Y, u16[4096] Blockindex (y, z, x), u8[64] Biomindex, u8 Himmelslicht da,
#       [2048] Himmelslicht (falls da)

import io
import json
import math
import struct
import sys
import zipfile
import zlib

RADIUS = 24
# Sections darunter sieht man von der Kamera aus nicht (Meeresboden liegt hoeher).
MIN_SECTION_Y = 1
HINTEN = 1.5
TAGESZEIT = 6000


def nbt(buf):
    r = io.BytesIO(buf)

    def lies(n):
        return r.read(n)

    def wert(typ):
        if typ == 1:
            return struct.unpack(">b", lies(1))[0]
        if typ == 2:
            return struct.unpack(">h", lies(2))[0]
        if typ == 3:
            return struct.unpack(">i", lies(4))[0]
        if typ == 4:
            return struct.unpack(">q", lies(8))[0]
        if typ == 5:
            return struct.unpack(">f", lies(4))[0]
        if typ == 6:
            return struct.unpack(">d", lies(8))[0]
        if typ == 7:
            n = struct.unpack(">i", lies(4))[0]
            return lies(n)
        if typ == 8:
            n = struct.unpack(">H", lies(2))[0]
            return lies(n).decode("utf-8", "replace")
        if typ == 9:
            et = lies(1)[0]
            n = struct.unpack(">i", lies(4))[0]
            return [wert(et) for _ in range(n)]
        if typ == 10:
            d = {}
            while True:
                t = lies(1)[0]
                if t == 0:
                    return d
                n = struct.unpack(">H", lies(2))[0]
                name = lies(n).decode("utf-8", "replace")
                d[name] = wert(t)
        if typ == 11:
            n = struct.unpack(">i", lies(4))[0]
            return list(struct.unpack(">%di" % n, lies(4 * n)))
        if typ == 12:
            n = struct.unpack(">i", lies(4))[0]
            return list(struct.unpack(">%dq" % n, lies(8 * n)))
        raise ValueError("NBT-Typ %d" % typ)

    t = lies(1)[0]
    n = struct.unpack(">H", lies(2))[0]
    lies(n)
    return wert(t)


def chunk(welt, cx, cz):
    pfad = "%s/region/r.%d.%d.mca" % (welt, cx >> 5, cz >> 5)
    try:
        f = open(pfad, "rb")
    except FileNotFoundError:
        return None
    with f:
        kopf = f.read(4096)
        i = 4 * ((cx & 31) + (cz & 31) * 32)
        ort = struct.unpack(">I", kopf[i:i + 4])[0]
        if ort == 0:
            return None
        f.seek((ort >> 8) * 4096)
        laenge, art = struct.unpack(">IB", f.read(5))
        roh = f.read(laenge - 1)
    if art != 2:
        raise ValueError("Kompression %d nicht unterstuetzt" % art)
    return nbt(zlib.decompress(roh))


def entpacken(daten, palette_laenge, anzahl, min_bits):
    if palette_laenge <= 1 or not daten:
        return [0] * anzahl
    bits = max(min_bits, math.ceil(math.log2(palette_laenge)))
    je_long = 64 // bits
    maske = (1 << bits) - 1
    out = []
    for lang in daten:
        lang &= (1 << 64) - 1
        for k in range(je_long):
            if len(out) == anzahl:
                break
            out.append((lang >> (k * bits)) & maske)
    return out


def zustand(eintrag):
    name = eintrag["Name"].removeprefix("minecraft:")
    props = eintrag.get("Properties", {})
    if not props:
        return name
    return name + "[" + ",".join("%s=%s" % (k, props[k]) for k in sorted(props)) + "]"


def biom_daten(jar, name):
    kurz = name.removeprefix("minecraft:")
    d = json.loads(jar.read("data/minecraft/worldgen/biome/%s.json" % kurz))
    # Die Farbe steht als Zahl oder als "#rrggbb" darin.
    wasser = d["effects"]["water_color"]
    wasser = int(wasser[1:], 16) if isinstance(wasser, str) else int(wasser)
    return (kurz, float(d["temperature"]), float(d["downfall"]), wasser, 1 if d["has_precipitation"] else 0)


def main():
    welt, jar_pfad = sys.argv[1], sys.argv[2]
    x, y, z, yaw, pitch, fov = (float(v) for v in sys.argv[3:9])
    ausgabe = sys.argv[9]
    jar = zipfile.ZipFile(jar_pfad)

    # Blickrichtung wie Vanilla: yaw 0 = +Z, 90 = -X.
    vx, vz = -math.sin(math.radians(yaw)), math.cos(math.radians(yaw))
    kx, kz = int(x) >> 4, int(z) >> 4
    spalten = set()
    for cx in range(kx - RADIUS, kx + RADIUS + 1):
        for cz in range(kz - RADIUS, kz + RADIUS + 1):
            dx, dz = cx - kx, cz - kz
            if dx * dx + dz * dz > RADIUS * RADIUS:
                continue
            # Abstand vor der Kamera, gemessen an der Chunkmitte, in Chunks.
            vor = ((cx * 16 + 8 - x) * vx + (cz * 16 + 8 - z) * vz) / 16
            if vor >= -HINTEN:
                spalten.add((cx, cz))

    zustaende, zustand_index = [], {}
    biome, biom_index = [], {}

    def zi(s):
        if s not in zustand_index:
            zustand_index[s] = len(zustaende)
            zustaende.append(s)
        return zustand_index[s]

    def bi(name):
        if name not in biom_index:
            biom_index[name] = len(biome)
            biome.append(biom_daten(jar, name))
        return biom_index[name]

    zi("air")
    ausgabe_spalten = []
    for (cx, cz) in sorted(spalten):
        c = chunk(welt, cx, cz)
        if c is None or c.get("Status", "").removeprefix("minecraft:") != "full":
            continue
        secs = []
        for s in c.get("sections", []):
            bs = s.get("block_states")
            if not bs or s["Y"] < MIN_SECTION_Y:
                continue
            pal = [zi(zustand(e)) for e in bs["palette"]]
            if len(pal) == 1 and pal[0] == 0 and "SkyLight" not in s:
                continue
            idx = entpacken(bs.get("data", []), len(pal), 4096, 4)
            bl = [pal[i] for i in idx]
            bio = s.get("biomes", {"palette": ["minecraft:plains"]})
            bpal = [bi(n) for n in bio["palette"]]
            bidx = entpacken(bio.get("data", []), len(bpal), 64, 1)
            himmel = s.get("SkyLight")
            secs.append((s["Y"], bl, [bpal[i] for i in bidx], himmel))
        ausgabe_spalten.append((cx, cz, secs))

    w = io.BytesIO()
    w.write(b"CRSC" + struct.pack("<I", 3))
    w.write(struct.pack("<dddfffqB", x, y, z, yaw, pitch, fov, TAGESZEIT, RADIUS - 1))

    def text(s):
        b = s.encode()
        w.write(struct.pack("<H", len(b)) + b)

    w.write(struct.pack("<I", len(zustaende)))
    for s in zustaende:
        text(s)
    w.write(struct.pack("<I", len(biome)))
    for (name, temp, nieder, wasser, regen) in biome:
        text(name)
        w.write(struct.pack("<ffIB", temp, nieder, wasser, regen))
    w.write(struct.pack("<I", len(ausgabe_spalten)))
    for (cx, cz, secs) in ausgabe_spalten:
        w.write(struct.pack("<iiB", cx, cz, len(secs)))
        for (sy, bl, bio, himmel) in secs:
            w.write(struct.pack("<b", sy))
            w.write(struct.pack("<4096H", *bl))
            w.write(bytes(bio))
            if himmel is not None and len(himmel) == 2048:
                w.write(b"\x01" + bytes(himmel))
            else:
                w.write(b"\x00")
    roh = w.getvalue()
    gepackt = zlib.compress(roh, 9)
    with open(ausgabe, "wb") as f:
        f.write(gepackt)
    print("%d Spalten, %d Zustaende, %d Biome, %d -> %d Byte" % (len(ausgabe_spalten), len(zustaende), len(biome), len(roh), len(gepackt)))


main()
