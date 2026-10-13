#!/usr/bin/env python3
# Baeckt state_props.bin: pro State-ID die Blockstate-Properties (z.B. half=top,
# waterlogged=true) aus dem Datengenerator-Report blocks.json. state_blocks.bin (s.
# block_registry.rs) kennt nur den Blocknamen pro State -- fuer die Formauswahl
# (Blockstate-Variante bzw. Multipart-"when"-Klauseln) braucht es aber die echten
# Property-Werte, die der Server im State mitschickt.
#
# Aufruf: python3 bake_state_props.py <pfad-zu-blocks.json> <ausgabe-bin>
#
# Format (little-endian):
#   u32 key_count,   je: u16 len + utf8      -- eindeutige Property-Namen ("half", ...)
#   u32 value_count, je: u16 len + utf8      -- eindeutige Property-Werte ("top", "true", ...)
#   u32 state_count
#   je State (Index = State-ID, luecken- und reihenfolgetreu):
#     u8 prop_count
#     je Property: u16 key_index, u16 value_index

import json
import struct
import sys


def main():
    if len(sys.argv) != 3:
        print("Aufruf: bake_state_props.py <blocks.json> <ausgabe.bin>", file=sys.stderr)
        sys.exit(1)

    with open(sys.argv[1]) as f:
        blocks = json.load(f)

    max_id = 0
    for b in blocks.values():
        for s in b["states"]:
            max_id = max(max_id, s["id"])
    state_count = max_id + 1

    state_props = [None] * state_count
    for b in blocks.values():
        for s in b["states"]:
            state_props[s["id"]] = s.get("properties", {})

    keys = {}
    values = {}

    def intern(table, s):
        if s not in table:
            table[s] = len(table)
        return table[s]

    encoded_states = []
    for props in state_props:
        props = props or {}
        pairs = []
        for k, v in props.items():
            ki = intern(keys, k)
            vi = intern(values, v)
            pairs.append((ki, vi))
        encoded_states.append(pairs)

    def sorted_strings(table):
        out = [None] * len(table)
        for s, i in table.items():
            out[i] = s
        return out

    key_list = sorted_strings(keys)
    value_list = sorted_strings(values)

    with open(sys.argv[2], "wb") as out:
        out.write(struct.pack("<I", len(key_list)))
        for s in key_list:
            b = s.encode("utf-8")
            out.write(struct.pack("<H", len(b)))
            out.write(b)

        out.write(struct.pack("<I", len(value_list)))
        for s in value_list:
            b = s.encode("utf-8")
            out.write(struct.pack("<H", len(b)))
            out.write(b)

        out.write(struct.pack("<I", state_count))
        for pairs in encoded_states:
            out.write(struct.pack("<B", len(pairs)))
            for ki, vi in pairs:
                out.write(struct.pack("<HH", ki, vi))

    print(f"{state_count} States, {len(key_list)} Property-Namen, {len(value_list)} Werte -> {sys.argv[2]}")


if __name__ == "__main__":
    main()
