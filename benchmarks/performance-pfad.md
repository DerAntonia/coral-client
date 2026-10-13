# Referenz: Performance-Pfad

Frametime des bestehenden Forward-Renderers (Performance-/PvP-Pfad). Jede Änderung an gemeinsam
genutzten Teilen (Chunk-Verwaltung, Meshing) wird in derselben Szene nachgemessen und hier
gegenübergestellt. Der Performance-Pfad darf dabei nicht langsamer werden.

## Szene

- Testserver: lokaler Paper 1.21.11, Offline-Mode, Seed `-3058182250925340416`
- Mob-Spawnen aus (`spawn_mobs=false`, alle `spawn-limits` 0), keine Entities in der Welt
- Tageszeit und Wetter eingefroren: Mittag (`time set noon`, `advance_time=false`), klar
- Umgebung mit Chunky vorgeneriert (Radius 640 um die Messstelle)
- Position `739.7 79 -1574.7`, Yaw `225.2`, Pitch `-18.6` (Wald am Wasser, Blick übers Meer)
- Renderdistanz 32, FOV 110, Vollbild 1920×1080, Present-Modus Immediate, kein Frame-Limiter
  (**bis einschließlich `ba7b996` forderte der Client fest nur 24 Chunks an** — alle Messungen
  bis dahin liefen also effektiv mit Sichtweite 24, s. `REQUESTED_VIEW_DISTANCE`)
- Hardware: AMD RX 9060 XT (8 GB), 16 CPU-Kerne, 120-Hz-Monitor

## Ablauf

1. `/tp <Spieler> 739.7 79 -1574.7 225.2 -18.6`, warten bis alles gemesht ist.
2. Frame-Profiler einschalten, mindestens 60 s stillhalten, ausschalten.
3. `scripts/frametime.py` (verwirft die ersten 5 s).

## Messungen

| Datum      | Commit    | Was                   | Frames | Mittel   | 1-%-Low  | GPU¹     |
|------------|-----------|-----------------------|--------|----------|----------|----------|
| 2026-10-03 | `6f9c0dc` | **Referenz** (Ausgang) | 17137 | 4,239 ms | 5,826 ms | 2,986 ms |
| 2026-10-03 | `ab33115` | Animationen versetzt, Upload im Animations-Thread, Profiler ohne GPU-Wait | 8785 | 4,094 ms | 5,751 ms | 2,838 ms |
| 2026-10-03 | `cb7810e` | Opakes Gelände kompakt (4×20 statt 6×32 Byte je Fläche), Mesh-VRAM 1557 → 762 MB | 14010 | 3,075 ms | 4,995 ms | 1,816 ms |
| 2026-10-03 | `44ceb2f` | Gelände in gemeinsamem Speicher, ein indirekter Aufruf je Seite (Encode 0,78 → 0,52 ms) — *nur 17 s, Szene mit Truhe/Brettern und 5 Entities* | 6084 | 2,773 ms | 4,536 ms | 1,813 ms |
| 2026-10-03 | `fa59910` | Abgewandte Flächengruppen je Spalte nicht gezeichnet (Gelände 1,31 → 1,05 ms GPU) | 21129 | 2,511 ms | 3,907 ms | 1,541 ms |
| 2026-10-03 | `044a959` | Wasser/Glas kompakt + gebündelt (0,43 → 0,28 ms GPU), Profiler-Tabelle nur alle 250 ms | 22546 | 1,934 ms | 3,656 ms | 1,371 ms |
| 2026-10-03 | `14f95b9` | Höhlen-Culling (Gelände 1,03 → 0,75 ms GPU, 3072 von 13351 Abschnitten gezeichnet) — *nur 17 s* | 10012 | 1,680 ms | 3,126 ms | 1,090 ms |
| 2026-10-03 | `a89a71c` | Sichtkegel je Abschnitt (Gelände 0,75 → 0,68 ms GPU, 2503 statt 3072 Abschnitte) | 32188 | 1,608 ms | 3,166 ms | 1,021 ms |
| 2026-10-03 | `d82c5a0` | Gelände-Vertex 16 statt 20 Byte (Gelände 0,68 → 0,44 ms, Wasser/Glas 0,28 → 0,21 ms GPU) — *29 s* | 23591 | 1,238 ms | 2,237 ms | 0,697 ms |

Streuung der Referenz über vier Viertel der Messung: Mittel 4,230–4,247 ms, 1-%-Low
5,57–6,35 ms. Unterschiede unter etwa 1 % im Mittel bzw. 10 % im 1-%-Low liegen im Rauschen.

¹ Referenz: „GPU nach Present“ (blockierende Messung). Ab `cc87a72`: Zeitstempel, Summe aller
Passes. Render-Ahead 0 bei allen Messungen.

## Vergleich mit OneClient (ohne Fence)

Gleiche Szene und Einstellungen (Sichtweite 32, FOV 110, 1920×1080, kein Limit, VSync aus),
beide 60 s mit MangoHud aufgenommen (`scripts/mangohud.py`). OneClient: Fabric-Modpack mit
Sodium, Iris-Shader aus, Texel-Interpolation Nearest. Coral Client: Render-Ahead 1.

| Datum      | Client                 | Frames | Mittel   | 1-%-Low  | GPU-Last / Takt / Leistung |
|------------|------------------------|--------|----------|----------|----------------------------|
| 2026-10-03 | OneClient              | 50785  | 1,181 ms (847 FPS)  | 2,817 ms (355 FPS) | 99 % / 3232 MHz / 174 W |
| 2026-10-03 | Coral Client `2cc3a8f`    | 79443  | 0,755 ms (1324 FPS) | 1,856 ms (539 FPS) | 100 % / 3260 MHz / 164 W |

**Nicht vergleichbar:** Coral Client forderte damals nur 24 Chunks an, OneClient lief mit 32
(rund 78 % mehr Fläche). Dazu zeichnet OneClient Himmel und Wolken, Coral Client noch nicht.
Der Vergleich wird in einer besseren Szene unter gleichen Bedingungen wiederholt.
