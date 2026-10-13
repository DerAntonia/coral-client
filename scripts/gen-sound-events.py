#!/usr/bin/env python3
"""Liest die Sound-Event-Registry aus javap-Bytecode und schreibt sie als Rust-Tabelle.

Wird von gen-sound-events.sh mit der javap-Ausgabe auf stdin aufgerufen; das Ziel steht im
ersten Argument. Warum das ueberhaupt gebraucht wird, steht im Kopf des Shell-Skripts.

ECHTER BUG GEWESEN, und zwar ein besonders unangenehmer: die erste Fassung hat nur Namen
gesammelt, die als Zeichenkette DIREKT vor einem register()-Aufruf stehen. Zwei Bloecke werden
aber nicht so registriert, sondern in Hilfsmethoden per Schleife (Ziegenhoerner und
Wolfsvarianten). Sie fehlten also -- und weil eine fehlende Registrierung ALLES DAHINTER
verschiebt, kamen im Spiel Namen aus der Nachbarschaft heraus: beim Schwertkampf spielten
Wasser- und Zombie-Geraeusche. Vorne stimmte es noch, hinten immer weniger.

Deshalb bricht dieses Skript jetzt ab, sobald im Initialisierer eine EIGENE Methode
aufgerufen wird, die es nicht kennt. Lieber gar keine Tabelle als eine verschobene: eine
verschobene faellt nicht als Fehler auf, sie klingt nur falsch.
"""

import json
import os
import re
import sys

# --- Die beiden Bloecke, die per Schleife registriert werden -------------------------------
#
# Beide sind aus dem Bytecode abgelesen, nicht geraten:
#   registerGoatHornSounds()  = IntStream.range(0, 8) -> "item.goat_horn.sound.\1"
#   createWolfSoundMap()      = Stream.of(WolfSoundVariants.Type.values()) -> je 6 Klaenge
#                               "entity.wolf\1.{ambient,death,growl,hurt,pant,whine}"
# Die Suffixe stehen in der Enum-Reihenfolge von WolfSoundVariants$Type; CLASSIC hat als
# einziger ein LEERES Suffix (also "entity.wolf.ambient", nicht "entity.wolf_classic.ambient").

GOAT_HORN = ["item.goat_horn.sound.%d" % i for i in range(8)]

WOLF_SUFFIXE = ["", "_puglin", "_sad", "_angry", "_grumpy", "_big", "_cute"]
WOLF_KLAENGE = ["ambient", "death", "growl", "hurt", "pant", "whine"]
WOLF = ["entity.wolf%s.%s" % (s, k) for s in WOLF_SUFFIXE for k in WOLF_KLAENGE]

# Eigene Methoden des Initialisierers, die Klaenge registrieren, ohne dass ihr Name als
# Zeichenkette dasteht. Alles, was hier NICHT steht, laesst das Skript abbrechen.
HELFER = {
    "registerGoatHornSounds": GOAT_HORN,
    "createWolfSoundMap": WOLF,
}

# Diese beiden registrieren je genau einen Klang und nehmen den Namen als Argument.
DIREKT = ("register", "registerReference")


def sammle(zeilen: list[str]) -> list[str]:
    """Den statischen Initialisierer in Reihenfolge durchgehen und die Namen einsammeln."""
    try:
        start = next(n for n, l in enumerate(zeilen) if l.strip() == "static {};")
    except StopIteration:
        sys.exit("Kein statischer Initialisierer in der javap-Ausgabe -- falsches Jar?")

    namen: list[str] = []
    offen: str | None = None
    for zeile in zeilen[start:]:
        m = re.search(r"ldc(?:_w)?\s+#\d+\s+// String (.*)$", zeile)
        if m:
            offen = m.group(1)
            continue

        # Aufruf einer EIGENEN Methode: in javap steht dann kein Klassenpfad davor
        # (fremde Klassen erscheinen als "// Method net/minecraft/.../Foo.bar:(...)").
        aufruf = re.search(r"invokestatic\s+#\d+\s+// Method ([\w$]+):\(", zeile)
        if not aufruf:
            continue
        methode = aufruf.group(1)

        if methode in DIREKT:
            if offen is None:
                sys.exit(f"register-Aufruf ohne vorangehenden Namen -- Format geaendert? ({zeile.strip()})")
            namen.append(offen)
            offen = None
        elif methode in HELFER:
            namen.extend(HELFER[methode])
            offen = None
        else:
            sys.exit(
                f"Unbekannte Hilfsmethode '{methode}' im Initialisierer von SoundEvents.\n"
                "Sie registriert moeglicherweise weitere Klaenge. Solange das nicht geklaert ist,\n"
                "waere die erzeugte Tabelle AB DIESER STELLE verschoben -- im Spiel klingt das\n"
                "nach falschen, aber plausiblen Geraeuschen und faellt sonst nirgends auf.\n"
                "Die Methode im Jar ansehen und in HELFER eintragen."
            )
    return namen


def gegenprobe(namen: list[str]) -> None:
    """Gegen die echte sounds.json pruefen, falls eine lokale Installation da ist.

    Das ist die Kontrolle, die den urspruenglichen Fehler sofort gezeigt haette: jeder
    registrierte Klang (ausser dem absichtlich leeren) hat einen Eintrag in sounds.json, und
    umgekehrt bleibt dort nichts uebrig. Blieben 50 Eintraege uebrig, fehlten genau die 50
    Klaenge der beiden Schleifen.
    """
    pfad = os.path.expanduser("~/.minecraft/assets")
    try:
        with open(os.path.join(pfad, "indexes", "29.json")) as f:
            index = json.load(f)["objects"]
        h = index["minecraft/sounds.json"]["hash"]
        with open(os.path.join(pfad, "objects", h[:2], h)) as f:
            sounds_json = json.load(f)
    except (OSError, KeyError, ValueError):
        print("Hinweis: keine lokale sounds.json gefunden -- Gegenprobe uebersprungen.", file=sys.stderr)
        return

    ohne_datei = [n for n in namen if n not in sounds_json and n != "intentionally_empty"]
    uebrig = [k for k in sounds_json if k not in set(namen)]
    print(f"Gegenprobe: {len(sounds_json)} Eintraege in sounds.json", file=sys.stderr)
    if ohne_datei:
        print(f"  WARNUNG: {len(ohne_datei)} registrierte Klaenge ohne Eintrag: {ohne_datei[:5]}", file=sys.stderr)
    if uebrig:
        print(
            f"  WARNUNG: {len(uebrig)} Eintraege in sounds.json haben KEINEN Registry-Namen:\n"
            f"  {uebrig[:8]}\n"
            "  Genau so sah der Verschiebungsfehler aus -- vermutlich fehlt eine Hilfsmethode.",
            file=sys.stderr,
        )
    if not ohne_datei and not uebrig:
        print("  in Ordnung: beide Seiten decken sich vollstaendig.", file=sys.stderr)


def main() -> None:
    if len(sys.argv) != 2:
        sys.exit("Aufruf: javap ... | gen-sound-events.py <ziel.rs>")
    ziel = sys.argv[1]

    namen = sammle(sys.stdin.read().split("\n"))

    # Zwei Plausibilitaetsschranken, damit ein stillschweigend kaputtes Format nicht als
    # gueltige (aber verschobene) Tabelle durchgeht.
    if len(namen) < 1000:
        sys.exit(f"Nur {len(namen)} Sound-Events gefunden -- Format geaendert?")
    if len(set(namen)) != len(namen):
        doppelt = sorted({n for n in namen if namen.count(n) > 1})
        sys.exit(f"Doppelte Namen in der Registry: {doppelt[:5]} -- Extraktion fehlerhaft.")

    gegenprobe(namen)

    with open(ziel, "w") as f:
        f.write("// ERZEUGT von scripts/gen-sound-events.sh -- nicht von Hand aendern.\n")
        f.write("//\n")
        f.write("// Die Namen der Sound-Events in REGISTRY-REIHENFOLGE (Reihenfolge der register()-\n")
        f.write("// Aufrufe in net.minecraft.sound.SoundEvents, 1.21.11). Der Index in dieser Tabelle\n")
        f.write("// IST die Zahl, die im Sound-Paket steht -- s. sound_event_name().\n")
        f.write("//\n")
        f.write("// Enthalten sind auch die beiden Bloecke, die Vanilla per SCHLEIFE registriert\n")
        f.write("// (Ziegenhoerner, Wolfsvarianten) -- sie stehen an genau der Stelle, an der ihre\n")
        f.write("// Hilfsmethode aufgerufen wird. Fehlten sie, waere alles dahinter verschoben.\n")
        f.write("//\n")
        f.write("// `static`, nicht `const`: ein const-Array dieser Groesse wuerde an JEDER\n")
        f.write("// Verwendungsstelle als eigene Kopie eingesetzt.\n")
        f.write("#[rustfmt::skip]\n")
        f.write("pub static SOUND_EVENTS: [&str; %d] = [\n" % len(namen))
        for n in namen:
            escaped = n.replace("\\", "\\\\").replace('"', '\\"')
            f.write('    "%s",\n' % escaped)
        f.write("];\n")

    print(f"{len(namen)} Sound-Events -> {ziel}", file=sys.stderr)


if __name__ == "__main__":
    main()
