// Beispiel-Addon fuer Coral Client: benutzt jeden Hook, jede Art von Einstellung und jede
// Abfrage bzw. Aktion einmal. Zum Anfangen kopieren und wegwerfen, was man nicht braucht.
//
// Bauen mit `cargo build --release` (das Ziel wasm32-unknown-unknown steht in
// .cargo/config.toml), dann target/wasm32-unknown-unknown/release/beispiel_addon.wasm nach
// ~/coral-client/mods kopieren und den Client neu starten. Im Modul-Menue erscheint die
// Kategorie "Beispiel" mit drei Modulen.
//
// Wie es laeuft: der Client ruft on_event an den angemeldeten Hooks auf. Was on_event
// zurueckgibt, entscheidet ueber das Ereignis (Flow::Cancel verwirft eine Chatzeile, ein Paket,
// eine Taste ...). Waehrenddessen fragt das Addon mit client::* den Spielzustand ab und loest
// Aktionen aus; die Aktionen fuehrt der Client direkt nach dem Hook aus.

use coral_addon::{client, export, Addon, Anchor, Event, Flow, Hook, HudItem, Input, Module, Registration, Setting, Target, Value};

#[derive(Default)]
struct Beispiel {
    // Welche Module an sind.
    info: bool,
    chat: bool,
    steuerung: bool,
    // Einstellungen.
    ziel_zeigen: bool,
    abstand: f64,
    ecke: String,
    farbe: u32,
    werbung_weg: bool,
    // Zaehler fuer das HUD: Pakete je Richtung im laufenden und letzten Zeitfenster.
    pakete_rein: u32,
    pakete_raus: u32,
    rate: (u32, u32),
    sekunde: f32,
    bilder: u32,
    fps: u32,
    // Steuerung: automatisch laufen (Taste R).
    autolauf: bool,
}

impl Addon for Beispiel {
    fn register(&mut self) -> Registration {
        Registration {
            name: "Beispiel".into(),
            hooks: vec![Hook::Tick, Hook::Frame, Hook::Join, Hook::Leave, Hook::ChatIn, Hook::ChatOut, Hook::PacketIn, Hook::PacketOut, Hook::Key, Hook::Mouse, Hook::Scroll, Hook::Death],
            modules: vec![
                Module {
                    id: "info".into(),
                    name: "Info-HUD".into(),
                    description: "Position, Blick, Ziel, Pakete und FPS im HUD".into(),
                    settings: vec![
                        Setting::toggle("ziel", "Ziel anzeigen", true),
                        Setting::slider("abstand", "Abstand zum Rand", 0.0, 50.0, 1.0, "px", 4.0),
                        Setting::choice("ecke", "Ecke", &["oben links", "oben rechts", "unten links", "unten rechts"], "oben links"),
                        Setting::color("farbe", "Textfarbe", "#7FE3FF"),
                        Setting::button("test", "Test-Benachrichtigung"),
                    ],
                },
                Module {
                    id: "chat".into(),
                    name: "Chat-Werkzeuge".into(),
                    description: ":shrug: wird zu ¯\\_(ツ)_/¯, Ton bei Erwaehnung, /beispiel".into(),
                    settings: vec![Setting::toggle("werbung", "Discord-Links ausblenden", false)],
                },
                Module {
                    id: "steuerung".into(),
                    name: "Steuerung".into(),
                    description: "R: automatisch laufen, mittlere Maustaste: Totem waehlen".into(),
                    settings: vec![Setting::button("norden", "Nach Norden schauen")],
                },
            ],
            commands: vec!["beispiel".into()],
        }
    }

    fn on_event(&mut self, event: Event) -> Flow {
        match event {
            // --- Stand der Module und Einstellungen ---
            Event::ModuleEnabled { module, on } => {
                match module.as_str() {
                    "info" => self.info = on,
                    "chat" => self.chat = on,
                    "steuerung" => {
                        self.steuerung = on;
                        if !on {
                            self.autolauf = false;
                            client::set_input(Input::default());
                        }
                    }
                    _ => {}
                }
                Flow::Continue
            }
            Event::Setting { key, value, .. } => {
                match key.as_str() {
                    "ziel" => self.ziel_zeigen = value.as_bool(),
                    "abstand" => self.abstand = value.as_f64().unwrap_or(4.0),
                    "ecke" => self.ecke = value.as_str().to_string(),
                    "farbe" => self.farbe = u32::from_str_radix(value.as_str().trim_start_matches('#'), 16).unwrap_or(0x7FE3FF),
                    "werbung" => self.werbung_weg = value.as_bool(),
                    _ => {}
                }
                Flow::Continue
            }
            Event::Button { key, .. } => {
                match key.as_str() {
                    "test" => client::notify("Beispiel", "Der Knopf geht!"),
                    "norden" => client::set_rotation(180.0, client::player().pitch),
                    _ => {}
                }
                Flow::Continue
            }

            // --- Zeit ---
            Event::Tick => {
                self.zeichnen();
                Flow::Continue
            }
            Event::Frame { dt } => {
                self.bilder += 1;
                self.sekunde += dt;
                if self.sekunde >= 1.0 {
                    self.fps = self.bilder;
                    self.rate = (self.pakete_rein, self.pakete_raus);
                    (self.bilder, self.pakete_rein, self.pakete_raus, self.sekunde) = (0, 0, 0, 0.0);
                }
                Flow::Continue
            }

            // --- Verbindung ---
            Event::Join { server } => {
                client::log(&format!("verbunden mit {server}"));
                Flow::Continue
            }
            Event::Leave { reason } => {
                client::log(&format!("getrennt: {reason}"));
                self.autolauf = false;
                client::set_input(Input::default());
                Flow::Continue
            }
            Event::Death => {
                let p = client::player();
                client::print(&format!("Gestorben bei {:.0} {:.0} {:.0}", p.x, p.y, p.z));
                Flow::Continue
            }

            // --- Chat ---
            Event::ChatIn { text } if self.chat => {
                if self.werbung_weg && text.contains("discord.gg/") {
                    return Flow::Cancel;
                }
                let name = client::player().name;
                if !name.is_empty() && text.contains(&name) && !text.starts_with(&format!("<{name}>")) {
                    client::sound("block.note_block.pling", 1.0, 1.5);
                }
                Flow::Continue
            }
            Event::ChatOut { text } if self.chat && text.contains(":shrug:") => Flow::ReplaceText(text.replace(":shrug:", "¯\\_(ツ)_/¯")),
            Event::Command { args, .. } => {
                self.befehl(args.trim());
                Flow::Continue
            }

            // --- Pakete (nur gezaehlt; Flow::Cancel bzw. ReplaceBytes wuerde eingreifen) ---
            Event::PacketIn { .. } => {
                self.pakete_rein += 1;
                Flow::Continue
            }
            Event::PacketOut { .. } => {
                self.pakete_raus += 1;
                Flow::Continue
            }

            // --- Eingabe ---
            Event::Key { key, pressed: true } if self.steuerung && key == "KeyR" => {
                self.autolauf = !self.autolauf;
                client::set_input(Input { forward: self.autolauf, ..Input::default() });
                // Geschluckt: das Spiel selbst sieht die Taste nicht.
                Flow::Cancel
            }
            Event::Mouse { button: 2, pressed: true } if self.steuerung => {
                let inventar = client::inventory();
                // Hotbar sind die Slots 36 bis 44.
                let totem = (0..9u8).find(|&i| inventar.get(36 + i as usize).and_then(|s| s.as_ref()).is_some_and(|s| s.id == "minecraft:totem_of_undying"));
                match totem {
                    Some(slot) => {
                        client::select_slot(slot);
                        Flow::Cancel
                    }
                    None => Flow::Continue,
                }
            }
            _ => Flow::Continue,
        }
    }
}

impl Beispiel {
    fn zeichnen(&self) {
        if !self.info {
            client::hud("info", vec![]);
            return;
        }
        let p = client::player();
        if !p.in_world {
            client::hud("info", vec![]);
            return;
        }
        let anker = match self.ecke.as_str() {
            "oben rechts" => Anchor::TopRight,
            "unten links" => Anchor::BottomLeft,
            "unten rechts" => Anchor::BottomRight,
            _ => Anchor::TopLeft,
        };
        let richtung = ["Sueden", "Westen", "Norden", "Osten"][((p.yaw.rem_euclid(360.0) + 45.0) / 90.0) as usize % 4];
        let mut zeilen = vec![
            format!("{:.1} {:.1} {:.1}", p.x, p.y, p.z),
            format!("Blick {richtung} ({:.0}°)", p.yaw.rem_euclid(360.0)),
            format!("{} FPS, Pakete {} rein / {} raus je s", self.fps, self.rate.0, self.rate.1),
        ];
        if self.ziel_zeigen {
            zeilen.push(match client::target() {
                Target::Block { block, .. } => format!("Ziel: {block}"),
                Target::Entity { id } => {
                    let name = client::entities(8.0).into_iter().find(|e| e.id == id).map(|e| if e.name.is_empty() { e.kind } else { e.name });
                    format!("Ziel: {}", name.unwrap_or_default())
                }
                Target::Miss => "Ziel: -".into(),
            });
        }
        if let Some(naechster) = client::entities(64.0).into_iter().find(|e| e.kind == "minecraft:player") {
            zeilen.push(format!("Naechster Spieler: {} ({:.1} m)", naechster.name, naechster.distance));
        }
        if self.autolauf {
            zeilen.push("Autolauf an (R)".into());
        }
        let a = self.abstand as f32;
        let mut items = vec![HudItem::Rect { x: a - 2.0, y: a - 2.0, w: 190.0, h: zeilen.len() as f32 * 10.0 + 4.0, color: 0x000000, alpha: 0.4, anchor: anker }];
        for (i, z) in zeilen.into_iter().enumerate() {
            items.push(HudItem::Text { text: z, x: a, y: a + i as f32 * 10.0, color: self.farbe, anchor: anker });
        }
        // Was gerade in der Hand liegt, als Symbol neben dem Kasten.
        if let Some(Some(item)) = client::inventory().get(36 + p.selected_slot as usize) {
            items.push(HudItem::Item { id: item.id.clone(), count: item.count, x: a + 194.0, y: a, anchor: anker });
        }
        client::hud("info", items);
    }

    // /beispiel <pos|spieler|inv|block|einstellung>
    fn befehl(&mut self, args: &str) {
        match args {
            "pos" => {
                let p = client::player();
                client::print(&format!("{:.2} {:.2} {:.2} in {}", p.x, p.y, p.z, p.dimension));
            }
            "spieler" => {
                let namen: Vec<String> = client::players().into_iter().map(|p| format!("{} ({} ms)", p.name, p.ping)).collect();
                client::print(&format!("{} Spieler: {}", namen.len(), namen.join(", ")));
            }
            "inv" => {
                let belegt = client::inventory().iter().flatten().count();
                client::print(&format!("{belegt} von 46 Slots belegt"));
            }
            "block" => {
                let p = client::player();
                let unten = client::block(p.x.floor() as i32, p.y.floor() as i32 - 1, p.z.floor() as i32);
                client::print(&format!("Unter dir: {}", unten.unwrap_or_else(|| "nicht geladen".into())));
            }
            "einstellung" => {
                let farbe = client::setting("info", "farbe");
                client::print(&format!("Textfarbe: {}", farbe.as_str()));
                // Eigene Einstellungen lassen sich auch setzen (ohne Echo als Event::Setting).
                self.ziel_zeigen = !self.ziel_zeigen;
                client::set_setting("info", "ziel", Value::Bool(self.ziel_zeigen));
            }
            _ => client::print("/beispiel pos | spieler | inv | block | einstellung"),
        }
    }
}

export!(Beispiel);
