# Pop-Klänge aus Toteming Pro Max

Die Klänge, die die Fabric-Mod **Toteming Pro Max** (eigene Mod,
`~/Dokumente/toteming pro max`) mitliefert. Genutzt vom Modul "Pop-Klaenge"
(`crates/app/src/toteming.rs`).

| Datei | Name im Menü | Standard für |
| --- | --- | --- |
| `glass_break_05.wav` | Glas hoch | eigener Pop aus der Haupthand (Tonhöhe 2,0) |
| `glass_break_01.wav` | Glas tief | eigener Pop aus der Zweithand (Tonhöhe 1,3) |
| -- | Orb | fremder Pop (kommt aus der Installation, s. unten) |
| `click.wav` | Klick | Explosion |

Herkunft wie in der Mod: die Kombination aus **Totem Pop Plus** und dem
Resourcepack **Click Explosion**, die die Mod ersetzt hat. Die Klänge darin wurden
damals als CC0 herausgesucht. Nachgeprüft am 2026-10-13:

| Datei | Quelle | Lizenz |
| --- | --- | --- |
| `click.wav` | Kenney, Interface Sounds, `click_002` | CC0 1.0 |
| `glass_break_01.wav` | rubberduck, [75 CC0 breaking / falling / hit sfx](https://opengameart.org/content/75-cc0-breaking-falling-hit-sfx), `bfh1_glass_breaking_01` (Metadaten-Tag `ARTIST=rubberduck`) | CC0 1.0 |
| `glass_break_05.wav` | **unklar** -- heisst wie `bfh1_glass_breaking_05`, ist aber ein anderer Klang (1,9 s statt 0,44 s) | ungeklärt |
| Orb | Vanillas `random/orb` (Erfahrungsklang), am Klang nachgeprüft -- deshalb **nicht** mitgeliefert: der Client spielt ihn zur Laufzeit aus der Minecraft-Installation (`toteming::ORB_EVENT`) | Mojang |

`glass_break_05.wav`: Quelle nicht mehr auffindbar (die Sitzung von damals ist geloescht). Bleibt
nach Entscheidung vom 2026-10-13 trotzdem drin -- ein generischer Glasbruch.

Umgewandelt aus den OGG-Dateien der Mod, dieselbe Kette wie beim Rest des Ordners:

```sh
ffmpeg -i <datei>.ogg -ac 1 -ar 48000 -sample_fmt s16 <datei>.wav
```
