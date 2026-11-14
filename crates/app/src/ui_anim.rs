// Cursor-Blinken, Easing, Hover- und Chat-Animationen, Ruestungs-HUD-Masse, Zeiger-Konstanten.

use crate::*;

// Blinkende Schreibmarke im Adressfeld -- eigener Name, damit sie nicht mit den neuen
// COLOR_CURSOR_*-Konstanten fuer den Mauszeiger verwechselt wird (zwei verschiedene Dinge).
// Markierter Text: Theme-Farbe "Markierung" (s. App::push_selection_span).
pub(crate) const CARET_WIDTH_PX: f32 = 3.0;
pub(crate) const CARET_BLINK_INTERVAL: Duration = Duration::from_secs(1);
// Schwaechste Deckkraft des Cursors im Blinken.
pub(crate) const CARET_BLINK_MIN: f32 = 0.15;

// Deckkraft des Cursors `seit` der letzten Bewegung: weich pulsierend wie in osu! (Sinus-Ein-
// und Ausblenden statt hartem An/Aus), nach jeder Bewegung voll.
pub(crate) fn caret_alpha(seit: Duration) -> f32 {
    let t = seit.as_secs_f32() / CARET_BLINK_INTERVAL.as_secs_f32();
    let welle = 0.5 + 0.5 * (t * std::f32::consts::TAU).cos();
    CARET_BLINK_MIN + (1.0 - CARET_BLINK_MIN) * welle
}

// Der Schreibcursor gleitet wie in osu! (OsuCaret: 60 ms, Easing.Out) an die neue Stelle,
// solange er in derselben Zeile bleibt; in eine andere Zeile oder ein eben erst gezeigtes Feld
// springt er. Nach jeder Bewegung blinkt er sichtbar von vorn (s. App::push_caret).
pub(crate) const CARET_GLIDE: Duration = Duration::from_millis(60);

// Easing.Out von osu-framework (Quad): schnell los, sanft an.
pub(crate) fn ease_out(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    1.0 - (1.0 - t) * (1.0 - t)
}

// Easing.OutQuint von osu-framework: noch schneller los, sehr lang auslaufend.
pub(crate) fn ease_out_quint(t: f32) -> f32 {
    1.0 - (1.0 - t.clamp(0.0, 1.0)).powi(5)
}

// Markierung wie in osu! (OsuCaret.DisplayAt): sie waechst in CARET_GLIDE aus dem Cursor
// heraus. An der wachsenden Kante leuchtet sie in der Cursorfarbe auf, als weicher Schein
// ueber SELECTION_GLOW_W Pixel, der mit der Kante wandert; steht die Kante, klingt er in
// SELECTION_FADE zur Theme-Farbe "Markierung" ab (s. App::push_selection_span).
pub(crate) const SELECTION_FADE: Duration = Duration::from_millis(200);
pub(crate) const SELECTION_FLASH_ALPHA: f32 = 0.7;
pub(crate) const SELECTION_GLOW_W: f32 = 48.0;
// Breite der Streifen, aus denen der Schein gezeichnet wird.
pub(crate) const SELECTION_SLICE: f32 = 2.0;

// Knopf-Hover wie in osu! (OsuButton.OnHover): die Hover-Farbe schiesst in HOVER_FLASH ueber
// das Ziel hinaus (doppelt so weit) und klingt in HOVER_SETTLE auf die Hover-Farbe des Themes
// ab; beim Verlassen geht sie in HOVER_SETTLE zurueck. Beides Easing.OutQuint.
pub(crate) const HOVER_FLASH: Duration = Duration::from_millis(40);
pub(crate) const HOVER_SETTLE: Duration = Duration::from_millis(800);
pub(crate) const HOVER_PEAK: f32 = 2.0;

// Stand der Hover-Animation eines Knopfs: seit wann (nicht) gehovert, mit welcher Staerke
// der Wechsel begann, und wann er zuletzt gezeichnet wurde.
#[derive(Clone, Copy)]
pub(crate) struct HoverAnim {
    pub(crate) hovered: bool,
    pub(crate) since: Instant,
    pub(crate) from: f32,
    pub(crate) seen: Instant,
}

impl HoverAnim {
    // 0 = ohne Hover, 1 = Hover-Farbe des Themes, darueber das Aufleuchten.
    pub(crate) fn level(&self, jetzt: Instant) -> f32 {
        let t = jetzt.duration_since(self.since);
        if !self.hovered {
            return self.from * (1.0 - ease_out_quint(t.as_secs_f32() / HOVER_SETTLE.as_secs_f32()));
        }
        if t < HOVER_FLASH {
            return self.from + (HOVER_PEAK - self.from) * ease_out_quint(t.as_secs_f32() / HOVER_FLASH.as_secs_f32());
        }
        let t = t.checked_sub(HOVER_FLASH).unwrap().as_secs_f32() / HOVER_SETTLE.as_secs_f32();
        HOVER_PEAK + (1.0 - HOVER_PEAK) * ease_out_quint(t)
    }
}

// Zwischen zwei Farben; `k` darf ueber 1 gehen (Aufleuchten ueber die Hover-Farbe hinaus).
pub(crate) fn mix_color(a: [f32; 4], b: [f32; 4], k: f32) -> [f32; 4] {
    std::array::from_fn(|i| (a[i] + (b[i] - a[i]) * k).clamp(0.0, 1.0))
}

pub(crate) fn mix_rect(a: Rect, b: Rect, k: f32) -> Rect {
    Rect { x: a.x + (b.x - a.x) * k, y: a.y + (b.y - a.y) * k, w: a.w + (b.w - a.w) * k, h: a.h + (b.h - a.h) * k }
}

// Wohin die Markierung eines Eingabefelds gleitet (s. App::push_field_selection).
#[derive(Clone, Copy)]
pub(crate) struct SelectionGlide {
    pub(crate) from: Rect,
    pub(crate) to: Rect,
    pub(crate) start: Instant,
    pub(crate) seen: Instant,
}

// Wo der Schreibcursor zuletzt stand und wohin er gleitet (s. CARET_GLIDE).
#[derive(Clone, Copy)]
pub(crate) struct CaretAnim {
    pub(crate) from_x: f32,
    pub(crate) to: Rect,
    pub(crate) start: Instant,
    // Zuletzt gezeichnet -- lange nicht mehr, dann springt er statt zu gleiten.
    pub(crate) seen: Instant,
}

impl CaretAnim {
    pub(crate) fn x(&self, jetzt: Instant) -> f32 {
        let t = jetzt.duration_since(self.start).as_secs_f32() / CARET_GLIDE.as_secs_f32();
        self.from_x + (self.to.x - self.from_x) * ease_out(t)
    }
}

// Blaettern im Chatverlauf (Pfeil oben/unten): der alte Text gleitet eine Zeile hinaus und
// blendet aus, der neue kommt von der anderen Seite herein -- Pfeil oben schiebt nach oben.
pub(crate) const CHAT_RECALL: Duration = Duration::from_millis(150);

pub(crate) struct ChatRecall {
    pub(crate) alt: String,
    pub(crate) nach_oben: bool,
    pub(crate) start: Instant,
}

// Im Chat steht der Cursor fest (der Text ist rechtsbuendig an ihm), dort gleitet statt seiner
// der Text links vom Cursor: so weit versetzt, wie er beim Tippen oder Loeschen sprang, und
// in CARET_GLIDE zurueck auf null.
pub(crate) struct ChatSlide {
    pub(crate) text: String,
    pub(crate) line: usize,
    pub(crate) line_x: f32,
    pub(crate) from: f32,
    pub(crate) start: Instant,
}

// Nach dem Loeschen bleibt der Text der letzten Eingabezeile stehen, statt nach rechts an den
// festen Cursor-Anker zu ruecken -- stattdessen wandert der Cursor nach links. `gap` ist der
// Abstand des Zeilenendes vom Anker; Tippen zehrt ihn wieder auf, bis die Zeile wie gewohnt am
// Anker steht. Gilt fuer den Text `text`, die Werte daneben fuer dessen letzte Zeile.
pub(crate) struct ChatGap {
    pub(crate) text: String,
    pub(crate) lines: usize,
    pub(crate) last_w: f32,
    pub(crate) gap: f32,
}

impl ChatSlide {
    pub(crate) fn offset(&self, jetzt: Instant) -> f32 {
        let t = jetzt.duration_since(self.start).as_secs_f32() / CARET_GLIDE.as_secs_f32();
        self.from * (1.0 - ease_out(t))
    }
}
// Der Chat-Cursor sitzt bewusst nicht exakt in der Bildschirmmitte, sondern etwas weiter
// rechts davon -- er bleibt ohnehin immer an derselben Stelle stehen (s.
// build_chat_overlay-Kommentar zum Text-Scrollen), ein reiner Aussehens-Versatz.
pub(crate) const CHAT_CARET_OFFSET_X: f32 = 48.0;

// Tearing-Test: mehrere helle Balken voller Bildschirmhoehe, die zeitbasiert von links
// nach rechts wandern. Nur sichtbar, wenn per Button aktiviert.
pub(crate) const TEARING_BAR_WIDTH_PX: f32 = 24.0;
pub(crate) const TEARING_BAR_SECONDS_PER_SWEEP: f32 = 1.2;
pub(crate) const TEARING_BAR_COLORS: [[f32; 4]; 5] =
    [[1.0, 0.15, 0.15, 1.0], [1.0, 0.6, 0.1, 1.0], [1.0, 1.0, 0.2, 1.0], [0.2, 1.0, 0.3, 1.0], [0.3, 0.6, 1.0, 1.0]];


// Masse des Ruestungs-HUD, s. App::armor_hud_metrics.
pub(crate) struct ArmorHudMetrics {
    pub(crate) size: f32,
    pub(crate) px: f32,
    pub(crate) text_scale: f32,
    pub(crate) gap: f32,
    pub(crate) rand: [f32; 4],
}

// Weiss fuer Text im Spiel (Chat, HUD, Schilder) -- haengt bewusst nicht am Theme der Menues.
pub(crate) const COLOR_WHITE: [f32; 4] = [1.0, 1.0, 1.0, 1.0];
// Haltbarkeitszahl im Ruestungs-HUD, wenn das Teil bald bricht (Vanillas Gold, §6).
pub(crate) const COLOR_ARMOR_LOW: [f32; 4] = [1.0, 170.0 / 255.0, 0.0, 1.0];
// Fast neue Ruestung (s. Settings::armor_hud_green): das Gruen von §a.
pub(crate) const COLOR_ARMOR_HIGH: [f32; 4] = [85.0 / 255.0, 1.0, 85.0 / 255.0, 1.0];
// Rahmen um die noch getragenen Teile, solange ein abgelegtes daneben steht (Minecrafts Gelb).
pub(crate) const COLOR_ARMOR_KEPT: [f32; 4] = [1.0, 1.0, 85.0 / 255.0, 1.0];
// Absorptionsherzen blenden so lange ein und steigen dabei so viele GUI-Punkte von unten auf.
pub(crate) const ABSORPTION_FADE_SECS: f32 = 0.25;
pub(crate) const ABSORPTION_FADE_RISE: f32 = 4.0;
// Fuellung des Luftbalkens (s. build_air_bar).
pub(crate) const COLOR_AIR_BAR: [f32; 3] = [0.25, 0.6, 1.0];

pub(crate) const VERTS_PER_QUAD: usize = 6;
// Text (UI und Weltbeschriftungen) hat je Viereck nur seine vier Ecken; die zwei Dreiecke
// kommen aus dem festen Indexpuffer (s. text_index_range, GpuState::text_index_buffer).
pub(crate) const TEXT_VERTS_PER_QUAD: usize = 4;
// Reichlich: der offene Chat bringt Unterstriche fuer Links mit, die Textauswahl eine
// Markierung je ausgewaehltem Textstueck.
pub(crate) const MAX_UI_RECTS: usize = 4096;
// Fuer den F3-CPU-Graphen (s. build_debug_overlay): ein Segment pro Punkt-Paar, HISTORY_LEN-1
// (89) Segmente pro Kern. 2048 deckt bis zu ~23 Kerne komplett gefuellt ab -- deutlich mehr
// als aktuell ueblich (dieser Rechner hat 16, s. nproc), mit Luft nach oben statt einer
// Zahl, die exakt am aktuellen Kernkontingent klebt.
// Die drei F3-Graphen brauchen allein ~13000 (je Kurve ~360 Stuecke, Flaeche 1 + weiche
// Linie 5 Vierecke).
pub(crate) const MAX_UI_LINE_SEGMENTS: usize = 16384;
// Doppelt so viel wie Zeichen je Bild noetig -- der Textschatten (s. shape_text) zeichnet
// jede Glyphe zweimal.
pub(crate) const MAX_UI_GLYPHS: usize = 32768; // Schatten zeichnet jedes Zeichen 8x mit (s. shape_text)
// Texturierte UI-Quads pro Frame (s. UiSprite): der Inventar-Screen ist der Spitzenwert --
// Hintergrund-Sheet + 46 Slots à bis zu drei Wuerfelseiten + Hotbar.
pub(crate) const MAX_UI_SPRITES: usize = 1024;
// Rand um die Bounding-Box des Zeigers: die Kantenglaettung braucht Platz bis ~0.5 px
// ausserhalb der Form, sonst wird genau der geglaettete Saum weggeschnitten.
pub(crate) const CURSOR_AA_PADDING_PX: f32 = 2.0;
