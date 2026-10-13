#!/usr/bin/env bash
# Erzeugt crates/protocol/src/sound_events.rs aus dem lokalen Yarn-gemappten Client-Jar.
#
# Warum das ueberhaupt gebraucht wird: die Sound-Pakete nennen ihren Klang als ZAHL, nicht als
# Name -- als Index in die Sound-Event-Registry. Diese Registry schickt der Server NICHT mit
# (sie gilt als statisch, anders als Biome oder Dimensionen), der Client muss ihre Reihenfolge
# also selbst kennen. Sie entsteht aus der Reihenfolge der register()-Aufrufe im statischen
# Initialisierer von net.minecraft.sound.SoundEvents -- genau die wird hier aus dem Bytecode
# gelesen.
#
# Bytecode statt Decompiler: javap gehoert zum JDK und gibt die Aufrufe in exakter Reihenfolge
# aus. Ein Decompiler waere ein zusaetzliches Werkzeug (und war beim Bauen dieses Skripts
# gerade nicht vorhanden), dessen Ausgabe man ausserdem erst wieder parsen muesste.
#
# Bei einem Versionswechsel neu laufen lassen: die Reihenfolge verschiebt sich mit jeder
# Version, in der Sounds dazukommen.
#
#   ./scripts/gen-sound-events.sh [pfad/zum/yarn-jar]
set -euo pipefail

HIER="$(cd "$(dirname "$0")" && pwd)"

# Ausdruecklich das YARN-gemappte Jar: daneben liegt ein "loom.mappings.layered"-Jar mit
# anderen Klassennamen, in dem net.minecraft.sound.SoundEvents gar nicht existiert -- javap
# faende dort nichts und das Skript liefe auf eine leere Tabelle zu.
JAR="${1:-$(ls -d "$HOME"/.gradle/caches/fabric-loom/minecraftMaven/net/minecraft/minecraft-merged/*net.fabricmc.yarn*/*.jar 2>/dev/null | sort -V | tail -1)}"

if [ ! -f "$JAR" ]; then
  echo "Kein Yarn-gemapptes Client-Jar gefunden. Pfad als erstes Argument uebergeben." >&2
  exit 1
fi
echo "Jar: $JAR" >&2

javap -p -c -classpath "$JAR" net.minecraft.sound.SoundEvents \
  | python3 "$HIER/gen-sound-events.py" "$HIER/../crates/protocol/src/sound_events.rs"
