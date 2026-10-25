// Low-Latency-Audio-Engine auf Basis von cpal. Dieser erste Schritt liefert nur die
// Infrastruktur (Geraete-Auswahl + Testton) -- echte Spielgeraeusche (Schritte, Bloecke,
// ...) sind eine eigene, spaetere Aufgabe (s. Placeholder-Texte in den Audio-Einstellungen).

use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex};

pub mod bank;
pub mod mic;
mod stream;
pub mod spatial;
pub mod vorbis;
pub mod wav;

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{BufferSize, SampleFormat, StreamConfig};

#[derive(Debug)]
pub enum AudioError {
    NoOutputDevice,
    DeviceNotFound(String),
    NoSupportedConfig,
    BuildStream(String),
    PlayStream(String),
}

impl std::fmt::Display for AudioError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AudioError::NoOutputDevice => write!(f, "kein Ausgabegeraet gefunden"),
            AudioError::DeviceNotFound(name) => write!(f, "Ausgabegeraet \"{name}\" nicht gefunden"),
            AudioError::NoSupportedConfig => write!(f, "Geraet unterstuetzt kein nutzbares Ausgabeformat"),
            AudioError::BuildStream(e) => write!(f, "Stream konnte nicht gebaut werden: {e}"),
            AudioError::PlayStream(e) => write!(f, "Stream konnte nicht gestartet werden: {e}"),
        }
    }
}

// Wie viele Klaenge gleichzeitig klingen koennen. Vorher war es genau EINER, und das war eine
// echte Grenze fuer einen PvP-Client: zwei Ereignisse im selben Augenblick (zwei Treffer,
// Totem und Treffer) haetten sich gegenseitig abgeschnitten -- der zweite Ton haette den
// ersten mitten im Klang ersetzt.
//
// Von 8 auf 64 erhoeht, als die Weltgeraeusche dazukamen: acht reichten fuer UI-Klaenge und
// den Totem-Pop, aber in einer belebten Szene sind allein die Schritte mehrerer Spieler schon
// mehr, und im Crystal-PvP kommen Explosionen zu Dutzenden. Bei Andrang weicht die am
// weitesten fortgeschrittene Stimme (s. einsetzen) -- mit acht Plaetzen haette das laufend
// gerade erst begonnene Klaenge abgeschnitten.
//
// Der Preis ist klein: ein belegter Platz kostet pro Sample eine Handvoll Rechenschritte, ein
// leerer nur eine Abfrage. Der Speicher haengt an der Verzoegerungsleitung je Stimme
// (s. DELAY_CAPACITY) -- rund 50 KB fuer den ganzen Mixer.
const MAX_VOICES: usize = 64;

// Zustand, den der Echtzeit-Callback liest. try_lock statt lock: wenn ein anderer Thread den
// Lock gerade haelt, gibt der Callback lieber einen Frame Stille aus als den Audio-Thread zu
// blockieren (Aussetzer waeren hoerbar, ein einzelner stiller Frame nicht). Die Haltezeiten
// der Produzenten sind ein Feldschreibvorgang lang, Konflikte also praktisch ausgeschlossen.
struct MixerState {
    test_tone_phase: f32,
    test_tone_active: bool,
    // Die laufenden Signaltoene. Fester Platz statt Vec: im Audio-Callback darf nichts
    // allokieren oder freigeben -- eine Allokation kann den Realtime-Thread blockieren.
    voices: [Option<Voice>; MAX_VOICES],
    // Fortlaufend, damit jede Stimme einen eigenen Rausch-Startwert bekommt.
    next_seed: u32,
    // Fortlaufende Nummer fuer nachgefuehrte Stimmen (s. CueSink::play_sample_tracked). 0 ist
    // "kein Griff" und wird nie vergeben.
    next_handle: u32,
    // Laufende Tonstroeme (Sprachchat), s. stream.rs. Nur der echte Mixer hat Plaetze; die
    // Offline-Renderer brauchen keine.
    streams: Vec<stream::StreamSlot>,
    // Vorrat in Samples, bevor ein Strom anlaeuft (s. CueSink::set_stream_buffer).
    stream_start_fill: usize,
}

/// Beschreibung eines kurzen Klangs. Alles, was die UI-Toene voneinander unterscheidet,
/// steckt hier -- der Mixer selbst kennt keine Namen.
///
/// Warum diese vier Regler und nicht mehr: ein Klick ist BREITBANDIG, kein Ton (deshalb
/// `noise`); er hat einen sofortigen Anschlag und faellt schnell ab (deshalb getrennte
/// `attack`/`decay` statt eines festen Ausblendens); und ein "Aufploppen" ist eine steigende
/// Tonhoehe (deshalb `hz_start`/`hz_end`). Mit einem reinen Sinus und fester Huellkurve kann
/// man keinen davon bauen -- es klingt immer nach Piepen.
#[derive(Clone, Copy, Debug)]
pub struct Cue {
    pub hz_start: f32,
    /// Endfrequenz der Rampe. Gleich `hz_start` = keine Rampe.
    pub hz_end: f32,
    /// Rauschanteil 0..1. 0 = reiner Sinus, 1 = reines Rauschen.
    pub noise: f32,
    /// Anschlagzeit in Sekunden. 0 waere ein harter Sprung und knackt -- ein bis zwei
    /// Millisekunden reichen, um das zu vermeiden, ohne weich zu klingen.
    pub attack: f32,
    pub decay: f32,
    pub gain: f32,
    /// -1 = ganz links, 0 = Mitte, +1 = ganz rechts.
    pub pan: f32,
}

impl Cue {
    pub fn duration(&self) -> f32 {
        self.attack + self.decay
    }

    /// Tonhoehe und Lautstaerke leicht streuen. Bei einem Klang, der hundertmal pro Minute
    /// kommt (Hover), hoert man sonst die Wiederholung; mit Streuung wird aus einer Folge
    /// gleicher Toene ein Rascheln.
    pub fn varied(mut self, amount: f32, seed: u32) -> Self {
        // Deterministisch aus dem Seed statt aus einem Zufallsgenerator: der Mixer bleibt
        // damit testbar, und der Aufrufer kann einfach hochzaehlen.
        let r = |n: u32| {
            let mut x = seed.wrapping_mul(2_654_435_761).wrapping_add(n.wrapping_mul(40_503));
            x ^= x >> 13;
            x = x.wrapping_mul(1_274_126_177);
            ((x >> 8) as f32 / 8_388_608.0) - 1.0
        };
        let faktor = 1.0 + r(1) * amount;
        self.hz_start *= faktor;
        self.hz_end *= faktor;
        self.gain *= 1.0 + r(2) * amount;
        self
    }
}

// Ein laufender Klang -- entweder synthetisiert oder eine abgespielte Aufnahme.
//
// Kein Copy mehr, seit `sample` dazugekommen ist: ein Arc laesst sich nicht kopieren, nur
// klonen. Im Audio-Callback ist das unbedenklich -- ein Arc-Klon ist ein Zaehler-Inkrement ohne
// Allokation, und freigegeben wird hier nie etwas, weil die Klangbank (s. SoundBank) jede
// Aufnahme dauerhaft haelt. Der Zaehler kann im Callback also gar nicht auf null fallen.
#[derive(Clone)]
struct Voice {
    cue: Cue,
    // Vergangene Sekunden -- steuert Huellkurve und Tonhoehen-Rampe.
    elapsed: f32,
    // Aufsummierte Phase. MUSS mitgefuehrt werden: bei einer Tonhoehen-Rampe waere
    // sin(t * f(t)) falsch (die Frequenz muss INTEGRIERT werden, sonst springt die Phase und
    // man hoert ein Knacken statt eines Gleitens).
    phase: f32,
    // Zustand des Rauschgenerators.
    rng: u32,
    // Letzter Wert des Rausch-Tiefpasses (s. NOISE_COLOR).
    noise_lp: f32,
    // Gesetzt = dieser Platz spielt eine AUFNAHME ab statt zu synthetisieren. Dann zaehlen von
    // `cue` nur noch `gain` und `pan`: Tonhoehe, Rauschanteil und Huellkurve stecken schon in
    // der Aufnahme, und eine zweite Huellkurve darueber wuerde sie nur verbiegen.
    sample: Option<Arc<wav::Wav>>,
    // Wiedergabegeschwindigkeit der Aufnahme, > 1 = schneller/hoeher, < 1 = langsamer/tiefer
    // (s. semitone_factor). Nur fuer Aufnahmen gelesen (im Aufnahme-Zweig von fill_buffer);
    // bei synthetisierten Klaengen bleibt sie auf 1.0 und wirkungslos -- der Master-Pitch-
    // Regler (s. app::Settings::ui_pitch_semitones) gilt bewusst nur fuer UI-Klaenge aus
    // Aufnahmen, nicht fuer Signaltoene wie den Totem-Pop (play/play_cue).
    pitch: f32,
    // Gesetzt = diese Stimme steht IM RAUM und wird ueber das Ohrenmodell ausgegeben statt
    // ueber `cue.pan` (s. spatial.rs und Ears). Ungesetzt bleibt alles wie vorher --
    // UI-Klaenge haben keine Position und sollen keine bekommen.
    ears: Option<Ears>,
    // Griff, ueber den der Aufrufer die Stimme spaeter verschieben oder anhalten kann
    // (s. CueSink::move_voices). 0 = nicht nachgefuehrt.
    handle: u32,
    // Kategorie und Name des Weltgeraeuschs -- nur fuer das Anhalten per Stop-Sound-Paket
    // (s. CueSink::stop_sounds). Als Zahl statt als String: eine Stimme endet im
    // Audio-Callback, und dort darf kein String freigegeben werden.
    tag: Option<SoundTag>,
    // Zeitpunkt (in `elapsed`), ab dem die Stimme ausgeblendet wird. Unendlich = laeuft bis
    // zu ihrem natuerlichen Ende.
    stop_at: f32,
}

/// Wozu ein Weltgeraeusch gehoert -- das, wonach Vanillas Stop-Sound-Paket filtert.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SoundTag {
    /// Vanillas SoundCategory-Nummer (0 = master .. 9 = voice).
    pub category: u8,
    /// `sound_name_hash` des vollen Klangnamens.
    pub name: u64,
}

/// Klangnamen fuer `SoundTag` verdichten. Ohne Namensraum gilt "minecraft:", wie bei jedem
/// Vanilla-Identifier -- "entity.cow.ambient" und "minecraft:entity.cow.ambient" sind derselbe
/// Klang.
pub fn sound_name_hash(name: &str) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    let mut schreibe = |bytes: &[u8]| {
        for b in bytes {
            h ^= *b as u64;
            h = h.wrapping_mul(0x0100_0000_01b3);
        }
    };
    if !name.contains(':') {
        schreibe(b"minecraft:");
    }
    schreibe(name.as_bytes());
    h
}

// Wie schnell eine nachgefuehrte Stimme ihrer neuen Lage folgt. Die Lage kommt einmal pro
// Bild; ein harter Sprung darauf waere bei jeder schnellen Drehung als Knistern zu hoeren
// (Pegel- und Laufzeitspruenge mitten in der Welle). 15 ms sind kuerzer als ein Bild bei
// 60 FPS -- die Ortung laeuft also nicht hinterher, springt aber auch nicht.
const GLIDE_SECONDS: f32 = 0.015;

// Ausblendzeit beim Anhalten (Stop-Sound). Kurz genug, dass es als "sofort" gilt, lang genug
// gegen das Knacken eines harten Abbruchs.
const STOP_FADE_SECONDS: f32 = 0.03;

// Der Zustand, den eine raeumliche Stimme mitfuehrt: die fertig gerechneten Ohrwerte plus die
// Verzoegerungsleitung und die beiden Ohrfilter.
//
// Warum das Ganze INLINE in der Stimme steckt und nicht hinter einer Box: eine Box muesste
// freigegeben werden, wenn die Stimme endet -- und das passiert im Audio-Callback, wo nichts
// allokieren oder freigeben darf (s. Voice-Kommentar). Der Preis sind
// DELAY_CAPACITY * 4 Byte pro Stimmplatz, also gut ein Kilobyte fuer den ganzen Mixer.
#[derive(Clone)]
struct Ears {
    spatial: spatial::Spatial,
    // Wohin `spatial` gleitet, seit die Quelle oder der Kopf sich bewegt hat
    // (s. GLIDE_SECONDS). Ohne Nachfuehrung gleich `spatial`.
    target: spatial::Spatial,
    // Ringpuffer des MONO-Signals dieser Stimme. Beide Ohren zapfen ihn an verschiedenen
    // Stellen an -- das ist die Laufzeitdifferenz.
    delay_line: [f32; DELAY_CAPACITY],
    // Naechster Schreibplatz im Ring.
    write: usize,
    // Letzte Werte der beiden Ohr-Tiefpaesse (Kopfschatten + Luftdaempfung).
    lp_left: f32,
    lp_right: f32,
}

// Laenge der Verzoegerungsleitung in Samples. Gebraucht wird spatial::MAX_ITD (0,66 ms); 192
// Plaetze reichen dafuer bis zu einer Abtastrate von 290 kHz, also mit grossem Abstand fuer
// jedes reale Geraet. Zwei Nachbarplaetze braucht die Interpolation zusaetzlich, deshalb wird
// beim Anzapfen ohnehin geklemmt.
const DELAY_CAPACITY: usize = 192;

impl Ears {
    fn new(spatial: spatial::Spatial) -> Ears {
        Ears { spatial, target: spatial, delay_line: [0.0; DELAY_CAPACITY], write: 0, lp_left: 0.0, lp_right: 0.0 }
    }

    // Wie lange die Stimme ueber ihr eigenes Ende hinaus weiterlaufen muss, damit das ferne
    // Ohr sein Signal noch vollstaendig bekommt. Ohne diesen Nachlauf waere eine seitliche
    // Quelle auf einem Ohr abgeschnitten -- bei kurzen Klaengen (ein 12-ms-Klick) ist 0,66 ms
    // ein merklicher Teil des Klangs.
    fn tail(&self) -> f32 {
        self.spatial.left.delay.max(self.spatial.right.delay)
    }

    fn push(&mut self, wert: f32) {
        self.delay_line[self.write] = wert;
        self.write = (self.write + 1) % DELAY_CAPACITY;
    }

    // Den Ring `verzoegerung` Samples in der Vergangenheit anzapfen, linear zwischen den
    // beiden Nachbarplaetzen interpoliert.
    //
    // Die Interpolation ist nicht Kosmetik: bei 48 kHz ist ein Sample 20,8 us, der ganze
    // Bereich der Laufzeitdifferenz aber nur 660 us. Ganzzahlig gerundet blieben also gerade
    // 32 unterscheidbare Richtungen, und eine sich drehende Quelle wuerde in hoerbaren Stufen
    // springen. Zwischenwerte machen daraus einen gleitenden Verlauf.
    fn tap(&self, verzoegerung: f32) -> f32 {
        let verzoegerung = verzoegerung.clamp(0.0, (DELAY_CAPACITY - 2) as f32);
        let ganz = verzoegerung.floor() as usize;
        let bruch = verzoegerung - ganz as f32;
        // write zeigt auf den NAECHSTEN Schreibplatz, der zuletzt geschriebene liegt also
        // eins davor. Plus DELAY_CAPACITY, damit die Subtraktion nicht unter null laeuft.
        let neuester = self.write + DELAY_CAPACITY - 1;
        let a = self.delay_line[(neuester - ganz) % DELAY_CAPACITY];
        let b = self.delay_line[(neuester - ganz - 1) % DELAY_CAPACITY];
        a * (1.0 - bruch) + b * bruch
    }

    // Einen Schritt Richtung `target` (s. GLIDE_SECONDS). Nahe genug wird eingerastet, damit
    // der Vergleich in `hoere` wieder greift und ruhende Stimmen nichts mehr kosten.
    fn gleite(&mut self, sample_rate: f32) {
        let a = 1.0 - (-1.0 / (GLIDE_SECONDS * sample_rate)).exp();
        let schritt = |ist: &mut f32, soll: f32, genug: f32| {
            *ist += (soll - *ist) * a;
            if (soll - *ist).abs() < genug {
                *ist = soll;
            }
        };
        for (ist, soll) in [(&mut self.spatial.left, self.target.left), (&mut self.spatial.right, self.target.right)] {
            schritt(&mut ist.delay, soll.delay, 1e-7);
            schritt(&mut ist.gain, soll.gain, 1e-5);
            schritt(&mut ist.cutoff, soll.cutoff, 1.0);
        }
    }

    // Ein Sample durch beide Ohren schicken: anzapfen, Tiefpass, Pegel.
    fn hoere(&mut self, wert: f32, sample_rate: f32) -> (f32, f32) {
        if self.spatial != self.target {
            self.gleite(sample_rate);
        }
        self.push(wert);
        let links = self.tap(self.spatial.left.delay * sample_rate);
        let rechts = self.tap(self.spatial.right.delay * sample_rate);
        let links = tiefpass(&mut self.lp_left, links, self.spatial.left.cutoff, sample_rate);
        let rechts = tiefpass(&mut self.lp_right, rechts, self.spatial.right.cutoff, sample_rate);
        (links * self.spatial.left.gain, rechts * self.spatial.right.gain)
    }
}

// Einpoliger Tiefpass, wie er auch das Rauschen faerbt (s. Voice::faerbe_rauschen) -- hier
// aber OHNE Amplitudenkorrektur: der Pegelverlust IST hier der gewollte Effekt (Kopfschatten,
// Luftdaempfung), waehrend er dort ein unerwuenschter Nebeneffekt der Klangfaerbung war.
//
// Oberhalb von 45 % der Abtastrate liegt die Grenzfrequenz jenseits dessen, was das Geraet
// ueberhaupt darstellen kann -- dann wird durchgereicht statt gefiltert, damit das nahe Ohr
// wirklich unberuehrt bleibt und nicht bei jedem Klang leicht dumpf wird.
fn tiefpass(zustand: &mut f32, eingang: f32, grenze: f32, sample_rate: f32) -> f32 {
    if grenze >= sample_rate * 0.45 {
        *zustand = eingang;
        return eingang;
    }
    let a = (1.0 - (-std::f32::consts::TAU * grenze / sample_rate).exp()).clamp(1e-4, 1.0);
    *zustand += a * (eingang - *zustand);
    *zustand
}

// Wie weit ueber der Tonhoehe das Rauschen abgeschnitten wird. Der Rauschgenerator liefert
// WEISSES Rauschen -- ein flaches Spektrum bis zur halben Abtastrate. Ungefiltert bestimmt es
// die wahrgenommene Helligkeit fast allein: der Tippton hatte eine Tonhoehe von 2400 Hz, sein
// gemessener spektraler Schwerpunkt lag aber bei 4637 Hz, weil er zu 85% aus Rauschen besteht.
// Die Tonhoehe abzusenken haette an so einem Klang deshalb praktisch nichts geaendert.
//
// Mit dem Tiefpass haengt das Rauschen an derselben Frequenz wie der Ton -- `hz_start`/`hz_end`
// steuern damit die Helligkeit des GANZEN Klangs, nicht nur seines tonalen Anteils. Der Faktor
// laesst dem Rauschen genug Luft oben, damit ein Klick knackig bleibt und nicht dumpf wird.
const NOISE_COLOR: f32 = 2.5;

impl Voice {
    // Einpoliger Tiefpass auf das Rauschen, Grenzfrequenz = hz * NOISE_COLOR.
    //
    // Die Amplitude wird gegengerechnet: ein Tiefpass nimmt Energie weg, und zwar um so mehr,
    // je tiefer die Grenzfrequenz liegt. Fuer weisses Rauschen durch einen einpoligen Filter
    // ist das Verhaeltnis der Effektivwerte sqrt(a / (2 - a)) -- der Kehrwert davon holt die
    // Lautheit zurueck. Ohne diese Korrektur waeren alle rauschlastigen Klaenge mit dem Filter
    // gleichzeitig dunkler UND deutlich leiser geworden, und die Lautheits-Rangfolge des
    // Klangsets (s. app::ui_sound) waere still durcheinandergeraten.
    fn faerbe_rauschen(&mut self, weiss: f32, hz: f32, sample_rate: f32) -> f32 {
        let grenze = (hz * NOISE_COLOR).min(sample_rate * 0.45);
        let a = (1.0 - (-std::f32::consts::TAU * grenze / sample_rate).exp()).clamp(1e-4, 1.0);
        self.noise_lp += a * (weiss - self.noise_lp);
        self.noise_lp * (2.0 - a).max(1e-6).sqrt() / a.sqrt()
    }
}

pub struct AudioEngine {
    stream: Option<cpal::Stream>,
    state: Arc<Mutex<MixerState>>,
    current_device_name: Option<String>,
    buffer: Option<BufferInfo>,
    // Was zuletzt an set_output_device ging (None = System-Standard) -- set_buffer_frames
    // baut den Stream damit neu, ohne aus "System-Standard" ein festes Geraet zu machen.
    requested_device: Option<String>,
    // Angefragte Puffergroesse in Frames, None = Vorgabe des Backends (s. low_latency_config).
    buffer_frames: Option<u32>,
}

/// Puffergroesse, mit der die Engine startet, solange nichts anderes gesetzt ist.
pub const DEFAULT_BUFFER_FRAMES: u32 = 256;

/// Puffer des laufenden Ausgabe-Streams, fuer die Anzeige in den Audio-Einstellungen.
///
/// `requested_frames` ist, was low_latency_config beim Backend angefragt hat (None = Vorgabe
/// des Backends). Was das Backend daraus macht, sagt cpal nicht -- daher zusaetzlich die
/// gemessene Callback-Groesse: so viele Frames hat der letzte Callback tatsaechlich gefuellt.
#[derive(Clone)]
pub struct BufferInfo {
    pub requested_frames: Option<u32>,
    pub sample_rate: u32,
    callback_frames: Arc<AtomicU32>,
}

impl BufferInfo {
    /// 0, solange noch kein Callback gelaufen ist.
    pub fn callback_frames(&self) -> u32 {
        self.callback_frames.load(Ordering::Relaxed)
    }

    pub fn frames_to_ms(&self, frames: u32) -> f32 {
        frames as f32 * 1000.0 / self.sample_rate.max(1) as f32
    }
}

// cpal::Stream haelt auf manchen Backends nicht-Send-Handles; die Engine wird nur vom
// Main-/UI-Thread aus angefasst (wie GpuState in main.rs), daher unproblematisch.
unsafe impl Send for AudioEngine {}

impl AudioEngine {
    /// Startet sofort auf dem Standard-Ausgabegeraet, falls eines existiert. Wenn nicht
    /// (z.B. headless CI), bleibt die Engine stumm nutzbar statt zu paniken.
    pub fn new() -> Self {
        Self::with_buffer_frames(Some(DEFAULT_BUFFER_FRAMES))
    }

    /// Wie new(), aber gleich mit dieser Puffergroesse -- spart den zweiten Stream-Aufbau,
    /// wenn die Einstellung schon beim Start feststeht.
    pub fn with_buffer_frames(buffer_frames: Option<u32>) -> Self {
        let state = Arc::new(Mutex::new(MixerState {
            test_tone_phase: 0.0,
            test_tone_active: false,
            voices: std::array::from_fn(|_| None),
            next_seed: 1,
            next_handle: 1,
            streams: (0..stream::MAX_STREAMS).map(|_| stream::StreamSlot::new()).collect(),
            stream_start_fill: stream::DEFAULT_START_FILL,
        }));
        let mut engine = AudioEngine { stream: None, state, current_device_name: None, buffer: None, requested_device: None, buffer_frames };
        if let Some(name) = default_output_device_name() {
            if let Err(e) = engine.set_output_device(Some(&name)) {
                // Ein gespeicherter Puffer, den das Geraet nicht kann, soll den Client nicht
                // stumm starten lassen -- dann eben mit der Vorgabe des Backends.
                if engine.buffer_frames.is_some() {
                    eprintln!("[audio] Puffer {:?} geht nicht ({e}), nehme die Backend-Vorgabe", engine.buffer_frames);
                    engine.buffer_frames = None;
                    let _ = engine.set_output_device(Some(&name));
                }
            }
            // Gestartet wurde mit dem Namen, gemeint war das System-Standardgeraet:
            // set_buffer_frames soll spaeter wieder dem Standard folgen, nicht diesem Namen.
            engine.requested_device = None;
        }
        engine
    }

    pub fn current_device_name(&self) -> Option<&str> {
        self.current_device_name.as_deref()
    }

    pub fn buffer_info(&self) -> Option<&BufferInfo> {
        self.buffer.as_ref()
    }

    /// Neue Puffergroesse (None = Backend-Vorgabe), der Stream wird sofort neu gebaut. Schlaegt
    /// das fehl, laeuft der alte Stream mit der alten Groesse weiter.
    pub fn set_buffer_frames(&mut self, frames: Option<u32>) -> Result<(), AudioError> {
        let alt = std::mem::replace(&mut self.buffer_frames, frames);
        if self.stream.is_none() {
            return Ok(());
        }
        let geraet = self.requested_device.clone();
        let ergebnis = self.set_output_device(geraet.as_deref());
        if ergebnis.is_err() {
            self.buffer_frames = alt;
        }
        ergebnis
    }

    /// `name = None` waehlt das System-Standardgeraet.
    pub fn set_output_device(&mut self, name: Option<&str>) -> Result<(), AudioError> {
        let host = cpal::default_host();
        let device = match name {
            Some(name) => host
                .output_devices()
                .map_err(|_| AudioError::NoOutputDevice)?
                .find(|d| d.name().map(|n| n == name).unwrap_or(false))
                .ok_or_else(|| AudioError::DeviceNotFound(name.to_string()))?,
            None => host.default_output_device().ok_or(AudioError::NoOutputDevice)?,
        };

        let (config, sample_format) = low_latency_config(&device, self.buffer_frames)?;
        let state = Arc::clone(&self.state);
        let channels = config.channels as usize;
        let sample_rate = config.sample_rate.0 as f32;
        let callback_frames = Arc::new(AtomicU32::new(0));
        let gemessen = Arc::clone(&callback_frames);
        let melde = move |len: usize| gemessen.store((len / channels.max(1)) as u32, Ordering::Relaxed);

        let err_fn = |e| eprintln!("[audio] Stream-Fehler: {e}");
        let stream = match sample_format {
            SampleFormat::F32 => device.build_output_stream(
                &config,
                move |data: &mut [f32], _| {
                    melde(data.len());
                    fill_buffer(data, channels, sample_rate, &state)
                },
                err_fn,
                None,
            ),
            SampleFormat::I16 => device.build_output_stream(
                &config,
                move |data: &mut [i16], _| {
                    melde(data.len());
                    let mut tmp = vec![0.0f32; data.len()];
                    fill_buffer(&mut tmp, channels, sample_rate, &state);
                    for (o, i) in data.iter_mut().zip(tmp.iter()) {
                        *o = (*i * i16::MAX as f32) as i16;
                    }
                },
                err_fn,
                None,
            ),
            SampleFormat::U16 => device.build_output_stream(
                &config,
                move |data: &mut [u16], _| {
                    melde(data.len());
                    let mut tmp = vec![0.0f32; data.len()];
                    fill_buffer(&mut tmp, channels, sample_rate, &state);
                    for (o, i) in data.iter_mut().zip(tmp.iter()) {
                        *o = (((i + 1.0) * 0.5) * u16::MAX as f32) as u16;
                    }
                },
                err_fn,
                None,
            ),
            other => return Err(AudioError::BuildStream(format!("nicht unterstuetztes Sample-Format {other:?}"))),
        }
        .map_err(|e| AudioError::BuildStream(e.to_string()))?;

        stream.play().map_err(|e| AudioError::PlayStream(e.to_string()))?;

        self.requested_device = name.map(str::to_string);
        self.current_device_name = Some(device.name().unwrap_or_else(|_| "unbekannt".to_string()));
        self.stream = Some(stream);
        let requested_frames = match config.buffer_size {
            BufferSize::Fixed(n) => Some(n),
            BufferSize::Default => None,
        };
        self.buffer = Some(BufferInfo { requested_frames, sample_rate: config.sample_rate.0, callback_frames });
        Ok(())
    }

    /// Kurzer Sinuston (440 Hz) zur Verifikation, dass Geraet + Stream tatsaechlich
    /// Ton ausgeben -- es gibt noch keine echten Spielgeraeusche zum Testen.
    pub fn play_test_tone(&self) {
        if let Ok(mut s) = self.state.lock() {
            s.test_tone_phase = 0.0;
            s.test_tone_active = true;
        }
    }

    pub fn stop_test_tone(&self) {
        if let Ok(mut s) = self.state.lock() {
            s.test_tone_active = false;
        }
    }

    /// Ein Griff auf den Mixer, den JEDER Thread halten darf (Clone, Send, Sync).
    ///
    /// Das ist der Kern der Entkopplung: Toene muessen ausgeloest werden koennen, wo das
    /// Ereignis entsteht -- im Netzwerk-Task, in dem Augenblick, in dem das Paket gelesen
    /// wird. Ginge es nur ueber die AudioEngine (die am Hauptthread haengt, weil cpal::Stream
    /// dort leben muss), muesste jeder Ton auf den naechsten Durchlauf der Fensterschleife
    /// warten und haenge damit an der Bildrate.
    pub fn sink(&self) -> CueSink {
        CueSink { state: Arc::clone(&self.state) }
    }

    /// Kurzes Signal, direkt an dieser Engine. Fuer Aufrufer, die den Hauptthread ohnehin
    /// schon haben; alle anderen nehmen sink().
    pub fn play_cue(&self, hz: f32, pan: f32) {
        self.sink().play(hz, pan);
    }
}

/// Ausloeser fuer Signaltoene, unabhaengig von der Fensterschleife (s. AudioEngine::sink).
///
/// Haelt bewusst NICHT den Stream, sondern nur den Mixer-Zustand: der ist ein
/// Arc<Mutex<..>> und damit echt threadsicher, waehrend cpal::Stream auf manchen Backends
/// Handles hat, die den Hauptthread brauchen.
#[derive(Clone)]
pub struct CueSink {
    state: Arc<Mutex<MixerState>>,
}

impl CueSink {
    /// Kurzes Signal mit Tonhoehe und Stereo-Position (-1 links .. +1 rechts). Gedacht fuer
    /// Ereignisse, bei denen die Seite selbst die Information ist -- der erste Anwendungsfall
    /// ist der Totem-Pop (Haupthand rechts, Zweithand links, s. app::modules).
    ///
    /// Bewusst ein synthetischer Ton und keine Vanilla-Audiodatei: dieser Client hat noch
    /// keinen Sample-Abspieler (s. Modulkommentar), und ein Sinus mit harter Panorama-Lage
    /// ist fuer "welche Seite?" sogar eindeutiger als ein Totem-Geraeusch.
    ///
    /// Sind alle Stimmen belegt, wird die AELTESTE ersetzt: ein neues Ereignis ist als Signal
    /// wichtiger als das Ausklingen eines alten.
    pub fn play(&self, hz: f32, pan: f32) {
        // Der Anschlag wird vom Abfall ABGEZOGEN, nicht angehaengt: die Gesamtdauer bleibt
        // damit exakt CUE_SECONDS wie vor der Huellkurven-Erweiterung. Der Totem-Ton soll
        // durch eine Mixer-Aenderung nicht laenger werden.
        self.play_cue(Cue {
            hz_start: hz,
            hz_end: hz,
            noise: 0.0,
            attack: CUE_ATTACK,
            decay: CUE_SECONDS - CUE_ATTACK,
            gain: CUE_GAIN,
            pan,
        });
    }

    /// Einen beschriebenen Klang abspielen (s. Cue).
    pub fn play_cue(&self, cue: Cue) {
        let Ok(mut s) = self.state.lock() else { return };
        let seed = s.next_seed;
        s.next_seed = s.next_seed.wrapping_add(1);
        let neu = Voice {
            cue: Cue { pan: cue.pan.clamp(-1.0, 1.0), ..cue },
            elapsed: 0.0,
            phase: 0.0,
            // Jede Stimme mit eigenem Rausch-Zustand, sonst klaengen gleichzeitige
            // Rausch-Toene identisch und wuerden sich zu einem lauteren Ton addieren.
            rng: seed | 1,
            noise_lp: 0.0,
            sample: None,
            pitch: 1.0,
            ears: None,
            handle: 0,
            tag: None,
            stop_at: f32::INFINITY,
        };
        self.einsetzen(&mut s, neu);
    }

    /// Eine AUFNAHME abspielen (s. SoundBank). `gain` und `pan` gelten wie beim synthetischen
    /// Klang; Laenge und Klangfarbe bringt die Aufnahme selbst mit.
    ///
    /// `pitch`: Wiedergabegeschwindigkeit, 1.0 = unveraendert (s. `semitone_factor`). Wie beim
    /// Halbton-Regler in `scripts/klaenge-anhoeren.sh` haengen Tonhoehe und Laenge zusammen --
    /// schneller heisst zwangslaeufig auch hoeher und kuerzer, das ist keine Vereinfachung
    /// sondern dieselbe Physik wie bei einer schneller abgespielten Aufnahme.
    pub fn play_sample(&self, sample: Arc<wav::Wav>, gain: f32, pan: f32, pitch: f32) {
        let Ok(mut s) = self.state.lock() else { return };
        let seed = s.next_seed;
        s.next_seed = s.next_seed.wrapping_add(1);
        // Gegen Division durch null/negative Werte -- ein Regler-Extrem darf die Stimme nicht
        // unendlich lang am Leben halten oder sie rueckwaerts abspielen.
        let pitch = pitch.max(1e-3);
        let neu = Voice {
            // Die Dauer traegt hier die Aufnahme -- decay wird auf ihre PITCH-ABHAENGIGE
            // Laenge gesetzt, damit der Platz genau dann frei wird, wenn die Aufnahme bei
            // dieser Geschwindigkeit tatsaechlich zu Ende ist (s. fill_buffer).
            cue: Cue {
                hz_start: 0.0,
                hz_end: 0.0,
                noise: 0.0,
                attack: 0.0,
                decay: sample.duration() / pitch,
                gain,
                pan: pan.clamp(-1.0, 1.0),
            },
            elapsed: 0.0,
            phase: 0.0,
            rng: seed | 1,
            noise_lp: 0.0,
            sample: Some(sample),
            pitch,
            ears: None,
            handle: 0,
            tag: None,
            stop_at: f32::INFINITY,
        };
        self.einsetzen(&mut s, neu);
    }

    /// Eine Aufnahme AN EINEM ORT abspielen -- der Weg fuer alles, was in der Welt passiert.
    ///
    /// Unterschied zu `play_sample`: dort legt `pan` den Klang nur lauter auf eine Seite,
    /// hier bekommt jedes Ohr eine eigene Laufzeit, einen eigenen Pegel und eine eigene
    /// Klangfarbe (s. spatial::spatial_for). Das ist der Unterschied zwischen "mehr von
    /// links" und "da links, ungefaehr zwanzig Bloecke weg".
    ///
    /// `gain` ist die Lautstaerke des Ereignisses selbst; die Entfernungsdaempfung kommt aus
    /// dem Ortsmodell obendrauf. Ist die Quelle ausser Reichweite, wird gar nicht erst eine
    /// Stimme belegt -- sonst verdraengte eine unhoerbare weit entfernte Quelle einen
    /// hoerbaren nahen Klang (s. einsetzen).
    pub fn play_sample_at(&self, sample: Arc<wav::Wav>, gain: f32, pitch: f32, placement: &spatial::Placement) {
        let spatial = spatial::spatial_for(placement);
        if spatial.left.gain <= 0.0 && spatial.right.gain <= 0.0 {
            return;
        }
        let Ok(mut s) = self.state.lock() else { return };
        let seed = s.next_seed;
        s.next_seed = s.next_seed.wrapping_add(1);
        let pitch = pitch.max(1e-3);
        let ears = Ears::new(spatial);
        let neu = Voice {
            // Der Nachlauf haengt hier an der Dauer mit dran: die Aufnahme ist zu Ende, das
            // ferne Ohr hoert sie aber noch (s. Ears::tail).
            cue: Cue {
                hz_start: 0.0,
                hz_end: 0.0,
                noise: 0.0,
                attack: 0.0,
                decay: sample.duration() / pitch + ears.tail(),
                gain,
                pan: 0.0,
            },
            elapsed: 0.0,
            phase: 0.0,
            rng: seed | 1,
            noise_lp: 0.0,
            sample: Some(sample),
            pitch,
            ears: Some(ears),
            handle: 0,
            tag: None,
            stop_at: f32::INFINITY,
        };
        self.einsetzen(&mut s, neu);
    }

    /// Ein Weltgeraeusch abspielen, das sich spaeter verschieben und anhalten laesst.
    ///
    /// `placement = None` spielt es kopffest (Musik, s. app::play_server_sound), sonst im Raum
    /// wie `play_sample_at` -- mit dem Unterschied, dass eine Quelle ausser Reichweite TROTZDEM
    /// eine Stimme bekommt: wer auf eine laufende Jukebox zugeht, soll sie ab der
    /// Reichweitengrenze hoeren und nicht erst beim naechsten Lied. Stumm bleibt sie bis dahin
    /// trotzdem (Pegel 0), nur ihre Uhr laeuft mit.
    ///
    /// Rueckgabe: der Griff fuer `move_voices`/`stop_handle`, oder None, wenn der Mixer nicht
    /// erreichbar war.
    pub fn play_sample_tracked(
        &self,
        sample: Arc<wav::Wav>,
        gain: f32,
        pitch: f32,
        placement: Option<&spatial::Placement>,
        tag: SoundTag,
    ) -> Option<u32> {
        let Ok(mut s) = self.state.lock() else { return None };
        let seed = s.next_seed;
        s.next_seed = s.next_seed.wrapping_add(1);
        let handle = s.next_handle;
        s.next_handle = s.next_handle.wrapping_add(1).max(1);
        let pitch = pitch.max(1e-3);
        let ears = placement.map(|p| Ears::new(spatial::spatial_for(p)));
        let nachlauf = ears.as_ref().map_or(0.0, |e| e.tail().max(spatial::MAX_ITD));
        let neu = Voice {
            // Der Nachlauf ist hier der GROESSTMOEGLICHE, nicht der aktuelle: die Quelle kann
            // waehrend des Abspielens zur Seite wandern (s. Ears::tail).
            cue: Cue { hz_start: 0.0, hz_end: 0.0, noise: 0.0, attack: 0.0, decay: sample.duration() / pitch + nachlauf, gain, pan: 0.0 },
            elapsed: 0.0,
            phase: 0.0,
            rng: seed | 1,
            noise_lp: 0.0,
            sample: Some(sample),
            pitch,
            ears,
            handle,
            tag: Some(tag),
            stop_at: f32::INFINITY,
        };
        self.einsetzen(&mut s, neu);
        Some(handle)
    }

    /// Nachgefuehrte Stimmen an ihre neue Lage setzen (einmal pro Bild). Der Uebergang
    /// gleitet (s. GLIDE_SECONDS). Unbekannte Griffe -- die Stimme ist schon zu Ende --
    /// werden still uebergangen.
    pub fn move_voices(&self, lagen: &[(u32, spatial::Placement)]) {
        if lagen.is_empty() {
            return;
        }
        let Ok(mut s) = self.state.lock() else { return };
        for voice in s.voices.iter_mut().flatten() {
            if voice.handle == 0 {
                continue;
            }
            let Some((_, lage)) = lagen.iter().find(|(h, _)| *h == voice.handle) else { continue };
            if let Some(ears) = voice.ears.as_mut() {
                ears.target = spatial::spatial_for(lage);
            }
        }
    }

    /// Weltgeraeusche anhalten wie Vanillas Stop-Sound-Paket: beide Filter leer = alle,
    /// sonst nur die passenden. UI-Klaenge (ohne Tag) bleiben unberuehrt.
    pub fn stop_sounds(&self, category: Option<u8>, name: Option<u64>) {
        let Ok(mut s) = self.state.lock() else { return };
        for voice in s.voices.iter_mut().flatten() {
            let Some(tag) = voice.tag else { continue };
            if category.is_some_and(|c| c != tag.category) || name.is_some_and(|n| n != tag.name) {
                continue;
            }
            voice.stop_at = voice.stop_at.min(voice.elapsed);
        }
    }

    /// Einen Rahmen eines Tonstroms (48 kHz, mono) anhaengen -- legt den Strom beim ersten
    /// Rahmen an. `placement` = Ort im Raum (None = kopffest), `gain` = Lautstaerke 0..1.
    /// Sind alle Plaetze belegt, wird der Rahmen verworfen.
    pub fn stream_push(&self, id: u64, pcm: &[i16], placement: Option<&spatial::Placement>, gain: f32) {
        let Ok(mut s) = self.state.lock() else { return };
        let fill = s.stream_start_fill;
        stream::push(&mut s.streams, id, pcm, placement, gain, fill);
    }

    /// Wie viele Rahmen (20 ms) ein Strom vorpuffert, bevor er anlaeuft -- mehr ist robuster
    /// gegen Netzschwankungen, aber spaeter.
    pub fn set_stream_buffer(&self, frames: usize) {
        let Ok(mut s) = self.state.lock() else { return };
        s.stream_start_fill = frames.clamp(1, 16) * 960;
    }

    /// Ort und Lautstaerke eines laufenden Stroms nachfuehren (einmal pro Bild).
    pub fn stream_place(&self, id: u64, placement: Option<&spatial::Placement>, gain: f32) {
        let Ok(mut s) = self.state.lock() else { return };
        stream::place(&mut s.streams, id, placement, gain);
    }

    /// Kein Nachschub mehr: den Rest abspielen, dann den Platz freigeben.
    pub fn stream_end(&self, id: u64) {
        let Ok(mut s) = self.state.lock() else { return };
        stream::end(&mut s.streams, id);
    }

    /// Eine einzelne nachgefuehrte Stimme anhalten (z.B. Jukebox ausgeschaltet).
    pub fn stop_handle(&self, handle: u32) {
        let Ok(mut s) = self.state.lock() else { return };
        for voice in s.voices.iter_mut().flatten() {
            if voice.handle == handle && handle != 0 {
                voice.stop_at = voice.stop_at.min(voice.elapsed);
            }
        }
    }

    // Freien Platz suchen, sonst die am weitesten fortgeschrittene Stimme verdraengen.
    fn einsetzen(&self, s: &mut MixerState, neu: Voice) {
        if let Some(frei) = s.voices.iter_mut().find(|v| v.is_none()) {
            *frei = Some(neu);
            return;
        }
        // Alle belegt -- die am weitesten fortgeschrittene Stimme weicht.
        let Some(aelteste) = s.voices.iter_mut().flatten().max_by(|a, b| a.elapsed.total_cmp(&b.elapsed)) else { return };
        *aelteste = neu;
    }
}

// Kurzer Anschlag gegen das Knacken eines harten Einsatzes.
const CUE_ATTACK: f32 = 0.002;
const CUE_SECONDS: f32 = 0.12;
const CUE_GAIN: f32 = 0.25;

// Lautstaerke links/rechts fuer eine Panorama-Lage. Konstante Leistung (sin/cos statt
// linear): bei linearem Verlauf klingt die Mitte hoerbar leiser als die Seiten, und genau in
// der Mitte liegt hier nichts Wichtiges -- aber die Gleichlautheit der beiden SEITEN ist
// wichtig, sonst wirkt eine Hand lauter als die andere und man verwechselt "leise" mit
// "andere Seite".
fn pan_gains(pan: f32) -> (f32, f32) {
    let angle = (pan.clamp(-1.0, 1.0) + 1.0) * 0.25 * std::f32::consts::PI;
    (angle.cos(), angle.sin())
}

const TEST_TONE_HZ: f32 = 440.0;
const TEST_TONE_SECONDS: f32 = 0.6;
const TEST_TONE_GAIN: f32 = 0.2;

// Die Ausgabestufe einer Stimme: aus einem Mono-Wert werden die beiden Seiten.
//
// Zwei Wege, und der Unterschied ist der ganze Punkt der Ortung: eine Stimme MIT Ort geht
// durch das Ohrenmodell (Laufzeit, Kopfschatten, Entfernung), eine ohne bleibt beim reinen
// Panorama. UI-Klaenge gehoeren bewusst zur zweiten Sorte -- ein Menue-Klick steht nicht in
// der Welt, und eine Laufzeitdifferenz waere dort nur eine Verschmierung ohne Aussage.
fn seiten(voice: &mut Voice, wert: f32, sample_rate: f32) -> (f32, f32) {
    match voice.ears.as_mut() {
        Some(ears) => ears.hoere(wert, sample_rate),
        None => {
            let (gl, gr) = pan_gains(voice.cue.pan);
            (wert * gl, wert * gr)
        }
    }
}

// Spitzenpegel links/rechts seit dem letzten Ablesen (f32-Bits; fuer positive Werte ordnet
// u32 genauso wie f32, fetch_max genuegt also). Global statt im MixerState: das Ablesen aus
// dem UI-Thread darf den Mixer-Lock nie belegen -- sonst fiele der Audio-Callback auf Stille.
static PEAK_LEFT: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
static PEAK_RIGHT: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);

/// Spitzenpegel (0..1, vor der Begrenzung) des linken und rechten Kanals seit dem letzten
/// Aufruf -- fuer die Pegelanzeige des Sound-Debug-Moduls. Setzt beide zurueck.
pub fn take_peak_levels() -> (f32, f32) {
    use std::sync::atomic::Ordering;
    (f32::from_bits(PEAK_LEFT.swap(0, Ordering::Relaxed)), f32::from_bits(PEAK_RIGHT.swap(0, Ordering::Relaxed)))
}

fn fill_buffer(data: &mut [f32], channels: usize, sample_rate: f32, state: &Arc<Mutex<MixerState>>) {
    let Ok(mut s) = state.try_lock() else {
        data.fill(0.0);
        return;
    };
    let phase_step = TEST_TONE_HZ / sample_rate;
    let (mut spitze_l, mut spitze_r) = (0.0f32, 0.0f32);
    for frame in data.chunks_mut(channels) {
        let sample = if s.test_tone_active {
            s.test_tone_phase += phase_step;
            if s.test_tone_phase >= TEST_TONE_SECONDS * TEST_TONE_HZ {
                s.test_tone_active = false;
            }
            (s.test_tone_phase * std::f32::consts::TAU).sin() * TEST_TONE_GAIN
        } else {
            0.0
        };
        // Signaltoene: jede Stimme mit eigener Lautstaerke je Kanal. Die Huelle ist ein
        // einfaches Ausblenden -- ohne sie knackt es am Ende hoerbar (harter Sprung auf 0
        // mitten in der Welle).
        let (mut left, mut right) = (0.0, 0.0);
        for slot in s.voices.iter_mut() {
            let Some(voice) = slot.as_mut() else { continue };
            let dt = 1.0 / sample_rate;
            // Erst LESEN, dann die Uhr weiterstellen. Andersherum (voice.elapsed += dt vor
            // dem Auswerten) begann jede Aufnahme bei sample_at(dt) statt bei sample_at(0) --
            // ihr allererstes Sample wurde nie ausgegeben und der ganze Klang kam ein Sample
            // zu spaet. Hoerbar war das nicht (20 us), aber es ist ein systematischer
            // Zeitfehler in genau der Groessenordnung, in der das Ohrenmodell arbeitet: sein
            // groesster Versatz sind 32 Samples. Ein fester Fehler von einem davon waere
            // beim Nachmessen jedes Mal wieder als Rauschen durchgegangen.
            let zeit = voice.elapsed;
            voice.elapsed += dt;
            let dauer = voice.cue.duration();
            if zeit >= dauer {
                *slot = None;
                continue;
            }
            // Aufnahme: einfach abspielen. KEINE Huellkurve darueber -- eine Aufnahme bringt
            // ihren eigenen Anschlag und ihr eigenes Ausklingen mit, und ein zweites
            // Ausblenden wuerde beides verbiegen.
            // Angehalten (s. CueSink::stop_sounds): kurz ausblenden, dann frei machen.
            let abblenden = if zeit >= voice.stop_at {
                let rest = 1.0 - (zeit - voice.stop_at) / STOP_FADE_SECONDS;
                if rest <= 0.0 {
                    *slot = None;
                    continue;
                }
                rest
            } else {
                1.0
            };
            if let Some(wav) = &voice.sample {
                let wert = wav.sample_at(zeit * voice.pitch) * voice.cue.gain * abblenden;
                let (l, r) = seiten(voice, wert, sample_rate);
                left += l;
                right += r;
                continue;
            }
            // Huellkurve: linear an, linear ab. Der kurze Anschlag verhindert das Knacken
            // eines harten Einsatzes, der Abfall traegt den Charakter (kurz = Klick,
            // laenger = Ton).
            let huelle = if zeit < voice.cue.attack {
                zeit / voice.cue.attack.max(1e-6)
            } else {
                1.0 - (zeit - voice.cue.attack) / voice.cue.decay.max(1e-6)
            };
            // Tonhoehe entlang der Rampe, Phase integriert (s. Voice::phase).
            let t = (zeit / dauer).clamp(0.0, 1.0);
            let hz = voice.cue.hz_start + (voice.cue.hz_end - voice.cue.hz_start) * t;
            voice.phase += hz * dt;
            let ton = (voice.phase * std::f32::consts::TAU).sin();
            // Rauschen: xorshift, gleichverteilt in -1..1.
            voice.rng ^= voice.rng << 13;
            voice.rng ^= voice.rng >> 17;
            voice.rng ^= voice.rng << 5;
            let weiss = (voice.rng >> 8) as f32 / 8_388_608.0 - 1.0;
            let rausch = voice.faerbe_rauschen(weiss, hz, sample_rate);
            let mischung = ton * (1.0 - voice.cue.noise) + rausch * voice.cue.noise;
            let wert = mischung * voice.cue.gain * huelle * abblenden;
            let (l, r) = seiten(voice, wert, sample_rate);
            left += l;
            right += r;
        }
        for st in s.streams.iter_mut().filter(|st| st.active) {
            let (l, r) = st.next(sample_rate);
            left += l;
            right += r;
        }
        spitze_l = spitze_l.max((sample + left).abs());
        spitze_r = spitze_r.max((sample + right).abs());
        for (i, out) in frame.iter_mut().enumerate() {
            // Bei Mono-Geraeten beide Seiten zusammen -- sonst waere ein hart nach links
            // gelegtes Signal dort schlicht unhoerbar.
            let cue_sample = match channels {
                1 => left + right,
                _ if i % 2 == 0 => left,
                _ => right,
            };
            *out = (sample + cue_sample).clamp(-1.0, 1.0);
        }
    }
    PEAK_LEFT.fetch_max(spitze_l.to_bits(), std::sync::atomic::Ordering::Relaxed);
    PEAK_RIGHT.fetch_max(spitze_r.to_bits(), std::sync::atomic::Ordering::Relaxed);
}

#[cfg(test)]
mod cue_tests {
    use super::*;

    fn leerer_mixer() -> MixerState {
        MixerState { test_tone_phase: 0.0, test_tone_active: false, voices: std::array::from_fn(|_| None), next_seed: 1, next_handle: 1, streams: Vec::new(), stream_start_fill: stream::DEFAULT_START_FILL }
    }

    // Einen Puffer durchrechnen und die groesste Auslenkung je Kanal zurueckgeben. Damit
    // laesst sich pruefen, was tatsaechlich aus dem Mixer herauskommt -- ohne Geraet.
    fn spitzen(state: &Arc<Mutex<MixerState>>, frames: usize, rate: f32) -> (f32, f32) {
        let mut daten = vec![0.0f32; frames * 2];
        fill_buffer(&mut daten, 2, rate, state);
        let mut l: f32 = 0.0;
        let mut r: f32 = 0.0;
        for frame in daten.chunks(2) {
            l = l.max(frame[0].abs());
            r = r.max(frame[1].abs());
        }
        (l, r)
    }

    #[test]
    // Der Grund fuer den Umbau: zwei Ereignisse im selben Augenblick muessen BEIDE klingen.
    // Vorher gab es genau eine Stimme, der zweite Ton hat den ersten mitten im Klang
    // ersetzt -- bei zwei Treffern gleichzeitig haette man einen davon nie gehoert.
    fn zwei_gleichzeitige_toene_klingen_beide() {
        let state = Arc::new(Mutex::new(leerer_mixer()));
        let sink = CueSink { state: Arc::clone(&state) };
        // Einer hart links, einer hart rechts -- dann verrät jeder Kanal, ob seine Stimme lebt.
        sink.play(880.0, -1.0);
        sink.play(1320.0, 1.0);
        assert_eq!(state.lock().unwrap().voices.iter().flatten().count(), 2);
        let (l, r) = spitzen(&state, 256, 44100.0);
        assert!(l > 0.01, "linke Stimme fehlt: {l}");
        assert!(r > 0.01, "rechte Stimme fehlt: {r}");
    }

    #[test]
    // Mehr Toene als Stimmen: der aelteste weicht, und es wird nichts ueberschrieben, was
    // gerade erst angefangen hat. Vor allem darf hier nichts panicken.
    fn ueberzahl_verdraengt_die_aelteste_stimme() {
        let state = Arc::new(Mutex::new(leerer_mixer()));
        let sink = CueSink { state: Arc::clone(&state) };
        for i in 0..MAX_VOICES {
            sink.play(440.0 + i as f32, 0.0);
            // Etwas Zeit vergehen lassen, damit sich die Stimmen im Alter unterscheiden.
            fill_buffer(&mut vec![0.0f32; 64], 2, 44100.0, &state);
        }
        let aeltestes_alter = state.lock().unwrap().voices.iter().flatten().map(|v| v.elapsed).fold(0.0f32, f32::max);
        sink.play(9999.0, 0.0);
        let s = state.lock().unwrap();
        assert_eq!(s.voices.iter().flatten().count(), MAX_VOICES, "die Stimmenzahl bleibt fest");
        assert!(s.voices.iter().flatten().any(|v| v.cue.hz_start == 9999.0), "der neue Ton muss drin sein");
        let neues_max = s.voices.iter().flatten().map(|v| v.elapsed).fold(0.0f32, f32::max);
        assert!(neues_max < aeltestes_alter, "die aelteste Stimme muss gewichen sein");
    }

    #[test]
    // Ausgeklungene Stimmen muessen ihren Platz freigeben, sonst waeren nach acht Toenen alle
    // Plaetze dauerhaft belegt.
    fn ausgeklungene_stimmen_werden_frei() {
        let state = Arc::new(Mutex::new(leerer_mixer()));
        CueSink { state: Arc::clone(&state) }.play(880.0, 0.0);
        // Ein Puffer laenger als die Tondauer.
        let frames = (CUE_SECONDS * 44100.0) as usize + 64;
        fill_buffer(&mut vec![0.0f32; frames * 2], 2, 44100.0, &state);
        assert_eq!(state.lock().unwrap().voices.iter().flatten().count(), 0);
    }

    #[test]
    // Der Griff muss ueber Threadgrenzen funktionieren -- das ist der ganze Zweck (s.
    // AudioEngine::sink). Kompiliert dieser Test nicht mehr, ist die Entkopplung kaputt.
    fn der_griff_loest_aus_einem_anderen_thread_aus() {
        let state = Arc::new(Mutex::new(leerer_mixer()));
        let sink = CueSink { state: Arc::clone(&state) };
        std::thread::spawn(move || sink.play(880.0, -1.0)).join().expect("Thread");
        assert_eq!(state.lock().unwrap().voices.iter().flatten().count(), 1);
    }

    #[test]
    // Zwei gleichzeitige Toene duerfen nicht uebersteuern. Der Mixer klemmt am Ende auf
    // -1..1; ohne die Klemmung waere die Summe hoerbar verzerrt.
    fn summe_bleibt_im_aussteuerbereich() {
        let state = Arc::new(Mutex::new(leerer_mixer()));
        let sink = CueSink { state: Arc::clone(&state) };
        for _ in 0..MAX_VOICES {
            sink.play(880.0, 0.0);
        }
        let (l, r) = spitzen(&state, 256, 44100.0);
        assert!(l <= 1.0 && r <= 1.0, "uebersteuert: {l}/{r}");
    }

    #[test]
    // Die Panorama-Lage muss die Seiten wirklich trennen -- sie IST die Information.
    fn panorama_trennt_die_seiten() {
        let (l, r) = pan_gains(-1.0);
        assert!(l > 0.99 && r < 0.01, "ganz links: {l}/{r}");
        let (l, r) = pan_gains(1.0);
        assert!(l < 0.01 && r > 0.99, "ganz rechts: {l}/{r}");
        let (l, r) = pan_gains(0.0);
        assert!((l - r).abs() < 1e-6, "Mitte muss gleich laut sein: {l}/{r}");
    }

    #[test]
    // Konstante Leistung: beide Seiten gleich laut, und die Mitte nicht lauter als die
    // Seiten. Bei linearem Panorama waere die Summe in der Mitte 1.0 statt 0.707 und damit
    // deutlich lauter -- man wuerde die Mitte fuer "wichtiger" halten.
    fn beide_seiten_sind_gleich_laut() {
        let leistung = |pan: f32| {
            let (l, r) = pan_gains(pan);
            l * l + r * r
        };
        for pan in [-1.0, -0.5, 0.0, 0.5, 1.0] {
            assert!((leistung(pan) - 1.0).abs() < 1e-5, "Leistung bei {pan} war {}", leistung(pan));
        }
    }
}

/// Die Abtastrate, die wir anstreben.
///
/// 48 kHz ist die Rate, mit der der Audio-Server hier tatsaechlich laeuft (PipeWire:
/// `clock.allowed-rates = [48000]`, nachgesehen mit `pw-metadata -n settings`). Die
/// ALSA-Kompatibilitaetsschicht bietet als Standard trotzdem 44100 an -- nimmt man die,
/// resampelt PipeWire jeden Puffer von 44100 auf 48000. Das kostet Rechenzeit, etwas
/// zusaetzliche Verzoegerung und Klangqualitaet, und zwar fuer nichts: unsere Toene sind
/// synthetisch und werden zur Laufzeit erzeugt, es gibt also keine 44,1-kHz-Quelle, der man
/// entgegenkommen muesste.
///
/// Nebeneffekt: bei gleicher Puffergroesse in FRAMES wird der Puffer kuerzer -- 256 Frames
/// sind bei 48 kHz 5,33 ms statt 5,80 ms.
const PREFERRED_SAMPLE_RATE: u32 = 48_000;

/// Waehlt, wo moeglich, eine kleine feste Puffergroesse statt der (meist grosszuegigen)
/// Backend-Vorgabe -- kleinerer Puffer = weniger Latenz zwischen Trigger und hoerbarem Ton --
/// und 48 kHz statt der Backend-Vorgabe (s. PREFERRED_SAMPLE_RATE).
///
/// Beides nur, WENN das Geraet es hergibt: die Vorgaben des Backends bleiben der Rueckfall.
/// Ein Geraet, das 48 kHz nicht kann, soll stumm bleiben duerfen war nie die Absicht.
fn low_latency_config(device: &cpal::Device, buffer_frames: Option<u32>) -> Result<(StreamConfig, SampleFormat), AudioError> {

    let default = device.default_output_config().map_err(|_| AudioError::NoSupportedConfig)?;
    let sample_format = default.sample_format();
    let mut config: StreamConfig = default.config();

    if let Ok(mut supported) = device.supported_output_configs() {
        if let Some(range) = supported.find(|r| r.channels() == config.channels && r.sample_format() == sample_format) {
            // Rate zuerst: sie entscheidet, wie lang ein Puffer aus N Frames dauert.
            if range.min_sample_rate().0 <= PREFERRED_SAMPLE_RATE && PREFERRED_SAMPLE_RATE <= range.max_sample_rate().0 {
                config.sample_rate = cpal::SampleRate(PREFERRED_SAMPLE_RATE);
            }
            if let (Some(ziel), cpal::SupportedBufferSize::Range { min, max }) = (buffer_frames, range.buffer_size()) {
                let frames = ziel.clamp(*min, *max);
                config.buffer_size = BufferSize::Fixed(frames);
            }
        }
    }

    Ok((config, sample_format))
}

/// Einen Klang ohne Geraet ausrechnen -- Mono, ein Wert je Frame.
///
/// Nimmt bewusst den ECHTEN Mixerweg (play_cue + fill_buffer) statt die Formel nachzubilden:
/// eine Nachbildung war frueher der Grund, warum das Analyse-Werkzeug etwas anderes gemessen
/// hat als der Client spielt. Alles, was am Klangerzeuger geaendert wird (zuletzt die
/// Rausch-Faerbung, s. NOISE_COLOR), landet damit automatisch auch in der Messung.
/// Die Panorama-Lage wird dabei herausgerechnet: gemessen wird der KLANG, nicht seine Position.
/// Ohne das Herausrechnen hinge die gemessene Lautheit an `pan` -- ein mittig gelegter Klang
/// stuende mit Faktor sqrt(2) da (0,707 auf jedem der beiden Kanaele, s. pan_gains), ein hart
/// seitlicher mit Faktor 1, und ein Vergleich zweier Klaenge waere wertlos.
pub fn render_cue(cue: Cue, sample_rate: f32) -> Vec<f32> {
    let state = Arc::new(Mutex::new(MixerState {
        test_tone_phase: 0.0,
        test_tone_active: false,
        voices: std::array::from_fn(|_| None),
        next_seed: 1,
        next_handle: 1,
        streams: Vec::new(),
            stream_start_fill: stream::DEFAULT_START_FILL,
    }));
    CueSink { state: Arc::clone(&state) }.play_cue(Cue { pan: 0.0, ..cue });
    let frames = (cue.duration() * sample_rate).ceil() as usize;
    let mut daten = vec![0.0f32; frames * 2];
    fill_buffer(&mut daten, 2, sample_rate, &state);
    let (gl, gr) = pan_gains(0.0);
    daten.chunks(2).map(|f| (f[0] + f[1]) / (gl + gr)).collect()
}

/// Wiedergabegeschwindigkeit fuer eine Verschiebung um `halbtoene` Halbtoene, positiv = hoeher.
/// Dieselbe Formel wie in `scripts/klaenge-anhoeren.sh` (dort mit awk berechnet) -- beide
/// muessen fuer denselben Halbtonwert denselben Klang ergeben.
pub fn semitone_factor(halbtoene: f32) -> f32 {
    2f32.powf(halbtoene / 12.0)
}

/// Eine Aufnahme durch den echten Mixerweg rendern -- fuer Tests und Analyse-Werkzeuge, s.
/// `render_cue`. Pan wird herausgerechnet, gemessen wird nur Klang und Zeitverlauf.
pub fn render_sample(sample: Arc<wav::Wav>, gain: f32, pitch: f32, sample_rate: f32) -> Vec<f32> {
    let state = Arc::new(Mutex::new(MixerState {
        test_tone_phase: 0.0,
        test_tone_active: false,
        voices: std::array::from_fn(|_| None),
        next_seed: 1,
        next_handle: 1,
        streams: Vec::new(),
            stream_start_fill: stream::DEFAULT_START_FILL,
    }));
    CueSink { state: Arc::clone(&state) }.play_sample(Arc::clone(&sample), gain, 0.0, pitch);
    let frames = ((sample.duration() / pitch.max(1e-3)) * sample_rate).ceil() as usize;
    let mut daten = vec![0.0f32; frames * 2];
    fill_buffer(&mut daten, 2, sample_rate, &state);
    let (gl, gr) = pan_gains(0.0);
    daten.chunks(2).map(|f| (f[0] + f[1]) / (gl + gr)).collect()
}

pub fn list_output_devices() -> Vec<String> {
    let host = cpal::default_host();
    host.output_devices().map(|it| it.filter_map(|d| d.name().ok()).collect()).unwrap_or_default()
}

pub fn default_output_device_name() -> Option<String> {
    cpal::default_host().default_output_device().and_then(|d| d.name().ok())
}

impl Default for AudioEngine {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod shape_tests {
    use super::*;

    fn mixer() -> Arc<Mutex<MixerState>> {
        Arc::new(Mutex::new(MixerState {
            test_tone_phase: 0.0,
            test_tone_active: false,
            voices: std::array::from_fn(|_| None),
            next_seed: 1,
            next_handle: 1,
            streams: Vec::new(),
            stream_start_fill: stream::DEFAULT_START_FILL,
        }))
    }

    // Einen Klang komplett ausrechnen und die Huellkurve (Betrag je kurzem Abschnitt)
    // zurueckgeben -- damit laesst sich die FORM pruefen, ohne ein Geraet zu haben.
    fn huellkurve(cue: Cue, abschnitte: usize) -> Vec<f32> {
        let state = mixer();
        CueSink { state: Arc::clone(&state) }.play_cue(cue);
        let rate = 48_000.0;
        let frames = (cue.duration() * rate) as usize;
        let mut daten = vec![0.0f32; frames * 2];
        fill_buffer(&mut daten, 2, rate, &state);
        let pro_abschnitt = (frames / abschnitte).max(1);
        daten
            .chunks(2)
            .collect::<Vec<_>>()
            .chunks(pro_abschnitt)
            .map(|block| block.iter().map(|f| f[0].abs().max(f[1].abs())).fold(0.0f32, f32::max))
            .take(abschnitte)
            .collect()
    }

    fn klick() -> Cue {
        Cue { hz_start: 1800.0, hz_end: 1800.0, noise: 0.8, attack: 0.002, decay: 0.033, gain: 0.3, pan: 0.0 }
    }

    #[test]
    // Die Huellkurve muss schnell anschlagen und dann abfallen -- das ist der Unterschied
    // zwischen "Klick" und "Piep". Geprueft an der Form: erster Abschnitt laut, letzter leise,
    // durchgehend fallend nach dem Anschlag.
    fn huellkurve_schlaegt_an_und_faellt_ab() {
        let werte = huellkurve(klick(), 10);
        assert!(werte[0] > 0.1, "muss sofort laut sein, war {}", werte[0]);
        assert!(werte[9] < werte[0] * 0.3, "muss deutlich abgefallen sein: {} -> {}", werte[0], werte[9]);
        for paar in werte.windows(2).skip(1) {
            assert!(paar[1] <= paar[0] + 0.02, "die Huellkurve darf nach dem Anschlag nicht wieder steigen");
        }
    }

    #[test]
    // Der Anschlag ist kurz, aber NICHT null: ein harter Einsatz knackt. Der erste
    // Abtastwert muss also klein sein, obwohl der Klang gleich darauf laut ist.
    fn kein_harter_einsatz() {
        let state = mixer();
        CueSink { state: Arc::clone(&state) }.play_cue(klick());
        let mut daten = vec![0.0f32; 8];
        fill_buffer(&mut daten, 2, 48_000.0, &state);
        assert!(daten[0].abs() < 0.05, "erster Abtastwert muss nahe null sein, war {}", daten[0]);
    }

    #[test]
    // Rauschanteil: ein reiner Sinus ist glatt (aufeinanderfolgende Werte liegen nah
    // beieinander), Rauschen springt. Ohne diesen Unterschied klingt ein "Klick" wie ein Piep.
    fn rauschen_ist_rau_und_sinus_glatt() {
        let sprunghaftigkeit = |noise: f32| {
            let state = mixer();
            CueSink { state: Arc::clone(&state) }.play_cue(Cue { noise, hz_start: 400.0, hz_end: 400.0, ..klick() });
            let mut daten = vec![0.0f32; 512];
            fill_buffer(&mut daten, 2, 48_000.0, &state);
            let links: Vec<f32> = daten.chunks(2).map(|f| f[0]).collect();
            links.windows(2).map(|p| (p[1] - p[0]).abs()).sum::<f32>() / links.len() as f32
        };
        let glatt = sprunghaftigkeit(0.0);
        let rau = sprunghaftigkeit(1.0);
        assert!(rau > glatt * 5.0, "Rauschen muss deutlich sprunghafter sein: {glatt} vs {rau}");
    }

    #[test]
    // Die Tonhoehen-Rampe: steigend und fallend muessen sich unterscheiden, und beide
    // muessen SAUBER gleiten. Wuerde die Phase nicht integriert (sin(t*f(t)) statt
    // phase += f*dt), gaebe es mitten im Klang einen Phasensprung -- hoerbar als Knacken.
    fn tonhoehen_rampe_gleitet_ohne_sprung() {
        let nulldurchgaenge = |von: f32, bis: f32| {
            let state = mixer();
            CueSink { state: Arc::clone(&state) }.play_cue(Cue {
                hz_start: von,
                hz_end: bis,
                noise: 0.0,
                attack: 0.002,
                decay: 0.078,
                gain: 0.3,
                pan: 0.0,
            });
            let rate = 48_000.0;
            let mut daten = vec![0.0f32; (0.08 * rate) as usize * 2];
            fill_buffer(&mut daten, 2, rate, &state);
            let links: Vec<f32> = daten.chunks(2).map(|f| f[0]).collect();
            // Groesster Einzelsprung -- bei einem Phasenfehler waere er nahe der vollen
            // Amplitude.
            let max_sprung = links.windows(2).map(|p| (p[1] - p[0]).abs()).fold(0.0f32, f32::max);
            let wechsel = links.windows(2).filter(|p| (p[0] < 0.0) != (p[1] < 0.0)).count();
            (wechsel, max_sprung)
        };
        let (steigend, sprung_auf) = nulldurchgaenge(400.0, 1600.0);
        let (fallend, sprung_ab) = nulldurchgaenge(1600.0, 400.0);
        assert!(sprung_auf < 0.1, "steigende Rampe darf nicht springen: {sprung_auf}");
        assert!(sprung_ab < 0.1, "fallende Rampe darf nicht springen: {sprung_ab}");
        // Beide durchlaufen denselben Frequenzbereich, also aehnlich viele Nulldurchgaenge --
        // aber die Richtung unterscheidet sie hoerbar. Geprueft wird hier nur, dass ueberhaupt
        // gleitet wird (deutlich mehr Wechsel als bei der niedrigen Frequenz allein).
        assert!(steigend > 40 && fallend > 40, "{steigend} / {fallend}");
    }

    #[test]
    // Dieselbe Formel wie im Durchhoer-Skript (dort per awk): +12 Halbtoene verdoppelt die
    // Geschwindigkeit (eine Oktave hoeher), -12 halbiert sie, 0 aendert nichts.
    fn semitone_factor_stimmt_mit_der_oktave_ueberein() {
        assert!((semitone_factor(0.0) - 1.0).abs() < 1e-6);
        assert!((semitone_factor(12.0) - 2.0).abs() < 1e-5);
        assert!((semitone_factor(-12.0) - 0.5).abs() < 1e-5);
    }

    // Eine synthetische WAV-Datei fuers Testen der Aufnahme-Wiedergabe -- ausreichend lang
    // (200 Werte), damit das Aufrunden auf ganze Abtastwerte (s. render_sample) bei einer
    // Verdoppelung/Halbierung der Geschwindigkeit nicht schon selbst den Vergleich verzerrt.
    fn testklang() -> std::sync::Arc<wav::Wav> {
        let samples = (0..200).map(|i| (i as f32 * 0.3).sin()).collect();
        std::sync::Arc::new(wav::Wav { samples, sample_rate: 1000.0 })
    }

    #[test]
    // Der Kern des Master-Pitch-Reglers: hoehere Tonhoehe (Faktor > 1) macht die Wiedergabe
    // KUERZER, tiefere (Faktor < 1) LAENGER -- dieselbe Kopplung wie bei asetrate im
    // Durchhoer-Skript, nicht zwei unabhaengige Regler.
    fn pitch_aendert_tonhoehe_und_laenge_gekoppelt() {
        let sample = testklang();
        let normal = render_sample(std::sync::Arc::clone(&sample), 1.0, 1.0, 1000.0);
        let hoeher = render_sample(std::sync::Arc::clone(&sample), 1.0, semitone_factor(12.0), 1000.0);
        let tiefer = render_sample(sample, 1.0, semitone_factor(-12.0), 1000.0);
        assert!(hoeher.len() < normal.len(), "eine Oktave hoeher muss kuerzer sein: {} vs {}", hoeher.len(), normal.len());
        assert!(tiefer.len() > normal.len(), "eine Oktave tiefer muss laenger sein: {} vs {}", tiefer.len(), normal.len());
        // Ungefaehr um den erwarteten Faktor, nicht nur "irgendwie kuerzer/laenger".
        let verhaeltnis_hoch = normal.len() as f32 / hoeher.len() as f32;
        let verhaeltnis_tief = tiefer.len() as f32 / normal.len() as f32;
        assert!((verhaeltnis_hoch - 2.0).abs() < 0.1, "{verhaeltnis_hoch}");
        assert!((verhaeltnis_tief - 2.0).abs() < 0.1, "{verhaeltnis_tief}");
    }

    #[test]
    // 1.0 muss die Wiedergabe unveraendert lassen -- der Regler in der Mitte darf keinen
    // Unterschied zum bisherigen Verhalten machen.
    fn pitch_1_0_aendert_nichts() {
        let sample = testklang();
        let referenz = render_sample(std::sync::Arc::clone(&sample), 1.0, 1.0, 1000.0);
        let nochmal = render_sample(sample, 1.0, 1.0, 1000.0);
        assert_eq!(referenz.len(), nochmal.len());
        for (a, b) in referenz.iter().zip(nochmal.iter()) {
            assert!((a - b).abs() < 1e-6);
        }
    }

    #[test]
    // Das Rauschen haengt an der Tonhoehe: ein tiefer Klang muss auch ein DUNKLERES Rauschen
    // haben. Ohne diese Kopplung war das Rauschen weiss und hat die Helligkeit fast allein
    // bestimmt -- der Tippton lag bei 2400 Hz Tonhoehe und trotzdem bei 4637 Hz gemessenem
    // Schwerpunkt. Die Tonhoehe abzusenken half an rauschlastigen Klaengen dann praktisch nicht.
    //
    // Gemessen als Sprunghaftigkeit: hohe Frequenzanteile springen von Abtastwert zu
    // Abtastwert, tiefe nicht.
    fn tiefe_toene_haben_dunkleres_rauschen() {
        let rauheit = |hz: f32| {
            let s = render_cue(Cue { hz_start: hz, hz_end: hz, noise: 1.0, attack: 0.002, decay: 0.05, gain: 0.3, pan: 0.0 }, 48_000.0);
            s.windows(2).map(|p| (p[1] - p[0]).abs()).sum::<f32>() / s.len() as f32
        };
        let tief = rauheit(400.0);
        let hoch = rauheit(3200.0);
        assert!(hoch > tief * 2.0, "hohe Klaenge muessen deutlich rauer rauschen: {tief} vs {hoch}");
    }

    #[test]
    // Die Gegenrechnung der Filter-Daempfung: das Faerben darf die LAUTHEIT nicht mitnehmen.
    // Ohne sie waeren tiefe Klaenge gleichzeitig dunkler und viel leiser geworden -- und die
    // Lautheits-Rangfolge des ganzen Klangsets (s. app::ui_sound) waere still verrutscht.
    fn faerbung_laesst_die_lautheit_in_ruhe() {
        let effektivwert = |hz: f32| {
            let s = render_cue(Cue { hz_start: hz, hz_end: hz, noise: 1.0, attack: 0.002, decay: 0.05, gain: 0.3, pan: 0.0 }, 48_000.0);
            (s.iter().map(|v| v * v).sum::<f32>() / s.len() as f32).sqrt()
        };
        let tief = effektivwert(400.0);
        let hoch = effektivwert(3200.0);
        let verhaeltnis = tief / hoch;
        assert!(verhaeltnis > 0.7 && verhaeltnis < 1.4, "Lautheit darf kaum an der Tonhoehe haengen: {tief} vs {hoch}");
    }

    #[test]
    // Streuung: gleiche Beschreibung, verschiedene Seeds -> verschiedene Tonhoehe, aber in
    // engen Grenzen. Ohne Streuung hoert man bei Hover-Folgen die Wiederholung.
    fn streuung_variiert_in_engen_grenzen() {
        let basis = klick();
        let a = basis.varied(0.08, 1);
        let b = basis.varied(0.08, 2);
        assert!((a.hz_start - b.hz_start).abs() > 1.0, "verschiedene Seeds muessen sich unterscheiden");
        for seed in 0..200 {
            let v = basis.varied(0.08, seed);
            let abweichung = (v.hz_start / basis.hz_start - 1.0).abs();
            assert!(abweichung <= 0.08 + 1e-4, "Streuung ausserhalb der Grenze: {abweichung}");
            assert!(v.gain > 0.0, "Lautstaerke darf nie negativ werden");
        }
    }
}

// Was tatsaechlich AUS DEM MIXER herauskommt, wenn eine Stimme einen Ort hat.
//
// Die Tests in spatial.rs pruefen die Rechnung, diese hier die Kette: Verzoegerungsleitung,
// Ohrfilter, Pegel. Ein Ohrenmodell, das richtig rechnet, aber dessen Ergebnis der Mixer
// nicht anwendet, klaenge exakt wie vorher -- und genau das faellt nur hier auf.
#[cfg(test)]
mod ortungs_tests {
    use super::*;

    const RATE: f32 = 48_000.0;

    fn mixer() -> Arc<Mutex<MixerState>> {
        Arc::new(Mutex::new(MixerState {
            test_tone_phase: 0.0,
            test_tone_active: false,
            voices: std::array::from_fn(|_| None),
            next_seed: 1,
            next_handle: 1,
            streams: Vec::new(),
            stream_start_fill: stream::DEFAULT_START_FILL,
        }))
    }

    // Ein einzelner Ausschlag, danach Stille. Als Messsignal ideal: sein Eintreffen ist ein
    // Zeitpunkt, kein Verlauf -- damit laesst sich die Laufzeit ABLESEN statt schaetzen.
    fn impuls() -> Arc<wav::Wav> {
        let mut samples = vec![0.0f32; 480];
        samples[0] = 1.0;
        Arc::new(wav::Wav { samples, sample_rate: RATE })
    }

    // Beide Kanaele Sample fuer Sample.
    fn spuren(state: &Arc<Mutex<MixerState>>, frames: usize) -> (Vec<f32>, Vec<f32>) {
        let mut daten = vec![0.0f32; frames * 2];
        fill_buffer(&mut daten, 2, RATE, state);
        let mut l = Vec::with_capacity(frames);
        let mut r = Vec::with_capacity(frames);
        for frame in daten.chunks(2) {
            l.push(frame[0]);
            r.push(frame[1]);
        }
        (l, r)
    }

    // Index des groessten Betrags -- der Zeitpunkt, an dem der Impuls an diesem Ohr ankommt.
    fn ankunft(spur: &[f32]) -> usize {
        spur.iter().enumerate().max_by(|a, b| a.1.abs().total_cmp(&b.1.abs())).map(|(i, _)| i).unwrap()
    }

    // Der eigentliche Nachweis: seitlich trifft der Impuls die beiden Ohren zu
    // VERSCHIEDENEN Zeitpunkten, und der Abstand hat die Groesse, die das Modell vorgibt.
    #[test]
    fn eine_quelle_rechts_erreicht_das_rechte_ohr_frueher() {
        let state = mixer();
        CueSink { state: Arc::clone(&state) }.play_sample_at(impuls(), 1.0, 1.0, &spatial::Placement { lateral: 1.0, distance: 1.0 });
        let (l, r) = spuren(&state, 480);

        let erwartet = (spatial::MAX_ITD * RATE).round() as usize;
        let gemessen = ankunft(&l) - ankunft(&r);
        assert!(ankunft(&r) < ankunft(&l), "rechts muss zuerst da sein");
        // Ein Sample Spielraum fuer die Interpolation zwischen den Ringplaetzen.
        assert!(gemessen.abs_diff(erwartet) <= 1, "Versatz {gemessen} Samples, erwartet {erwartet} (= {} ms)", spatial::MAX_ITD * 1000.0);
    }

    // Spiegelbildlich, damit ein vertauschtes Vorzeichen auffaellt. Ein seitenverkehrtes
    // Ohrenmodell waere schlimmer als gar keins: man liefe zuverlaessig in die falsche
    // Richtung.
    #[test]
    fn eine_quelle_links_erreicht_das_linke_ohr_frueher() {
        let state = mixer();
        CueSink { state: Arc::clone(&state) }.play_sample_at(impuls(), 1.0, 1.0, &spatial::Placement { lateral: -1.0, distance: 1.0 });
        let (l, r) = spuren(&state, 480);
        assert!(ankunft(&l) < ankunft(&r), "links muss zuerst da sein");
    }

    // Geradeaus muessen beide Ohren dasselbe hoeren -- sonst wanderte jeder Klang aus der
    // Mitte, obwohl er dort steht.
    #[test]
    fn geradeaus_kommen_beide_ohren_gleichzeitig_und_gleich_laut() {
        let state = mixer();
        CueSink { state: Arc::clone(&state) }.play_sample_at(impuls(), 1.0, 1.0, &spatial::Placement { lateral: 0.0, distance: 1.0 });
        let (l, r) = spuren(&state, 480);
        assert_eq!(l, r);
    }

    // Der Nachlauf (s. Ears::tail): das ferne Ohr darf nicht abgeschnitten werden, wenn die
    // Aufnahme zu Ende ist. Gemessen an einem Klang, der KUERZER ist als die Laufzeit --
    // ohne Nachlauf waere er auf dem fernen Ohr komplett stumm.
    #[test]
    fn das_ferne_ohr_wird_nicht_abgeschnitten() {
        let state = mixer();
        // 10 Samples = 0,21 ms, deutlich kuerzer als die 0,66 ms Laufzeitdifferenz.
        let kurz = Arc::new(wav::Wav {
            samples: {
                let mut v = vec![0.0f32; 10];
                v[0] = 1.0;
                v
            },
            sample_rate: RATE,
        });
        CueSink { state: Arc::clone(&state) }.play_sample_at(kurz, 1.0, 1.0, &spatial::Placement { lateral: 1.0, distance: 1.0 });
        let (l, _) = spuren(&state, 480);
        assert!(l.iter().any(|v| v.abs() > 1e-4), "das linke Ohr blieb stumm");
    }

    // Ausser Reichweite belegt gar nicht erst einen Stimmplatz. Sonst koennte eine Kette
    // unhoerbarer Geraeusche vom anderen Ende der Welt die hoerbaren nahen verdraengen
    // (s. einsetzen: bei Andrang weicht die aelteste Stimme).
    #[test]
    fn unhoerbar_weit_weg_belegt_keinen_platz() {
        let state = mixer();
        let sink = CueSink { state: Arc::clone(&state) };
        sink.play_sample_at(impuls(), 1.0, 1.0, &spatial::Placement { lateral: 0.0, distance: 10_000.0 });
        assert!(state.lock().unwrap().voices.iter().all(|v| v.is_none()));
    }

    // UI-Klaenge bleiben unberuehrt: ohne Ort kein Filter, keine Verzoegerung, kein
    // Pegelverlust -- der Weg durch seiten() darf am alten Verhalten nichts aendern.
    #[test]
    fn ein_klang_ohne_ort_klingt_unveraendert() {
        let state = mixer();
        CueSink { state: Arc::clone(&state) }.play_sample(impuls(), 1.0, 0.0, 1.0);
        let (l, r) = spuren(&state, 64);
        assert_eq!(l, r, "mittig panoramiert = beide Seiten gleich");
        // Ungefiltert und unverzoegert: der Impuls steht unveraendert im ersten Sample.
        let (gl, _) = pan_gains(0.0);
        assert!((l[0] - gl).abs() < 1e-6, "Impuls sollte sofort und in voller Hoehe kommen, war {}", l[0]);
        assert!(l[1..].iter().all(|v| v.abs() < 1e-6), "kein Nachschwingen erwartet");
    }
}

#[cfg(test)]
mod tracking_tests {
    use super::*;

    fn mixer() -> Arc<Mutex<MixerState>> {
        Arc::new(Mutex::new(MixerState {
            test_tone_phase: 0.0,
            test_tone_active: false,
            voices: std::array::from_fn(|_| None),
            next_seed: 1,
            next_handle: 1,
            streams: Vec::new(),
            stream_start_fill: stream::DEFAULT_START_FILL,
        }))
    }

    // Eine Sekunde Gleichspannung -- die Pegel je Kanal lassen sich dann direkt ablesen.
    fn gleichton() -> Arc<wav::Wav> {
        Arc::new(wav::Wav { samples: vec![0.5; 48_000], sample_rate: 48_000.0 })
    }

    fn letzte_pegel(state: &Arc<Mutex<MixerState>>, frames: usize) -> (f32, f32) {
        let mut daten = vec![0.0f32; frames * 2];
        fill_buffer(&mut daten, 2, 48_000.0, state);
        (daten[frames * 2 - 2], daten[frames * 2 - 1])
    }

    const TAG: SoundTag = SoundTag { category: 2, name: 7 };

    #[test]
    // Wer sich von der Quelle entfernt, hoert sie leiser werden -- ohne Knacken, also nach
    // einer kurzen Gleitzeit und nicht sofort.
    fn verschobene_stimme_wird_mit_dem_abstand_leiser() {
        let state = mixer();
        let sink = CueSink { state: Arc::clone(&state) };
        let nah = spatial::Placement { lateral: 0.0, distance: 2.0 };
        let h = sink.play_sample_tracked(gleichton(), 1.0, 1.0, Some(&nah), TAG).unwrap();
        let (vorher, _) = letzte_pegel(&state, 480);
        sink.move_voices(&[(h, spatial::Placement { lateral: 0.0, distance: 30.0 })]);
        let (direkt, _) = letzte_pegel(&state, 1);
        let (spaeter, _) = letzte_pegel(&state, 4800);
        assert!(direkt > spaeter * 2.0, "kein harter Sprung: {direkt} -> {spaeter}");
        assert!((spaeter - vorher * spatial::distance_gain(30.0) / spatial::distance_gain(2.0)).abs() < 1e-3);
    }

    #[test]
    // Ausser Reichweite gestartet: stumm, aber die Stimme lebt und wird hoerbar, sobald man
    // naeher kommt (die Jukebox, auf die man zugeht).
    fn ausser_reichweite_gestartet_wird_beim_naeherkommen_hoerbar() {
        let state = mixer();
        let sink = CueSink { state: Arc::clone(&state) };
        let fern = spatial::Placement { lateral: 0.0, distance: 100.0 };
        let h = sink.play_sample_tracked(gleichton(), 1.0, 1.0, Some(&fern), TAG).unwrap();
        assert_eq!(letzte_pegel(&state, 480), (0.0, 0.0));
        sink.move_voices(&[(h, spatial::Placement { lateral: 0.0, distance: 3.0 })]);
        assert!(letzte_pegel(&state, 4800).0 > 0.1);
    }

    #[test]
    fn stop_trifft_nur_passende_weltgeraeusche() {
        let state = mixer();
        let sink = CueSink { state: Arc::clone(&state) };
        let lage = spatial::Placement { lateral: 0.0, distance: 1.0 };
        sink.play_sample_tracked(gleichton(), 1.0, 1.0, Some(&lage), TAG).unwrap();
        sink.play_sample_tracked(gleichton(), 1.0, 1.0, Some(&lage), SoundTag { category: 4, name: 9 }).unwrap();
        sink.play_sample(gleichton(), 1.0, 0.0, 1.0);
        sink.stop_sounds(Some(2), None);
        letzte_pegel(&state, 4800);
        assert_eq!(state.lock().unwrap().voices.iter().flatten().count(), 2, "nur die Records-Stimme endet");
        sink.stop_sounds(None, None);
        letzte_pegel(&state, 4800);
        assert_eq!(state.lock().unwrap().voices.iter().flatten().count(), 1, "UI-Klaenge bleiben");
    }

    #[test]
    fn klangnamen_ohne_namensraum_gelten_als_minecraft() {
        assert_eq!(sound_name_hash("music_disc.cat"), sound_name_hash("minecraft:music_disc.cat"));
        assert_ne!(sound_name_hash("minecraft:music_disc.cat"), sound_name_hash("minecraft:music_disc.far"));
    }
}
