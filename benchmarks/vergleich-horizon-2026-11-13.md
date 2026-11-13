# Vergleich Coral Client – Horizon, 2026-11-13

Bildrate und Speicher im selben Testserver, gemessen mit MangoHud. Die Logs liegen in
`mangohud-2026-11-13/`.

## Aufbau

- **System:** AMD Ryzen 7 9700X, AMD Radeon RX 9060 XT, 16 GB RAM, Linux Mint 22.3,
  Kernel 7.0.0-38, Mesa 26.2.5.
- **Server:** Fabric 1.21.11 mit ViaFabric/ViaVersion. Die Testwelt ist voll mit Entities,
  Redstone-Uhren und Command-Blöcken.
- **Gleiche Bedingungen:**
  - beide Clients am selben Standort mit derselben Blickrichtung;
  - Zeit und Wetter per Gamerule eingefroren (`advance_time false`, `advance_weather false`);
  - Nachtsicht per Datapack;
  - 20 Chatzeilen;
  - kein Resourcepack;
  - Bildrate ohne Begrenzung, VSync aus.
- **Einstellungen:** vorab abgeglichen. Bei Horizon sind das Sichtweite 32, Simulationsweite 32,
  Entity-Distanz 5,0, Biom-Übergang 7, keine Wolken und GUI-Skala 3.
- **Versionen:** Horizon nightly-2026-11-13-9f42064. Coral ist ein Entwicklungsstand nach v26.11.2,
  als Release-Build.
- **Messung:** MangoHud per `LD_PRELOAD`, Aufnahme mit Shift+F2 und automatischem Stopp nach 30 s.
  Ausgewertet sind nur volle 30-s-Logs.

## Bildrate

| Client | Log | Ø FPS | Median | p99 | 1-%-Low | 0,1-%-Low | längster Frame | GPU-Last / Leistung |
|---|---|---|---|---|---|---|---|---|
| Horizon | `horizon-client_…_20-53-26` | 799 | 1,187 ms | 2,118 ms | 414 FPS | 272 FPS | 6,18 ms | 55 % / 53 W |
| Coral, Motion Blur an | `coral-client-…_20-57-45` | 909 | 1,046 ms | 1,673 ms | 494 FPS | 321 FPS | 6,67 ms | 80 % / 60 W |
| Coral, Motion Blur aus | `coral-client-…_21-02-43` | 969 | 0,985 ms | 1,491 ms | 577 FPS | 384 FPS | 3,80 ms | 69 % / 54 W |

So sind die Werte gerechnet:
- **Ø FPS:** Anzahl der Frames geteilt durch die Summe der Frametimes.
- **1-%-Low und 0,1-%-Low:** Mittel der langsamsten 1 % bzw. 0,1 % der Frames.
- Für Ø FPS und 1-%-Low gibt es ein Skript: `scripts/mangohud.py`.

Die `_summary.csv` von MangoHud rechnet anders und zeigt deshalb leicht abweichende Werte.

Coral ohne Motion Blur liefert 21 % mehr Bilder im Schnitt als Horizon und 39 % mehr beim
1-%-Low.

## Speicher

Gemessen wurde je Prozess:
- RAM: `/proc/<pid>/smaps_rollup`;
- VRAM und GTT: DRM-fdinfo.

| Client | Laufzeit | PSS | RSS | Höchststand RSS | VRAM | GTT | Threads |
|---|---|---|---|---|---|---|---|
| Coral | 3 min | 909 MiB | 957 MiB | 1000 MiB | 658 MiB | 44 MiB | 49 |
| Horizon | 32 s | 784 MiB | 832 MiB | 891 MiB | 396 MiB | 8 MiB | 45 |

Horizon lief bei der Messung erst 32 Sekunden und war damit wohl noch nicht eingeschwungen.

## Nicht ausgewertet

- Kurze Logs, die vor 30 s abgebrochen wurden: `20-45-22`, `20-55-55` und `20-57-12`.
- Logs aus früheren Sitzungen: `00-31-10` und `00-32-10`. Sie stammen von einem älteren Coral-Stand
  und liefen nicht unter den Bedingungen von oben.
- **OneClient 2.5.1 (Fabric 1.21.11, Sodium):** Im Spiel waren 300–400 FPS zu sehen. Ein sauberes
  MangoHud-Log kam nicht zustande, deshalb fehlt der Client in der Tabelle.
