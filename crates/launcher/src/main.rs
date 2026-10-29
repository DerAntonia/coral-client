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
mod hero;
mod i18n;
mod style;
mod title;
mod update;

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::mpsc::{channel, Receiver};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

use assets::{Candidate, GameSource};
use eframe::egui;
use i18n::tr;

use update::{Progress, Release};

// Muss zur MC_VERSION im Client passen (crates/app/src/main.rs).
const MC_VERSION: &str = "1.21.11";

// Fenstergroesse: nur das Startbild, bzw. die Einstellungen (s. Launcher::set_page).
const BILD_GROESSE: [f32; 2] = [500.0, 330.0];
const EINSTELLUNGEN_GROESSE: [f32; 2] = [1000.0, 660.0];
const EINSTELLUNGEN_MIN: [f32; 2] = [780.0, 540.0];

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
    let conf = LauncherConf::load();
    i18n::set_language(conf.lang);
    let installed = conf.installed;
    move_client_from_launcher_dir(&installed);
    name_client_with_version(&installed);
    update::cleanup_old_files(&client_path(&installed));
    // Laeuft der Client schon, gleich die Logs (s. Launcher::new), sonst nur das Startbild.
    let laeuft = running_client();
    let (groesse, min) = if laeuft.is_some() { (EINSTELLUNGEN_GROESSE, EINSTELLUNGEN_MIN) } else { (BILD_GROESSE, BILD_GROESSE) };
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default().with_title("Coral Client Launcher").with_app_id("coral-client-launcher").with_inner_size(groesse).with_min_inner_size(min),
        ..Default::default()
    };
    eframe::run_native("Coral Client Launcher", options, Box::new(move |cc| Ok(Box::new(Launcher::new(&cc.egui_ctx, laeuft)))))
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
    // Prozess-ID des laufenden Clients -- auch eines, den ein frueherer Launcher gestartet hat
    // (s. running_client); alle CLIENT_SCAN neu nachgesehen.
    client: Option<u32>,
    client_scan: Instant,
    // Seit wann SIGTERM geschickt ist -- der Knopf zaehlt mit (s. log_section).
    stopping: Option<Instant>,
    status: Option<String>,
    configs: Vec<String>,
    config: String,
    config_edit: Option<ConfigEdit>,
    game_data: GameData,
    log: ClientLog,
    page: Page,
    hero: hero::Hero,
    title: title::Title,
}

/// Seiten der Seitenleiste. Auf der Startseite der Spielen-Knopf und das Wichtigste in Kurzform,
/// auf den anderen die vollstaendigen Einstellungen.
#[derive(Clone, Copy, PartialEq)]
enum Page {
    Start,
    Installation,
    Configs,
    Updates,
    Diagnose,
    Launcher,
}

impl Page {
    const ALL: [Page; 6] = [Page::Start, Page::Installation, Page::Configs, Page::Updates, Page::Diagnose, Page::Launcher];

    fn title(self) -> &'static str {
        match self {
            Page::Start => "Start",
            Page::Installation => "Installation",
            Page::Configs => "Configs",
            Page::Updates => "Updates",
            Page::Diagnose => "Log & Flags",
            Page::Launcher => "Launcher",
        }
    }
}

/// Was der Client nach stdout/stderr schreibt (client.log, s. play), zum Mitlesen im Launcher.
/// Neu gelesen wird nur, wenn sich Groesse oder Aenderungszeit der Datei geaendert haben.
#[derive(Default)]
struct ClientLog {
    stand: Option<(u64, std::time::SystemTime)>,
    zeilen: Vec<String>,
}

// So viele letzte Zeilen zeigt der Log-Bereich; aeltere stehen weiter in der Datei.
const LOG_ZEILEN: usize = 2000;

impl ClientLog {
    fn refresh(&mut self) {
        let Ok(meta) = std::fs::metadata(log_path()) else {
            self.stand = None;
            self.zeilen.clear();
            return;
        };
        let stand = (meta.len(), meta.modified().unwrap_or(std::time::UNIX_EPOCH));
        if self.stand == Some(stand) {
            return;
        }
        self.stand = Some(stand);
        let text = std::fs::read(log_path()).map(|b| String::from_utf8_lossy(&b).into_owned()).unwrap_or_default();
        let alle: Vec<&str> = text.lines().collect();
        self.zeilen = alle[alle.len().saturating_sub(LOG_ZEILEN)..].iter().map(|z| z.to_string()).collect();
    }
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

fn check_client(release: Option<&Release>, path: &Path) -> ClientCheck {
    let Some(asset) = release.and_then(|r| r.asset_for_platform()) else { return ClientCheck::Unknown };
    if !path.is_file() {
        return ClientCheck::Missing;
    }
    match (asset.sha256(), update::sha256_file(path)) {
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
    fn new(ctx: &egui::Context, laeuft: Option<u32>) -> Self {
        style::apply(ctx);
        let mut launcher = Self {
            candidates: Vec::new(),
            selected: assets::load_selection(),
            custom_path: String::new(),
            conf: LauncherConf::load(),
            update: UpdateState::NoRelease,
            releases: Vec::new(),
            chosen_tag: None,
            client_check: ClientCheck::Unknown,
            client: laeuft,
            client_scan: Instant::now(),
            stopping: None,
            status: None,
            configs: Vec::new(),
            config: assets::startup_config(None),
            config_edit: None,
            game_data: GameData::Status(assets::game_data_status(MC_VERSION)),
            log: ClientLog::default(),
            page: if laeuft.is_some() { Page::Diagnose } else { Page::Start },
            hero: hero::Hero::start(ctx),
            title: title::Title::start(ctx),
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
        ui.heading(tr("Spieldaten"));
        let gruen = style::SUCCESS;
        let orange = style::WARNING;
        let rot = style::DANGER;
        let installation_ok = self.selected_candidate().is_some_and(|c| c.files.is_ok());
        let mut starten = false;
        let mut knopf = |ui: &mut egui::Ui, text: &str| {
            if ui.add_enabled(installation_ok, egui::Button::new(tr(text))).on_disabled_hover_text(tr("Erst eine nutzbare Installation waehlen.")).clicked() {
                starten = true;
            }
        };
        match &self.game_data {
            GameData::Extracting { step, .. } => {
                ui.horizontal(|ui| {
                    ui.spinner();
                    let schritt = step.lock().map(|s| s.clone()).unwrap_or_default();
                    ui.label(tr(&format!("Extrahiere aus Minecraft {MC_VERSION} ... {schritt}")));
                });
            }
            GameData::Failed(e) => {
                ui.colored_label(rot, tr(&format!("Extrahieren fehlgeschlagen: {e}")));
                knopf(ui, "Erneut versuchen");
            }
            GameData::Status(status) => {
                ui.horizontal(|ui| {
                    match status {
                        S::Current => ui.colored_label(gruen, tr("aktuell")),
                        S::Missing => ui.colored_label(orange, tr("fehlen -- ohne sie fehlen Entity-Modelle und das Kreativinventar")),
                        S::JarChanged => ui.colored_label(orange, tr("Minecraft wurde aktualisiert oder eine andere Installation gewaehlt -- neu extrahieren")),
                        S::DumperChanged => ui.colored_label(orange, tr("passen nicht zu diesem Client -- neu extrahieren")),
                    };
                    knopf(ui, if *status == S::Missing { "Extrahieren" } else { "Neu extrahieren" });
                });
            }
        }
        if let Some(dir) = assets::game_data_dir(MC_VERSION) {
            ui.weak(tr(&format!("Ordner: {}", dir.display())));
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
                let check = check_client(bezug, &client_path(&installed));
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
        let (target, thread_progress, ctx) = (client_path(&release.tag_name), progress.clone(), ctx.clone());
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
                            remove_other_clients(tag);
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

        if self.client_scan.elapsed() >= CLIENT_SCAN {
            self.client_scan = Instant::now();
            self.client = running_client();
            if self.client.is_none() {
                self.stopping = None;
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
        ui.heading(tr("Config"));
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
                if ui.button(tr("Neu")).on_hover_text(tr("Leere Config mit Standardwerten")).clicked() {
                    edit = start(ConfigEditKind::New, String::new());
                }
                if ui.button(tr("Kopieren")).on_hover_text(tr("Neue Config als Kopie der gewaehlten")).clicked() {
                    edit = start(ConfigEditKind::Duplicate, tr(&format!("{} Kopie", self.config)).into_owned());
                }
                // Umbenennen/Loeschen nicht, waehrend der Client mit ihr laeuft -- er wuerde beim
                // naechsten Speichern die alte Datei wieder anlegen.
                if ui.add_enabled(!running, egui::Button::new(tr("Umbenennen"))).clicked() {
                    edit = start(ConfigEditKind::Rename, self.config.clone());
                }
                let deletable = !running && self.configs.len() > 1;
                if ui.add_enabled(deletable, egui::Button::new(tr("Loeschen"))).on_disabled_hover_text(tr("Die letzte Config bleibt stehen.")).clicked() {
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
                ui.label(tr(match edit.kind {
                    ConfigEditKind::New => "Name der neuen Config:",
                    ConfigEditKind::Duplicate => "Name der Kopie:",
                    ConfigEditKind::Rename => "Neuer Name:",
                }));
                let field = ui.add(egui::TextEdit::singleline(&mut edit.name).desired_width(180.0));
                if std::mem::take(&mut edit.focus) {
                    field.request_focus();
                }
                if field.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                    apply = true;
                }
                if ui.button(tr("OK")).clicked() {
                    apply = true;
                }
                if ui.button(tr("Abbrechen")).clicked() || ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                    cancel = true;
                }
            });
            if let Some(error) = &edit.error {
                ui.colored_label(style::DANGER, tr(error));
            }
        }
        if cancel {
            self.config_edit = None;
        } else if apply {
            self.apply_config_edit();
        }
        ui.weak(tr("Einstellungen werden im Client bearbeitet und in der gewaehlten Config gespeichert."));
    }

    // Ausklappbarer Reiter mit den Debug-Flags. Gilt ab dem naechsten Start des Clients.
    fn flags_section(&mut self, ui: &mut egui::Ui) {
        let aktiv = self.conf.flags.len();
        let titel = if aktiv == 0 { "Flags".to_string() } else { format!("Flags ({aktiv} an)") };
        egui::CollapsingHeader::new(tr(&titel)).id_salt("flags").show(ui, |ui| {
            ui.weak(tr(&format!("Diagnose-Ausgaben landen in {}. Gilt ab dem naechsten Start.", log_path().display())));
            let mut geaendert = false;
            for (name, label, hilfe) in DEBUG_FLAGS {
                let mut an = self.conf.flags.iter().any(|f| f == name);
                if ui.checkbox(&mut an, tr(label)).on_hover_text(format!("{}\n\n{name}", tr(hilfe))).changed() {
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

    /// Startet den Client; true, wenn er laeuft.
    fn play(&mut self) -> bool {
        let path = client_path(&self.conf.installed);
        let log = std::fs::File::create(log_path());
        let mut command = Command::new(&path);
        command.arg("--config").arg(&self.config);
        // Eingeschaltete Debug-Flags als Umgebungsvariablen (s. DEBUG_FLAGS).
        for flag in &self.conf.flags {
            if flag == "CORAL_PKTLOG" {
                command.env(flag, log_path().with_file_name("pktlog.txt"));
            } else if flag == "CORAL_PKTRAW" {
                command.env(flag, log_path().with_file_name("pktraw.bin"));
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
                self.client = Some(child.id());
                self.status = None;
                true
            }
            Err(e) => {
                self.status = Some(format!("{} liess sich nicht starten: {e}", path.display()));
                false
            }
        }
    }

    /// Client beenden: "-TERM" hoeflich (die Zeit seitdem zaehlt der Knopf), "-KILL" hart.
    fn stop(&mut self, sig: &str) {
        let Some(pid) = self.client else { return };
        if sig == "-TERM" && self.stopping.is_none() {
            self.stopping = Some(Instant::now());
        }
        signal(pid, sig);
    }

    fn installation_section(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.heading(tr("Minecraft-Installation"));
            if ui.small_button(tr("Neu suchen")).clicked() {
                self.rescan();
            }
        });
        ui.label(tr(&format!("Gebraucht wird Minecraft {MC_VERSION}. Die Version muss dort schon einmal gestartet worden sein.")));
        ui.add_space(4.0);

        let mut clicked = None;
        egui::ScrollArea::vertical().max_height(240.0).show(ui, |ui| {
            if self.candidates.is_empty() {
                ui.label(tr("Keine Installation gefunden."));
            }
            for candidate in &self.candidates {
                let is_selected = self.selected.as_ref() == Some(&candidate.source);
                ui.horizontal(|ui| {
                    let text = match &candidate.version {
                        Some(v) => format!("{}  ({v})", tr(&candidate.label)),
                        None => tr(&candidate.label).into_owned(),
                    };
                    if ui.radio(is_selected, text).on_hover_text(candidate.source.describe()).clicked() {
                        clicked = Some(candidate.source.clone());
                    }
                    match &candidate.files {
                        Ok(_) => ui.colored_label(style::SUCCESS, tr("bereit")),
                        Err(reason) => ui.colored_label(style::WARNING, tr("nicht nutzbar")).on_hover_text(reason),
                    };
                });
            }
        });
        if let Some(source) = clicked {
            self.select(source);
        }

        ui.add_space(4.0);
        ui.horizontal(|ui| {
            ui.label(tr("Eigener Ordner:"));
            ui.add(egui::TextEdit::singleline(&mut self.custom_path).hint_text(tr("Pfad zu einem .minecraft-Ordner")).desired_width(260.0));
            if ui.add_enabled(!self.custom_path.trim().is_empty(), egui::Button::new(tr("Hinzufuegen"))).clicked() {
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
        ui.heading(tr("Updates"));
        ui.label(tr(&format!("Installiert: {}", self.conf.installed)));
        let checkbox = ui
            .checkbox(&mut self.conf.self_update, tr("Launcher mit aktualisieren"))
            .on_hover_text(tr("Beim Herunterladen eines Updates auch den Launcher selbst ersetzen. Aus: nur der Client wird aktualisiert, den Launcher tauschst du bei Bedarf von Hand."));
        if checkbox.changed() {
            self.conf.save();
        }
        let beta = ui
            .checkbox(&mut self.conf.beta, tr("Beta-Versionen anzeigen"))
            .on_hover_text(tr("Auch Vorabversionen (auf GitHub als Pre-release markiert) anbieten. Sie sind neuer, aber weniger getestet."));
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
                ui.label(tr("Suche auf GitHub ..."));
            }
            UpdateState::UpToDate(release) => {
                ui.label(tr("Aktuell."));
                let (text, color, button) = match self.client_check {
                    ClientCheck::Matches => ("Client geprueft.", style::SUCCESS, None),
                    ClientCheck::Missing => ("Client fehlt.", style::WARNING, Some("Client herunterladen")),
                    ClientCheck::Differs => ("Client weicht vom Release ab.", style::WARNING, Some("Neu herunterladen")),
                    ClientCheck::NoDigest => ("Client nicht pruefbar (keine Pruefsumme im Release).", ui.visuals().weak_text_color(), Some("Neu herunterladen")),
                    ClientCheck::Unknown => ("", ui.visuals().weak_text_color(), None),
                };
                if !text.is_empty() {
                    ui.colored_label(color, tr(text));
                }
                if let Some(label) = button {
                    if release.asset_for_platform().is_some() && ui.button(tr(label)).clicked() {
                        repair = Some(release.clone());
                    }
                }
            }
            // Releases gibt es, aber alle sind Betas und der Schalter ist aus.
            UpdateState::NoRelease if !self.releases.is_empty() => {
                ui.label(tr("Noch keine stabile Version. Die Betas zeigt \"Beta-Versionen anzeigen\"."));
            }
            UpdateState::NoRelease => {
                ui.label(tr(&format!("{} hat noch kein Release.", update::REPO)));
            }
            UpdateState::Available(release) => {
                ui.label(tr(&format!("Neue Version: {}", release.name.as_deref().unwrap_or(&release.tag_name))));
                if release.asset_for_platform().is_some() {
                    if ui.button(tr("Herunterladen")).clicked() {
                        action = Some(release.clone());
                    }
                } else {
                    ui.colored_label(style::WARNING, tr(&format!("(kein {} im Release)", release.client_asset_name())));
                }
                ui.hyperlink_to(tr("auf GitHub"), &release.html_url);
            }
            UpdateState::Downloading { progress, .. } => {
                let progress = progress.lock().unwrap();
                let fraction = if progress.total > 0 { progress.done as f32 / progress.total as f32 } else { 0.0 };
                ui.add(egui::ProgressBar::new(fraction).show_percentage().desired_width(260.0));
            }
            UpdateState::Installed { tag, launcher_updated } => {
                ui.label(tr(&format!("{tag} installiert.")));
                // Der laufende Launcher ist noch die alte Fassung -- neu starten holt die neue.
                if *launcher_updated && ui.button(tr("Launcher neu starten")).clicked() {
                    restart = true;
                }
            }
            UpdateState::Failed(e) => {
                ui.colored_label(style::DANGER, tr(e));
            }
        });
        if let UpdateState::Available(release) = &self.update {
            if let Some(body) = release.body.as_deref().filter(|b| !b.trim().is_empty()) {
                egui::CollapsingHeader::new(tr("Aenderungen")).show(ui, |ui| {
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
        if ui.add_enabled(!busy, egui::Button::new(tr("Nach Updates suchen"))).clicked() {
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
            tr(&text).into_owned()
        };
        let gewaehlt = self.chosen_tag.clone().unwrap_or_else(|| self.conf.installed.clone());
        let mut neu = None;
        let mut installieren = None;
        ui.horizontal(|ui| {
            ui.label(tr("Version:"));
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
            if ui.add_enabled(moeglich, egui::Button::new(tr("Installieren"))).clicked() {
                installieren = release.cloned();
            }
        });
        if let Some(tag) = neu {
            self.chosen_tag = Some(tag);
        }
        installieren
    }

    /// Warum Spielen gerade nicht geht, und wo man es behebt; None = bereit.
    fn play_blocker(&self) -> Option<(String, Page)> {
        if self.client.is_some() {
            Some(("Client laeuft.".to_string(), Page::Diagnose))
        } else if matches!(self.game_data, GameData::Extracting { .. }) {
            Some(("Spieldaten werden extrahiert ...".to_string(), Page::Installation))
        } else if matches!(self.update, UpdateState::Downloading { .. }) {
            Some(("Update wird heruntergeladen ...".to_string(), Page::Updates))
        } else if !client_path(&self.conf.installed).is_file() {
            Some((format!("{} fehlt -- unter Updates herunterladen.", client_path(&self.conf.installed).display()), Page::Updates))
        } else if self.selected_candidate().is_none_or(|c| c.files.is_err()) {
            Some(("Erst unter Installation eine nutzbare Minecraft-Installation waehlen.".to_string(), Page::Installation))
        } else if !matches!(self.game_data, GameData::Status(assets::GameDataStatus::Current | assets::GameDataStatus::JarChanged)) {
            Some(("Erst die Spieldaten extrahieren.".to_string(), Page::Installation))
        } else {
            None
        }
    }

    /// Zwischen Startbild und Einstellungen wechselt auch die Fenstergroesse.
    fn set_page(&mut self, ctx: &egui::Context, page: Page) {
        let (war, wird) = (self.page == Page::Start, page == Page::Start);
        self.page = page;
        if war == wird {
            return;
        }
        if wird {
            ctx.send_viewport_cmd(egui::ViewportCommand::MinInnerSize(BILD_GROESSE.into()));
            ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(BILD_GROESSE.into()));
        } else {
            ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(EINSTELLUNGEN_GROESSE.into()));
            ctx.send_viewport_cmd(egui::ViewportCommand::MinInnerSize(EINSTELLUNGEN_MIN.into()));
        }
    }

    /// Nur das Bild: Linksklick startet den Client und schliesst den Launcher, Rechtsklick
    /// oeffnet die Einstellungen. Geht Starten nicht, landet man auf der Seite, die es behebt.
    fn bild_ansicht(&mut self, ctx: &egui::Context) {
        egui::CentralPanel::default().frame(egui::Frame::new().fill(style::BG_APP)).show(ctx, |ui| {
            let flaeche = ui.max_rect();
            style::paint_hero(ui.painter(), flaeche, self.hero.texture());
            if self.conf.hint {
                style::paint_hint(ui.painter(), flaeche.min + egui::vec2(12.0, 12.0), self.title.texture(), &title::lines());
            }
            let klick = ui.interact(flaeche, ui.id().with("bild"), egui::Sense::click()).on_hover_cursor(egui::CursorIcon::PointingHand);
            if klick.secondary_clicked() {
                self.set_page(ctx, Page::Installation);
            } else if klick.clicked() {
                match self.play_blocker() {
                    Some((_, page)) => self.set_page(ctx, page),
                    // Bleibt der Launcher offen, zeigt er die Logs -- wie beim Oeffnen mit laufendem Client.
                    None if self.play() && self.conf.close_on_launch => ctx.send_viewport_cmd(egui::ViewportCommand::Close),
                    None => self.set_page(ctx, Page::Diagnose),
                }
            }
        });
    }

    fn launcher_section(&mut self, ui: &mut egui::Ui) {
        ui.heading(tr("Sprache"));
        ui.horizontal(|ui| {
            for lang in i18n::Language::ALL {
                if ui.selectable_label(self.conf.lang == lang, lang.label()).clicked() && self.conf.lang != lang {
                    self.conf.lang = lang;
                    self.conf.save();
                    i18n::set_language(lang);
                    // Der Hinweis im Startbild ist einmal gerastert -- in der neuen Sprache neu.
                    self.title = title::Title::start(ui.ctx());
                }
            }
        });
        ui.add_space(8.0);
        ui.heading(tr("Startbild"));
        if ui.checkbox(&mut self.conf.hint, tr("Hinweis oben links")).on_hover_text(tr("\u{201e}Linksklick für Start, Rechtsklick für Einstellungen\u{201c} über dem Bild.")).changed() {
            self.conf.save();
        }
        if ui.checkbox(&mut self.conf.close_on_launch, tr("Launcher beim Start schließen")).on_hover_text(tr("Aus: nach dem Start bleibt der Launcher offen und zeigt die Logs.")).changed() {
            self.conf.save();
        }
    }

    fn sidebar(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        ui.add_space(6.0);
        ui.label(egui::RichText::new("Coral Client").size(22.0));
        ui.label(egui::RichText::new(concat!("Launcher v", env!("CARGO_PKG_VERSION"))).size(12.0).color(style::TEXT_2));
        ui.add_space(18.0);
        ui.spacing_mut().item_spacing.y = 4.0;
        let update_da = matches!(self.update, UpdateState::Available(_));
        for page in Page::ALL {
            if style::nav_button(ui, &tr(page.title()), self.page == page, page == Page::Updates && update_da).clicked() {
                self.set_page(ctx, page);
            }
        }
        if self.client.is_some() {
            ui.with_layout(egui::Layout::bottom_up(egui::Align::Min), |ui| {
                ui.colored_label(style::SUCCESS, tr("Client laeuft"));
            });
        }
    }

    // Das Log des Clients (frueher ein Panel auf seiner Server-Seite): Verbindungsphasen,
    // Addons, Fehler. Folgt dem Ende, solange man nicht hochscrollt.
    fn log_section(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        self.log.refresh();
        ui.horizontal(|ui| {
            ui.heading(tr("Log"));
            if ui.small_button(tr("Kopieren")).clicked() {
                ctx.copy_text(self.log.zeilen.join("\n"));
            }
            if self.client.is_some() {
                // Nach SIGTERM: Sekunden seitdem, eine Nachkommastelle.
                let term = match self.stopping {
                    Some(seit) => format!("SIGTERM {:.1} s", seit.elapsed().as_secs_f32()).replace('.', ","),
                    None => "SIGTERM".to_string(),
                };
                let knopf = |text: String, farbe| egui::Button::new(egui::RichText::new(text).color(style::TEXT_ON_ACCENT)).fill(farbe);
                if ui.add(knopf(term, style::WARNING)).on_hover_text(tr("Hoeflich beenden")).clicked() {
                    self.stop("-TERM");
                }
                if ui.add(knopf("SIGKILL".to_string(), style::DANGER)).on_hover_text(tr("Sofort hart beenden")).clicked() {
                    self.stop("-KILL");
                }
            }
            ui.label(egui::RichText::new(log_path().display().to_string()).small());
        });
        ui.add_space(4.0);
        egui::Frame::group(ui.style()).show(ui, |ui| {
            egui::ScrollArea::both().id_salt("client_log").max_height(320.0).auto_shrink([false, true]).stick_to_bottom(true).show(ui, |ui| {
                if self.log.zeilen.is_empty() {
                    ui.label(tr("Noch nichts -- der Client schreibt hierher, sobald er laeuft."));
                }
                for zeile in &self.log.zeilen {
                    ui.add(egui::Label::new(egui::RichText::new(zeile).monospace()).extend());
                }
            });
        });
    }
}

impl eframe::App for Launcher {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.poll();
        // Waehrend eines Downloads bzw. solange der Client laeuft regelmaessig nachsehen --
        // sonst zeichnet egui erst beim naechsten Mausereignis neu.
        if matches!(self.update, UpdateState::Downloading { .. }) {
            ctx.request_repaint_after(std::time::Duration::from_millis(100));
        } else if self.stopping.is_some() {
            // Der SIGTERM-Knopf zaehlt Zehntelsekunden.
            ctx.request_repaint_after(std::time::Duration::from_millis(100));
        } else if self.client.is_some() {
            // Haeufig genug, dass das Log beim Verbinden fluessig mitlaeuft.
            ctx.request_repaint_after(std::time::Duration::from_millis(250));
        }

        self.hero.poll(ctx);
        self.title.poll(ctx);

        if self.page == Page::Start {
            self.bild_ansicht(ctx);
            return;
        }
        egui::SidePanel::left("seiten")
            .exact_width(200.0)
            .resizable(false)
            .frame(egui::Frame::new().fill(style::BG_PANEL).inner_margin(egui::Margin::symmetric(12, 16)))
            .show(ctx, |ui| self.sidebar(ui, ctx));
        egui::CentralPanel::default().frame(egui::Frame::new().fill(style::BG_APP).inner_margin(egui::Margin::same(20))).show(ctx, |ui| {
            ui.label(egui::RichText::new(tr(self.page.title())).size(24.0));
            if let Some(status) = &self.status {
                ui.colored_label(style::DANGER, tr(status));
            }
            ui.add_space(4.0);
            egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| match self.page {
                Page::Installation => {
                    style::card(ui, |ui| self.installation_section(ui));
                    style::card(ui, |ui| self.game_data_section(ui, ctx));
                }
                Page::Configs => style::card(ui, |ui| self.config_section(ui)),
                Page::Updates => style::card(ui, |ui| self.update_section(ui, ctx)),
                Page::Diagnose => {
                    style::card(ui, |ui| self.flags_section(ui));
                    style::card(ui, |ui| self.log_section(ui, ctx));
                }
                Page::Launcher => style::card(ui, |ui| self.launcher_section(ui)),
                Page::Start => {}
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
fn client_dir() -> PathBuf {
    assets::user_dir().map(|d| d.join("binary")).unwrap_or_else(launcher_dir)
}

/// Der Client einer Version: coral-client-v26.10.1 (unter Windows mit .exe). Es liegt immer nur
/// der installierte dort, s. remove_other_clients.
fn client_path(tag: &str) -> PathBuf {
    client_dir().join(format!("coral-client-{tag}{}", std::env::consts::EXE_SUFFIX))
}

/// Bis 1.0.0 hiess der Client ohne Version so.
fn legacy_client_file_name() -> String {
    format!("coral-client{}", std::env::consts::EXE_SUFFIX)
}

/// Einmalig: ein Client ohne Version im Namen (bis 1.0.0) bekommt die installierte Version.
fn name_client_with_version(installed: &str) {
    let (old, new) = (client_dir().join(legacy_client_file_name()), client_path(installed));
    if old.is_file() && !new.exists() {
        if let Err(e) = std::fs::rename(&old, &new) {
            eprintln!("[launcher] {} nicht in {} umbenannt: {e}", old.display(), new.display());
        }
    }
}

/// Nach einem Update: die anderen Versionen weg, nur der installierte Client bleibt. Laeuft unter
/// Windows noch ein alter, laesst er sich nicht loeschen -- dann beim naechsten Update.
fn remove_other_clients(installed: &str) {
    let behalten = client_path(installed);
    let Ok(eintraege) = std::fs::read_dir(client_dir()) else { return };
    for eintrag in eintraege.flatten() {
        let pfad = eintrag.path();
        let name = eintrag.file_name().to_string_lossy().into_owned();
        let ist_client = name.strip_suffix(std::env::consts::EXE_SUFFIX).is_some_and(|n| n == "coral-client" || n.starts_with("coral-client-v"));
        if ist_client && pfad != behalten && pfad.is_file() {
            let _ = std::fs::remove_file(&pfad);
        }
    }
}

fn launcher_dir() -> PathBuf {
    launcher_exe().and_then(|exe| exe.parent().map(PathBuf::from)).unwrap_or_default()
}

/// Frueher lag der Client neben dem Launcher: einmalig nach ~/coral-client/binary umziehen, statt
/// ihn neu herunterzuladen. Ein schon vorhandener Client dort bleibt unangetastet.
fn move_client_from_launcher_dir(installed: &str) {
    let (old, new) = (launcher_dir().join(legacy_client_file_name()), client_path(installed));
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

// So oft wird nachgesehen, ob der Client (noch) laeuft.
const CLIENT_SCAN: Duration = Duration::from_secs(1);

/// Prozess-ID eines laufenden Clients: ein Prozess, dessen Programm im Client-Ordner liegt und
/// coral-client(-v...) heisst -- auch wenn die Datei inzwischen ersetzt wurde ("(deleted)").
/// Nur unter Linux (/proc); sonst None.
fn running_client() -> Option<u32> {
    #[cfg(target_os = "linux")]
    {
        let dir = client_dir();
        for eintrag in std::fs::read_dir("/proc").ok()?.flatten() {
            let Some(pid) = eintrag.file_name().to_str().and_then(|n| n.parse::<u32>().ok()) else { continue };
            let Ok(exe) = std::fs::read_link(eintrag.path().join("exe")) else { continue };
            let exe = exe.to_string_lossy();
            let exe = Path::new(exe.strip_suffix(" (deleted)").unwrap_or(&exe));
            let name = exe.file_name().and_then(|n| n.to_str()).unwrap_or("");
            if exe.parent() == Some(dir.as_path()) && (name == "coral-client" || name.starts_with("coral-client-v")) {
                return Some(pid);
            }
        }
        None
    }
    #[cfg(not(target_os = "linux"))]
    None
}

/// Signal an einen Prozess ("-TERM", "-KILL"). Unter Windows gibt es keine Signale: dort beendet
/// taskkill hart (laufende Clients erkennt der Launcher dort ohnehin nicht, s. running_client).
fn signal(pid: u32, sig: &str) {
    #[cfg(unix)]
    let _ = Command::new("kill").arg(sig).arg(pid.to_string()).status();
    #[cfg(not(unix))]
    {
        let _ = sig;
        let _ = Command::new("taskkill").args(["/F", "/PID", &pid.to_string()]).status();
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
    // Hinweis oben links im Startbild (s. launcher_section).
    hint: bool,
    // Nach dem Start des Clients den Launcher schliessen (s. bild_ansicht).
    close_on_launch: bool,
    // Sprache des Launchers (s. i18n, launcher_section).
    lang: i18n::Language,
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
    ("CORAL_SCREENSHOT", "Screenshot", "F2 kopiert das gerade gezeigte Bild in die Zwischenablage (auch im Exclusive-Fullscreen)."),
    ("CORAL_PERFORMANCE", "Hohe Prioritaet", "Den Client mit erhoehter Prozess-Prioritaet starten (fuer Latenzmessungen)."),
    ("CORAL_SOUNDLOG", "Klang-Mitschnitt", "Jeden abgespielten Klang (vom Server und vom Client selbst) mit Zeit, Verursacher und Abstand in soundlog.txt neben dem Client-Log mitschreiben."),
    ("CORAL_PKTLOG", "Paket-Mitschnitt", "Jedes Paket in pktlog.txt neben dem Client-Log mitschreiben (wird gross)."),
    ("CORAL_PKTRAW", "Roh-Mitschnitt", "Jedes Paket mit allen Bytes in pktraw.bin neben dem Client-Log mitschreiben (wird sehr gross)."),
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
            hint: value("hint").is_none_or(|v| v != "false"),
            close_on_launch: value("close_on_launch").is_none_or(|v| v != "false"),
            lang: value("lang").and_then(|v| i18n::Language::from_code(&v)).unwrap_or(i18n::Language::German),
        }
    }

    fn save(&self) {
        if let Some(path) = launcher_conf_path() {
            if let Some(parent) = path.parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            let _ = std::fs::write(
                path,
                format!(
                    "installed={}\nself_update={}\nbeta={}\nflags={}\nhint={}\nclose_on_launch={}\nlang={}\n",
                    self.installed,
                    self.self_update,
                    self.beta,
                    self.flags.join(","),
                    self.hint,
                    self.close_on_launch,
                    self.lang.code()
                ),
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
