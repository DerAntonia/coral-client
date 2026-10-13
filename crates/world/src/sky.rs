//! Die Farbe des Himmels nach der Tageszeit.
//!
//! ACHTUNG, das Wichtigste zuerst: das hier ist NICHT die Rechnung von 1.21.11. Bis 1.21.8
//! stand sie als `World.getSkyColor` im Code und liess sich abschreiben. Seit 1.21.9 gibt es
//! sie dort nicht mehr -- der Himmel kommt aus `EnvironmentAttributes.SKY_COLOR_VISUAL`, einem
//! datengetriebenen System aus Umgebungsdefinitionen, Interpolatoren und Dimensionsdaten, das
//! der Server mitschickt. Im Jar findet sich weder `getSkyColor` noch `getSkyAngle`.
//!
//! Dieses Modul baut deshalb die KLASSISCHE Formel nach, die Minecraft ein Jahrzehnt lang
//! benutzt hat und die den vertrauten Tag/Nacht-Verlauf erzeugt. Sie ist eine Naeherung, kein
//! Nachbau -- sie steht hier bewusst statt eines festen Blaus, weil ein Himmel, der sich mit
//! der Tageszeit verdunkelt, naeher an dem ist, was der Server zeigt, als einer, der es nie
//! tut. Wer sie eines Tages exakt haben will, muss die Umgebungsattribute mitlesen.
//!
//! Die beiden nachgebauten Schritte:
//!
//!   Sonnenstand   d = frac(zeit / 24000 - 0.25)
//!                 e = 0.5 - cos(d * PI) / 2
//!                 winkel = (d * 2 + e) / 3
//!   Helligkeit    h = clamp(cos(winkel * 2PI) * 2 + 0.5, 0, 1)
//!
//! Der Knick in `e` ist der Grund, warum Daemmerung und Morgengrauen laenger dauern als ein
//! gleichmaessiger Umlauf: die Sonne haengt an den Horizonten nach.

/// Ein voller Minecraft-Tag in Ticks.
pub const DAY_TICKS: i64 = 24_000;

/// Sonnenstand als Bruchteil eines Umlaufs (0 = Mittag, 0.5 = Mitternacht).
pub fn sky_angle(time_of_day: i64) -> f32 {
    let d = (time_of_day.rem_euclid(DAY_TICKS) as f64 / DAY_TICKS as f64 - 0.25).rem_euclid(1.0);
    let e = 0.5 - (d * std::f64::consts::PI).cos() / 2.0;
    ((d * 2.0 + e) / 3.0) as f32
}

/// Helligkeitsfaktor des Himmels, 0 (Nacht) bis 1 (Tag).
///
/// Der Faktor 2 vor dem Kosinus ist der Grund, warum der Himmel den groessten Teil des Tages
/// VOLL hell ist und nur um die Daemmerung herum kippt: der Wert laeuft von -1.5 bis 2.5 und
/// wird an beiden Enden abgeschnitten.
pub fn daylight_factor(time_of_day: i64) -> f32 {
    let winkel = sky_angle(time_of_day);
    (winkel * std::f32::consts::TAU).cos().mul_add(2.0, 0.5).clamp(0.0, 1.0)
}

/// Die Himmelsfarbe zur Tageszeit: Grundfarbe mal Helligkeit, beides im ANZEIGE-Raum (sRGB).
///
/// Die Grundfarbe ist die Tagfarbe des Overworld-Himmels. Biomabhaengig ist sie hier bewusst
/// NICHT: Vanilla mischt dafuer die Himmelsfarben der umliegenden Biome ueber ein weiches
/// 3D-Raster (CubicSampler). Ohne diese Mischung sprangen die Farben an jeder Biomgrenze --
/// ein gleichmaessiger Himmel ist der ehrlichere Kompromiss.
pub fn time_of_day_sky(base_srgb: [f32; 3], time_of_day: i64) -> [f32; 3] {
    let f = daylight_factor(time_of_day);
    [base_srgb[0] * f, base_srgb[1] * f, base_srgb[2] * f]
}

/// Regen und Gewitter trueben den Himmel (Vanillas Wetter-Ebene der Himmelsfarbe,
/// WeatherAttributes): die Farbe zieht zu einem Grau aus ihrer eigenen Helligkeit (Luminanz
/// mit 0.3/0.59/0.11). Regen: zu 75 % auf ein Grau mit 60 % dieser Helligkeit; Gewitter: zu
/// 94 % auf eins mit 24 %. Gewichtet mit dem Regen OHNE den Gewitteranteil bzw. mit der
/// Gewitterstaerke -- im vollen Gewitter gilt also nur noch dessen dunkleres Grau.
///
/// `thunder` ist wie Vanillas getThunderLevel schon mit der Regenstaerke multipliziert.
pub fn weather_sky(srgb: [f32; 3], rain: f32, thunder: f32) -> [f32; 3] {
    let grau = |hell: f32, anteil: f32| {
        let g = (srgb[0] * 0.3 + srgb[1] * 0.59 + srgb[2] * 0.11) * hell;
        srgb.map(|c| c + (g - c) * anteil)
    };
    let mische = |a: [f32; 3], b: [f32; 3], t: f32| [0, 1, 2].map(|i| a[i] + (b[i] - a[i]) * t);
    let gewitter = thunder.clamp(0.0, 1.0);
    let regen = (rain.clamp(0.0, 1.0) - gewitter).max(0.0);
    let c = mische(srgb, grau(0.6, 0.75), regen);
    mische(c, grau(0.24, 0.94), gewitter)
}

/// sRGB (Anzeige-Raum) -> linear.
///
/// Gebraucht, weil wgpu die Clear-Farbe LINEAR erwartet und die sRGB-Oberflaeche sie beim
/// Schreiben wieder aufhellt. Eine direkt hingeschriebene Anzeige-Farbe kaeme deutlich zu hell
/// heraus -- genau diese Falle steht schon am alten festen Himmelsblau angeschrieben.
pub fn srgb_to_linear(c: f32) -> f32 {
    if c <= 0.04045 {
        c / 12.92
    } else {
        ((c + 0.055) / 1.055).powf(2.4)
    }
}

/// linear -> sRGB, die Umkehrung von srgb_to_linear.
pub fn linear_to_srgb(c: f32) -> f32 {
    if c <= 0.0031308 {
        c * 12.92
    } else {
        1.055 * c.powf(1.0 / 2.4) - 0.055
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn regen_und_gewitter_trueben_den_himmel() {
        let blau = [0.47, 0.65, 1.0];
        assert_eq!(weather_sky(blau, 0.0, 0.0), blau);
        let lum = 0.47 * 0.3 + 0.65 * 0.59 + 1.0 * 0.11;
        // Voller Regen: 25 % Farbe, 75 % Grau mit 60 % Helligkeit.
        let regen = weather_sky(blau, 1.0, 0.0);
        for i in 0..3 {
            assert!((regen[i] - (blau[i] * 0.25 + lum * 0.6 * 0.75)).abs() < 1e-5, "{regen:?}");
        }
        // Volles Gewitter ist dunkler und grauer als voller Regen.
        let gewitter = weather_sky(blau, 1.0, 1.0);
        assert!(gewitter.iter().sum::<f32>() < regen.iter().sum::<f32>());
        assert!((gewitter[2] - gewitter[0]) < (regen[2] - regen[0]));
    }

    #[test]
    // Die vier Eckpunkte des Tages. 6000 ist Mittag (Sonne im Zenit), 18000 Mitternacht.
    fn mittag_ist_hell_und_mitternacht_dunkel() {
        assert!((daylight_factor(6000) - 1.0).abs() < 1e-6, "Mittag muss voll hell sein");
        assert_eq!(daylight_factor(18000), 0.0, "Mitternacht muss ganz dunkel sein");
    }

    #[test]
    // Der Tag ist laenger hell, als ein reiner Kosinus waere -- das ist der Sinn des Faktors 2:
    // von kurz nach Sonnenaufgang bis kurz vor Sonnenuntergang steht der Wert bei voll 1.
    //
    // Tick 0 ist NICHT Mittag, sondern Sonnenaufgang (0.93) -- das ist die Falle beim
    // Hinschreiben solcher Tabellen und der Grund, warum hier echte Ticks stehen.
    fn der_helle_teil_des_tages_ist_breit() {
        for t in [500, 1000, 3000, 6000, 9000, 11_000, 11_500] {
            assert_eq!(daylight_factor(t), 1.0, "Tick {t} sollte voll hell sein");
        }
        for t in [14_000, 16_000, 18_000, 20_000, 22_000] {
            assert_eq!(daylight_factor(t), 0.0, "Tick {t} sollte ganz dunkel sein");
        }
        // Die beiden Uebergaenge liegen um Sonnenuntergang (12000) und Sonnenaufgang (23000).
        for t in [12_500, 13_000, 23_000, 23_500] {
            let f = daylight_factor(t);
            assert!(f > 0.0 && f < 1.0, "Tick {t} ist Daemmerung, nicht hell und nicht dunkel: {f}");
        }
    }

    #[test]
    // Dazwischen liegt die Daemmerung: sie muss durchlaufen, nicht springen.
    fn die_daemmerung_ist_ein_weicher_uebergang() {
        let mut vorher = daylight_factor(11_500);
        let mut groesster_sprung = 0.0f32;
        let mut gab_zwischenwerte = false;
        for t in 11_501..13_500 {
            let jetzt = daylight_factor(t);
            groesster_sprung = groesster_sprung.max((jetzt - vorher).abs());
            if jetzt > 0.01 && jetzt < 0.99 {
                gab_zwischenwerte = true;
            }
            vorher = jetzt;
        }
        assert!(gab_zwischenwerte, "zwischen hell und dunkel muss es Zwischenwerte geben");
        assert!(groesster_sprung < 0.02, "Sprung von {groesster_sprung} in der Daemmerung");
    }

    #[test]
    // Die Zeit laeuft um: Tick 24000 ist wieder Tick 0, und negative Zeiten (ein Server, der
    // rueckwaerts setzt) duerfen nicht in eine Panik oder einen schwarzen Himmel laufen.
    fn die_zeit_laeuft_rund() {
        assert_eq!(daylight_factor(0), daylight_factor(DAY_TICKS));
        assert_eq!(daylight_factor(6000), daylight_factor(6000 + 5 * DAY_TICKS));
        assert_eq!(daylight_factor(-6000), daylight_factor(18_000));
    }

    #[test]
    fn die_grundfarbe_wird_nur_abgedunkelt_nie_verfaerbt() {
        let base = [0.45, 0.65, 0.95];
        let mittag = time_of_day_sky(base, 6000);
        assert_eq!(mittag, base, "am Mittag bleibt die Grundfarbe unveraendert");
        assert_eq!(time_of_day_sky(base, 18_000), [0.0, 0.0, 0.0]);
        // Und mittendrin bleibt das VERHAELTNIS der Kanaele gleich -- es wird gedimmt,
        // nicht umgefaerbt.
        let daemmerung = time_of_day_sky(base, 12_600);
        let f = daemmerung[0] / base[0];
        assert!((daemmerung[1] / base[1] - f).abs() < 1e-6);
        assert!((daemmerung[2] / base[2] - f).abs() < 1e-6);
    }

    #[test]
    // Die Umrechnung muss die bekannten Eckwerte treffen, sonst ist der ganze Himmel zu hell.
    fn srgb_nach_linear_trifft_die_eckwerte() {
        assert!((srgb_to_linear(0.0) - 0.0).abs() < 1e-6);
        assert!((srgb_to_linear(1.0) - 1.0).abs() < 1e-6);
        // Mittleres Grau der Anzeige ist linear deutlich dunkler -- der klassische Fehler,
        // den man sieht, wenn jemand die Umrechnung weglaesst.
        assert!((srgb_to_linear(0.5) - 0.2140).abs() < 1e-3, "{}", srgb_to_linear(0.5));
    }
}
