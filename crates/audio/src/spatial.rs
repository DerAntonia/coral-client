//! Wo kommt der Ton her? -- das Ohrenmodell.
//!
//! Bis hierher konnte der Mixer eine Stimme nur PANORAMIEREN: links lauter, rechts leiser
//! (s. `pan_gains`). Das ist keine Ortung. Ein reiner Lautstaerkeunterschied ist mehrdeutig --
//! ein leiser Schritt geradeaus und ein lauter Schritt weit links klingen gleich, und die
//! Richtung faellt mit der Entfernung zusammen. Deshalb ortet man ueber Kopfhoerer mit reinem
//! Panorama nichts, man hoert nur "mehr auf der einen Seite".
//!
//! Der tragende Hinweis ist die LAUFZEIT: die Ohren stehen rund 17,5 cm auseinander, Schall
//! braucht fuer diesen Umweg um den Kopf bis zu 0,66 ms, und das Gehirn misst genau diesen
//! Versatz. Er haengt NUR an der Richtung, nicht an der Entfernung und nicht an der
//! Lautstaerke -- deshalb ist er eindeutig, wo das Panorama es nicht ist.
//!
//! Dieses Modul rechnet Richtung + Entfernung in die drei Groessen um, die der Mixer pro Ohr
//! braucht: Verzoegerung, Pegel, Grenzfrequenz. Es kennt weder Weltkoordinaten noch die
//! Kamera -- der Aufrufer reicht die Richtung schon im Kopf-System herein (s. `Placement`).
//!
//! **Was das Modell NICHT kann.** Vorne und hinten klingen gleich, und oben und unten auch.
//! Das ist kein Versaeumnis, sondern die Grenze eines Kugelkopfs: alle Punkte auf einem Kegel
//! um die Ohrachse haben dieselbe Laufzeitdifferenz (der "Verwechslungskegel"). Echte Ohren
//! loesen das ueber die Ohrmuschel, die aus jeder Richtung ein anderes Kammfilter macht --
//! das braucht gemessene HRTF-Daten, und die haette der Client nicht. Wichtig fuer die
//! Erwartung: mit Kopfhoerern wird die Seite sicher, die Front/Rueck-Unterscheidung nicht.

/// Wo eine Schallquelle relativ zum Kopf steht.
pub struct Placement {
    /// Seitwaerts-Anteil der Richtung zur Quelle: -1 = genau links, 0 = Mittelebene
    /// (vorne, hinten, oben, unten), +1 = genau rechts.
    ///
    /// Warum genau diese eine Zahl und kein Winkelpaar: die Ohrachse ist die einzige Achse,
    /// die fuer die Laufzeit zaehlt, und das Skalarprodukt der Richtung mit dieser Achse ist
    /// bereits sin(Azimut) * cos(Hoehenwinkel) -- also genau der Wert, den die
    /// Laufzeitformel braucht. Der Aufrufer bildet ihn als
    /// `richtung_zur_quelle.dot(rechts_vektor)` (s. `render::Camera::right`); der ist vom
    /// Nicken unabhaengig, weil eine Drehung UM die Ohrachse die Ohrachse nicht bewegt.
    pub lateral: f32,
    /// Abstand in Bloecken (= Metern).
    pub distance: f32,
}

/// Was der Mixer fuer EIN Ohr braucht.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Ear {
    /// Zusaetzliche Laufzeit bis zu diesem Ohr, in Sekunden. Immer >= 0; das naehere Ohr
    /// bekommt 0, das fernere den vollen Versatz.
    ///
    /// Bewusst RELATIV zwischen den Ohren und nicht die absolute Laufzeit von der Quelle
    /// hierher: 50 Bloecke Entfernung waeren echte 145 ms, und ein Ton, der 145 ms nach dem
    /// Ereignis kommt, ist als Reaktionssignal wertlos (s. Ton-Latenz-Anforderung). Fuer die
    /// Ortung traegt die absolute Laufzeit ohnehin nichts bei -- man kennt den Abschusszeit-
    /// punkt ja nicht. Nur der UNTERSCHIED zwischen den Ohren ortet.
    pub delay: f32,
    /// Lautstaerkefaktor: Entfernung mal Kopfabschattung.
    pub gain: f32,
    /// Grenzfrequenz des Tiefpasses vor diesem Ohr, in Hz. Traegt zwei Effekte zugleich:
    /// den Kopfschatten (das abgewandte Ohr hoert dumpfer) und die Luftdaempfung (weit weg
    /// heisst dumpfer). Ein Wert oberhalb der halben Abtastrate bedeutet "kein Filter".
    pub cutoff: f32,
}

/// Beide Ohren.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Spatial {
    pub left: Ear,
    pub right: Ear,
}

/// Halber Ohrabstand in Metern. Rund 17,5 cm zwischen den Gehoergaengen ist der ueblich
/// angesetzte Mittelwert erwachsener Koepfe.
const HEAD_RADIUS: f32 = 0.0875;

/// Schallgeschwindigkeit in Metern pro Sekunde, trockene Luft bei 20 Grad.
const SPEED_OF_SOUND: f32 = 343.0;

/// Groesste Laufzeitdifferenz, die dieses Modell erzeugen kann (Quelle genau seitlich):
/// (r/c) * (pi/2 + 1) -- rund 0,66 ms. Der Mixer dimensioniert daran seine Verzoegerungs-
/// leitung, deshalb steht der Wert hier und wird nicht dort zweitgeschaetzt.
pub const MAX_ITD: f32 = HEAD_RADIUS / SPEED_OF_SOUND * (std::f32::consts::FRAC_PI_2 + 1.0);

/// Grenzfrequenz vor dem vollstaendig abgeschatteten Ohr. Der Kopf ist fuer tiefe Toene
/// (grosse Wellenlaenge) durchlaessig und wirft erst oberhalb von ein bis zwei Kilohertz
/// einen echten Schatten -- genau deshalb ORTET das Gehirn tiefe Toene ueber die Laufzeit und
/// hohe ueber den Pegel.
const SHADOW_CUTOFF: f32 = 1500.0;

/// "Kein Filter". Liegt oberhalb des Hoerbereichs; der Mixer schaltet den Tiefpass ab, sobald
/// die Grenzfrequenz nah an die halbe Abtastrate kommt.
pub const OPEN_CUTOFF: f32 = 20_000.0;

/// Breitbandiger Pegelverlust am abgewandten Ohr bei voller Seitenlage. -3 dB; der groessere
/// Teil des Kopfschattens steckt in der Klangfarbe, nicht in der Gesamtlautstaerke, und den
/// traegt hier die Grenzfrequenz.
const SHADOW_ATTENUATION: f32 = 0.3;

/// Abstand, ab dem die Lautstaerke zu fallen beginnt. Innerhalb davon bleibt ein Klang voll
/// laut -- ohne diese Schwelle wuerde eine Quelle direkt am Ohr ins Unendliche laut.
const REFERENCE_DISTANCE: f32 = 1.5;

/// Ab hier ist nichts mehr zu hoeren. Vanilla klemmt bei 16 Bloecken mal Lautstaerke; hier
/// ist mehr Reichweite gewollt, weil der Ton Spielsignal ist und ein herannahender Gegner
/// frueher hoerbar sein soll als er sichtbar ist.
const MAX_DISTANCE: f32 = 64.0;

/// Letztes Stueck vor `MAX_DISTANCE`, ueber das linear auf null ausgeblendet wird. Ohne diese
/// Rampe saesse am Reichweitenrand eine Stufe: ein Klang waere noch mit gut einem Zwanzigstel
/// zu hoeren und beim naechsten Block schlagartig weg.
const FADE_DISTANCE: f32 = 16.0;

/// Weglaenge, ueber die die Luft die Hoehen wegnimmt. Die Grenzfrequenz faellt exponentiell
/// mit `exp(-abstand / ABSORPTION_LENGTH)`: bei 40 Bloecken bleiben rund 7 kHz, bei 64 rund
/// 4 kHz. Groessenordnung und Richtung stimmen mit echter Luftdaempfung ueberein (die trifft
/// hohe Frequenzen um ein Vielfaches staerker als tiefe); als exakte Absorptionskurve ist es
/// nicht gemeint -- gebraucht wird ein hoerbarer ENTFERNUNGS-Hinweis neben der Lautstaerke,
/// denn Lautstaerke allein verwechselt man mit einem leisen nahen Geraeusch.
const ABSORPTION_LENGTH: f32 = 40.0;

/// Richtung + Entfernung in die Ohrgroessen umrechnen.
pub fn spatial_for(placement: &Placement) -> Spatial {
    let lateral = placement.lateral.clamp(-1.0, 1.0);
    let distance = placement.distance.max(0.0);

    // Seitenwinkel gegen die Mittelebene, -pi/2 .. +pi/2. asin faltet dabei genau richtig:
    // vorne-rechts und hinten-rechts haben denselben Seitwaerts-Anteil und damit dieselbe
    // Laufzeit -- der Verwechslungskegel aus dem Modulkommentar, kein Rundungsfehler.
    let angle = lateral.asin();

    // Woodworths Formel fuer den Umweg um eine Kugel: der Schall laeuft zum fernen Ohr
    // r*sin(theta) laenger geradeaus UND r*theta um die Kopfrundung herum. Deshalb
    // theta + sin(theta) und nicht einfach sin(theta) -- der Bogenanteil macht bei voller
    // Seitenlage mehr als die Haelfte des Versatzes aus, ein reines sin(theta) waere um
    // 60 % zu klein.
    let itd = HEAD_RADIUS / SPEED_OF_SOUND * (angle + angle.sin());

    // Positiv = rechts. Das rechte Ohr ist dann NAEHER, bekommt also keine Verzoegerung,
    // und das linke die volle.
    let (delay_left, delay_right) = if itd >= 0.0 { (itd, 0.0) } else { (0.0, -itd) };

    let distance_gain = distance_gain(distance);
    // Der Kopfschatten haengt am Betrag der Seitenlage: bei |lateral| = 1 steht ein Ohr voll
    // im Schatten, in der Mittelebene keins.
    let shadow = lateral.abs();
    let air_cutoff = OPEN_CUTOFF * (-distance / ABSORPTION_LENGTH).exp();

    // Der Schatten trifft immer nur das ABGEWANDTE Ohr. Geometrisch interpoliert, damit der
    // Weg von 20 kHz auf 1,5 kHz gleichmaessig in Oktaven verlaeuft -- linear waere er in der
    // Mitte schon fast ganz unten, weil zwischen 20 kHz und 1,5 kHz gut dreieinhalb Oktaven
    // liegen.
    let shadow_cutoff = OPEN_CUTOFF * (SHADOW_CUTOFF / OPEN_CUTOFF).powf(shadow);
    let (cutoff_left, cutoff_right) = if lateral >= 0.0 { (shadow_cutoff, OPEN_CUTOFF) } else { (OPEN_CUTOFF, shadow_cutoff) };
    let shadow_gain = 1.0 - SHADOW_ATTENUATION * shadow;
    let (gain_left, gain_right) = if lateral >= 0.0 { (shadow_gain, 1.0) } else { (1.0, shadow_gain) };

    Spatial {
        left: Ear { delay: delay_left, gain: distance_gain * gain_left, cutoff: cutoff_left.min(air_cutoff) },
        right: Ear { delay: delay_right, gain: distance_gain * gain_right, cutoff: cutoff_right.min(air_cutoff) },
    }
}

/// Lautstaerke ueber die Entfernung: bis `REFERENCE_DISTANCE` voll, danach 1/r wie im freien
/// Feld, am Rand linear auf null ausgeblendet.
///
/// Kein konstanter Leistungsausgleich wie beim Panorama (s. `pan_gains`): dort wandert
/// dieselbe Energie zwischen den Seiten und die Summe soll gleich bleiben, hier wird sie
/// tatsaechlich weniger.
pub fn distance_gain(distance: f32) -> f32 {
    if distance >= MAX_DISTANCE {
        return 0.0;
    }
    let inverse = REFERENCE_DISTANCE / distance.max(REFERENCE_DISTANCE);
    let fade = ((MAX_DISTANCE - distance) / FADE_DISTANCE).clamp(0.0, 1.0);
    inverse * fade
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bei(lateral: f32, distance: f32) -> Spatial {
        spatial_for(&Placement { lateral, distance })
    }

    // Der Kern der ganzen Uebung: seitlich MUSS ein Laufzeitversatz entstehen, und zwar mit
    // dem naeheren Ohr zuerst.
    #[test]
    fn seitliche_quelle_erreicht_das_nahe_ohr_zuerst() {
        let rechts = bei(1.0, 5.0);
        assert_eq!(rechts.right.delay, 0.0, "das nahe Ohr wartet nicht");
        assert!(rechts.left.delay > 0.0, "das ferne Ohr muss spaeter dran sein");

        let links = bei(-1.0, 5.0);
        assert_eq!(links.left.delay, 0.0);
        assert!(links.right.delay > 0.0);
        // Spiegelbildlich: derselbe Betrag, nur die Seite getauscht.
        assert!((links.right.delay - rechts.left.delay).abs() < 1e-9);
    }

    // Geradeaus (und ebenso senkrecht darueber, gleicher Seitwaerts-Anteil) darf KEIN Versatz
    // entstehen -- sonst wanderte ein Klang aus der Mitte, obwohl er dort steht.
    #[test]
    fn die_mittelebene_bleibt_versatzfrei_und_symmetrisch() {
        let mitte = bei(0.0, 5.0);
        assert_eq!(mitte.left.delay, 0.0);
        assert_eq!(mitte.right.delay, 0.0);
        assert_eq!(mitte.left.gain, mitte.right.gain);
        assert_eq!(mitte.left.cutoff, mitte.right.cutoff);
    }

    // Die Groessenordnung ist der Punkt, nicht nur das Vorzeichen: liegt der Versatz daneben,
    // klingt es entweder wie eine Mono-Quelle (zu klein) oder wie ein Echo (zu gross). Der
    // menschliche Hoechstwert liegt bei 0,6 bis 0,7 ms.
    #[test]
    fn der_versatz_liegt_im_menschlichen_bereich() {
        let voll = bei(1.0, 5.0);
        assert!((voll.left.delay - MAX_ITD).abs() < 1e-9, "volle Seitenlage = MAX_ITD");
        assert!((0.0006..=0.0007).contains(&voll.left.delay), "0,6-0,7 ms erwartet, war {} s", voll.left.delay);
    }

    // Zwischenrichtungen muessen sich einordnen, sonst waere die Ortung stufig.
    #[test]
    fn der_versatz_waechst_mit_der_seitenlage() {
        let mut vorher = 0.0;
        for schritt in 0..=10 {
            let d = bei(schritt as f32 / 10.0, 5.0).left.delay;
            assert!(d >= vorher, "Schritt {schritt}: {d} < {vorher}");
            vorher = d;
        }
    }

    // Vorne und hinten (gleicher Seitwaerts-Anteil, entgegengesetzte Blickrichtung) klingen
    // gleich. Festgehalten, damit es als ENTSCHEIDUNG dasteht und nicht eines Tages als Bug
    // "repariert" wird: ohne Ohrmuschel-Daten geht es nicht anders.
    #[test]
    fn vorne_und_hinten_sind_nicht_unterscheidbar() {
        // 45 Grad rechts vorne und 135 Grad rechts hinten haben beide lateral = sin(45 Grad).
        let vorne = bei(std::f32::consts::FRAC_PI_4.sin(), 5.0);
        let hinten = bei((std::f32::consts::PI - std::f32::consts::FRAC_PI_4).sin(), 5.0);
        assert_eq!(vorne, hinten);
    }

    // Das abgewandte Ohr hoert leiser UND dumpfer. Beides zusammen ist der Kopfschatten --
    // nur leiser waere wieder blosses Panorama.
    #[test]
    fn das_abgewandte_ohr_hoert_leiser_und_dumpfer() {
        let rechts = bei(1.0, 5.0);
        assert!(rechts.left.gain < rechts.right.gain);
        assert!(rechts.left.cutoff < rechts.right.cutoff);
        assert!(rechts.left.cutoff <= SHADOW_CUTOFF * 1.001, "voll abgeschattet erwartet");
    }

    // Entfernung: leiser und dumpfer, ohne Sprung, und ausserhalb der Reichweite still.
    #[test]
    fn entfernung_nimmt_lautstaerke_und_hoehen() {
        let nah = bei(0.0, 2.0);
        let fern = bei(0.0, 30.0);
        assert!(fern.left.gain < nah.left.gain);
        assert!(fern.left.cutoff < nah.left.cutoff);
        assert_eq!(distance_gain(MAX_DISTANCE), 0.0);
        assert_eq!(distance_gain(MAX_DISTANCE + 100.0), 0.0);
    }

    // Ganz nah darf nicht ins Unendliche laut werden -- 1/r allein taete genau das.
    #[test]
    fn ganz_nah_bleibt_die_lautstaerke_gedeckelt() {
        assert_eq!(distance_gain(0.0), distance_gain(REFERENCE_DISTANCE));
        assert!(distance_gain(0.0) <= 1.0);
    }

    // Kein Sprung ueber den ganzen Bereich. Geprueft wird STETIGKEIT, nicht Steilheit: in
    // Hundertstelschritten darf sich die Lautstaerke nur um Hundertstel aendern. Eine Sprung-
    // stelle (etwa ein hart abgeschnittener Reichweitenrand) waere als Knacken hoerbar, wenn
    // sich eine Quelle bewegt -- und genau die faellt hier auf, waehrend das naturgemaess
    // steile 1/r direkt hinter REFERENCE_DISTANCE durchgeht.
    #[test]
    fn die_lautstaerke_faellt_ohne_stufe() {
        let mut vorher = distance_gain(0.0);
        for schritt in 0..=(MAX_DISTANCE as usize * 100 + 1000) {
            let jetzt = distance_gain(schritt as f32 / 100.0);
            assert!(jetzt <= vorher + 1e-6, "darf nicht wieder steigen");
            assert!(vorher - jetzt < 0.01, "Stufe bei {} Bloecken", schritt as f32 / 100.0);
            vorher = jetzt;
        }
    }

    // Verrutschte Eingaben duerfen keine unsinnigen Ohrwerte erzeugen (NaN im Audio-Callback
    // waere ein Dauerknall, kein stiller Fehler).
    #[test]
    fn ausreisser_werden_geklemmt() {
        for lateral in [-9.0, 9.0] {
            let s = bei(lateral, -5.0);
            for ohr in [s.left, s.right] {
                assert!(ohr.delay.is_finite() && ohr.delay >= 0.0 && ohr.delay <= MAX_ITD);
                assert!(ohr.gain.is_finite() && (0.0..=1.0).contains(&ohr.gain));
                assert!(ohr.cutoff.is_finite() && ohr.cutoff > 0.0);
            }
        }
    }
}
