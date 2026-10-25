// Was die Audio-Kette auf DIESEM Rechner tatsaechlich kostet. #[ignore] wie die uebrigen
// Tests, die echte Hardware brauchen -- manuell:
//   cargo test -p audio --test latency_probe -- --ignored --nocapture
//
// Der Anlass: dieser Client benutzt Ton als Signal (Totem-Pop, s. app::modules), und die
// Puffergroesse ist die eine Groesse in der Kette, die man NICHT aus dem Code ablesen kann --
// wir FORDERN 256 Frames an, aber ob das Backend sie gewaehrt, entscheidet der Audio-Server.
// PipeWire laeuft hier standardmaessig mit Quantum 1024 (21,3 ms bei 48 kHz); wird unsere
// Anforderung ignoriert, ist die Ausgabelatenz viermal so hoch wie gedacht.
//
// Der Test bewertet nichts, er BERICHTET -- was "gut genug" ist, haengt vom Geraet ab.

#[test]
#[ignore]
fn berichtet_die_echte_puffergroesse() {
    use cpal::traits::{DeviceTrait, HostTrait};

    let Some(device) = cpal::default_host().default_output_device() else {
        println!("kein Ausgabegeraet -- nichts zu messen");
        return;
    };
    let name = device.name().unwrap_or_else(|_| "?".into());
    let default = device.default_output_config().expect("Standardkonfiguration");
    let rate = default.sample_rate().0;
    println!("Geraet: {name}");
    println!("  Format: {:?}, {} Kanaele, {} Hz", default.sample_format(), default.channels(), rate);

    // Dieselbe Auswahl wie audio::low_latency_config -- der Test misst also den Weg, den der
    // Client wirklich nimmt, nicht einen daneben.
    const TARGET: u32 = audio::DEFAULT_BUFFER_FRAMES;
    const TARGET_RATE: u32 = 48_000;
    let mut gewaehlt = None;
    let mut rate = rate;
    if let Ok(mut supported) = device.supported_output_configs() {
        if let Some(range) = supported.find(|r| r.channels() == default.channels() && r.sample_format() == default.sample_format()) {
            // Die Rate zuerst, sie bestimmt die Pufferdauer. Der Client zieht 48 kHz vor,
            // weil der Audio-Server ueblicherweise damit laeuft -- alles andere laesst
            // PipeWire resampeln (s. audio::PREFERRED_SAMPLE_RATE).
            println!("  Raten erlaubt: {}..{} Hz", range.min_sample_rate().0, range.max_sample_rate().0);
            if range.min_sample_rate().0 <= TARGET_RATE && TARGET_RATE <= range.max_sample_rate().0 {
                rate = TARGET_RATE;
                println!("  -> {TARGET_RATE} Hz moeglich, der Client nimmt sie (kein Resampling)");
            } else {
                println!("  -> {TARGET_RATE} Hz NICHT moeglich, es bleibt bei {rate} Hz (PipeWire resampelt dann)");
            }
            match range.buffer_size() {
                cpal::SupportedBufferSize::Range { min, max } => {
                    println!("  Puffer erlaubt: {min}..{max} Frames");
                    gewaehlt = Some(TARGET.clamp(*min, *max));
                }
                cpal::SupportedBufferSize::Unknown => {
                    println!("  Puffergroesse: Backend macht keine Angabe -- der Client laesst dann die Vorgabe stehen");
                }
            }
        }
    }

    match gewaehlt {
        Some(frames) => {
            let ms = frames as f64 * 1000.0 / rate as f64;
            println!("  angefordert: {TARGET} Frames -> gewaehlt {frames} Frames = {ms:.2} ms pro Puffer");
            if frames != TARGET {
                println!("  ACHTUNG: {TARGET} war nicht moeglich, das Geraet klemmt auf {frames}");
            }
        }
        None => println!("  keine passende Konfiguration gefunden -- der Client bleibt bei der Backend-Vorgabe"),
    }

    // Die Zahl, um die es geht: ein Signalton wird frueheStens im naechsten Callback
    // begonnen, also ist ein Puffer die untere Grenze fuer die Verzoegerung zwischen
    // play_cue() und hoerbarem Ton (plus die Ausgabelatenz des Geraets selbst, die cpal
    // nicht meldet).
    println!("  -> untere Grenze play_cue bis hoerbar: ein Puffer");
}
