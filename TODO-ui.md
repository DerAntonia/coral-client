# TODO: Einheitliche Menue-Optik + Themes

Warum das hier steht: die Oberflaeche ist ueber mehrere Sitzungen zusammengewachsen und
sieht deshalb an verschiedenen Stellen verschieden aus, obwohl sie ueberall dasselbe tut.
Das Modul-Menue hat das sichtbar gemacht (Nutzer: "das Menu sieht scheisse aus") -- es ist
aber kein Problem DIESES Menues, sondern eines fehlenden gemeinsamen Fundaments.

Das Ziel ist ausdruecklich NICHT "das Modul-Menue huebscher machen", sondern: jedes Menue im
Client bekommt seine Masse und Farben aus derselben Quelle, und diese Quelle ist umschaltbar.

## Stand heute (das eigentliche Problem)

**Zwei unabhaengige Layout-Systeme.** Die Einstellungs-Screens rechnen mit
`SETTINGS_ROW_H`/`SETTINGS_LABEL_X`/`SETTINGS_CTRL_X`/`SETTINGS_CTRL_H`/`TOGGLE_W` in festen
Bildschirmpixeln (main.rs, ~Zeile 201). Die In-Welt-Overlays (Inventar, Modul-Menue) rechnen
stattdessen mit `App::MODULE_*`-Konstanten mal `inventory_scale()`, also in GUI-Einheiten.
Deshalb wirken die beiden Bereiche unterschiedlich dicht, und dieselbe Aenderung muss man
zweimal machen.

Das hat inzwischen einmal zugeschlagen: das Modul-Menue hat `stepper_at()` benutzt, also die
FESTEN Pixelmasse der Settings-Screens, mitten in einer skalierten Zeile -- bei GUI-Skalierung
3 waren die Pfeile 40x36 Pixel neben einem 168x48 grossen Wertfeld. Notbehelf ist jetzt ein
zweiter Bauer `stepper_at_scaled()`. Genau diese Verdopplung verschwindet mit Punkt 3.

**26 lose Farbkonstanten.** `grep '^const COLOR_' crates/app/src/main.rs` -- alle einzeln
hingeschrieben, mehrere fast gleich (`COLOR_FIELD_BG` vs. `COLOR_SCROLL_TRACK` vs.
`COLOR_PANEL_BG`), keine benannte Rolle. Eine Farbe zu aendern heisst heute: alle Stellen
suchen, an denen zufaellig dieselbe Zahl steht.

**Bedienelemente ohne gemeinsame Definition.** Ein An/Aus-Schalter wird an drei Stellen
verschieden gebaut (`buttons()` fuer Screens, `module_menu_buttons()` fuer das Overlay,
Sonderfaelle im HUD-Editor). Gleiches gilt fuer Stepper (`stepper_at()` gibt es immerhin
schon) und fuer Wertfelder.

## Zielbild

1. **Ein Theme-Struct statt 26 Konstanten.** Farben bekommen ROLLEN, nicht Namen nach ihrem
   ersten Einsatzort:
   `background`, `surface`, `surface_raised`, `border`, `text`, `text_muted`, `text_disabled`,
   `accent`, `accent_text`, `on`, `off`, `danger`, `scroll_track`, `scroll_thumb`.
   Die heutigen `COLOR_*` werden zu Feldern eines `Theme` und verschwinden als freie
   Konstanten. Wer eine Farbe braucht, fragt die Rolle -- nicht den Farbwert.

2. **Themes umschaltbar, mit dem heutigen Aussehen als Vorgabe.** Mindestens ein zweites
   mitgeliefertes Theme, damit die Abstraktion sofort belegt ist und nicht nur behauptet.
   Umschalter in die Einstellungen (Darstellung), gespeichert als `ui_theme=<name>` in der
   settings.conf -- gleiche Konvention wie alle anderen Einstellungen
   (s. `Settings::parse`/`save`).
   Vorsicht bei eigenen Farbwerten des Nutzers: die Hitbox-Farben (HSV, s.
   `module_hitbox_*_hue`) gehoeren NICHT ins Theme -- das sind Nutzdaten, kein Aussehen.

3. **Ein Masssystem.** Alle Menues rechnen in GUI-Einheiten mal `inventory_scale()`, wie es
   die Overlays schon tun. Die festen Pixelmasse der Settings-Screens werden darauf
   umgestellt -- damit skalieren sie endlich mit und sehen auf 4K nicht mehr winzig aus.
   `SETTINGS_ROW_H` &Co werden zu Theme-/Metrik-Feldern (`row_h`, `label_x`, `control_w`, ...).

4. **Widget-Bauer statt Struct-Literale.** Je EINE Funktion fuer Schalter, Stepper, Wertfeld,
   Farbfeld, Panel-Rahmen, Scrollbalken -- alle nehmen Theme + Metriken entgegen. Vorbild ist
   `App::stepper_at()`, das genau das schon richtig macht.

5. **Zeilen als Daten, ueberall.** Das Modul-Menue hat das seit dieser Sitzung
   (`ModuleRow` + `module_detail_rows()`): eine Liste beschreibt die Zeilen, Trefferpruefung
   und Zeichnen lesen dieselbe Liste. Vorher waren es zwei spiegelbildliche Funktionen, die
   auseinanderlaufen konnten. Die Settings-Screens sollten denselben Weg gehen -- dort steht
   die Doppelung (`buttons()` vs. `build_settings_*_tab()`) noch.

## Reihenfolge

Punkt 5 zuerst (rein interner Umbau, kein sichtbarer Unterschied, macht alles Weitere
kleiner), dann 1, dann 4, dann 3, dann 2. Punkt 3 aendert als einziger das Aussehen
merklich -- deshalb spaet, wenn der Rest schon stabil ist.

## Nicht vergessen

- Der UI-Pass hat KEINEN Tiefentest: allein die Reihenfolge in `rects`/`texts` entscheidet,
  was oben liegt. Bildschirm-Overlays gehoeren deshalb ans ENDE von `build_hud()`
  (war ein echter Bug: Namensschilder und Chat wurden ueber das Modul-Menue gezeichnet).
- Ein Theme darf den Kontrast nicht dem Zufall ueberlassen. Abgeblendeter Text
  (`text_disabled`) muss auf `surface` noch lesbar bleiben -- am besten als Test, der die
  Helligkeitsdifferenz der ausgelieferten Themes nachrechnet, statt es nur zu behaupten.

# Erledigt: Speer-Animationen

Nach Vanillas `Lancing` (1.21.11) nachgebaut, nur per Tests geprueft -- im Spiel noch gegenzuchecken:

- Ego-Ansicht: Stich statt Schwung (`first_person_stab_transform`), Ansturm bei gehaltenem
  Rechtsklick (`first_person_spear_charge`, Zeitmarken aus `item_registry::spear_kinetic`).
- Schwungdauer je Speer (13..23 Ticks, `item_registry::swing_duration_ticks`) fuer uns und
  fuer fremde Spieler, auch in `restart_swing`.
- Dritte Person: Speerhaltung (Arm zeigt in Blickrichtung, `apply_spear_arm_pose`), Stich im
  Arm (`apply_hand_swing_typed`) und am Speer selbst (`third_person_stab_item_offset`).

Offen: der Schwank des Arms beim Ansturm in der dritten Person (braucht die Benutzungsdauer
fremder Spieler) und der Rueckstoss nach einem Ansturm-Treffer (`timeSinceLastKineticAttack`).

# Erledigt: Kleinigkeiten

Alle zwoelf umgesetzt (Branch todo-kleinigkeiten), aber nur per Tests geprueft -- im Spiel
noch gegenzuchecken:

1. Umriss des anvisierten Blocks: eigenes Modul "Blockumriss" (Farbe, Deckkraft, Breite),
   aus = Vanilla.
2. Emojis: Noto Emoji (einfarbig) im DM-Sans-Atlas statt Minecraft-Pixelzeichen.
3. Totem-X: Strichdicke 0,8 statt 1,3 GUI-Pixel.
4. Trank-Partikel: entity_effect-Partikel und POTION_SWIRLS an Entities und am eigenen Spieler.
5. Weiche Beleuchtung: Staerke 0..100 % statt An/Aus.
6. Droppen: sofort lokal aus der Hotbar, Handschwung; mit "Stapel verschieben" der ganze Stapel.
7. Mipmaps: Farbe durchsichtiger Kacheln alpha-gewichtet gemittelt (keine schwarzen Raender).
8. Essen: Benutzungszaehler bei jedem Start/Ende zurueckgesetzt.
9. Platzieren: Rechtsklick trifft ein naeheres Entity (Kristall) statt des Blocks dahinter.
10. Skin laden: im Hintergrund-Thread, Fortschritt als Benachrichtigung.
11. Einsammeln per Doppelklick: auch fuer Stapel mit Components (Pfeile mit Trank).
12. Tab-Liste: Schleier wie Vanilla im sRGB-Raum gemischt (keine milchig weissen Zeilen).
