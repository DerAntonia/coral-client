// Zusatzmodule -- Funktionen, die Vanilla NICHT hat und die deshalb bewusst getrennt von
// allem anderen stehen. Erreichbar ueber das Modul-Menue (Keybind, s. main.rs
// KeyBindAction::OpenModuleMenu).
//
// Das erste Modul: erkennbar machen, WELCHES Totem gepoppt ist.
//
// Warum das ueberhaupt ein Problem ist: der Server schickt beim Pop nur ein
// EntityStatusS2CPacket mit Status 35 (EntityId + Statusbyte, sonst nichts). Die Hand steht
// nicht drin. Vanillas Anzeige hilft auch nicht weiter -- sie sucht sich fuer das
// Flug-Item einfach das ERSTE Totem, das noch irgendwo in einer Hand liegt
// (ClientPlayNetworkHandler, getActiveTotemOfUndying), also unter Umstaenden das falsche.
// Wer im CPvP das Hotbar-Totem nachlegt, obwohl das Zweithand-Totem gepoppt ist, steht beim
// naechsten Treffer ohne Schutz da.
//
// Die Hand ist trotzdem EINDEUTIG bestimmbar, ohne Raten und ohne Heuristik. Vanillas
// Verbrauchsregel (LivingEntity.tryUseDeathProtector) geht die Haende der Reihe nach durch
// und verbraucht das erste Totem, das sie findet -- in der Reihenfolge [MAIN_HAND, OFF_HAND]. Die HAUPTHAND hat also Vorrang: liegt dort ein
// Totem, poppt dieses -- die Zweithand kommt nur dran, wenn die Haupthand keines hat. Mehr
// braucht es nicht, und ein Vergleich von Leben oder "welcher Slot ist im naechsten Tick
// leer" waere sogar schlechter, weil beides von der Paketreihenfolge abhaengt.

// Aus welcher Hand das Totem kam.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TotemHand {
    // Der aktive Hotbar-Slot.
    MainHand,
    // Der Zweithand-Slot.
    OffHand,
}

impl TotemHand {
    pub fn label(self) -> &'static str {
        match self {
            TotemHand::MainHand => "HOTBAR",
            TotemHand::OffHand => "ZWEITHAND",
        }
    }

    // Einfarbige Kennzeichnung. Gruen/Cyan sind mit Absicht weit auseinander und beide auch
    // bei Rot-Gruen-Schwaeche unterscheidbar (deutlich verschiedene Helligkeit und Blauanteil)
    // -- eine Unterscheidung, die man im Kampf in einem Viertelsekunden-Blick treffen muss,
    // darf nicht an zwei aehnlichen Farbtoenen haengen.
    pub fn tint(self) -> [f32; 3] {
        match self {
            TotemHand::MainHand => [1.0, 0.85, 0.25],
            TotemHand::OffHand => [0.25, 0.75, 1.0],
        }
    }

    // Auf welche Seite das Flug-Item wandert: Haupthand rechts, Zweithand links. Dieselbe
    // Zuordnung wie beim Ton (s. sound_pan) und wie die Haende auf dem Bildschirm liegen.
    pub fn side(self) -> f32 {
        match self {
            TotemHand::MainHand => 1.0,
            TotemHand::OffHand => -1.0,
        }
    }

    // Stereo-Position des Signaltons, -1 = links, +1 = rechts.
    pub fn sound_pan(self) -> f32 {
        self.side()
    }

    // Als Byte fuer die threaduebergreifende Veroeffentlichung (s. App::totem_hand_published).
    // Ein AtomicU8 statt eines Mutex: der Sofort-Ton-Haken laeuft im Netzwerk-Task und soll
    // dort nichts sperren muessen.
    pub fn to_published(self) -> u8 {
        match self {
            TotemHand::MainHand => 0,
            TotemHand::OffHand => 1,
        }
    }

    // Umkehrung von to_published. Unbekannte Werte koennen nicht auftreten (nur diese
    // Funktion und to_published schreiben/lesen das Byte); die Zweithand ist trotzdem die
    // sichere Vorgabe, weil sie im CPvP der haeufigere Fall ist.
    pub fn from_published(value: u8) -> Self {
        if value == 0 {
            TotemHand::MainHand
        } else {
            TotemHand::OffHand
        }
    }

    // Tonhoehe des Signaltons. Zwei klar getrennte Toene (eine Quinte auseinander), damit
    // sie auch ohne Kopfhoerer -- also ohne die Seiteninformation -- unterscheidbar sind.
    pub fn sound_hz(self) -> f32 {
        match self {
            TotemHand::MainHand => 1320.0,
            TotemHand::OffHand => 880.0,
        }
    }
}

// Merkt sich, was in den beiden Haenden lag, und bestimmt daraus beim Pop die Hand.
//
// Der Grund fuer die Historie: das Statuspaket und die Slot-Aktualisierung sind ZWEI Pakete.
// Der Server verbraucht das Totem, schickt dann den Status, und die neuen Slotinhalte kommen
// am Ende des Server-Ticks. In der Praxis kommt der Status also zuerst, aber darauf will ich
// mich nicht verlassen -- welche Reihenfolge tatsaechlich beim Client ankommt, kann ich ohne
// laufenden Server nicht nachpruefen, und ein Modul, das bei jedem zwanzigsten Pop die
// falsche Hand nennt, ist schlimmer als keines.
//
// Deshalb beide Faelle abgedeckt: liegt jetzt noch ein Totem in der Haupthand, war es die
// Haupthand (Paket noch nicht eingetroffen, oder der Stapel war groesser als 1). Lag im Tick
// davor eines dort und jetzt nicht mehr, war es AUCH die Haupthand (Aktualisierung war
// schneller). Erst wenn beides nicht zutrifft, bleibt die Zweithand.
#[derive(Clone, Copy, Debug, Default)]
pub struct TotemTracker {
    main_hand_had_totem: bool,
    off_hand_had_totem: bool,
}

impl TotemTracker {
    pub fn new() -> Self {
        Self::default()
    }

    // Einmal pro Tick mit dem aktuellen Stand aufrufen, BEVOR resolve() gebraucht wird.
    pub fn tick(&mut self, main_hand_has_totem: bool, off_hand_has_totem: bool) {
        self.main_hand_had_totem = main_hand_has_totem;
        self.off_hand_had_totem = off_hand_has_totem;
    }

    // Welche Hand hat gepoppt? `*_now` ist der Stand in genau diesem Augenblick.
    pub fn resolve(&self, main_hand_has_totem: bool, off_hand_has_totem: bool) -> TotemHand {
        if main_hand_has_totem || self.main_hand_had_totem {
            return TotemHand::MainHand;
        }
        // Nur der Vollstaendigkeit wegen abgefragt: hatte die Zweithand eines, ist es
        // ohnehin sie. Hatte sie auch keines (beide Snapshots veraltet, etwa direkt nach dem
        // Verbinden), bleibt Zweithand die richtige Antwort -- ein Totem MUSS gepoppt sein,
        // sonst haette der Server den Status nicht geschickt, und die Haupthand ist oben
        // schon ausgeschlossen.
        let _ = (off_hand_has_totem, self.off_hand_had_totem);
        TotemHand::OffHand
    }
}

// Vanillas Flug-Item-Animation (InGameOverlayRenderer.renderFloatingItem), 40 Ticks lang.
pub const TOTEM_ANIMATION_TICKS: u32 = 40;

// Der Zustand einer laufenden Anzeige.
#[derive(Clone, Copy, Debug)]
pub struct TotemPopAnimation {
    pub hand: TotemHand,
    // Restliche Ticks, wie Vanillas floatingItemTimer.
    ticks_left: u32,
}

impl TotemPopAnimation {
    pub fn start(hand: TotemHand) -> Self {
        Self { hand, ticks_left: TOTEM_ANIMATION_TICKS }
    }

    // Einmal pro Tick (InGameOverlayRenderer.tickFloatingItemTimer).
    pub fn tick(&mut self) -> bool {
        self.ticks_left = self.ticks_left.saturating_sub(1);
        self.ticks_left > 0
    }

    // Fortschritt 0..1, zwischen den Ticks interpoliert -- Vanillas
    // `f = (40 - timer + tickProgress) / 40`.
    pub fn progress(&self, tick_progress: f32) -> f32 {
        let elapsed = (TOTEM_ANIMATION_TICKS - self.ticks_left) as f32;
        ((elapsed + tick_progress) / TOTEM_ANIMATION_TICKS as f32).clamp(0.0, 1.0)
    }
}

// Vanillas Bewegungskurve, 1:1:
//
//     g = f*f;  h = f*g;
//     j = 10.25*h*g - 24.95*g*g + 25.5*h - 13.8*g + 4.0*f
//     k = j * PI
//
// Das Polynom ist keine glatte Rampe, aber auch kein Hin und Her: k steigt EINMAL monoton
// von 0 auf genau PI und haengt dabei in der Mitte auf einem Plateau (j bleibt von f=0.3 bis
// f=0.7 bei etwa 0.5, nachgerechnet in kurve_steigt_einmal_monoton_mit_plateau_in_der_mitte).
// Daraus entsteht die Bewegung: sin(k) macht einen einzigen Bogen -- heranschnellen, kurz
// vor dem Gesicht verweilen, zurueckweichen --, waehrend der Versatz mit |sin(2k)| zweimal
// ausschwingt. Deshalb ist die Kurve abgeschrieben und nicht durch eine glatte Rampe
// ersetzt: die haette dasselbe Anfangs- und Endbild und saehe trotzdem falsch aus.
pub fn animation_phase(progress: f32) -> f32 {
    let f = progress;
    let g = f * f;
    let h = f * g;
    let j = 10.25 * h * g - 24.95 * g * g + 25.5 * h - 13.8 * g + 4.0 * f;
    j * std::f32::consts::PI
}

// Was die Anzeige in diesem Augenblick zeichnen soll.
//
// Vanilla zeichnet ein 3D-Item-Modell in einem perspektivischen Overlay: es wandert in der
// Tiefe (-10 + 9*sin k), dreht sich 900 Grad um Y und taumelt 6 Grad um X/Z. Dieser Client
// zeichnet Items ueberall als flache Sprites (s. item_icons), also wird daraus die
// gleichwertige 2D-Fassung -- und zwar aus DENSELBEN Kurven:
//
//   * die Tiefe wird zur Groesse (naeher = groesser, 1/Abstand wie bei einer Perspektive)
//   * die Y-Drehung wird zum waagerechten Stauchen (ein flaches Bild, um Y gedreht, wird
//     genau um |cos| schmaler) -- das ergibt dasselbe "Umklappen" wie beim 3D-Modell
//   * der Versatz laeuft wie in Vanilla mit |sin(2k)|, nur mit fester Seite statt zufaellig:
//     die Seite IST hier die Information (s. TotemHand::side)
//
// Das ist ausdruecklich eine Uebertragung, keine pixelgleiche Kopie -- mit flachen Sprites
// kann es keine sein.
pub struct TotemFrame {
    // Versatz aus der Bildschirmmitte, in Bruchteilen der halben Bildschirmhoehe.
    pub offset_x: f32,
    pub offset_y: f32,
    // Groessenfaktor.
    pub scale: f32,
    // Waagerechte Stauchung 0..1 (die Drehung).
    pub squash: f32,
}

impl TotemPopAnimation {
    pub fn frame(&self, tick_progress: f32, aspect: f32) -> TotemFrame {
        let f = self.progress(tick_progress);
        let k = animation_phase(f);
        let swing = (k * 2.0).sin().abs();
        // Vanilla: m = offsetX * 0.3 * aspect, n = offsetY * 0.3; der Versatz laeuft mit
        // |sin(2k)|. offsetX ist bei uns die Seite (fest), offsetY bleibt bei 0 -- die
        // senkrechte Zufallskomponente wuerde die Seiteninformation nur verwaschen.
        let offset_x = self.hand.side() * 0.3 * aspect * swing;
        let offset_y = 0.0;
        // Tiefe -10 + 9*sin(k): bei sin(k) = 1 ist das Item bei -1 (ganz nah), bei -1 bei
        // -19 (fern). Als Groesse: der Kehrwert des Abstands, auf die Naehe normiert, damit
        // der groesste Zustand 1.0 ist. Vanillas Grundskalierung 0.8 steckt mit drin.
        let depth = 10.0 - 9.0 * k.sin();
        let scale = 0.8 * (1.0 / depth.max(0.5));
        // 900 Grad * |sin k| um Y -- als flaches Bild die Breite mal |cos| davon. Ein
        // Mindestwert, damit das Sprite beim Kanten-an nicht ganz verschwindet und flackert.
        let spin = 900.0f32.to_radians() * k.sin().abs();
        let squash = spin.cos().abs().max(0.05);
        TotemFrame { offset_x, offset_y, scale, squash }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Der Kern des Moduls: die Haupthand hat Vorrang (Vanillas Hand.values() mit break).
    #[test]
    fn haupthand_hat_vorrang() {
        let tracker = TotemTracker::new();
        assert_eq!(tracker.resolve(true, true), TotemHand::MainHand, "beide Hände voll -> Haupthand poppt");
        assert_eq!(tracker.resolve(true, false), TotemHand::MainHand);
        assert_eq!(tracker.resolve(false, true), TotemHand::OffHand);
    }

    // Der eigentliche Anwendungsfall: Waffe in der Haupthand, Totem in der
    // Zweithand. Muss ZWEITHAND ergeben -- die Verwechslung hier hat den Tod gekostet.
    #[test]
    fn waffe_in_der_hand_und_totem_in_der_zweithand_ergibt_zweithand() {
        let mut tracker = TotemTracker::new();
        tracker.tick(false, true);
        assert_eq!(tracker.resolve(false, true), TotemHand::OffHand);
        // Und auch dann, wenn die Slot-Aktualisierung schon durch ist (Zweithand jetzt leer).
        assert_eq!(tracker.resolve(false, false), TotemHand::OffHand);
    }

    // Die Slot-Aktualisierung kann VOR dem Statuspaket ankommen. Dann ist die Haupthand im
    // Augenblick des Pops schon leer, und nur der Tick davor verraet noch, dass dort ein
    // Totem lag. Ohne diese Historie waere die Antwort falsch -- und zwar genau in der
    // Richtung, die dem Nutzer schon einmal teuer geworden ist.
    #[test]
    fn slot_aktualisierung_vor_dem_status_wird_erkannt() {
        let mut tracker = TotemTracker::new();
        tracker.tick(true, true); // beide Haende hatten eines
        assert_eq!(tracker.resolve(false, true), TotemHand::MainHand, "Haupthand war es, auch wenn sie jetzt leer ist");
    }

    // Ein Totem in der Haupthand, das den Stapel nicht leert (2 Totems im Slot): der Slot
    // ist danach nicht leer, die Antwort muss trotzdem Haupthand sein.
    #[test]
    fn voller_stapel_in_der_haupthand() {
        let mut tracker = TotemTracker::new();
        tracker.tick(true, false);
        assert_eq!(tracker.resolve(true, false), TotemHand::MainHand);
    }

    // Ohne jede Vorgeschichte (frisch verbunden, noch kein Tick gelaufen) darf nichts
    // paniken, und die Antwort muss die plausible sein: die Haupthand ist ausgeschlossen,
    // also war es die Zweithand.
    #[test]
    fn ohne_vorgeschichte_bleibt_zweithand() {
        assert_eq!(TotemTracker::new().resolve(false, false), TotemHand::OffHand);
    }

    // Die Animation laeuft genau 40 Ticks und meldet sich dann als fertig.
    #[test]
    fn animation_laeuft_vierzig_ticks() {
        let mut anim = TotemPopAnimation::start(TotemHand::MainHand);
        let mut ticks = 0;
        while anim.tick() {
            ticks += 1;
            assert!(ticks < 100, "Endlosschleife");
        }
        assert_eq!(ticks + 1, TOTEM_ANIMATION_TICKS as i32, "40 Ticks, dann Schluss");
        assert_eq!(anim.progress(0.0), 1.0);
    }

    // Vanillas Kurve, an drei Stuetzstellen nachgerechnet. Sie ist der Grund fuer das
    // charakteristische Zucken -- eine glatte Ersatzkurve saehe sofort falsch aus.
    #[test]
    fn kurve_stimmt_mit_vanilla_ueberein() {
        // f = 0 -> alle Terme 0.
        assert_eq!(animation_phase(0.0), 0.0);
        // f = 1: 10.25 - 24.95 + 25.5 - 13.8 + 4.0 = 1.0, also genau PI.
        let bei_eins = animation_phase(1.0);
        assert!((bei_eins - std::f32::consts::PI).abs() < 1e-4, "bei f=1 muss k genau PI sein, war {bei_eins}");
        // f = 0.5: 10.25*0.125*0.25 - 24.95*0.0625 + 25.5*0.125 - 13.8*0.25 + 2.0
        //        = 0.3203125 - 1.559375 + 3.1875 - 3.45 + 2.0 = 0.4984375
        let bei_halb = animation_phase(0.5) / std::f32::consts::PI;
        assert!((bei_halb - 0.498_437_5).abs() < 1e-4, "bei f=0.5 war der Faktor {bei_halb}");
    }

    // Die Form der Kurve, nachgerechnet statt vermutet: k laeuft EINMAL monoton von 0 bis
    // PI, und in der Mitte fast gar nicht (j bleibt zwischen f=0.3 und f=0.7 bei etwa 0.5).
    // Daraus entsteht die Bewegung: sin(k) macht einen einzigen Bogen -- das Item schnellt
    // heran und weicht zurueck -- und weil j in der Mitte auf einem Plateau liegt, haengt es
    // dort einen Moment vor dem Gesicht. Genau dieser Verlauf macht die Animation
    // wiedererkennbar; eine gleichmaessige Rampe haette dasselbe Anfangs- und Endbild und
    // saehe trotzdem voellig anders aus.
    //
    // Mein erster Anlauf dieses Tests behauptete das Gegenteil (mehrfaches Schwingen) und
    // ist deshalb fehlgeschlagen -- die Zahlen unten sind ausgerechnet, nicht geschaetzt.
    #[test]
    fn kurve_steigt_einmal_monoton_mit_plateau_in_der_mitte() {
        let mut letztes = animation_phase(0.0);
        for i in 1..=400 {
            let jetzt = animation_phase(i as f32 / 400.0);
            assert!(jetzt >= letztes - 1e-5, "k muss monoton steigen, Bruch bei f={}", i as f32 / 400.0);
            assert!(jetzt <= std::f32::consts::PI + 1e-4, "k darf PI nicht überschreiten, war {jetzt}");
            letztes = jetzt;
        }
        // Das Plateau: zwischen f=0.3 und f=0.7 bewegt sich der Faktor kaum.
        let bei = |f: f32| animation_phase(f) / std::f32::consts::PI;
        assert!((bei(0.3) - 0.469).abs() < 1e-3, "f=0.3 -> {}", bei(0.3));
        assert!((bei(0.7) - 0.517).abs() < 1e-3, "f=0.7 -> {}", bei(0.7));
        assert!((bei(0.7) - bei(0.3)).abs() < 0.06, "die Mitte muss ein Plateau sein");
        // Und sin(k) wechselt folglich NIE das Vorzeichen: ein Bogen, kein Hin und Her.
        for i in 0..=400 {
            assert!(animation_phase(i as f32 / 400.0).sin() >= -1e-6, "sin(k) darf nicht negativ werden");
        }
    }

    // Der Versatz laeuft mit |sin(2k)| und schwingt deshalb ZWEIMAL aus, waehrend das Item
    // nur einmal herankommt. Auch das ist nachgerechnet (zwei Huegel ueber die Laufzeit).
    #[test]
    fn der_versatz_schwingt_zweimal_aus() {
        let werte: Vec<f32> = (0..=400).map(|i| (animation_phase(i as f32 / 400.0) * 2.0).sin().abs()).collect();
        let huegel = (1..400).filter(|&i| werte[i] > werte[i - 1] && werte[i] >= werte[i + 1]).count();
        assert_eq!(huegel, 2, "|sin(2k)| muss genau zwei Hügel haben, waren {huegel}");
    }

    // Die beiden Haende MUESSEN sich in allen drei Kanaelen unterscheiden -- das ist der
    // ganze Sinn des Moduls. Ein Gleichstand in einem Kanal waere ein stiller Rueckschritt.
    #[test]
    fn beide_haende_sind_in_farbe_seite_und_ton_verschieden() {
        let (a, b) = (TotemHand::MainHand, TotemHand::OffHand);
        assert_ne!(a.tint(), b.tint(), "Farbe");
        assert_ne!(a.side(), b.side(), "Seite");
        assert_ne!(a.sound_hz(), b.sound_hz(), "Tonhöhe");
        assert_ne!(a.sound_pan(), b.sound_pan(), "Stereo-Seite");
        assert_ne!(a.label(), b.label(), "Beschriftung");
        // Und die Seiten muessen tatsaechlich entgegengesetzt sein, nicht nur verschieden.
        assert!(a.side() > 0.0 && b.side() < 0.0, "Haupthand rechts, Zweithand links");
    }

    // Das Flug-Item darf nie aus dem Bild laufen und nie unsichtbar werden.
    #[test]
    fn animation_bleibt_im_sichtbaren_bereich() {
        for hand in [TotemHand::MainHand, TotemHand::OffHand] {
            let mut anim = TotemPopAnimation::start(hand);
            loop {
                for schritt in 0..4 {
                    let frame = anim.frame(schritt as f32 / 4.0, 16.0 / 9.0);
                    assert!(frame.offset_x.abs() <= 0.6, "Versatz zu groß: {}", frame.offset_x);
                    assert!(frame.scale > 0.0 && frame.scale <= 2.0, "Größe unplausibel: {}", frame.scale);
                    assert!(frame.squash >= 0.05 && frame.squash <= 1.0, "Stauchung unplausibel: {}", frame.squash);
                }
                if !anim.tick() {
                    break;
                }
            }
        }
    }

    // Die Seite des Versatzes muss immer dieselbe sein -- ueber die ganze Animation. Ein
    // Vorzeichenwechsel mittendrin (etwa durch sin statt |sin|) wuerde die Information
    // zerstoeren, obwohl die Animation weiter "richtig" aussieht.
    #[test]
    fn die_seite_wechselt_nie_waehrend_der_animation() {
        for hand in [TotemHand::MainHand, TotemHand::OffHand] {
            let mut anim = TotemPopAnimation::start(hand);
            loop {
                let frame = anim.frame(0.5, 16.0 / 9.0);
                assert!(frame.offset_x * hand.side() >= 0.0, "{:?}: Versatz {} zeigt auf die falsche Seite", hand, frame.offset_x);
                if !anim.tick() {
                    break;
                }
            }
        }
    }
}
