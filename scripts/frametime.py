#!/usr/bin/env python3
# Wertet frame_profile.csv des Frame-Profilers aus (s. crates/app/src/profiler.rs): Durchschnitt
# und 1-%-Low der Frametime, dazu die GPU-Zeit -- fuer den Vergleich mit der Referenzmessung des
# Performance-Pfads.
#
#   scripts/frametime.py [csv] [--skip SEKUNDEN]
#
# 1-%-Low = Mittel der langsamsten 1 % der Frames (nicht das 99. Perzentil): so zaehlt jeder
# einzelne Ruckler mit, statt an einer Schwelle abgeschnitten zu werden.
import csv
import os
import sys

args = sys.argv[1:]
skip_s = 5.0
if "--skip" in args:
    i = args.index("--skip")
    skip_s = float(args[i + 1])
    del args[i : i + 2]
path = args[0] if args else os.path.expanduser("~/.config/coral-client/frame_profile.csv")

rows = list(csv.DictReader(open(path)))
# Die ersten Sekunden verwerfen: Profiler gerade eingeschaltet, Menue geschlossen, Caches kalt.
rows = [r for r in rows if float(r["t_ms"]) >= skip_s * 1000.0 and float(r["interval_ms"]) > 0.0]
if not rows:
    sys.exit(f"keine Frames nach {skip_s} s in {path}")

intervals = sorted((float(r["interval_ms"]) for r in rows), reverse=True)
n = len(intervals)
mean = sum(intervals) / n
worst = intervals[: max(1, n // 100)]
low = sum(worst) / len(worst)
gpu = [float(r["gpu_ms"]) for r in rows if float(r["gpu_ms"]) > 0.0]
# GPU-Zeit je Pass aus Zeitstempeln (s. crates/app/src/gpu_timer.rs), falls die CSV sie hat.
gpu_cols = [c for c in rows[0].keys() if c.startswith("gpu_") and c.endswith("_ms") and c not in ("gpu_ms", "gpu_phase_ms")]
dauer_s = (float(rows[-1]["t_ms"]) - float(rows[0]["t_ms"])) / 1000.0

print(f"{path}")
print(f"Frames:        {n} in {dauer_s:.1f} s")
print(f"Durchschnitt:  {mean:.3f} ms  ({1000.0 / mean:.0f} FPS)")
print(f"1-%-Low:       {low:.3f} ms  ({1000.0 / low:.0f} FPS)")
if gpu:
    print(f"GPU nach Present (Mittel): {sum(gpu) / len(gpu):.3f} ms")
for col in gpu_cols:
    values = sorted((float(r[col]) for r in rows if r[col] not in ("", "NaN", "nan")), reverse=True)
    if values:
        worst = values[: max(1, len(values) // 100)]
        name = col[4:-3]
        print(f"GPU {name:8} Mittel {sum(values) / len(values):.3f} ms, langsamstes 1 % {sum(worst) / len(worst):.3f} ms")
