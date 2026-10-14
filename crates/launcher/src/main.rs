// coral-client-launcher: kleines Fenster vor dem eigentlichen Client.
//
// - Installation waehlen: offizieller Launcher, Prism-Instanzen oder ein eigener Ordner. Die
//   Auswahl landet in ~/.config/coral-client/installation.conf und wird vom Client beim Start
//   gelesen (s. assets/src/source.rs) -- der Client laeuft also auch ohne Launcher mit der
//   zuletzt gewaehlten Installation.
// - Updates: sucht auf GitHub nach einem neueren Release und laedt das Client-Binary und den
//   Launcher selbst (s. update.rs); danach laesst sich der Launcher neu starten.
// - Config: welche Einstellungs-Datei der Client benutzt (anlegen, kopieren, umbenennen,
//   loeschen; s. assets/src/configs.rs). Bearbeitet werden die Einstellungen im Client selbst.
// - Spieldaten: extrahiert, was der Client aus dem Jar braucht (Entity-Modelle, Kreativ-Tabs),
//   einmal in einen festen Ordner (s. assets::extract_game_data) -- beim ersten Start von
//   selbst, nach einem Update der Version durch Mojang auf Knopfdruck.
// - Spielen: startet das Client-Binary, das neben dem Launcher liegt, mit `--config <Name>`.

// Unter Windows kein zusaetzliches Konsolenfenster hinter dem Launcher.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

#[cfg(target_os = "linux")]
mod desktop_entry;
mod update;

use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::mpsc::{channel, Receiver};
use std::sync::{Arc, Mutex, OnceLock};

use assets::{Candidate, GameSource};
use eframe::egui;

use update::{Progress, Release};

// Muss zur MC_VERSION im Client passen (crates/app/src/main.rs).
const MC_VERSION: &str = "1.21.11";

// Pfad des laufenden Launchers, beim Start festgehalten: nach einem Selbst-Update zeigt
// current_exe() unter Linux auf die ersetzte, geloeschte Datei ("... (deleted)").
static LAUNCHER_EXE: OnceLock<Option<PathBuf>> = OnceLock::new();

fn launcher_exe() -> Option<PathBuf> {
    LAUNCHER_EXE.get_or_init(|| std::env::current_exe().ok()).clone()
}

fn main() -> eframe::Result {
    assets::migrate_legacy_dirs();
    if let Some(exe) = launcher_exe() {
        update::cleanup_old_files(&exe);
        #[cfg(target_os = "linux")]
        desktop_entry::install(&exe);
    }
    update::cleanup_old_files(&client_path());
    move_client_from_launcher_dir();
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default().with_title("Coral Client Launcher").with_app_id("coral-client-launcher").with_inner_size([560.0, 620.0]).with_min_inner_size([420.0, 380.0]),
        ..Default::default()
    };
    eframe::run_native("Coral Client Launcher", options, Box::new(|cc| Ok(Box::new(Launcher::new(&cc.egui_ctx)))))
}

enum UpdateState {
    Checking(Receiver<Result<(Vec<Release>, ClientCheck), String>>),
    // Kein neueres Release -- das aktuelle bleibt fuer "Client herunterladen" (s. ClientCheck).
    UpToDate(Release),
    NoRelease,
    Available(Release),
    // Ergebnis: ob der Launcher selbst mit ersetzt wurde.
    Downloading { tag: String, progress: Arc<Mutex<Progress>>, result: Receiver<Result<bool, String>> },
    Installed { tag: String, launcher_updated: bool },
    Failed(String),
}

struct Launcher {
    candidates: Vec<Candidate>,
    selected: Option<GameSource>,
    custom_path: String,
    conf: LauncherConf,
    client_check: ClientCheck,
    update: UpdateState,
    // Alle Releases vom letzten Update-Check, neueste zuerst (stabile und Betas, s.
    // update::visible) -- fuer die Versionsauswahl.
    releases: Vec<Release>,
    // In der Versionsauswahl gewaehlter Tag; None = die installierte Version.
    chosen_tag: Option<String>,
    client: Option<Child>,
    status: Option<String>,
    configs: Vec<String>,
    config: String,
    config_edit: Option<ConfigEdit>,
    game_data: GameData,
}

/// Stand der extrahierten Spieldaten (s. game_data_section).
enum GameData {
    Status(assets::GameDataStatus),
    // Laufende Extraktion: der aktuelle Schritt und das Ergebnis.
    Extracting { step: Arc<Mutex<String>>, result: Receiver<Result<(), String>> },
    Failed(String),
}

/// Passt der Client neben dem Launcher zum neuesten Release? Beim Update-Check im Hintergrund
/// per SHA-256 geprueft (133 MB, ein Bruchteil einer Sekunde).
#[derive(Clone, Copy, PartialEq)]
enum ClientCheck {
    // Nichts zu vergleichen (kein Release, kein Client-Asset).
    Unknown,
    Missing,
    Matches,
    // Anders als im Release: beschaedigt, veraltet oder selbst gebaut.
    Differs,
    // GitHub nennt keine Pruefsumme (alte Releases).
    NoDigest,
}

fn check_client(release: Option<&Release>) -> ClientCheck {
    let Some(asset) = release.and_then(|r| r.asset_for_platform()) else { return ClientCheck::Unknown };
    let path = client_path();
    if !path.is_file() {
        return ClientCheck::Missing;
    }
    match (asset.sha256(), update::sha256_file(&path)) {
        (None, _) => ClientCheck::NoDigest,
        (Some(expected), Ok(actual)) if actual == expected => ClientCheck::Matches,
        _ => ClientCheck::Differs,
    }
}

/// Laufende Namenseingabe im Config-Bereich.
struct ConfigEdit {
    kind: ConfigEditKind,
    name: String,
    error: Option<String>,
    // Beim Oeffnen einmal ins Feld springen, danach nicht mehr erzwingen.
    focus: bool,
}

#[derive(Clone, Copy, PartialEq)]
enum ConfigEditKind {
    New,
    Duplicate,
    Rename,
}

impl Launcher {
    fn new(ctx: &egui::Context) -> Self {
        let mut launcher = Self {
            candidates: Vec::new(),
            selected: assets::load_selection(),
            custom_path: String::new(),
            conf: LauncherConf::load(),
            update: UpdateState::NoRelease,
            releases: Vec::new(),
            chosen_tag: None,
            client_check: ClientCheck::Unknown,
            client: None,
            status: None,
            configs: Vec::new(),
            config: assets::startup_config(None),
            config_edit: None,
            game_data: GameData::Status(assets::game_data_status(MC_VERSION)),
        };
        launcher.reload_configs();
        launcher.rescan();
        launcher.check_for_update(ctx);
        launcher.auto_extract(ctx);
        launcher
    }

    fn rescan(&mut self) {
        self.candidates = assets::discover(MC_VERSION);
        // Eine frueher gewaehlte Quelle, die die Suche nicht findet (eigener Ordner), trotzdem
        // anzeigen -- sonst waere sie nach dem Neustart des Launchers unsichtbar gewaehlt.
        if let Some(selected) = &self.selected {
            if !self.candidates.iter().any(|c| &c.source == selected) {
                self.candidates.push(custom_candidate(selected.clone()));
            }
        }
        // Noch nichts gewaehlt: die erste brauchbare Quelle vorschlagen (gespeichert wird erst
        // beim Klick oder beim Spielen).
        if self.selected.is_none() {
            self.selected = self.candidates.iter().find(|c| c.files.is_ok()).map(|c| c.source.clone());
        }
    }

    fn select(&mut self, source: GameSource) {
        if let Err(e) = assets::save_selection(&source) {
            self.status = Some(format!("Auswahl nicht gespeichert: {e}"));
        }
        self.selected = Some(source);
        if !matches!(self.game_data, GameData::Extracting { .. }) {
            self.game_data = GameData::Status(assets::game_data_status(MC_VERSION));
        }
    }

    /// Fehlen die Spieldaten oder passen sie nicht zu diesem Client, gleich extrahieren --
    /// ohne sie fehlen dem Client Modelle und das Kreativinventar.
    fn auto_extract(&mut self, ctx: &egui::Context) {
        let noetig = matches!(self.game_data, GameData::Status(assets::GameDataStatus::Missing | assets::GameDataStatus::DumperChanged));
        if noetig && self.selected_candidate().is_some_and(|c| c.files.is_ok()) {
            self.start_extract(ctx);
        }
    }

    fn start_extract(&mut self, ctx: &egui::Context) {
        // Extrahiert wird aus der gewaehlten Installation -- die muss dafuer gespeichert sein.
        if let Some(source) = self.selected.clone() {
            self.select(source);
        }
        let step = Arc::new(Mutex::new(String::new()));
        let (tx, rx) = channel();
        let (thread_step, ctx) = (step.clone(), ctx.clone());
        std::thread::spawn(move || {
            let melden = |s: &str| {
                if let Ok(mut st) = thread_step.lock() {
                    *st = s.to_string();
                }
                ctx.request_repaint();
            };
            let result = assets::extract_game_data(MC_VERSION, &melden).map_err(|e| e.to_string());
            let _ = tx.send(result);
            ctx.request_repaint();
        });
        self.game_data = GameData::Extracting { step, result: rx };
    }

    fn game_data_section(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        use assets::GameDataStatus as S;
        ui.heading("Spieldaten");
        let gruen = egui::Color32::from_rgb(90, 180, 90);
        let orange = egui::Color32::from_rgb(210, 150, 60);
        let rot = egui::Color32::from_rgb(220, 90, 90);
        let installation_ok = self.selected_candidate().is_some_and(|c| c.files.is_ok());
        let mut starten = false;
        let mut knopf = |ui: &mut egui::Ui, text: &str| {
            if ui.add_enabled(installation_ok, egui::Button::new(text)).on_disabled_hover_text("Erst eine nutzbare Installation waehlen.").clicked() {
                starten = true;
            }
        };
        match &self.game_data {
            GameData::Extracting { step, .. } => {
                ui.horizontal(|ui| {
                    ui.spinner();
                    let schritt = step.lock().map(|s| s.clone()).unwrap_or_default();
                    ui.label(format!("Extrahiere aus Minecraft {MC_VERSION} ... {schritt}"));
                });
            }
            GameData::Failed(e) => {
                ui.colored_label(rot, format!("Extrahieren fehlgeschlagen: {e}"));
                knopf(ui, "Erneut versuchen");
            }
            GameData::Status(status) => {
                ui.horizontal(|ui| {
                    match status {
                        S::Current => ui.colored_label(gruen, "aktuell"),
                        S::Missing => ui.colored_label(orange, "fehlen -- ohne sie fehlen Entity-Modelle und das Kreativinventar"),
                        S::JarChanged => ui.colored_label(orange, "Minecraft wurde aktualisiert oder eine andere Installation gewaehlt -- neu extrahieren"),
                        S::DumperChanged => ui.colored_label(orange, "passen nicht zu diesem Client -- neu extrahieren"),
                    };
                    knopf(ui, if *status == S::Missing { "Extrahieren" } else { "Neu extrahieren" });
                });
            }
        }
        if let Some(dir) = assets::game_data_dir(MC_VERSION) {
            ui.weak(format!("Ordner: {}", dir.display()));
        }
        if starten {
            self.start_extract(ctx);
        }
    }

    fn selected_candidate(&self) -> Option<&Candidate> {
        let selected = self.selected.as_ref()?;
        self.candidates.iter().find(|c| &c.source == selected)
    }

    fn check_for_update(&mut self, ctx: &egui::Context) {
        let (tx, rx) = channel();
        let ctx = ctx.clone();
        let (installed, beta) = (self.conf.installed.clone(), self.conf.beta);
        std::thread::spawn(move || {
            let result = update::fetch_releases().map(|releases| {
                // Geprueft wird der Client gegen das Release, das installiert sein SOLLTE --
                // sonst gegen das neueste im Kanal.
                let bezug = releases.iter().find(|r| r.tag_name == installed).or_else(|| update::newest(&releases, beta));
                let check = check_client(bezug);
                (releases, check)
            });
            let _ = tx.send(result);
            ctx.request_repaint();
        });
        self.update = UpdateState::Checking(rx);
    }

    // `with_launcher`: auch den Launcher ersetzen (beim echten Update, sofern eingeschaltet --
    // nicht beim blossen Nachladen des Clients).
    fn start_download(&mut self, ctx: &egui::Context, release: Release, with_launcher: bool) {
        let Some(asset) = release.asset_for_platform().cloned() else { return };
        // Der Launcher selbst, falls gewuenscht, das Release ihn hat und wir wissen, wo wir liegen.
        let launcher = release.launcher_asset().cloned().zip(launcher_exe()).filter(|_| with_launcher && self.conf.self_update);
        let total = asset.size + launcher.as_ref().map_or(0, |(a, _)| a.size);
        let progress = Arc::new(Mutex::new(Progress { done: 0, total }));
        let (tx, rx) = channel();
        let (target, thread_progress, ctx) = (client_path(), progress.clone(), ctx.clone());
        std::thread::spawn(move || {
            let result = update::download(&asset, &target, thread_progress.clone()).and_then(|()| match &launcher {
                Some((launcher_asset, exe)) => update::download(launcher_asset, exe, thread_progress).map(|()| true),
                None => Ok(false),
            });
            let _ = tx.send(result);
            ctx.request_repaint();
        });
        self.update = UpdateState::Downloading { tag: release.tag_name, progress, result: rx };
    }

    /// Die neue Fassung des Launchers starten und diese beenden. Ein laufender Client laeuft
    /// weiter -- er ist ein eigener Prozess.
    fn restart(&mut self, ctx: &egui::Context) {
        let Some(exe) = launcher_exe() else { return };
        match Command::new(&exe).args(std::env::args_os().skip(1)).spawn() {
            Ok(_) => ctx.send_viewport_cmd(egui::ViewportCommand::Close),
            Err(e) => self.status = Some(format!("Neustart fehlgeschlagen: {e} -- bitte {} von Hand starten.", exe.display())),
        }
    }

    /// Aus der Release-Liste und dem Kanal (stabil/Beta) den Update-Zustand bestimmen. Auch
    /// beim Umschalten des Beta-Schalters -- dafuer muss GitHub nicht erneut gefragt werden.
    fn evaluate_releases(&mut self) {
        self.update = match update::newest(&self.releases, self.conf.beta) {
            None => UpdateState::NoRelease,
            Some(neueste) if update::is_newer(&neueste.tag_name, &self.conf.installed) => UpdateState::Available(neueste.clone()),
            Some(neueste) => {
                let installiert = self.releases.iter().find(|r| r.tag_name == self.conf.installed);
                UpdateState::UpToDate(installiert.unwrap_or(neueste).clone())
            }
        };
        // Eine gewaehlte Beta verschwindet mit dem Ausschalten aus der Liste.
        if let Some(tag) = &self.chosen_tag {
            if !update::visible(&self.releases, self.conf.beta).any(|r| &r.tag_name == tag) {
                self.chosen_tag = None;
            }
        }
    }

    /// Ergebnisse der Hintergrund-Threads und den Zustand des gestarteten Clients abholen.
    fn poll(&mut self) {
        match &self.update {
            UpdateState::Checking(rx) => {
                if let Ok(result) = rx.try_recv() {
                    match result {
                        Ok((releases, check)) => {
                            self.client_check = check;
                            self.releases = releases;
                            self.evaluate_releases();
                        }
                        Err(e) => self.update = UpdateState::Failed(e),
                    }
                }
            }
            UpdateState::Downloading { tag, result, .. } => {
                if let Ok(result) = result.try_recv() {
                    self.update = match result {
                        Ok(launcher_updated) => {
                            self.conf.installed = tag.clone();
                            self.conf.save();
                            self.chosen_tag = None;
                            self.client_check = ClientCheck::Matches;
                            UpdateState::Installed { tag: tag.clone(), launcher_updated }
                        }
                        Err(e) => UpdateState::Failed(e),
                    };
                }
            }
            _ => {}
        }

        if let GameData::Extracting { result, .. } = &self.game_data {
            if let Ok(result) = result.try_recv() {
                self.game_data = match result {
                    Ok(()) => GameData::Status(assets::game_data_status(MC_VERSION)),
                    Err(e) => GameData::Failed(e),
                };
            }
        }

        if let Some(child) = &mut self.client {
            if let Ok(Some(exit)) = child.try_wait() {
                self.client = None;
                self.status = (!exit.success()).then(|| format!("Client mit Fehler beendet ({exit}). Log: {}", log_path().display()));
            }
        }
    }

    fn reload_configs(&mut self) {
        self.configs = assets::list_configs();
        if !self.configs.contains(&self.config) {
            self.config = self.configs[0].clone();
        }
    }

    fn select_config(&mut self, name: String) {
        assets::set_last_used(&name);
        self.config = name;
    }

    fn apply_config_edit(&mut self) {
        let Some(edit) = &mut self.config_edit else { return };
        let result = match edit.kind {
            ConfigEditKind::New => assets::create_config(&edit.name, None),
            ConfigEditKind::Duplicate => assets::create_config(&edit.name, Some(&self.config)),
            ConfigEditKind::Rename => assets::rename_config(&self.config, &edit.name),
        };
        match result {
            Ok(name) => {
                self.config_edit = None;
                self.select_config(name);
                self.reload_configs();
            }
            Err(e) => edit.error = Some(e.to_string()),
        }
    }

    fn config_section(&mut self, ui: &mut egui::Ui) {
        ui.heading("Config");
        let editing = self.config_edit.is_some();
        let running = self.client.is_some();

        let mut chosen = None;
        ui.horizontal(|ui| {
            ui.add_enabled_ui(!editing, |ui| {
                egui::ComboBox::from_id_salt("config").selected_text(&self.config).width(200.0).show_ui(ui, |ui| {
                    for name in &self.configs {
                        if ui.selectable_label(*name == self.config, name).clicked() {
                            chosen = Some(name.clone());
                        }
                    }
                });
                let start = |kind, name: String| Some(ConfigEdit { kind, name, error: None, focus: true });
                let mut edit = None;
                if ui.button("Neu").on_hover_text("Leere Config mit Standardwerten").clicked() {
                    edit = start(ConfigEditKind::New, String::new());
                }
                if ui.button("Kopieren").on_hover_text("Neue Config als Kopie der gewaehlten").clicked() {
                    edit = start(ConfigEditKind::Duplicate, format!("{} Kopie", self.config));
                }
                // Umbenennen/Loeschen nicht, waehrend der Client mit ihr laeuft -- er wuerde beim
                // naechsten Speichern die alte Datei wieder anlegen.
                if ui.add_enabled(!running, egui::Button::new("Umbenennen")).clicked() {
                    edit = start(ConfigEditKind::Rename, self.config.clone());
                }
                let deletable = !running && self.configs.len() > 1;
                if ui.add_enabled(deletable, egui::Button::new("Loeschen")).on_disabled_hover_text("Die letzte Config bleibt stehen.").clicked() {
                    match assets::delete_config(&self.config) {
                        Ok(()) => self.reload_configs(),
                        Err(e) => self.status = Some(format!("Loeschen fehlgeschlagen: {e}")),
                    }
                    assets::set_last_used(&self.config);
                }
                if edit.is_some() {
                    self.config_edit = edit;
                }
            });
        });
        if let Some(name) = chosen {
            self.select_config(name);
        }

        let mut apply = false;
        let mut cancel = false;
        if let Some(edit) = &mut self.config_edit {
            ui.horizontal(|ui| {
                ui.label(match edit.kind {
                    ConfigEditKind::New => "Name der neuen Config:",
                    ConfigEditKind::Duplicate => "Name der Kopie:",
                    ConfigEditKind::Rename => "Neuer Name:",
                });
                let field = ui.add(egui::TextEdit::singleline(&mut edit.name).desired_width(180.0));
                if std::mem::take(&mut edit.focus) {
                    field.request_focus();
                }
                if field.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                    apply = true;
                }
                if ui.button("OK").clicked() {
                    apply = true;
                }
                if ui.button("Abbrechen").clicked() || ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                    cancel = true;
                }
            });
            if let Some(error) = &edit.error {
                ui.colored_label(egui::Color32::from_rgb(220, 90, 90), error);
            }
        }
        if cancel {
            self.config_edit = None;
        } else if apply {
            self.apply_config_edit();
        }
        ui.weak("Einstellungen werden im Client bearbeitet und in der gewaehlten Config gespeichert.");
    }

    // Ausklappbarer Reiter mit den Debug-Flags. Gilt ab dem naechsten Start des Clients.
    fn flags_section(&mut self, ui: &mut egui::Ui) {
        let aktiv = self.conf.flags.len();
        let titel = if aktiv == 0 { "Flags".to_string() } else { format!("Flags ({aktiv} an)") };
        egui::CollapsingHeader::new(titel).id_salt("flags").show(ui, |ui| {
            ui.weak(format!("Diagnose-Ausgaben landen in {}. Gilt ab dem naechsten Start.", log_path().display()));
            let mut geaendert = false;
            for (name, label, hilfe) in DEBUG_FLAGS {
                let mut an = self.conf.flags.iter().any(|f| f == name);
                if ui.checkbox(&mut an, *label).on_hover_text(format!("{hilfe}\n\n{name}")).changed() {
                    geaendert = true;
                    if an {
                        self.conf.flags.push(name.to_string());
                    } else {
                        self.conf.flags.retain(|f| f != name);
                    }
                }
            }
            if geaendert {
                self.conf.save();
            }
        });
    }

    fn play(&mut self) {
        let path = client_path();
        let log = std::fs::File::create(log_path());
        let mut command = Command::new(&path);
        command.arg("--config").arg(&self.config);
        // Eingeschaltete Debug-Flags als Umgebungsvariablen (s. DEBUG_FLAGS).
        for flag in &self.conf.flags {
            if flag == "CORAL_PKTLOG" {
                command.env(flag, log_path().with_file_name("pktlog.txt"));
            } else if flag == "CORAL_SOUNDLOG" {
                command.env(flag, log_path().with_file_name("soundlog.txt"));
            } else if DEBUG_FLAGS.iter().any(|(name, _, _)| name == flag) {
                command.env(flag, "1");
            }
        }
        assets::set_last_used(&self.config);
        if let Some(dir) = path.parent() {
            command.current_dir(dir);
        }
        match log {
            Ok(log) => {
                let stderr = log.try_clone().map(Stdio::from).unwrap_or(Stdio::null());
                command.stdout(log).stderr(stderr);
            }
            Err(_) => {
                command.stdout(Stdio::null()).stderr(Stdio::null());
            }
        }
        match command.spawn() {
            Ok(child) => {
                self.client = Some(child);
                self.status = None;
            }
            Err(e) => self.status = Some(format!("{} liess sich nicht starten: {e}", path.display())),
        }
    }

    fn installation_section(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.heading("Minecraft-Installation");
            if ui.small_button("Neu suchen").clicked() {
                self.rescan();
            }
        });
        ui.label(format!("Gebraucht wird Minecraft {MC_VERSION}. Die Version muss dort schon einmal gestartet worden sein."));
        ui.add_space(4.0);

        let mut clicked = None;
        egui::ScrollArea::vertical().max_height(240.0).show(ui, |ui| {
            if self.candidates.is_empty() {
                ui.label("Keine Installation gefunden.");
            }
            for candidate in &self.candidates {
                let is_selected = self.selected.as_ref() == Some(&candidate.source);
                ui.horizontal(|ui| {
                    let text = match &candidate.version {
                        Some(v) => format!("{}  ({v})", candidate.label),
                        None => candidate.label.clone(),
                    };
                    if ui.radio(is_selected, text).on_hover_text(candidate.source.describe()).clicked() {
                        clicked = Some(candidate.source.clone());
                    }
                    match &candidate.files {
                        Ok(_) => ui.colored_label(egui::Color32::from_rgb(90, 180, 90), "bereit"),
                        Err(reason) => ui.colored_label(egui::Color32::from_rgb(210, 150, 60), "nicht nutzbar").on_hover_text(reason),
                    };
                });
            }
        });
        if let Some(source) = clicked {
            self.select(source);
        }

        ui.add_space(4.0);
        ui.horizontal(|ui| {
            ui.label("Eigener Ordner:");
            ui.add(egui::TextEdit::singleline(&mut self.custom_path).hint_text("Pfad zu einem .minecraft-Ordner").desired_width(260.0));
            if ui.add_enabled(!self.custom_path.trim().is_empty(), egui::Button::new("Hinzufuegen")).clicked() {
                let source = GameSource::Official { dir: PathBuf::from(self.custom_path.trim()) };
                if !self.candidates.iter().any(|c| c.source == source) {
                    self.candidates.push(custom_candidate(source.clone()));
                }
                self.select(source);
                self.custom_path.clear();
            }
        });
    }

    fn update_section(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        ui.heading("Updates");
        ui.label(format!("Installiert: {}", self.conf.installed));
        let checkbox = ui
            .checkbox(&mut self.conf.self_update, "Launcher mit aktualisieren")
            .on_hover_text("Beim Herunterladen eines Updates auch den Launcher selbst ersetzen. Aus: nur der Client wird aktualisiert, den Launcher tauschst du bei Bedarf von Hand.");
        if checkbox.changed() {
            self.conf.save();
        }
        let beta = ui
            .checkbox(&mut self.conf.beta, "Beta-Versionen anzeigen")
            .on_hover_text("Auch Vorabversionen (auf GitHub als Pre-release markiert) anbieten. Sie sind neuer, aber weniger getestet.");
        if beta.changed() {
            self.conf.save();
            let busy = matches!(self.update, UpdateState::Checking(_) | UpdateState::Downloading { .. });
            if !busy && !self.releases.is_empty() {
                self.evaluate_releases();
            }
        }

        let mut action = None;
        let mut repair = None;
        let mut restart = false;
        ui.horizontal(|ui| match &self.update {
            UpdateState::Checking(_) => {
                ui.spinner();
                ui.label("Suche auf GitHub ...");
            }
            UpdateState::UpToDate(release) => {
                ui.label("Aktuell.");
                let (text, color, button) = match self.client_check {
                    ClientCheck::Matches => ("Client geprueft.", egui::Color32::from_rgb(110, 190, 110), None),
                    ClientCheck::Missing => ("Client fehlt.", egui::Color32::from_rgb(210, 150, 60), Some("Client herunterladen")),
                    ClientCheck::Differs => ("Client weicht vom Release ab.", egui::Color32::from_rgb(210, 150, 60), Some("Neu herunterladen")),
                    ClientCheck::NoDigest => ("Client nicht pruefbar (keine Pruefsumme im Release).", ui.visuals().weak_text_color(), Some("Neu herunterladen")),
                    ClientCheck::Unknown => ("", ui.visuals().weak_text_color(), None),
                };
                if !text.is_empty() {
                    ui.colored_label(color, text);
                }
                if let Some(label) = button {
                    if release.asset_for_platform().is_some() && ui.button(label).clicked() {
                        repair = Some(release.clone());
                    }
                }
            }
            // Releases gibt es, aber alle sind Betas und der Schalter ist aus.
            UpdateState::NoRelease if !self.releases.is_empty() => {
                ui.label("Noch keine stabile Version. Die Betas zeigt \"Beta-Versionen anzeigen\".");
            }
            UpdateState::NoRelease => {
                ui.label(format!("{} hat noch kein Release.", update::REPO));
            }
            UpdateState::Available(release) => {
                ui.label(format!("Neue Version: {}", release.name.as_deref().unwrap_or(&release.tag_name)));
                if release.asset_for_platform().is_some() {
                    if ui.button("Herunterladen").clicked() {
                        action = Some(release.clone());
                    }
                } else {
                    ui.colored_label(egui::Color32::from_rgb(210, 150, 60), format!("(kein {} im Release)", update::ASSET_NAME));
                }
                ui.hyperlink_to("auf GitHub", &release.html_url);
            }
            UpdateState::Downloading { progress, .. } => {
                let progress = progress.lock().unwrap();
                let fraction = if progress.total > 0 { progress.done as f32 / progress.total as f32 } else { 0.0 };
                ui.add(egui::ProgressBar::new(fraction).show_percentage().desired_width(260.0));
            }
            UpdateState::Installed { tag, launcher_updated } => {
                ui.label(format!("{tag} installiert."));
                // Der laufende Launcher ist noch die alte Fassung -- neu starten holt die neue.
                if *launcher_updated && ui.button("Launcher neu starten").clicked() {
                    restart = true;
                }
            }
            UpdateState::Failed(e) => {
                ui.colored_label(egui::Color32::from_rgb(220, 90, 90), e);
            }
        });
        if let UpdateState::Available(release) = &self.update {
            if let Some(body) = release.body.as_deref().filter(|b| !b.trim().is_empty()) {
                egui::CollapsingHeader::new("Aenderungen").show(ui, |ui| {
                    egui::ScrollArea::vertical().max_height(120.0).show(ui, |ui| ui.label(body));
                });
            }
        }
        let busy = matches!(self.update, UpdateState::Checking(_) | UpdateState::Downloading { .. });
        if let Some(release) = self.version_picker(ui, busy) {
            // Den Launcher nur ersetzen, wenn die gewaehlte Version NEUER ist als er selbst --
            // ein aelterer Launcher kennt womoeglich noch keine Betas und keine Auswahl.
            let with_launcher = update::is_newer(&release.tag_name, concat!("v", env!("CARGO_PKG_VERSION")));
            action = None;
            self.start_download(ctx, release, with_launcher);
        }
        if let Some(release) = action {
            self.start_download(ctx, release, true);
        }
        if let Some(release) = repair {
            self.start_download(ctx, release, false);
        }
        if restart {
            self.restart(ctx);
        }
        let busy = matches!(self.update, UpdateState::Checking(_) | UpdateState::Downloading { .. });
        if ui.add_enabled(!busy, egui::Button::new("Nach Updates suchen")).clicked() {
            self.check_for_update(ctx);
        }
    }

    /// Eine bestimmte Version auswaehlen und installieren -- auch eine Beta (mit eingeschaltetem
    /// Schalter) oder eine aeltere, um zurueckzugehen. Rueckgabe: das zu installierende Release.
    fn version_picker(&mut self, ui: &mut egui::Ui, busy: bool) -> Option<Release> {
        if self.releases.is_empty() {
            return None;
        }
        let beschriftung = |r: &Release, installiert: &str| {
            let mut text = r.tag_name.clone();
            if r.prerelease {
                text.push_str("  (Beta)");
            }
            if r.tag_name == installiert {
                text.push_str("  - installiert");
            }
            text
        };
        let gewaehlt = self.chosen_tag.clone().unwrap_or_else(|| self.conf.installed.clone());
        let mut neu = None;
        let mut installieren = None;
        ui.horizontal(|ui| {
            ui.label("Version:");
            let anzeige = self
                .releases
                .iter()
                .find(|r| r.tag_name == gewaehlt)
                .map(|r| beschriftung(r, &self.conf.installed))
                .unwrap_or_else(|| gewaehlt.clone());
            egui::ComboBox::from_id_salt("version").selected_text(anzeige).width(220.0).show_ui(ui, |ui| {
                for r in update::visible(&self.releases, self.conf.beta) {
                    if ui.selectable_label(r.tag_name == gewaehlt, beschriftung(r, &self.conf.installed)).clicked() {
                        neu = Some(r.tag_name.clone());
                    }
                }
            });
            let release = self.releases.iter().find(|r| r.tag_name == gewaehlt);
            let moeglich = !busy && gewaehlt != self.conf.installed && release.is_some_and(|r| r.asset_for_platform().is_some());
            if ui.add_enabled(moeglich, egui::Button::new("Installieren")).clicked() {
                installieren = release.cloned();
            }
        });
        if let Some(tag) = neu {
            self.chosen_tag = Some(tag);
        }
        installieren
    }

    fn play_section(&mut self, ui: &mut egui::Ui) {
        let client_exists = client_path().is_file();
        let installation_ok = self.selected_candidate().is_some_and(|c| c.files.is_ok());
        let running = self.client.is_some();
        let downloading = matches!(self.update, UpdateState::Downloading { .. });

        let reason = if running {
            Some("Client laeuft.".to_string())
        } else if matches!(self.game_data, GameData::Extracting { .. }) {
            Some("Spieldaten werden extrahiert ...".to_string())
        } else if !client_exists {
            Some(format!("{} fehlt -- ueber Updates herunterladen.", client_path().display()))
        } else if !installation_ok {
            Some("Erst eine nutzbare Installation waehlen.".to_string())
        } else {
            None
        };

        ui.horizontal(|ui| {
            let button = egui::Button::new(egui::RichText::new("Spielen").size(20.0)).min_size(egui::vec2(140.0, 40.0));
            if ui.add_enabled(reason.is_none() && !downloading, button).clicked() {
                self.play();
            }
            if let Some(reason) = &reason {
                ui.label(reason);
            }
        });
        if let Some(status) = &self.status {
            ui.colored_label(egui::Color32::from_rgb(220, 90, 90), status);
        }
    }
}

impl eframe::App for Launcher {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.poll();
        // Waehrend eines Downloads bzw. solange der Client laeuft regelmaessig nachsehen --
        // sonst zeichnet egui erst beim naechsten Mausereignis neu.
        if matches!(self.update, UpdateState::Downloading { .. }) {
            ctx.request_repaint_after(std::time::Duration::from_millis(100));
        } else if self.client.is_some() {
            ctx.request_repaint_after(std::time::Duration::from_secs(1));
        }

        egui::CentralPanel::default().show(ctx, |ui| {
            // Scrollbar, weil der aufgeklappte Flags-Reiter das Fenster sonst sprengt.
            egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
                self.installation_section(ui);
                ui.separator();
                self.config_section(ui);
                ui.separator();
                self.game_data_section(ui, ctx);
                ui.separator();
                self.update_section(ui, ctx);
                ui.separator();
                self.flags_section(ui);
                ui.separator();
                self.play_section(ui);
            });
        });
    }
}

fn custom_candidate(source: GameSource) -> Candidate {
    let files = source.resolve(MC_VERSION).map_err(|e| e.to_string());
    let version = files.is_ok().then(|| MC_VERSION.to_string());
    Candidate { source, label: "Eigener Ordner".into(), version, files }
}

/// Das Client-Binary liegt in ~/coral-client/binary (bei den Skins), damit der Launcher ueberall
/// liegen kann -- z.B. allein auf dem Schreibtisch. Ohne Benutzerordner neben dem Launcher.
fn client_path() -> PathBuf {
    let dir = assets::user_dir().map(|d| d.join("binary")).unwrap_or_else(launcher_dir);
    dir.join(client_file_name())
}

fn client_file_name() -> String {
    format!("coral-client{}", std::env::consts::EXE_SUFFIX)
}

fn launcher_dir() -> PathBuf {
    launcher_exe().and_then(|exe| exe.parent().map(PathBuf::from)).unwrap_or_default()
}

/// Frueher lag der Client neben dem Launcher: einmalig nach ~/coral-client/binary umziehen, statt
/// ihn neu herunterzuladen. Ein schon vorhandener Client dort bleibt unangetastet.
fn move_client_from_launcher_dir() {
    let (old, new) = (launcher_dir().join(client_file_name()), client_path());
    if old == new || !old.is_file() || new.exists() {
        return;
    }
    let result = new.parent().map_or(Ok(()), std::fs::create_dir_all).and_then(|()| {
        // rename geht nicht ueber Dateisystemgrenzen -- dann kopieren und das Original loeschen.
        std::fs::rename(&old, &new).or_else(|_| std::fs::copy(&old, &new).and_then(|_| std::fs::remove_file(&old)))
    });
    if let Err(e) = result {
        eprintln!("[launcher] {} nicht nach {} verschoben: {e}", old.display(), new.display());
    }
}

fn log_path() -> PathBuf {
    assets::config_dir().unwrap_or_else(std::env::temp_dir).join("client.log")
}

fn launcher_conf_path() -> Option<PathBuf> {
    Some(assets::config_dir()?.join("launcher.conf"))
}

/// Tag des zuletzt heruntergeladenen Releases. Ohne Eintrag gilt die eigene Version: Launcher
/// und Client werden zusammen gebaut und ausgeliefert.
/// launcher.conf: installierte Version und ob der Launcher sich beim Update selbst mit
/// ersetzt. Zeilen "schluessel=wert"; fehlende Schluessel bekommen ihre Vorgabe.
struct LauncherConf {
    installed: String,
    self_update: bool,
    // Betas (Pre-releases) anbieten, s. update::visible.
    beta: bool,
    // Eingeschaltete Debug-Flags (Umgebungsvariablen aus DEBUG_FLAGS), s. flags_section.
    flags: Vec<String>,
}

/// Ein gespeicherter Schaltername, wie er heute heisst -- oder None, wenn es den Schalter nicht
/// (mehr) gibt. Bis zur Umbenennung in Coral Client hiessen die Variablen MC_CLIENT_*, MC_PKTLOG
/// und MC_SOUNDLOG; ohne Uebersetzung zaehlte die Ueberschrift sie als "an", obwohl kein
/// Haekchen dazu passte.
fn known_flag(name: &str) -> Option<String> {
    let name = name.trim();
    let neu = match name.strip_prefix("MC_CLIENT_") {
        Some(rest) => format!("CORAL_{rest}"),
        None => match name {
            "MC_PKTLOG" => "CORAL_PKTLOG".to_string(),
            "MC_SOUNDLOG" => "CORAL_SOUNDLOG".to_string(),
            _ => name.to_string(),
        },
    };
    DEBUG_FLAGS.iter().any(|(flag, _, _)| *flag == neu).then_some(neu)
}

/// Die Diagnose-Schalter des Clients: Umgebungsvariable, Anzeigename, Erklaerung. Alle
/// schreiben in das Client-Log (s. log_path) -- bis auf den Paket-Mitschnitt und den Klang-Mitschnitt,
/// die je eine eigene Datei daneben bekommen (pktlog.txt, soundlog.txt).
const DEBUG_FLAGS: &[(&str, &str, &str)] = &[
    ("CORAL_TITLE_DEBUG", "Titel", "Jeden Titel/Untertitel mit Zeichencodes loggen, samt Zeichen, die die Schrift nicht kennt."),
    ("CORAL_LABEL_DEBUG", "Namensschilder", "Einmal pro Sekunde jede gezeichnete Beschriftung samt Quelle und Entity loggen."),
    ("CORAL_ITEM_DEBUG", "Items in der Hand", "Loggen, als was ein gehaltenes Item aufgeloest wird und mit welcher Haltung."),
    ("CORAL_INTERACT_DEBUG", "Interaktionen", "Jedes gesendete Interaktions- und Tick-Paket in Sendereihenfolge loggen (GrimAC-Paketreihenfolge)."),
    ("CORAL_NET_DEBUG", "Bewegungspakete", "Gesendete Bewegungspakete loggen (Grim Simulation/GroundSpoof)."),
    ("CORAL_POSE_DEBUG", "Posenwechsel", "Posenwechsel je Tick loggen."),
    ("CORAL_SNEAK_DEBUG", "Schleichen", "Schleich-Eingabe und Posen-Aufloesung je Tick loggen."),
    ("CORAL_SWIM_DEBUG", "Schwimmen", "Schwimm-Zustand je Tick loggen."),
    ("CORAL_WATER_DEBUG", "Wasser-Bewegung", "Bewegung im Wasser je Tick loggen (Sprint-Schwimmen)."),
    ("CORAL_SOUND_DEBUG", "Klaenge", "Abgespielte Server-Klaenge loggen."),
    ("CORAL_PACKET_SCAN", "Login-Paket", "Das erste Play-Paket (Login) mit seinen ersten Bytes loggen."),
    ("CORAL_PERFORMANCE", "Hohe Prioritaet", "Den Client mit erhoehter Prozess-Prioritaet starten (fuer Latenzmessungen)."),
    ("CORAL_SOUNDLOG", "Klang-Mitschnitt", "Jeden abgespielten Klang (vom Server und vom Client selbst) mit Zeit, Verursacher und Abstand in soundlog.txt neben dem Client-Log mitschreiben."),
    ("CORAL_PKTLOG", "Paket-Mitschnitt", "Jedes Paket in pktlog.txt neben dem Client-Log mitschreiben (wird gross)."),
];

impl LauncherConf {
    fn load() -> Self {
        let text = launcher_conf_path().and_then(|path| std::fs::read_to_string(path).ok()).unwrap_or_default();
        let value = |key: &str| text.lines().find_map(|line| line.strip_prefix(key)?.strip_prefix('=').map(|v| v.trim().to_string()));
        Self {
            installed: value("installed").filter(|v| !v.is_empty()).unwrap_or_else(|| format!("v{}", env!("CARGO_PKG_VERSION"))),
            // Vorgabe an: so war es seit v0.2.0.
            self_update: value("self_update").is_none_or(|v| v != "false"),
            beta: value("beta").is_some_and(|v| v == "true"),
            flags: value("flags").map(|v| v.split(',').filter_map(known_flag).collect()).unwrap_or_default(),
        }
    }

    fn save(&self) {
        if let Some(path) = launcher_conf_path() {
            if let Some(parent) = path.parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            let _ = std::fs::write(
                path,
                format!("installed={}\nself_update={}\nbeta={}\nflags={}\n", self.installed, self.self_update, self.beta, self.flags.join(",")),
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::known_flag;

    #[test]
    fn alte_schalternamen_werden_uebersetzt_unbekannte_verworfen() {
        assert_eq!(known_flag("MC_SOUNDLOG").as_deref(), Some("CORAL_SOUNDLOG"));
        assert_eq!(known_flag("MC_PKTLOG").as_deref(), Some("CORAL_PKTLOG"));
        assert_eq!(known_flag(" MC_CLIENT_SWIM_DEBUG ").as_deref(), Some("CORAL_SWIM_DEBUG"));
        assert_eq!(known_flag("CORAL_SWIM_DEBUG").as_deref(), Some("CORAL_SWIM_DEBUG"));
        assert_eq!(known_flag("GIBT_ES_NICHT"), None);
        assert_eq!(known_flag(""), None);
    }
}
