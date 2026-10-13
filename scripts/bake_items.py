#!/usr/bin/env python3
# Baeckt items.bin: pro Item-ID (numerische Registry-ID aus dem Protokoll) den Itemnamen
# ohne "minecraft:"-Praefix und die maximale Stapelgroesse. Quellen sind die
# Datengenerator-Reports registries.json (IDs) und items.json (Default-Components), erzeugt
# mit java -DbundlerMainClass=net.minecraft.data.Main -jar <server.jar> --reports --
# derselbe Weg wie bei state_blocks.bin/state_props.bin.
#
# Die Stapelgroesse braucht der Client fuer die Klick-Vorhersage im Inventar (wie viel beim
# Zusammenlegen zweier Stapel tatsaechlich passt, s. app::inventory).
#
# Aufruf: python3 bake_items.py <registries.json> <items.json> <ausgabe-bin>
#
# Format (little-endian):
#   u32 item_count
#   je Item (Index = protocol_id, luecken- und reihenfolgetreu):
#     u16 len + utf8, dann u8 max_stack_size

import json
import struct
import sys


def main():
    if len(sys.argv) != 4:
        print("Aufruf: bake_items.py <registries.json> <items.json> <ausgabe.bin>", file=sys.stderr)
        sys.exit(1)

    with open(sys.argv[1]) as f:
        registries = json.load(f)
    with open(sys.argv[2]) as f:
        item_defs = json.load(f)

    entries = registries["minecraft:item"]["entries"]
    names = [None] * len(entries)
    for full_name, entry in entries.items():
        names[entry["protocol_id"]] = full_name.removeprefix("minecraft:")

    if any(n is None for n in names):
        print("Luecke in den Item-IDs -- Report unvollstaendig?", file=sys.stderr)
        sys.exit(1)

    out = bytearray()
    out += struct.pack("<I", len(names))
    for name in names:
        raw = name.encode("utf-8")
        components = item_defs.get(f"minecraft:{name}", {}).get("components", {})
        # Fehlt der Eintrag (sollte nicht vorkommen), gilt Vanillas Default 64.
        max_stack = int(components.get("minecraft:max_stack_size", 64))
        out += struct.pack("<H", len(raw)) + raw + struct.pack("<B", max(1, min(99, max_stack)))

    with open(sys.argv[3], "wb") as f:
        f.write(out)
    print(f"{len(names)} Items -> {sys.argv[3]} ({len(out)} Bytes)")


if __name__ == "__main__":
    main()
