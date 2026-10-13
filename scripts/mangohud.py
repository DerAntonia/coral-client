#!/usr/bin/env python3
# Wertet ein MangoHud-Log aus (output_folder, Shift+F2) -- fuer den Vergleich mit anderen Clients,
# die keine eigene frame_profile.csv schreiben. Kennzahlen wie scripts/frametime.py: Mittel der
# Frametime und 1-%-Low als Mittel der langsamsten 1 % der Frames.
#
#   scripts/mangohud.py [log.csv ...]     (ohne Argument: das neueste Log in ~/mangologs)
import csv
import glob
import os
import sys

paths = sys.argv[1:]
if not paths:
    logs = [p for p in glob.glob(os.path.expanduser("~/mangologs/*.csv")) if not p.endswith("_summary.csv")]
    if not logs:
        sys.exit("kein Log in ~/mangologs")
    paths = [max(logs, key=os.path.getmtime)]

for path in paths:
    with open(path) as f:
        lines = f.read().splitlines()
    # Kopf: Systemzeile, Werte, dann die eigentliche Tabelle ab der Zeile mit "fps,frametime".
    start = next(i for i, l in enumerate(lines) if l.startswith("fps,frametime"))
    rows = list(csv.DictReader(lines[start:]))
    ft = sorted((float(r["frametime"]) for r in rows if float(r["frametime"]) > 0.0), reverse=True)
    n = len(ft)
    mean = sum(ft) / n
    worst = ft[: max(1, n // 100)]
    low = sum(worst) / len(worst)
    mittel = lambda k: sum(float(r[k]) for r in rows) / len(rows)
    print(path)
    print(f"Frames:        {n}")
    print(f"Durchschnitt:  {mean:.3f} ms  ({1000.0 / mean:.0f} FPS)")
    print(f"1-%-Low:       {low:.3f} ms  ({1000.0 / low:.0f} FPS)")
    print(f"GPU-Last {mittel('gpu_load'):.0f} %, Takt {mittel('gpu_core_clock'):.0f} MHz, {mittel('gpu_power'):.0f} W, CPU-Last {mittel('cpu_load'):.0f} %")
