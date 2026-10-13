# Offene Optimierungen: Performance-Pfad

Was am Forward-Renderer noch zu holen ist, nach Aufwand und erwartetem Gewinn sortiert.
Gemessen wird jeder Schritt in der Referenzszene (s. `performance-pfad.md`). Render-Ahead 0
bleibt dabei unangetastet; gespart wird nur echte CPU- und GPU-Arbeit.

Stand nach dem Höhlen-Culling (`14f95b9`): GPU Gelände 0,75 ms, Wasser/Glas 0,28 ms,
3072 von 13351 Abschnitten gezeichnet.

## 1. Sichtkegel pro Abschnitt statt pro Spalte — erledigt (`a89a71c`)

Gemessen: Gelände 0,75 → 0,68 ms GPU, Frametime 1,680 → 1,608 ms, 2503 statt 3072 Abschnitte.

- **Aufwand:** klein. **Risiko:** keins (keine Verzögerung, nichts Sichtbares ändert sich).
- Bisher wird nur die ganze 16×16-Spalte gegen den Sichtkegel geprüft. Liegt sie am Bildrand
  im Kegel, wird sie über die ganze Höhe gezeichnet, auch Abschnitte weit über oder unter
  dem Bild.
- Ein Box-Test je Abschnitt kostet auf der CPU nur Mikrosekunden. Der F3-Zähler
  („X von Y Abschnitten“) zeigt sofort, wie viel wegfällt.

## 2. Wasser/Glas pro Abschnitt cullen

- **Aufwand:** mittel.
- Wasser/Glas kostet 0,28 ms. Bisher fallen nur Spalten weg, die ganz hinter Fels liegen;
  unterirdische Seen und Aquifere hinter Fels werden voll gezeichnet.
- Hürde: Das Wasser ist je Fläche von hinten nach vorn sortiert. Das Sortieren müsste
  innerhalb jedes Abschnitts laufen, damit Abschnitte als Bereiche weggelassen werden können.

## 3. Kompakteres Vertex-Format (16 statt 20 Byte) — erledigt (`d82c5a0`)

Gemessen: Gelände 0,68 → 0,44 ms GPU, Wasser/Glas 0,28 → 0,21 ms, Frametime 1,608 → 1,238 ms.

- **Aufwand:** größer. **Gewinn:** 20 % weniger Vertex-Daten, die die GPU fürs Gelände lesen
  muss; Bandbreite ist auf der RX 9060 XT der Engpass.
- Positionen relativ zum Abschnitt als 3 × u16 statt 3 × f32. Die 1/1024-Versätze gegen
  Z-Fighting bleiben exakt, weil ein Abschnitt nur 16 Blöcke groß ist.
- Nachteil: Jeder Abschnitt braucht seinen eigenen Versatz, die zusammengelegten
  indirekten Aufrufe werden dadurch etwas mehr.

## 4. Hi-Z-Occlusion-Culling auf der GPU — zurückgestellt bis zum Fernsicht-LOD

Entschieden 2026-10-03: Bei 0,44 ms Gelände-GPU bliebe netto zu wenig übrig. Mit dem
Chunk-Saver (Punkt 5) gibt es wieder viel zu verdecken, dann wird es gebaut.

- **Aufwand:** groß (neuer Compute-Pass, zwei Zeichenphasen).
- Fängt ab, was das Höhlen-Culling nicht kann: Gelände hinter Bergen, Bäumen und Häusern
  und Höhlen hinter einem kleinen Loch.
- Ablauf im selben Frame, ohne Rücklesen auf die CPU (keine zusätzliche Latenz):
  1. Zeichnen, was im letzten Frame sichtbar war; daraus eine Tiefen-Pyramide bauen.
  2. Compute-Shader prüft jeden Abschnitt gegen die Pyramide und setzt verdeckte in den
     indirekten Aufrufen auf 0 Dreiecke.
  3. Neu sichtbar Gewordenes im selben Frame nachzeichnen, damit keine Löcher entstehen.
- Kosten: grob 0,1–0,2 ms GPU, und der Tiefenpuffer muss gespeichert statt verworfen werden
  (Bandbreite).
- Erst angehen, wenn 1 bis 3 ausgereizt sind und die Messung noch viel Gelände-Zeit für
  verdeckte Abschnitte zeigt.

## 5. Fernsicht-LOD: Modul „Chunk-Saver“ — erste Fassung (Performance-Pfad)

Auch im Performance-Pfad, als Modul abschaltbar (im PvP aus). Vorbild im Funktionsumfang ist
die Mod Voxy; deren Code (All-Rights-Reserved) wird nicht angesehen, die Umsetzung ist eigen.

- **Quelle:** Der Client speichert jeden Chunk, den er je bekommen hat, lokal je Server und
  Welt. Wo man nie war, bleibt es leer.
- **Detail:** volle Blockdetails wie bei Voxy (Bäume, Gras, Wasser), in der Ferne nach
  Bildschirmgröße vergröbert.
- **Einstellungen** (wie Voxy): Hintergrund-Threads, Chunk-Aufnahme an/aus, LOD-Zeichnen
  an/aus, Unterteilungsschwelle in Pixel² (Voxy-Vorgabe 28), Sichtweite in Chunks (Voxy bis
  650), Umgebungsnebel an/aus.
- **Kosten:** eigene GPU-Zeit im Profiler, damit sich der Preis jederzeit messen lässt.

Umsetzung: Crate `chunk_saver` (Speicher, Stufen 0–7 mit 32³-Voxel-Knoten, Mesher, Auswahl),
`render::lod_renderer` + `lod.wgsl` (Flächen als 16-Byte-Einträge in Speicherseiten, eigene
Tiefe mit umgekehrtem Z, Spaltenmaske gegen das echte Gelände), `app/src/lod.rs` (Modul).
GPU-Zeit steht im Profiler als „Fernsicht“. Gemessen auf der CPU (synthetisches Gelände,
Release): Aufnahme 0,46 ms je Chunk, Meshing ~0,3 ms je Knoten, ~1 KB je Chunk auf der Platte.

Offen: Shader-Pfad (dort zeichnet die Fernsicht noch nicht), Blocklicht/Himmelslicht in der
Ferne, gespeicherte Lightmaps (s. Shader-Pfad), GPU-Messung in der Referenzszene.

