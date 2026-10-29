//! Die Zeiger-Effekte aus osu!lazer: Aufleuchten beim Druecken und Drehen beim Ziehen.
//!
//! Alle Zahlen stammen aus `MenuCursorContainer.cs` und `DefaultEasingFunction.cs` der
//! ppy/osu- bzw. ppy/osu-framework-Repos, nicht aus dem Gedaechtnis -- sie sind der ganze
//! Unterschied zwischen "sieht aehnlich aus" und "fuehlt sich an wie das Original". Besonders
//! die krummen: der Winkelversatz 24.3 Grad und die Ruecklaufdauer 400 * (0.5 + |winkel|/960).
//!
//! Das Modul ist bewusst frei von Fenster- und GPU-Zustand: es bekommt Zeitpunkte und
//! Zeigerpositionen und liefert Zahlen. Genau deshalb laesst sich der ganze Ablauf ohne
//! laufendes Fenster pruefen.

use std::time::Instant;

/// Die drei Easing-Kurven, die der Zeiger braucht (osu!framework, DefaultEasingFunction).
// Die Namen wie in osu!framework.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[allow(clippy::enum_variant_names)]
pub enum Easing {
    /// `1 - (1-t)^5` -- schnell los, weich aus.
    OutQuint,
    /// Federt ueber das Ziel hinaus und schwingt aus.
    OutElastic,
    /// Wie OutElastic, aber mit einem Viertel der Schwingungen -- fuer den Rueckdreher.
    OutElasticQuarter,
}

// Die Konstanten der Elastic-Kurven, nach DefaultEasingFunction.cs.
const ELASTIC_CONST: f32 = 2.0 * std::f32::consts::PI / 0.3;
const ELASTIC_CONST2: f32 = 0.3 / 4.0;

impl Easing {
    /// `t` ist der Fortschritt 0..1, Rueckgabe der Anteil 0..1 (kann ueber 1 schwingen).
    pub fn apply(self, t: f32) -> f32 {
        let t = t.clamp(0.0, 1.0);
        match self {
            Easing::OutQuint => {
                let u = t - 1.0;
                u * u * u * u * u + 1.0
            }
            // Der letzte Summand ist kein Zierrat: ohne ihn endet die Kurve nicht exakt bei 1,
            // und der Zeiger bliebe um ein Haar neben seiner Ruhegroesse stehen.
            Easing::OutElastic => {
                let offset = 2f32.powi(-11);
                2f32.powf(-10.0 * t) * ((t - ELASTIC_CONST2) * ELASTIC_CONST).sin() + 1.0 - offset * t
            }
            Easing::OutElasticQuarter => {
                let offset = 2f32.powi(-10) * ((0.25 - ELASTIC_CONST2) * ELASTIC_CONST).sin();
                2f32.powf(-10.0 * t) * ((0.25 * t - ELASTIC_CONST2) * ELASTIC_CONST).sin() + 1.0 - offset * t
            }
        }
    }
}

/// Ein laufender Uebergang von einem Wert zum naechsten.
///
/// Startwert ist immer der Wert, der GERADE gilt -- nicht der Zielwert des letzten Uebergangs.
/// Wer mitten im Aufleuchten loslaesst, blendet von dort aus zurueck statt zu springen; genau
/// so verhalten sich osu!s Transforms.
#[derive(Clone, Copy, Debug)]
pub struct Tween {
    from: f32,
    to: f32,
    start: Instant,
    duration_ms: f32,
    easing: Easing,
}

impl Tween {
    pub fn constant(value: f32, now: Instant) -> Self {
        Tween { from: value, to: value, start: now, duration_ms: 0.0, easing: Easing::OutQuint }
    }

    pub fn value(&self, now: Instant) -> f32 {
        if self.duration_ms <= 0.0 {
            return self.to;
        }
        let t = now.duration_since(self.start).as_secs_f32() * 1000.0 / self.duration_ms;
        self.from + (self.to - self.from) * self.easing.apply(t)
    }

    pub fn retarget(&mut self, now: Instant, to: f32, duration_ms: f32, easing: Easing) {
        *self = Tween { from: self.value(now), to, start: now, duration_ms, easing };
    }
}

/// Wo im Zieh-Ablauf wir sind (osu!: DragRotationState).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum DragState {
    NotDragging,
    /// Taste ist unten, aber noch nicht weit genug bewegt.
    DragStarted,
    Rotating,
}

// Die Laenge der "Leine", an der der Zeiger seinen Schleppunkt hinter sich herzieht. Sie ist
// zugleich die Entfernung, ab der ueberhaupt gedreht wird -- osu!s Kommentar dazu: sonst
// "can have an annoying effect", jeder kleine Ruckler beim Klicken wuerde den Zeiger verdrehen.
const LEASH_PX: f32 = 80.0;
// Der krumme Versatz steht so im Original. Er richtet die gedrehte Zeigerform an der
// Ziehrichtung aus -- die Spitze zeigt sonst nicht dorthin, wo man herkommt.
const ROTATION_OFFSET_DEG: f32 = 24.3;

/// Druecken: Zeiger schrumpft auf 90 % und leuchtet rosa auf.
const PRESS_SCALE: f32 = 0.90;
const PRESS_MS: f32 = 800.0;
/// Loslassen: federt auf die volle Groesse zurueck, das Leuchten blendet aus.
const RELEASE_SCALE_MS: f32 = 500.0;
const RELEASE_FADE_MS: f32 = 500.0;
/// Drehen waehrend des Ziehens.
const ROTATE_MS: f32 = 120.0;

/// Der ganze Zeiger-Zustand.
pub struct CursorFx {
    scale: Tween,
    additive: Tween,
    rotation: Tween,
    drag: DragState,
    /// Der nachgezogene Schleppunkt: liegt immer hoechstens `LEASH_PX` hinter dem Zeiger und
    /// wird nur mitgeschleift, wenn der Zeiger die Leine straff zieht. Aus ihm ergibt sich die
    /// Blickrichtung der Spitze.
    tail: (f32, f32),
    /// Wie viele Tasten gerade unten sind -- osu! federt erst zurueck, wenn KEINE mehr haelt.
    buttons_down: u32,
    /// Welche Tasten (Bit je Taste) unten sind, s. `button`.
    held: u32,
    /// Der letzte Druck kam von der rechten Taste: dann leuchtet der Zeiger in Akzent 2 statt
    /// im Akzent, auch noch beim Ausblenden nach dem Loslassen.
    right: bool,
}

impl CursorFx {
    pub fn new(now: Instant) -> Self {
        CursorFx {
            scale: Tween::constant(1.0, now),
            additive: Tween::constant(0.0, now),
            rotation: Tween::constant(0.0, now),
            drag: DragState::NotDragging,
            tail: (0.0, 0.0),
            buttons_down: 0,
            held: 0,
            right: false,
        }
    }

    #[cfg(test)]
    pub fn scale(&self, now: Instant) -> f32 {
        self.scale.value(now)
    }

    pub fn additive_alpha(&self, now: Instant) -> f32 {
        self.additive.value(now).clamp(0.0, 1.0)
    }

    pub fn right_tint(&self) -> bool {
        self.right
    }

    pub fn rotation_deg(&self, now: Instant) -> f32 {
        self.rotation.value(now)
    }

    // Bewusst KEINE "laeuft gerade etwas?"-Abfrage: der Client zeichnet ohnehin in festem
    // Takt weiter (ControlFlow::WaitUntil), die Animation laeuft also von selbst ab. Eine
    // Abfrage, die niemand braucht, waere nur eine zweite Stelle, die man pflegen muss.

    /// Eine Maustaste ging runter.
    pub fn press(&mut self, now: Instant, pos: (f32, f32)) {
        self.buttons_down += 1;
        // osu! setzt die Groesse erst hart auf 1 und startet DANN das Schrumpfen -- ein zweiter
        // Klick waehrend des Ausfederns faengt also wieder oben an.
        self.scale = Tween::constant(1.0, now);
        self.scale.retarget(now, PRESS_SCALE, PRESS_MS, Easing::OutQuint);
        // FadeInFromZero: ebenfalls von vorn, nicht vom aktuellen Wert.
        self.additive = Tween::constant(0.0, now);
        self.additive.retarget(now, 1.0, PRESS_MS, Easing::OutQuint);

        if self.drag == DragState::NotDragging {
            self.drag = DragState::DragStarted;
            self.tail = pos;
        }
    }

    /// Taste `id` ging runter oder hoch. Anders als press/release zaehlt jede Taste nur
    /// einmal: geht ein Loslassen verloren (unter Windows beim Alt-Tab zurueck ins Spiel),
    /// heilt der naechste Klick derselben Taste den Zustand, statt dass der Zeiger fuer immer
    /// gedrueckt bleibt.
    pub fn button(&mut self, now: Instant, pos: (f32, f32), id: u32, down: bool) {
        let bit = 1u32 << id.min(31);
        if down == (self.held & bit != 0) {
            return;
        }
        self.held ^= bit;
        if down {
            self.right = id == 1;
            self.press(now, pos);
        } else {
            self.release(now);
        }
    }

    /// Alle Tasten los, z.B. bei Fokusverlust: ein Loslassen ausserhalb des Fensters kommt
    /// nie an.
    pub fn release_all(&mut self, now: Instant) {
        if self.buttons_down == 0 && self.held == 0 {
            return;
        }
        self.held = 0;
        self.buttons_down = 0;
        self.spring_back(now);
    }

    /// Eine Maustaste ging hoch.
    pub fn release(&mut self, now: Instant) {
        self.buttons_down = self.buttons_down.saturating_sub(1);
        if self.buttons_down > 0 {
            return;
        }
        self.spring_back(now);
    }

    fn spring_back(&mut self, now: Instant) {
        self.scale.retarget(now, 1.0, RELEASE_SCALE_MS, Easing::OutElastic);
        self.additive.retarget(now, 0.0, RELEASE_FADE_MS, Easing::OutQuint);
        if self.drag != DragState::NotDragging {
            self.drag = DragState::NotDragging;
            // Je weiter der Zeiger verdreht ist, desto laenger dreht er zurueck -- sonst
            // wirkte ein halber Umlauf genauso hastig wie ein kleiner Schwenk.
            let winkel = self.rotation.value(now);
            let dauer = 400.0 * (0.5 + winkel.abs() / 960.0);
            self.rotation.retarget(now, 0.0, dauer, Easing::OutElasticQuarter);
        }
    }

    /// Die Maus hat sich bewegt.
    pub fn mouse_move(&mut self, now: Instant, pos: (f32, f32)) {
        if self.drag == DragState::NotDragging {
            return;
        }
        let offset = (pos.0 - self.tail.0, pos.1 - self.tail.1);
        let distance = (offset.0 * offset.0 + offset.1 * offset.1).sqrt();
        // Solange die Leine durchhaengt, bleibt der Schleppunkt liegen -- und mit ihm die
        // Richtung. Erst wenn sie straff wird, wird er nachgezogen, und zwar nur so weit, dass
        // er GENAU `LEASH_PX` hinter dem Zeiger bleibt. Genau das macht aus dem starren
        // Ausrichten auf den Druckpunkt einen Schlitten, der hinterherrutscht: die Spitze zeigt
        // immer dorthin, wo der Zeiger zuletzt hergekommen ist, nicht dorthin, wo der Zug begann.
        if distance > LEASH_PX {
            let k = LEASH_PX / distance;
            self.tail = (pos.0 - offset.0 * k, pos.1 - offset.1 * k);
            self.drag = DragState::Rotating;
        }
        // Abstand 0 wuerde atan2(0,0) fragen -- osu! faengt genau das ab.
        if self.drag != DragState::Rotating || distance <= 0.0 {
            return;
        }
        let ziel = (-offset.0).atan2(offset.1).to_degrees() + ROTATION_OFFSET_DEG;
        // Immer den KURZEN Weg drehen: ohne diese Normierung nimmt der Zeiger beim Sprung
        // ueber die 180-Grad-Grenze den langen Weg einmal herum.
        let jetzt = self.rotation.value(now);
        let mut diff = (ziel - jetzt) % 360.0;
        if diff < -180.0 {
            diff += 360.0;
        }
        if diff > 180.0 {
            diff -= 360.0;
        }
        self.rotation.retarget(now, jetzt + diff, ROTATE_MS, Easing::OutQuint);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    fn nach(start: Instant, ms: u64) -> Instant {
        start + Duration::from_millis(ms)
    }

    #[test]
    // Die Eckpunkte jeder Easing-Kurve: bei 0 nichts, bei 1 fertig.
    fn die_easings_beginnen_bei_null_und_enden_bei_eins() {
        for e in [Easing::OutQuint, Easing::OutElastic, Easing::OutElasticQuarter] {
            assert!(e.apply(0.0).abs() < 1e-3, "{e:?} beginnt nicht bei 0: {}", e.apply(0.0));
            assert!((e.apply(1.0) - 1.0).abs() < 1e-3, "{e:?} endet nicht bei 1: {}", e.apply(1.0));
        }
    }

    #[test]
    // OutQuint ist schnell vorne: nach einem Viertel der Zeit schon ueber zwei Dritteln.
    fn out_quint_ist_vorne_schnell() {
        assert!(Easing::OutQuint.apply(0.25) > 0.75, "{}", Easing::OutQuint.apply(0.25));
        assert!(Easing::OutQuint.apply(0.5) > 0.96);
    }

    #[test]
    // Elastic MUSS ueber das Ziel hinausschwingen -- das ist der ganze Sinn des Zurueckfederns.
    fn out_elastic_schwingt_ueber_das_ziel() {
        let hoechstwert = (0..100).map(|i| Easing::OutElastic.apply(i as f32 / 100.0)).fold(0.0f32, f32::max);
        assert!(hoechstwert > 1.05, "OutElastic federt nicht über: {hoechstwert}");
    }

    #[test]
    fn ein_druck_laesst_den_zeiger_schrumpfen_und_aufleuchten() {
        let t0 = Instant::now();
        let mut fx = CursorFx::new(t0);
        assert_eq!(fx.scale(t0), 1.0);
        assert_eq!(fx.additive_alpha(t0), 0.0);

        fx.press(t0, (100.0, 100.0));
        let mitte = nach(t0, 400);
        assert!(fx.scale(mitte) < 1.0 && fx.scale(mitte) > PRESS_SCALE, "{}", fx.scale(mitte));
        assert!(fx.additive_alpha(mitte) > 0.5, "das Leuchten kommt schnell: {}", fx.additive_alpha(mitte));
        let ende = nach(t0, 800);
        assert!((fx.scale(ende) - PRESS_SCALE).abs() < 1e-3);
        assert!((fx.additive_alpha(ende) - 1.0).abs() < 1e-3);
    }

    #[test]
    // Loslassen MITTEN im Druck darf nicht springen: es geht vom aktuellen Wert weiter.
    fn loslassen_setzt_beim_aktuellen_wert_an() {
        let t0 = Instant::now();
        let mut fx = CursorFx::new(t0);
        fx.press(t0, (0.0, 0.0));
        let los = nach(t0, 200);
        let vorher = fx.scale(los);
        fx.release(los);
        assert!((fx.scale(los) - vorher).abs() < 1e-4, "beim Loslassen darf die Größe nicht springen");
        // Und am Ende ist er wieder ganz da.
        assert!((fx.scale(nach(t0, 200 + 500)) - 1.0).abs() < 1e-2);
        assert!(fx.additive_alpha(nach(t0, 200 + 500)) < 1e-2);
    }

    #[test]
    // Zwei Tasten: erst wenn die LETZTE losgelassen wird, federt der Zeiger zurueck.
    fn solange_eine_taste_haelt_bleibt_der_zeiger_klein() {
        let t0 = Instant::now();
        let mut fx = CursorFx::new(t0);
        fx.press(t0, (0.0, 0.0));
        fx.press(t0, (0.0, 0.0));
        fx.release(nach(t0, 800));
        assert!((fx.scale(nach(t0, 900)) - PRESS_SCALE).abs() < 1e-3, "eine Taste hält noch");
        fx.release(nach(t0, 900));
        assert!(fx.scale(nach(t0, 1400)) > 0.99, "jetzt muss er zurueck sein");
    }

    #[test]
    // Verlorenes Loslassen: der naechste Klick derselben Taste loest den Zeiger wieder, und
    // Fokusverlust laesst alles los.
    fn verlorenes_loslassen_haengt_nicht_fest() {
        let t0 = Instant::now();
        let mut fx = CursorFx::new(t0);
        fx.button(t0, (0.0, 0.0), 0, true);
        fx.button(nach(t0, 100), (0.0, 0.0), 0, true);
        fx.button(nach(t0, 200), (0.0, 0.0), 0, false);
        assert!(fx.additive_alpha(nach(t0, 800)) < 1e-2, "ein Klick muss reichen");

        fx.button(nach(t0, 1000), (0.0, 0.0), 1, true);
        fx.release_all(nach(t0, 1100));
        assert!(fx.scale(nach(t0, 1700)) > 0.99 && fx.additive_alpha(nach(t0, 1700)) < 1e-2);
        fx.button(nach(t0, 1800), (0.0, 0.0), 1, false);
        assert!(fx.additive_alpha(nach(t0, 1800)) < 1e-2, "Loslassen nach release_all aendert nichts");
    }

    #[test]
    // Kurze Wege drehen NICHT -- sonst verdreht sich der Zeiger bei jedem zittrigen Klick.
    fn unter_achtzig_pixeln_dreht_sich_nichts() {
        let t0 = Instant::now();
        let mut fx = CursorFx::new(t0);
        fx.press(t0, (500.0, 500.0));
        fx.mouse_move(nach(t0, 10), (540.0, 530.0)); // 50 px
        assert_eq!(fx.rotation_deg(nach(t0, 200)), 0.0);
        fx.mouse_move(nach(t0, 20), (600.0, 560.0)); // gut 116 px
        assert_ne!(fx.rotation_deg(nach(t0, 200)), 0.0, "ab 80 px muss er drehen");
    }

    #[test]
    // Die Drehung zeigt in die Ziehrichtung: nach rechts gezogen ergibt einen anderen Winkel
    // als nach unten, und beide liegen um den bekannten Versatz herum.
    fn die_richtung_bestimmt_den_winkel() {
        let t0 = Instant::now();
        let weit = 300.0;
        let mut rechts = CursorFx::new(t0);
        rechts.press(t0, (500.0, 500.0));
        rechts.mouse_move(nach(t0, 10), (500.0 + weit, 500.0));
        let mut runter = CursorFx::new(t0);
        runter.press(t0, (500.0, 500.0));
        runter.mouse_move(nach(t0, 10), (500.0, 500.0 + weit));
        // Nach dem Ende des 120-ms-Uebergangs steht der Zielwinkel.
        let a = rechts.rotation_deg(nach(t0, 200));
        let b = runter.rotation_deg(nach(t0, 200));
        assert!((a - b).abs() > 45.0, "verschiedene Richtungen müssen verschiedene Winkel geben: {a} / {b}");
        // atan2(-dx, dy): nach rechts -> -90 Grad, plus Versatz.
        assert!((a - (-90.0 + ROTATION_OFFSET_DEG)).abs() < 1.0, "{a}");
        assert!((b - ROTATION_OFFSET_DEG).abs() < 1.0, "{b}");
    }

    #[test]
    // Der kurze Weg: von 170 auf -170 Grad sind es 20 Grad ueber die Grenze, nicht 340 zurueck.
    fn der_zeiger_dreht_immer_den_kurzen_weg() {
        let t0 = Instant::now();
        let mut fx = CursorFx::new(t0);
        fx.press(t0, (500.0, 500.0));
        // Erst weit nach links unten ziehen, dann knapp darueber -- der Zielwinkel springt
        // dabei ueber die 180-Grad-Grenze.
        fx.mouse_move(nach(t0, 10), (500.0 - 300.0, 500.0 - 5.0));
        let vorher = fx.rotation_deg(nach(t0, 200));
        fx.mouse_move(nach(t0, 200), (500.0 - 300.0, 500.0 + 5.0));
        let nachher = fx.rotation_deg(nach(t0, 400));
        assert!((nachher - vorher).abs() < 180.0, "der lange Weg wurde genommen: {vorher} -> {nachher}");
    }

    #[test]
    // Der Kern des Schlittens: nach einem Richtungswechsel zeigt die Spitze in die NEUE
    // Richtung. Waere sie weiter am Druckpunkt ausgerichtet, ergaebe sich hier die Diagonale.
    fn nach_dem_richtungswechsel_zeigt_die_spitze_in_die_neue_richtung() {
        let t0 = Instant::now();
        let mut fx = CursorFx::new(t0);
        fx.press(t0, (500.0, 500.0));
        fx.mouse_move(nach(t0, 10), (900.0, 500.0)); // weit nach rechts
        // Jetzt nach unten, in mehreren Schritten wie eine echte Maus.
        for i in 1..=8 {
            fx.mouse_move(nach(t0, 10 + i * 10), (900.0, 500.0 + i as f32 * 50.0));
        }
        let winkel = fx.rotation_deg(nach(t0, 400));
        // Nach unten gezogen ist der Zielwinkel genau der Versatz (atan2(-0, +dy) = 0).
        assert!((winkel - ROTATION_OFFSET_DEG).abs() < 5.0, "die Spitze hängt noch am Druckpunkt: {winkel}");
    }

    #[test]
    // Die Leine haelt den Schleppunkt auf genau LEASH_PX Abstand -- egal wie weit gezogen wird.
    fn der_schleppunkt_bleibt_auf_leinenlaenge() {
        let t0 = Instant::now();
        let mut fx = CursorFx::new(t0);
        fx.press(t0, (0.0, 0.0));
        for i in 1..=20 {
            fx.mouse_move(nach(t0, i * 10), (i as f32 * 120.0, i as f32 * 70.0));
        }
        let pos = (20.0 * 120.0, 20.0 * 70.0);
        let d = ((pos.0 - fx.tail.0).powi(2) + (pos.1 - fx.tail.1).powi(2)).sqrt();
        assert!((d - LEASH_PX).abs() < 1e-2, "Leinenlänge verletzt: {d}");
    }

    #[test]
    // Durchhaengende Leine: bewegt sich der Zeiger auf den Schleppunkt ZU, bleibt der liegen.
    // Sonst wuerde der Schlitten auch rueckwaerts mitgerissen und die Spitze zappeln.
    fn eine_durchhaengende_leine_zieht_den_schlitten_nicht() {
        let t0 = Instant::now();
        let mut fx = CursorFx::new(t0);
        fx.press(t0, (500.0, 500.0));
        fx.mouse_move(nach(t0, 10), (700.0, 500.0));
        let vorher = fx.tail;
        fx.mouse_move(nach(t0, 20), (650.0, 500.0));
        assert_eq!(fx.tail, vorher, "bei lockerer Leine darf der Schlitten nicht rutschen");
    }

    #[test]
    fn beim_loslassen_dreht_er_zurueck_auf_null() {
        let t0 = Instant::now();
        let mut fx = CursorFx::new(t0);
        fx.press(t0, (500.0, 500.0));
        fx.mouse_move(nach(t0, 10), (800.0, 500.0));
        assert_ne!(fx.rotation_deg(nach(t0, 200)), 0.0);
        fx.release(nach(t0, 200));
        // Die Ruecklaufdauer haengt am Winkel; nach reichlich Zeit muss er bei 0 stehen.
        assert!(fx.rotation_deg(nach(t0, 2000)).abs() < 0.5, "{}", fx.rotation_deg(nach(t0, 2000)));
    }
}
