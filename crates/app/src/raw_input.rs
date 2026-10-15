// GRUNDREGEL: alles, was man in den Einstellungen belegen kann, ist raw. Tasten, Maustasten
// und das Mausrad kommen fuer die Steuerung ueber diesen Weg -- auch dort, wo sie im Menue
// wirken (Lautstaerke per Taste oder Rad), damit sich jede Belegung ueberall gleich schnell
// und gleich verhaelt. Ueber winit laeuft nur, was Oberflaeche ist: Texteingabe (Chat,
// Adressfelder -- dafuer braucht es das Tastaturlayout, das evdev nicht kennt), Klicks und
// Scrollen unter dem gezeichneten Zeiger (Knoepfe, Listen, Links im Chat) sowie Escape und
// Alt+F4. Ohne Zugriff auf /dev/input faellt alles auf winit zurueck.
//
// Optionaler X11-Bypass fuer Maus- UND Tastatur-Eingabe: liest die Events direkt vom
// Kernel-evdev-Device statt ueber Kernel-HID -> libinput -> XInput2 -> winit. Faellt
// automatisch auf den ueblichen winit-Weg zurueck, falls das Device nicht gefunden wird
// oder kein Zugriff besteht (z.B. weil der User nicht in der "input"-Gruppe ist).
//
// Warum auch die Tastatur: X11 bringt fuer Tastatureingaben eigene Eigenheiten mit
// (Key-Repeat, Fokus-Routing, Layout-Umsetzung), die bei latenzkritischen Aktionen (z.B.
// Schlagen/Bewegung auf eine Taste gelegt) genauso stoeren wie beim Klick. Alles ueber
// denselben evdev-Pfad zu lesen ist einheitlich und vorhersagbar. Die Menue-Texteingabe
// bleibt bewusst bei winit -- evdev liefert nur physische Tastencodes, keine
// layout-abhaengigen Zeichen (s. main.rs).

// Angesteckt WAEHREND der Client laeuft: frueher sah der evdev-Pfad nur, was beim Start
// schon da war -- eine neue Tastatur blieb bis zum Neustart tot (ist der Bypass aktiv, ist er
// die ALLEINIGE Quelle fuer Spieltasten). Jetzt schaut ein Waechter regelmaessig nach neuen
// Geraeten, haengt sie an denselben Kanal wie die alten und meldet sie der App (s.
// DeviceWatcher), die daraus eine Benachrichtigung macht.

/// Ein neu erfasstes Geraet, so wie es der Nutzer sieht: ein Stueck Hardware, nicht jeder
/// einzelne Kernel-Endpunkt (eine Gaming-Tastatur bringt gern drei davon mit).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DeviceAdded {
    pub name: String,
    pub keyboard: bool,
    pub pointer: bool,
    /// Worueber gleiche Meldungen zusammengefasst werden (s. physical_device).
    pub key: String,
}

impl DeviceAdded {
    pub fn kind_label(&self) -> &'static str {
        match (self.keyboard, self.pointer) {
            (true, true) => "Tastatur und Maus",
            (true, false) => "Tastatur",
            _ => "Maus",
        }
    }
}

/// Ein einzelner Endpunkt (/dev/input/eventN), wie ihn der Waechter vorfindet.
#[derive(Clone, Debug)]
pub struct Endpoint {
    pub name: String,
    pub phys: String,
    pub keyboard: bool,
    pub pointer: bool,
}

/// Der physische Anschluss ohne den Endpunkt: "usb-0000:00:14.0-2/input1" wird
/// "usb-0000:00:14.0-2". Alle Endpunkte einer Tastatur teilen ihn.
pub fn physical_device(phys: &str) -> &str {
    phys.split('/').next().unwrap_or(phys)
}

/// Die Endpunkte eines Durchgangs zu Geraeten zusammenfassen. Als Name gilt der kuerzeste:
/// "Logitech G915" statt "Logitech G915 Keyboard" und "Logitech G915 Mouse".
pub fn group_endpoints(endpoints: Vec<Endpoint>) -> Vec<DeviceAdded> {
    let mut out: Vec<DeviceAdded> = Vec::new();
    for ep in endpoints {
        if !ep.keyboard && !ep.pointer {
            continue;
        }
        let key = if ep.phys.is_empty() { ep.name.clone() } else { physical_device(&ep.phys).to_string() };
        match out.iter_mut().find(|d| d.key == key) {
            Some(d) => {
                d.keyboard |= ep.keyboard;
                d.pointer |= ep.pointer;
                if ep.name.len() < d.name.len() {
                    d.name = ep.name;
                }
            }
            None => out.push(DeviceAdded { name: ep.name, keyboard: ep.keyboard, pointer: ep.pointer, key }),
        }
    }
    out
}

#[cfg(target_os = "linux")]
mod linux {
    use super::{group_endpoints, DeviceAdded, Endpoint};
    use evdev::{Device, InputEventKind, Key, RelativeAxisType};
    use std::collections::HashSet;
    use std::path::{Path, PathBuf};
    use std::sync::mpsc::{self, Receiver, Sender};
    use std::sync::{Arc, Mutex};
    use std::thread;
    use std::time::{Duration, Instant};
    use winit::keyboard::KeyCode;
    use super::evdev_key_to_keycode;

    // Wie oft der Waechter nachsieht. Ein halbe Sekunde nach dem Einstecken ist fuer den
    // Menschen "sofort", und das Verzeichnis zu lesen kostet so gut wie nichts.
    const WATCH_INTERVAL: Duration = Duration::from_millis(500);
    // Kommen die Endpunkte eines Geraets ueber zwei Durchgaenge verteilt an, gibt es trotzdem
    // nur eine Meldung.
    const ANNOUNCE_DEDUP: Duration = Duration::from_secs(5);

    // Die Endpunkte, die schon jemand geoeffnet (oder als uninteressant abgehakt) hat.
    type Known = Arc<Mutex<HashSet<PathBuf>>>;

    // Nimmt einen Endpunkt wieder aus `Known`, sobald sein Lesefaden endet (Geraet
    // abgesteckt) -- wird es wieder eingesteckt, erkennt der Waechter es neu.
    struct Forget {
        path: PathBuf,
        known: Known,
    }

    impl Drop for Forget {
        fn drop(&mut self) {
            if let Ok(mut k) = self.known.lock() {
                k.remove(&self.path);
            }
        }
    }

    fn event_nodes(entries: std::fs::ReadDir) -> impl Iterator<Item = PathBuf> {
        entries.flatten().map(|e| e.path()).filter(|p| p.file_name().and_then(|n| n.to_str()).is_some_and(|n| n.starts_with("event")))
    }

    // Echtes Zeigergeraet = hat BEIDE relativen Achsen X und Y (s. open). Tastatur = hat
    // Buchstabentasten (KEY_A UND KEY_Z).
    fn classify(device: &Device) -> (bool, bool) {
        let pointer = device
            .supported_relative_axes()
            .is_some_and(|axes| axes.contains(RelativeAxisType::REL_X) && axes.contains(RelativeAxisType::REL_Y));
        let keyboard = device.supported_keys().is_some_and(|keys| keys.contains(Key::KEY_A) && keys.contains(Key::KEY_Z));
        (pointer, keyboard)
    }

    // Einen geoeffneten Endpunkt an die passenden Kanaele haengen. Ein Kanal fehlt, wenn der
    // zugehoerige Bypass nicht aktiv ist -- dann liest winit dieses Geraet ohnehin mit.
    fn attach(device: Device, path: &Path, mouse_tx: Option<&Sender<RawMouseEvent>>, key_tx: Option<&Sender<(KeyCode, bool)>>, known: &Known) -> Endpoint {
        let (pointer, keyboard) = classify(&device);
        let name = device.name().unwrap_or("<unbekannt>").to_string();
        let phys = device.physical_path().unwrap_or("").to_string();
        let forget = || Forget { path: path.to_path_buf(), known: known.clone() };
        let mut device = Some(device);
        // Ein Endpunkt kann beides sein; der zweite Lesefaden braucht dann ein eigenes Handle.
        if keyboard {
            if let Some(tx) = key_tx {
                if let Some(d) = device.take().or_else(|| Device::open(path).ok()) {
                    spawn_key_reader(d, tx.clone(), forget());
                }
            }
        }
        if pointer {
            if let Some(tx) = mouse_tx {
                if let Some(d) = device.take().or_else(|| Device::open(path).ok()) {
                    spawn_reader(d, tx.clone(), forget());
                }
            }
        }
        Endpoint { name, phys, keyboard, pointer }
    }

    /// Meldet neu angesteckte Eingabegeraete (s. Modulkommentar oben).
    pub struct DeviceWatcher {
        rx: Receiver<DeviceAdded>,
    }

    impl DeviceWatcher {
        pub fn drain(&self) -> Vec<DeviceAdded> {
            self.rx.try_iter().collect()
        }
    }

    fn spawn_watcher(known: Known, mouse_tx: Option<Sender<RawMouseEvent>>, key_tx: Option<Sender<(KeyCode, bool)>>) -> DeviceWatcher {
        let (tx, rx) = mpsc::channel();
        thread::spawn(move || {
            let mut angekuendigt: Vec<(String, Instant)> = Vec::new();
            loop {
                thread::sleep(WATCH_INTERVAL);
                let Ok(entries) = std::fs::read_dir("/dev/input") else { continue };
                let da: HashSet<PathBuf> = event_nodes(entries).collect();
                // Verschwundene Knoten vergessen: der Kernel vergibt ihre Nummern neu.
                let neu: Vec<PathBuf> = {
                    let Ok(mut k) = known.lock() else { return };
                    k.retain(|p| da.contains(p));
                    da.difference(&k).cloned().collect()
                };
                let mut endpunkte = Vec::new();
                for path in neu {
                    // Direkt nach dem Einstecken setzt udev die Rechte oft erst einen Moment
                    // spaeter -- dann klappt es beim naechsten Durchgang.
                    let Ok(device) = Device::open(&path) else { continue };
                    if let Ok(mut k) = known.lock() {
                        k.insert(path.clone());
                    }
                    let ep = attach(device, &path, mouse_tx.as_ref(), key_tx.as_ref(), &known);
                    if ep.keyboard || ep.pointer {
                        println!("[latency] Eingabegerät angesteckt und erfasst: {} ({})", ep.name, path.display());
                    }
                    endpunkte.push(ep);
                }
                angekuendigt.retain(|(_, t)| t.elapsed() < ANNOUNCE_DEDUP);
                for geraet in group_endpoints(endpunkte) {
                    if angekuendigt.iter().any(|(k, _)| *k == geraet.key) {
                        continue;
                    }
                    angekuendigt.push((geraet.key.clone(), Instant::now()));
                    if tx.send(geraet).is_err() {
                        return; // App weg
                    }
                }
            }
        });
        DeviceWatcher { rx }
    }

    // Echtes Anstecken, nachgestellt ueber uinput: braucht Zugriff auf /dev/uinput und
    // /dev/input (Gruppen "uinput" und "input"), deshalb nur auf Zuruf:
    //   cargo test -p app angesteckte_tastatur -- --ignored
    // Es wird bewusst KEINE Taste gedrueckt -- die virtuelle Tastatur sieht auch der Desktop,
    // und ein "a" landete im gerade offenen Fenster.
    #[cfg(test)]
    mod tests {
        use super::*;
        use evdev::uinput::VirtualDeviceBuilder;
        use evdev::AttributeSet;

        #[test]
        #[ignore]
        fn angesteckte_tastatur_wird_erfasst_und_angekuendigt() {
            let (_maus, _tastatur, waechter) = open();
            let waechter = waechter.expect("Wächter läuft");
            let mut keys = AttributeSet::<Key>::new();
            keys.insert(Key::KEY_A);
            keys.insert(Key::KEY_Z);
            let _geraet = VirtualDeviceBuilder::new()
                .expect("/dev/uinput")
                .name("Coral Client Testtastatur")
                .with_keys(&keys)
                .expect("Tasten")
                .build()
                .expect("virtuelles Gerät");
            let bis = Instant::now() + Duration::from_secs(3);
            let mut gesehen = Vec::new();
            while Instant::now() < bis {
                gesehen.extend(waechter.drain());
                if gesehen.iter().any(|d: &DeviceAdded| d.name == "Coral Client Testtastatur") {
                    break;
                }
                thread::sleep(Duration::from_millis(50));
            }
            let d = gesehen.iter().find(|d| d.name == "Coral Client Testtastatur").unwrap_or_else(|| panic!("nicht angekuendigt: {gesehen:?}"));
            assert!(d.keyboard && !d.pointer);
            // Nur einmal angekuendigt, auch nach weiteren Durchgaengen.
            thread::sleep(Duration::from_millis(1200));
            assert!(!waechter.drain().iter().any(|d| d.name == "Coral Client Testtastatur"));
        }
    }

    /// Oeffnet alle Maeuse und Tastaturen (s. RawMouse/RawKeyboard) und startet den Waechter
    /// fuer spaeter angesteckte Geraete.
    pub fn open() -> (Option<RawMouse>, Option<RawKeyboard>, Option<DeviceWatcher>) {
        let entries = match std::fs::read_dir("/dev/input") {
            Ok(entries) => entries,
            Err(e) => {
                println!("[latency] Raw-Input-Bypass: /dev/input nicht lesbar: {e}");
                return (None, None, None);
            }
        };
        let known: Known = Arc::new(Mutex::new(HashSet::new()));
        let (mouse_tx, mouse_rx) = mpsc::channel();
        let (key_tx, key_rx) = mpsc::channel();
        let mut maeuse: Vec<String> = Vec::new();
        let mut tastaturen: Vec<String> = Vec::new();
        let mut open_errors: Vec<String> = Vec::new();
        for path in event_nodes(entries) {
            match Device::open(&path) {
                Ok(device) => {
                    if let Ok(mut k) = known.lock() {
                        k.insert(path.clone());
                    }
                    let ep = attach(device, &path, Some(&mouse_tx), Some(&key_tx), &known);
                    if ep.pointer {
                        maeuse.push(ep.name.clone());
                    }
                    if ep.keyboard {
                        tastaturen.push(ep.name);
                    }
                }
                Err(e) => open_errors.push(format!("{}: {e}", path.display())),
            }
        }

        let mouse = if maeuse.is_empty() {
            println!("[latency] Raw-Mouse-Bypass: kein Zeigergerät (REL_X/REL_Y) gefunden/zugreifbar");
            if let Some(first_error) = open_errors.first() {
                println!(
                    "[latency]   {} Device(s) ohne Zugriff (z.B. {}), prüfe 'groups' bzw. 'input'-Gruppenmitgliedschaft",
                    open_errors.len(),
                    first_error
                );
            }
            None
        } else {
            println!(
                "[latency] Raw-Mouse-Bypass aktiv (direkter evdev-Zugriff, kein X11-Umweg): {} Gerät(e) [{}]",
                maeuse.len(),
                maeuse.join(", ")
            );
            Some(RawMouse { rx: mouse_rx })
        };
        let keyboard = if tastaturen.is_empty() {
            println!(
                "[latency] Raw-Keyboard-Bypass: keine zugreifbare Tastatur gefunden -- \
                 pruefe 'input'-Gruppenmitgliedschaft. Fallback auf winit-Tastatur."
            );
            None
        } else {
            println!(
                "[latency] Raw-Keyboard-Bypass aktiv (direkter evdev-Zugriff, kein X11-Umweg): {} Gerät(e) [{}]",
                tastaturen.len(),
                tastaturen.join(", ")
            );
            Some(RawKeyboard { rx: key_rx })
        };
        // Neue Geraete nur an die Kanaele haengen, deren Bypass laeuft. Ohne Bypass liest
        // winit ohnehin alle Geraete; eine Maus, die dann ploetzlich doch roh gelesen wuerde,
        // zaehlte doppelt. Angekuendigt werden neue Geraete trotzdem.
        let watcher = spawn_watcher(known, mouse.is_some().then_some(mouse_tx), keyboard.is_some().then_some(key_tx));
        (mouse, keyboard, Some(watcher))
    }

    pub struct RawMouse {
        rx: Receiver<RawMouseEvent>,
    }

    // Bewegung und Tastendruck laufen in EINEM Kanal, in der Reihenfolge, in der der Kernel
    // sie geliefert hat.
    //
    // ECHTER BUG gewesen (im Inventar gingen schnelle Linksklicks verloren):
    // vorher gab es zwei Kanaele, und der Frame hat erst die GESAMTE Bewegung des Frames auf
    // den Zeiger gerechnet und danach die Klicks ausgewertet. Ein Klick mitten in einer
    // Bewegung wurde damit an der Position vom FRAME-ENDE ausgewertet -- bei 8 ms Frame und
    // einer schnellen Mausbewegung ist das im Inventar leicht ein Slot weiter oder die Ritze
    // dazwischen, und der Klick tat scheinbar nichts. Vanilla kann das nicht: bei GLFW
    // stehen Position und Tastendruck im selben geordneten Eventstrom.
    #[derive(Clone, Copy, Debug)]
    pub enum RawMouseEvent {
        Motion(f64, f64),
        Button(MouseButton, bool),
        // Mausrad in Rasten, positiv = hoch (evdev REL_WHEEL). Laeuft im selben geordneten
        // Strom, damit die Hotbar im Spiel ueber denselben schnellen Weg blaettert wie der
        // Klick -- und nicht mehr ueber X11.
        Wheel(f32),
    }

    #[derive(Clone, Copy, PartialEq, Eq, Debug)]
    pub enum MouseButton {
        Left,
        Right,
        Middle,
        // Die beiden Daumentasten. evdev nennt sie BTN_SIDE/BTN_EXTRA, winit Back/Forward --
        // gemeint sind dieselben zwei Knoepfe.
        Side,
        Extra,
    }

    impl RawMouse {
        // Maeuse = ALLE echten Zeigergeraete (jedes /dev/input/event*, das REL_X UND REL_Y
        // meldet), ihre Deltas in einem Kanal zusammengefuehrt -- genau wie winits MouseMotion,
        // das ebenfalls alle Maeuse zusammenfasst, nur eben ueber den direkten evdev-Pfad. Kein
        // fest verdrahtetes Vendor/Product: mit zwei Maeusen (z.B. Sharkoon + ROCCAT) wuerde
        // ein Filter auf genau eine dazu fuehren, dass die andere Maus tot ist -- und seit der
        // winit-Fallback in device_event abgeschaltet ist (Doppel-Zaehlung), waere der Cursor
        // dann eingefroren. Geraete mit nur REL_WHEEL (Tastaturen mit Rad) fallen ueber die
        // REL_X/REL_Y-Pflicht heraus. Jedes Device wird selbst geoeffnet (statt
        // evdev::enumerate(), das Zugriffsfehler still verschluckt), damit es bei Problemen
        // (z.B. fehlende "input"-Gruppe) eine konkrete Fehlermeldung gibt. Geoeffnet wird in
        // open() weiter oben.

        // Alles seit dem letzten Aufruf, in der Reihenfolge des Kernels -- Bewegung und
        // Klicks gemischt (s. RawMouseEvent).
        pub fn drain(&self) -> Vec<RawMouseEvent> {
            let mut out = Vec::new();
            while let Ok(event) = self.rx.try_recv() {
                out.push(event);
            }
            out
        }
    }

    fn spawn_reader(mut device: Device, tx: Sender<RawMouseEvent>, _forget: Forget) {
        thread::spawn(move || loop {
            let _forget = &_forget;
            let events = match device.fetch_events() {
                Ok(events) => events,
                Err(_) => break, // Device weg (z.B. abgesteckt) -> Thread beenden
            };

            // Bewegung wird weiterhin zusammengefasst -- aber nur bis zum naechsten Klick,
            // der sie vorher rausschickt (s. RawMouseEvent). Damit bleibt die Reihenfolge
            // "erst bewegt, dann geklickt" bzw. "erst geklickt, dann bewegt" erhalten.
            let mut delta = (0.0, 0.0);
            let mut receiver_gone = false;
            for ev in events {
                match ev.kind() {
                    InputEventKind::RelAxis(axis) => match axis {
                        RelativeAxisType::REL_X => delta.0 += ev.value() as f64,
                        RelativeAxisType::REL_Y => delta.1 += ev.value() as f64,
                        // Nur die ganzen Rasten (REL_WHEEL), nicht REL_WHEEL_HI_RES: ein Rad
                        // mit feiner Aufloesung meldet beides, und eine Raste soll genau ein
                        // Hotbar-Platz sein.
                        RelativeAxisType::REL_WHEEL if ev.value() != 0 => {
                            // Wie beim Klick: die Bewegung davor zuerst.
                            if delta != (0.0, 0.0) {
                                if tx.send(RawMouseEvent::Motion(delta.0, delta.1)).is_err() {
                                    receiver_gone = true;
                                }
                                delta = (0.0, 0.0);
                            }
                            if tx.send(RawMouseEvent::Wheel(ev.value() as f32)).is_err() {
                                receiver_gone = true;
                            }
                        }
                        _ => {}
                    },
                    // Maustasten laufen ueber denselben evdev-Strom -- der Klick nimmt
                    // damit denselben Low-Latency-Pfad wie die Bewegung (fuers PvP der
                    // wichtigste einzelne Input ueberhaupt). Wert 1 = Druck, 0 = Loslassen,
                    // 2 = Repeat (bei Maustasten praktisch nie, wird ignoriert).
                    InputEventKind::Key(key) if ev.value() != 2 => {
                        let button = match key {
                            Key::BTN_LEFT => Some(MouseButton::Left),
                            Key::BTN_RIGHT => Some(MouseButton::Right),
                            Key::BTN_MIDDLE => Some(MouseButton::Middle),
                            Key::BTN_SIDE => Some(MouseButton::Side),
                            Key::BTN_EXTRA => Some(MouseButton::Extra),
                            _ => None,
                        };
                        if let Some(button) = button {
                            // Die bis hierher gesammelte Bewegung MUSS vor dem Klick raus,
                            // sonst wertet der Frame den Klick an der Position danach aus.
                            if delta != (0.0, 0.0) {
                                if tx.send(RawMouseEvent::Motion(delta.0, delta.1)).is_err() {
                                    receiver_gone = true;
                                }
                                delta = (0.0, 0.0);
                            }
                            if tx.send(RawMouseEvent::Button(button, ev.value() == 1)).is_err() {
                                receiver_gone = true;
                            }
                        }
                    }
                    _ => {}
                }
            }

            if receiver_gone || (delta != (0.0, 0.0) && tx.send(RawMouseEvent::Motion(delta.0, delta.1)).is_err()) {
                break; // Empfaenger weg -> Thread beenden
            }
        });
    }

    // Tastatur-Pendant zu RawMouse. Liefert einzelne Tasten-Uebergaenge (KeyCode, gedrueckt)
    // in Reihenfolge, nicht akkumuliert -- eine Taste "gedrueckt/losgelassen" ist ein
    // Zustandswechsel, kein Summenwert wie Maus-Deltas.
    pub struct RawKeyboard {
        rx: Receiver<(KeyCode, bool)>,
    }

    impl RawKeyboard {
        // Tastaturen = ALLE /dev/input/event*, die Buchstabentasten unterstuetzen (KEY_A UND
        // KEY_Z, damit BTN_*-Geraete und Ein-Tasten-Endpunkte wie Power/Video-Bus rausfallen),
        // ihre Tasten-Uebergaenge in einem Kanal zusammengefuehrt. Genau wie bei der Maus: ein
        // "nimm das erste Treffer-Geraet"-Filter kann auf einem Companion-Endpunkt landen (z.B.
        // die Makro-"Keyboard"-Schnittstelle der ROCCAT-Maus), auf dem physisch nie getippt
        // wird -> gar keine Eingabe. winit fasst ebenfalls alle Tastaturen zusammen; wir machen
        // dasselbe ueber den direkten evdev-Pfad. Ungenutzte Endpunkte senden im Ruhezustand
        // nichts, das Zusammenfuehren ist also harmlos. Geoeffnet wird in open() weiter oben.

        // Alle seit dem letzten Aufruf eingegangenen Tasten-Uebergaenge, in Reihenfolge.
        pub fn drain(&self) -> Vec<(KeyCode, bool)> {
            let mut out = Vec::new();
            while let Ok(event) = self.rx.try_recv() {
                out.push(event);
            }
            out
        }
    }

    fn spawn_key_reader(mut device: Device, tx: Sender<(KeyCode, bool)>, _forget: Forget) {
        thread::spawn(move || loop {
            let _forget = &_forget;
            let events = match device.fetch_events() {
                Ok(events) => events,
                Err(_) => break,
            };
            for ev in events {
                if let InputEventKind::Key(key) = ev.kind() {
                    // 1 = Druck, 0 = Loslassen, 2 = Auto-Repeat (fuer gehaltene Tasten
                    // irrelevant -- den Halte-Zustand tragen die Booleans in der App).
                    let pressed = match ev.value() {
                        1 => true,
                        0 => false,
                        _ => continue,
                    };
                    if let Some(code) = evdev_key_to_keycode(key.code()) {
                        if tx.send((code, pressed)).is_err() {
                            return; // Empfaenger weg
                        }
                    }
                }
            }
        });
    }

}

// Linux-Input-Event-Codes (stabil, entsprechen input-event-codes.h) auf winits KeyCode.
// Beide bezeichnen die PHYSISCHE Taste (US-QWERTY-Referenzlage), stimmen also ueberein --
// dadurch passt der hier gelieferte KeyCode direkt zu den ueber winit erfassten Keybinds
// (s. Keybinds in main.rs). Nicht abgebildete Codes liefern None und loesen keine Aktion
// aus (dann muesste der Nutzer auf eine abgebildete Taste umbelegen). Auch fuer Windows:
// die evdev-Codes 1..88 SIND die PC-Scancodes (Satz 1), s. windows::scancode_to_evdev.
#[cfg(any(target_os = "linux", windows))]
fn evdev_key_to_keycode(code: u16) -> Option<winit::keyboard::KeyCode> {
    use winit::keyboard::KeyCode;
    {
        Some(match code {
            1 => KeyCode::Escape,
            2 => KeyCode::Digit1,
            3 => KeyCode::Digit2,
            4 => KeyCode::Digit3,
            5 => KeyCode::Digit4,
            6 => KeyCode::Digit5,
            7 => KeyCode::Digit6,
            8 => KeyCode::Digit7,
            9 => KeyCode::Digit8,
            10 => KeyCode::Digit9,
            11 => KeyCode::Digit0,
            12 => KeyCode::Minus,
            13 => KeyCode::Equal,
            14 => KeyCode::Backspace,
            15 => KeyCode::Tab,
            16 => KeyCode::KeyQ,
            17 => KeyCode::KeyW,
            18 => KeyCode::KeyE,
            19 => KeyCode::KeyR,
            20 => KeyCode::KeyT,
            21 => KeyCode::KeyY,
            22 => KeyCode::KeyU,
            23 => KeyCode::KeyI,
            24 => KeyCode::KeyO,
            25 => KeyCode::KeyP,
            26 => KeyCode::BracketLeft,
            27 => KeyCode::BracketRight,
            28 => KeyCode::Enter,
            29 => KeyCode::ControlLeft,
            30 => KeyCode::KeyA,
            31 => KeyCode::KeyS,
            32 => KeyCode::KeyD,
            33 => KeyCode::KeyF,
            34 => KeyCode::KeyG,
            35 => KeyCode::KeyH,
            36 => KeyCode::KeyJ,
            37 => KeyCode::KeyK,
            38 => KeyCode::KeyL,
            39 => KeyCode::Semicolon,
            40 => KeyCode::Quote,
            41 => KeyCode::Backquote,
            42 => KeyCode::ShiftLeft,
            43 => KeyCode::Backslash,
            44 => KeyCode::KeyZ,
            45 => KeyCode::KeyX,
            46 => KeyCode::KeyC,
            47 => KeyCode::KeyV,
            48 => KeyCode::KeyB,
            49 => KeyCode::KeyN,
            50 => KeyCode::KeyM,
            51 => KeyCode::Comma,
            52 => KeyCode::Period,
            53 => KeyCode::Slash,
            54 => KeyCode::ShiftRight,
            55 => KeyCode::NumpadMultiply,
            56 => KeyCode::AltLeft,
            57 => KeyCode::Space,
            58 => KeyCode::CapsLock,
            59 => KeyCode::F1,
            60 => KeyCode::F2,
            61 => KeyCode::F3,
            62 => KeyCode::F4,
            63 => KeyCode::F5,
            64 => KeyCode::F6,
            65 => KeyCode::F7,
            66 => KeyCode::F8,
            67 => KeyCode::F9,
            68 => KeyCode::F10,
            69 => KeyCode::NumLock,
            70 => KeyCode::ScrollLock,
            71 => KeyCode::Numpad7,
            72 => KeyCode::Numpad8,
            73 => KeyCode::Numpad9,
            74 => KeyCode::NumpadSubtract,
            75 => KeyCode::Numpad4,
            76 => KeyCode::Numpad5,
            77 => KeyCode::Numpad6,
            78 => KeyCode::NumpadAdd,
            79 => KeyCode::Numpad1,
            80 => KeyCode::Numpad2,
            81 => KeyCode::Numpad3,
            82 => KeyCode::Numpad0,
            83 => KeyCode::NumpadDecimal,
            87 => KeyCode::F11,
            88 => KeyCode::F12,
            96 => KeyCode::NumpadEnter,
            97 => KeyCode::ControlRight,
            98 => KeyCode::NumpadDivide,
            100 => KeyCode::AltRight,
            102 => KeyCode::Home,
            103 => KeyCode::ArrowUp,
            104 => KeyCode::PageUp,
            105 => KeyCode::ArrowLeft,
            106 => KeyCode::ArrowRight,
            107 => KeyCode::End,
            108 => KeyCode::ArrowDown,
            109 => KeyCode::PageDown,
            110 => KeyCode::Insert,
            111 => KeyCode::Delete,
            125 => KeyCode::SuperLeft,
            126 => KeyCode::SuperRight,
            127 => KeyCode::ContextMenu,
            _ => return None,
        })
    }
}

#[cfg(target_os = "linux")]
pub use linux::{open, DeviceWatcher, MouseButton, RawKeyboard, RawMouse, RawMouseEvent};

// Windows: derselbe Bypass ueber Raw Input (WM_INPUT). Ein eigener Thread mit einem
// unsichtbaren Nachrichtenfenster empfaengt die Rohdaten von Maus und Tastatur -- nicht ueber
// die Nachrichtenschleife des Spielfensters, die waehrend des Renderns steht. Dieselben
// Kanaele wie unter Linux: Bewegung, Tasten und Rad in EINEM geordneten Strom (s.
// RawMouseEvent), Tastenwechsel als physische KeyCodes.
//
// Raw Input gilt je Prozess und Geraeteklasse fuer EIN Zielfenster: diese Registrierung loest
// die von winit ab (winit registriert beim Anlegen der Event-Loop, open() laeuft danach).
// winit liefert dann keine DeviceEvents mehr -- die ignoriert die App bei aktivem Bypass
// ohnehin (s. device_event). Die normalen Fensternachrichten (Texteingabe, Escape, Klicks
// auf die Oberflaeche) bleiben, es gibt kein RIDEV_NOLEGACY.
//
// Kein Waechter fuer neue Geraete: Raw Input liefert von selbst alle Maeuse und Tastaturen,
// auch spaeter angesteckte.
#[cfg(windows)]
mod windows {
    use super::{MouseButton, RawMouseEvent};
    use std::sync::mpsc::{self, Receiver, Sender};
    use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
    use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
    use windows_sys::Win32::UI::Input::{
        GetRawInputData, RegisterRawInputDevices, HRAWINPUT, MOUSE_MOVE_ABSOLUTE, RAWINPUT, RAWINPUTDEVICE, RAWINPUTHEADER, RIDEV_INPUTSINK, RID_INPUT,
        RIM_TYPEKEYBOARD, RIM_TYPEMOUSE,
    };
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        CreateWindowExW, DefWindowProcW, DispatchMessageW, GetMessageW, RegisterClassExW, HWND_MESSAGE, MSG, RI_KEY_BREAK, RI_KEY_E0, RI_KEY_E1,
        RI_MOUSE_BUTTON_4_DOWN, RI_MOUSE_BUTTON_4_UP, RI_MOUSE_BUTTON_5_DOWN, RI_MOUSE_BUTTON_5_UP, RI_MOUSE_LEFT_BUTTON_DOWN, RI_MOUSE_LEFT_BUTTON_UP,
        RI_MOUSE_MIDDLE_BUTTON_DOWN, RI_MOUSE_MIDDLE_BUTTON_UP, RI_MOUSE_RIGHT_BUTTON_DOWN, RI_MOUSE_RIGHT_BUTTON_UP, RI_MOUSE_WHEEL, WM_INPUT,
        WNDCLASSEXW,
    };
    use winit::keyboard::KeyCode;

    pub struct RawMouse {
        rx: Receiver<RawMouseEvent>,
    }

    impl RawMouse {
        // Alles seit dem letzten Aufruf, in der Reihenfolge von Windows (s. Linux-Pendant).
        pub fn drain(&self) -> Vec<RawMouseEvent> {
            self.rx.try_iter().collect()
        }
    }

    pub struct RawKeyboard {
        rx: Receiver<(KeyCode, bool)>,
    }

    impl RawKeyboard {
        pub fn drain(&self) -> Vec<(KeyCode, bool)> {
            self.rx.try_iter().collect()
        }
    }

    // Kanaele des Lesefadens. Thread-lokal, weil die Fensterprozedur nur ueber globale Wege an
    // sie herankommt; das Fenster gehoert genau diesem einen Faden.
    thread_local! {
        static KANAELE: std::cell::RefCell<Option<(Sender<RawMouseEvent>, Sender<(KeyCode, bool)>)>> = const { std::cell::RefCell::new(None) };
    }

    /// Satz-1-Scancode (mit E0-Praefix) auf den evdev-Code derselben Taste. Ohne E0 sind beide
    /// gleich; die E0-Tasten (Pfeile, rechte Strg/Alt, Navigationsblock) hat evdev auf eigenen
    /// Codes. None fuer die unechten Umschalttasten, die Windows vor manche E0-Tasten setzt.
    pub fn scancode_to_evdev(code: u16, e0: bool) -> Option<u16> {
        if !e0 {
            return Some(code);
        }
        Some(match code {
            0x1C => 96,  // Ziffernblock-Enter
            0x1D => 97,  // Strg rechts
            0x35 => 98,  // Ziffernblock /
            0x38 => 100, // Alt rechts (AltGr)
            0x47 => 102, // Pos1
            0x48 => 103, // Pfeil hoch
            0x49 => 104, // Bild hoch
            0x4B => 105, // Pfeil links
            0x4D => 106, // Pfeil rechts
            0x4F => 107, // Ende
            0x50 => 108, // Pfeil runter
            0x51 => 109, // Bild runter
            0x52 => 110, // Einfg
            0x53 => 111, // Entf
            0x5B => 125, // Windows links
            0x5C => 126, // Windows rechts
            0x5D => 127, // Kontextmenue
            _ => return None,
        })
    }

    fn senden(mouse: &[RawMouseEvent], key: Option<(KeyCode, bool)>) {
        KANAELE.with(|k| {
            if let Some((mtx, ktx)) = k.borrow().as_ref() {
                for e in mouse {
                    let _ = mtx.send(*e);
                }
                if let Some(t) = key {
                    let _ = ktx.send(t);
                }
            }
        });
    }

    fn maus(m: &windows_sys::Win32::UI::Input::RAWMOUSE) {
        let mut out = Vec::new();
        // Bewegung zuerst, dann Tasten: ein Paket kann beides tragen, und der Klick soll an
        // der Position NACH der Bewegung gelten (s. RawMouseEvent). Absolute Meldungen
        // (Grafiktablett, Fernsitzung) sind keine Maus-Deltas.
        if m.usFlags & MOUSE_MOVE_ABSOLUTE == 0 && (m.lLastX != 0 || m.lLastY != 0) {
            out.push(RawMouseEvent::Motion(m.lLastX as f64, m.lLastY as f64));
        }
        // SAFETY: bei RIM_TYPEMOUSE ist der Tasten-Zweig der Union belegt.
        let (flags, daten) = unsafe { (m.Anonymous.Anonymous.usButtonFlags as u32, m.Anonymous.Anonymous.usButtonData) };
        let tasten = [
            (RI_MOUSE_LEFT_BUTTON_DOWN, RI_MOUSE_LEFT_BUTTON_UP, MouseButton::Left),
            (RI_MOUSE_RIGHT_BUTTON_DOWN, RI_MOUSE_RIGHT_BUTTON_UP, MouseButton::Right),
            (RI_MOUSE_MIDDLE_BUTTON_DOWN, RI_MOUSE_MIDDLE_BUTTON_UP, MouseButton::Middle),
            (RI_MOUSE_BUTTON_4_DOWN, RI_MOUSE_BUTTON_4_UP, MouseButton::Side),
            (RI_MOUSE_BUTTON_5_DOWN, RI_MOUSE_BUTTON_5_UP, MouseButton::Extra),
        ];
        for (runter, hoch, taste) in tasten {
            if flags & runter as u32 != 0 {
                out.push(RawMouseEvent::Button(taste, true));
            }
            if flags & hoch as u32 != 0 {
                out.push(RawMouseEvent::Button(taste, false));
            }
        }
        // Rad: WHEEL_DELTA (120) je Raste, positiv = von sich weg = hoch, wie evdev REL_WHEEL.
        // Feine Raeder melden Bruchteile -- die zaehlen anteilig.
        if flags & RI_MOUSE_WHEEL as u32 != 0 {
            let rasten = daten as i16 as f32 / 120.0;
            if rasten != 0.0 {
                out.push(RawMouseEvent::Wheel(rasten));
            }
        }
        if !out.is_empty() {
            senden(&out, None);
        }
    }

    fn tastatur(k: &windows_sys::Win32::UI::Input::RAWKEYBOARD) {
        // E1 = Pause/Untbr, 0xFF = Ueberlauf: beide keine belegbaren Tasten.
        if k.Flags as u32 & RI_KEY_E1 as u32 != 0 || k.MakeCode == 0xFF || k.MakeCode == 0 {
            return;
        }
        let e0 = k.Flags as u32 & RI_KEY_E0 as u32 != 0;
        let gedrueckt = k.Flags as u32 & RI_KEY_BREAK as u32 == 0;
        if let Some(code) = scancode_to_evdev(k.MakeCode, e0).and_then(super::evdev_key_to_keycode) {
            senden(&[], Some((code, gedrueckt)));
        }
    }

    unsafe extern "system" fn fensterprozedur(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
        if msg == WM_INPUT {
            let mut puffer = std::mem::MaybeUninit::<RAWINPUT>::uninit();
            let mut groesse = std::mem::size_of::<RAWINPUT>() as u32;
            let gelesen = GetRawInputData(lparam as HRAWINPUT, RID_INPUT, puffer.as_mut_ptr().cast(), &mut groesse, std::mem::size_of::<RAWINPUTHEADER>() as u32);
            if gelesen != u32::MAX && gelesen as usize >= std::mem::size_of::<RAWINPUTHEADER>() {
                let ri = puffer.assume_init_ref();
                match ri.header.dwType {
                    RIM_TYPEMOUSE => maus(&ri.data.mouse),
                    RIM_TYPEKEYBOARD => tastatur(&ri.data.keyboard),
                    _ => {}
                }
            }
        }
        DefWindowProcW(hwnd, msg, wparam, lparam)
    }

    // Lesefaden mit eigenem Nachrichtenfenster. Meldet ueber `bereit`, ob die Registrierung
    // geklappt hat, und laeuft danach so lange wie der Prozess.
    fn lesefaden(mouse_tx: Sender<RawMouseEvent>, key_tx: Sender<(KeyCode, bool)>, bereit: Sender<Result<(), String>>) {
        KANAELE.with(|k| *k.borrow_mut() = Some((mouse_tx, key_tx)));
        let klasse: Vec<u16> = "CoralRawInput\0".encode_utf16().collect();
        // SAFETY: Klasse und Fenster gehoeren diesem Faden; alle Zeiger leben bis zum Aufruf.
        let hwnd = unsafe {
            let instanz = GetModuleHandleW(std::ptr::null());
            let wc = WNDCLASSEXW {
                cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
                lpfnWndProc: Some(fensterprozedur),
                hInstance: instanz,
                lpszClassName: klasse.as_ptr(),
                ..std::mem::zeroed()
            };
            RegisterClassExW(&wc);
            CreateWindowExW(0, klasse.as_ptr(), klasse.as_ptr(), 0, 0, 0, 0, 0, HWND_MESSAGE, std::ptr::null_mut(), instanz, std::ptr::null())
        };
        if hwnd.is_null() {
            let _ = bereit.send(Err("Nachrichtenfenster nicht erstellt".into()));
            return;
        }
        // Maus (Usage 2) und Tastatur (Usage 6) auf der Generic-Desktop-Seite. INPUTSINK: auch
        // ohne Vordergrund empfangen, wie evdev -- die App wertet nur mit Fokus aus und leert die
        // Kanaele sonst (s. about_to_wait).
        let geraete = [2u16, 6].map(|usage| RAWINPUTDEVICE { usUsagePage: 1, usUsage: usage, dwFlags: RIDEV_INPUTSINK, hwndTarget: hwnd });
        // SAFETY: gueltiges Array, Groesse passt.
        let ok = unsafe { RegisterRawInputDevices(geraete.as_ptr(), geraete.len() as u32, std::mem::size_of::<RAWINPUTDEVICE>() as u32) } != 0;
        if !ok {
            let _ = bereit.send(Err(format!("RegisterRawInputDevices: {}", std::io::Error::last_os_error())));
            return;
        }
        let _ = bereit.send(Ok(()));
        // SAFETY: Standard-Nachrichtenschleife dieses Fadens.
        unsafe {
            let mut msg: MSG = std::mem::zeroed();
            while GetMessageW(&mut msg, std::ptr::null_mut(), 0, 0) > 0 {
                DispatchMessageW(&msg);
            }
        }
    }

    pub fn open() -> (Option<RawMouse>, Option<RawKeyboard>, Option<super::DeviceWatcher>) {
        let (mouse_tx, mouse_rx) = mpsc::channel();
        let (key_tx, key_rx) = mpsc::channel();
        let (bereit_tx, bereit_rx) = mpsc::channel();
        let gestartet = std::thread::Builder::new().name("raw-input".into()).spawn(move || lesefaden(mouse_tx, key_tx, bereit_tx));
        match gestartet.ok().and_then(|_| bereit_rx.recv().ok()) {
            Some(Ok(())) => {
                println!("[latency] Raw-Input-Bypass aktiv (Windows WM_INPUT in eigenem Faden): Maus und Tastatur");
                (Some(RawMouse { rx: mouse_rx }), Some(RawKeyboard { rx: key_rx }), None)
            }
            Some(Err(e)) => {
                println!("[latency] Raw-Input-Bypass aus ({e}), Fallback auf winit");
                (None, None, None)
            }
            None => {
                println!("[latency] Raw-Input-Bypass aus (Faden nicht gestartet), Fallback auf winit");
                (None, None, None)
            }
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn scancodes_treffen_die_richtigen_tasten() {
            let taste = |c, e0| scancode_to_evdev(c, e0).and_then(crate::raw_input::evdev_key_to_keycode);
            assert_eq!(taste(0x11, false), Some(KeyCode::KeyW));
            assert_eq!(taste(0x1E, false), Some(KeyCode::KeyA));
            assert_eq!(taste(0x39, false), Some(KeyCode::Space));
            assert_eq!(taste(0x2A, false), Some(KeyCode::ShiftLeft));
            assert_eq!(taste(0x1D, false), Some(KeyCode::ControlLeft));
            assert_eq!(taste(0x1D, true), Some(KeyCode::ControlRight));
            assert_eq!(taste(0x48, true), Some(KeyCode::ArrowUp));
            assert_eq!(taste(0x48, false), Some(KeyCode::Numpad8));
            assert_eq!(taste(0x57, false), Some(KeyCode::F11));
            // Unechtes Umschalt vor E0-Tasten zaehlt nicht.
            assert_eq!(taste(0x2A, true), None);
        }
    }
}

#[cfg(windows)]
pub use windows::{open, RawKeyboard, RawMouse};

#[cfg(not(target_os = "linux"))]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum MouseButton {
    Left,
    Right,
    Middle,
    Side,
    Extra,
}

#[cfg(not(any(target_os = "linux", windows)))]
pub struct RawMouse;

// Ohne evdev bzw. Raw Input gibt es keinen Bypass und keinen Waechter.
#[cfg(not(any(target_os = "linux", windows)))]
pub fn open() -> (Option<RawMouse>, Option<RawKeyboard>, Option<DeviceWatcher>) {
    (None, None, None)
}

#[cfg(not(target_os = "linux"))]
pub struct DeviceWatcher;

#[cfg(not(target_os = "linux"))]
impl DeviceWatcher {
    pub fn drain(&self) -> Vec<DeviceAdded> {
        Vec::new()
    }
}

#[cfg(not(any(target_os = "linux", windows)))]
impl RawMouse {
    pub fn drain(&self) -> Vec<RawMouseEvent> {
        Vec::new()
    }
}

#[cfg(not(target_os = "linux"))]
#[derive(Clone, Copy, Debug)]
pub enum RawMouseEvent {
    Motion(f64, f64),
    Button(MouseButton, bool),
    Wheel(f32),
}

#[cfg(not(any(target_os = "linux", windows)))]
pub struct RawKeyboard;

#[cfg(not(any(target_os = "linux", windows)))]
impl RawKeyboard {
    pub fn drain(&self) -> Vec<(winit::keyboard::KeyCode, bool)> {
        Vec::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ep(name: &str, phys: &str, keyboard: bool, pointer: bool) -> Endpoint {
        Endpoint { name: name.into(), phys: phys.into(), keyboard, pointer }
    }

    #[test]
    fn endpunkte_eines_geraets_werden_eine_meldung() {
        let g = group_endpoints(vec![
            ep("Logitech G915 Keyboard", "usb-0000:00:14.0-2/input0", true, false),
            ep("Logitech G915 Consumer Control", "usb-0000:00:14.0-2/input1", false, false),
            ep("Logitech G915", "usb-0000:00:14.0-2/input2", false, true),
            ep("Sharkoon Maus", "usb-0000:00:14.0-3/input0", false, true),
        ]);
        assert_eq!(g.len(), 2);
        assert_eq!(g[0].name, "Logitech G915");
        assert_eq!(g[0].kind_label(), "Tastatur und Maus");
        assert_eq!(g[1].name, "Sharkoon Maus");
        assert_eq!(g[1].kind_label(), "Maus");
    }

    #[test]
    fn uninteressante_endpunkte_melden_nichts() {
        assert!(group_endpoints(vec![ep("Power Button", "LNXPWRBN/button/input0", false, false)]).is_empty());
    }

    #[test]
    fn ohne_anschluss_zaehlt_der_name() {
        let g = group_endpoints(vec![ep("A", "", true, false), ep("B", "", true, false)]);
        assert_eq!(g.len(), 2);
    }
}
