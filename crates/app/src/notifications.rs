//! Benachrichtigungen oben rechts, wie die Toasts in osu!lazer: eine Karte gleitet von rechts
//! herein, steht ein paar Sekunden und verschwindet dann wieder nach rechts. Man kann sie
//! vorher greifen und wegwerfen, nach links wie nach rechts: waehrend des Ziehens kippt sie um den Griffpunkt,
//! losgelassen faellt sie mit Schwung und Drall unter der Schwerkraft aus dem Bild. Ein Klick
//! ohne Ziehen schliesst sie ebenfalls.
//!
//! Die API fuer den Rest des Clients ist `Notifications::push` (und `dismiss`). Alles
//! andere -- Stapeln, Einblenden, Ziehen, Wegwerfen -- passiert hier drin.
//!
//! Wie cursor_fx ist das Modul frei von Fenster- und GPU-Zustand: es bekommt Zeitpunkte,
//! Zeigerpositionen und die Hoehe jeder Karte und liefert Rechtecke. Die Hoehe misst der
//! Aufrufer, weil nur er die Schrift kennt (Zeilenumbruch des Textes).

use std::time::{Duration, Instant};

use crate::cursor_fx::Easing;

/// Art der Meldung -- bestimmt nur die Akzentfarbe.
// Oeffentliche API: nicht jede Art hat schon einen Aufrufer.
#[allow(dead_code)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Info,
    Success,
    Warning,
    Error,
}

/// Eine Meldung, wie der Aufrufer sie abgibt.
#[derive(Clone, Debug)]
pub struct Notification {
    pub kind: Kind,
    pub title: String,
    /// Darf leer sein -- dann besteht die Karte nur aus dem Titel.
    pub body: String,
    /// Wie lange die Karte stehen bleibt. Die Zeit laeuft nur, solange der Zeiger NICHT auf
    /// der Karte liegt: wer gerade liest, dem verschwindet sie nicht unter der Maus.
    pub lifetime: Duration,
}

#[allow(dead_code)] // Oeffentliche API, s. Kind.
impl Notification {
    pub fn new(kind: Kind, title: impl Into<String>, body: impl Into<String>) -> Self {
        Notification { kind, title: title.into(), body: body.into(), lifetime: DEFAULT_LIFETIME }
    }

    pub fn info(title: impl Into<String>, body: impl Into<String>) -> Self {
        Self::new(Kind::Info, title, body)
    }

    pub fn success(title: impl Into<String>, body: impl Into<String>) -> Self {
        Self::new(Kind::Success, title, body)
    }

    pub fn warning(title: impl Into<String>, body: impl Into<String>) -> Self {
        Self::new(Kind::Warning, title, body)
    }

    pub fn error(title: impl Into<String>, body: impl Into<String>) -> Self {
        Self::new(Kind::Error, title, body)
    }

    pub fn with_lifetime(mut self, lifetime: Duration) -> Self {
        self.lifetime = lifetime;
        self
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct NotificationId(u64);

// --- Masse (Bildschirmpixel) ---
pub const CARD_WIDTH: f32 = 340.0;
/// Abstand zum oberen und rechten Bildschirmrand.
pub const SCREEN_MARGIN: f32 = 16.0;
/// Abstand zwischen zwei Karten.
pub const CARD_GAP: f32 = 10.0;

// --- Zeiten ---
pub const DEFAULT_LIFETIME: Duration = Duration::from_secs(5);
const ENTER: Duration = Duration::from_millis(500);
const LEAVE: Duration = Duration::from_millis(350);
/// Mehr Karten stehen nie gleichzeitig; die aelteste raeumt vorzeitig ihren Platz.
const MAX_VISIBLE: usize = 5;

// --- Ziehen ---
/// Ab so viel Bewegung ist ein Druck ein Ziehen und kein Klick mehr.
const CLICK_SLOP: f32 = 4.0;
/// Losgelassen jenseits dieses Anteils der Kartenbreite (in beide Richtungen): weg.
const DISMISS_FRACTION: f32 = 0.35;
/// ... oder schnell genug seitlich geworfen (px/s), egal wie weit.
const FLING_SPEED: f32 = 900.0;
/// Mindesttempo, mit dem eine abgelaufene Karte nach rechts hinausfaehrt (px/s).
const LEAVE_SPEED: f32 = 1400.0;
/// Ueber dieses Zeitfenster wird die Zeigergeschwindigkeit gemittelt.
const VELOCITY_WINDOW: Duration = Duration::from_millis(80);
/// Glaettung fuer Nachruecken und Zurueckgleiten (1/s): groesser = schneller.
const FOLLOW_RATE: f32 = 14.0;

// --- Kippen und Fallen ---
//
// Eine gezogene Karte haengt am Griffpunkt wie ein Blatt zwischen zwei Fingern: zieht man
// schnell, bleibt ihr unteres Ende zurueck, und sie kippt gegen die Zugrichtung. Der Winkel
// laeuft dabei nicht starr mit, sondern haengt an einer gedaempften Feder -- beim Anhalten
// schwingt die Karte kurz nach, statt schlagartig gerade zu stehen.
//
// Weggeworfen wird sie ein freier Koerper: sie behaelt das Tempo des Zeigers, bekommt dazu
// einen Drall und faellt unter der Schwerkraft aus dem Bild.
/// Kippwinkel je px/s Zeigertempo (rad).
const TILT_PER_SPEED: f32 = 0.00035;
/// Staerker kippt sie nie (rad, ~35 Grad).
const MAX_TILT: f32 = 0.6;
/// Federhaerte (1/s^2) und Daempfung (1/s) des Kippwinkels. Unterkritisch gedaempft: ein
/// kleines Nachschwingen ist gewollt.
const TILT_STIFFNESS: f32 = 180.0;
const TILT_DAMPING: f32 = 12.0;
/// Schwerkraft fuer weggeworfene Karten (px/s^2).
const GRAVITY: f32 = 3200.0;
/// Drall beim Loslassen je px/s waagerechtem Wurftempo (rad/s).
const SPIN_PER_SPEED: f32 = 0.0025;
/// Ein langsam weggelegter Wurf driftet trotzdem etwas zur Seite, in die Richtung, in die
/// die Karte gezogen wurde (px/s) -- sonst fiele die
/// Karte senkrecht mitten durch die darunterliegenden.
const MIN_THROW_SPEED: f32 = 150.0;
/// Groesster Zeitschritt der Simulation. Laengere Bilder (Ruckler) werden in solche Schritte
/// zerlegt, damit Feder und Fall nicht explodieren.
const MAX_STEP: f32 = 1.0 / 240.0;

#[derive(Clone, Copy, Debug, PartialEq)]
enum Phase {
    Live,
    /// Blendet aus und faehrt dabei mit `velocity` px/s ab `from_x` (abgelaufen: nach
    /// rechts; angeklickt: an Ort und Stelle).
    Leaving { since: Instant, from_x: f32, velocity: f32 },
    /// Weggeworfen: Mittelpunkt und Geschwindigkeit des freien Falls.
    Falling { center: (f32, f32), velocity: (f32, f32) },
}

#[derive(Debug)]
struct Toast {
    id: NotificationId,
    note: Notification,
    born: Instant,
    /// Wie lange die Karte schon unbeobachtet stand (s. Notification::lifetime).
    shown: Duration,
    height: f32,
    /// Oberkante des Stapelplatzes -- folgt ihm weich, statt zu springen.
    y: Option<f32>,
    /// Versatz gegenueber dem Stapelplatz durch Ziehen bzw. Zuruecklaufen.
    offset: (f32, f32),
    /// Kippwinkel (rad, positiv = im Uhrzeigersinn) und seine Winkelgeschwindigkeit.
    angle: f32,
    omega: f32,
    /// Drehpunkt relativ zur linken oberen Ecke: dort, wo die Karte zuletzt gegriffen wurde.
    pivot: (f32, f32),
    phase: Phase,
}

#[derive(Debug)]
struct Drag {
    id: NotificationId,
    press: (f32, f32),
    /// Versatz der Karte beim Greifen -- der Zug kommt obendrauf.
    base: (f32, f32),
    moved: bool,
    samples: Vec<(Instant, (f32, f32))>,
}

/// Was der Aufrufer fuer eine Karte zeichnet: das ungedrehte Rechteck (x, y, w, h), gedreht
/// um `angle` (rad, im Uhrzeigersinn) um den Bildschirmpunkt `pivot`.
#[derive(Clone, Copy, Debug)]
pub struct Card<'a> {
    pub id: NotificationId,
    pub note: &'a Notification,
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
    pub angle: f32,
    pub pivot: (f32, f32),
    /// 0..1, fuer Ein- und Ausblenden.
    pub alpha: f32,
    pub hovered: bool,
    /// Weggeworfen und im Fall -- gehoert ueber alle stehenden Karten.
    pub falling: bool,
    /// Wie viel der Standzeit noch uebrig ist (1 = gerade erschienen, 0 = gleich weg).
    pub remaining: f32,
}

#[derive(Debug, Default)]
pub struct Notifications {
    toasts: Vec<Toast>,
    next_id: u64,
    drag: Option<Drag>,
    last_update: Option<Instant>,
    screen: (f32, f32),
    cursor: Option<(f32, f32)>,
    // Ein Loslassen, das schon ausgewertet wurde. Derselbe Klick kann zweimal ankommen (Roh-
    // Maus und Fenstersystem); der zweite soll nicht zum Inventar durchrutschen.
    release_handled: bool,
    // Freier Streifen oben, unter dem die Karten beginnen (die Leiste der Menues).
    top_inset: f32,
}

// Einen Punkt um `angle` (im Uhrzeigersinn, y zeigt nach unten) um `pivot` drehen. Dieselbe
// Drehung wendet der Zeichner an -- s. main.rs, rotate_ndc.
pub fn rotate_about(p: (f32, f32), pivot: (f32, f32), angle: f32) -> (f32, f32) {
    let (s, c) = angle.sin_cos();
    let (dx, dy) = (p.0 - pivot.0, p.1 - pivot.1);
    (pivot.0 + dx * c - dy * s, pivot.1 + dx * s + dy * c)
}

impl Notifications {
    pub fn new() -> Self {
        Self::default()
    }

    /// Die Karten beginnen erst unter diesem Streifen (z. B. der Leiste oben in den Menues).
    pub fn set_top_inset(&mut self, inset: f32) {
        self.top_inset = inset;
    }

    /// Eine neue Meldung zeigen. Sie erscheint zuoberst, die aelteren ruecken nach unten.
    pub fn push(&mut self, note: Notification, now: Instant) -> NotificationId {
        let id = NotificationId(self.next_id);
        self.next_id += 1;
        self.toasts.insert(
            0,
            Toast {
                id,
                note,
                born: now,
                shown: Duration::ZERO,
                height: 0.0,
                y: None,
                offset: (0.0, 0.0),
                angle: 0.0,
                omega: 0.0,
                pivot: (0.0, 0.0),
                phase: Phase::Live,
            },
        );
        // Zu viele: die aeltesten stehenden Karten gehen vorzeitig.
        let mut live = 0;
        for t in &mut self.toasts {
            if t.phase == Phase::Live {
                live += 1;
                if live > MAX_VISIBLE {
                    t.phase = Phase::Leaving { since: now, from_x: t.offset.0, velocity: LEAVE_SPEED };
                }
            }
        }
        id
    }

    /// Eine Meldung von aussen schliessen (z.B. weil sich ihr Anlass erledigt hat).
    #[allow(dead_code)] // Oeffentliche API, s. Kind.
    pub fn dismiss(&mut self, id: NotificationId, now: Instant) {
        if let Some(t) = self.toasts.iter_mut().find(|t| t.id == id && t.phase == Phase::Live) {
            t.phase = Phase::Leaving { since: now, from_x: t.offset.0, velocity: LEAVE_SPEED };
        }
    }

    /// Titel und Text einer stehenden Meldung austauschen -- fuer Fortschrittsanzeigen, die
    /// nicht bei jedem Schritt eine neue Karte aufmachen sollen.
    pub fn set_text(&mut self, id: NotificationId, title: impl Into<String>, body: impl Into<String>) {
        if let Some(t) = self.toasts.iter_mut().find(|t| t.id == id) {
            t.note.title = title.into();
            t.note.body = body.into();
        }
    }

    /// Eine noch stehende Meldung neu beschriften und ihre Standzeit von vorn beginnen lassen
    /// -- fuer Zaehler, die weiterlaufen, solange der Anlass anhaelt. false, wenn die Karte
    /// schon geht oder weg ist (dann macht der Aufrufer eine neue auf).
    pub fn renew(&mut self, id: NotificationId, title: impl Into<String>, body: impl Into<String>) -> bool {
        let Some(t) = self.toasts.iter_mut().find(|t| t.id == id && t.phase == Phase::Live) else { return false };
        t.note.title = title.into();
        t.note.body = body.into();
        t.shown = Duration::ZERO;
        true
    }

    #[cfg(test)]
    pub fn is_empty(&self) -> bool {
        self.toasts.is_empty()
    }

    /// Einmal je Bild. `cursor` ist None, wenn es gerade keinen Zeiger gibt (Maus steuert
    /// die Kamera) -- dann kann nichts gehovert oder gezogen werden. `height_of` misst eine
    /// Karte (s. Modulkommentar).
    pub fn update(&mut self, now: Instant, screen: (f32, f32), cursor: Option<(f32, f32)>, height_of: impl Fn(&Notification) -> f32) {
        let dt = self.last_update.map(|t| now.saturating_duration_since(t)).unwrap_or_default();
        self.last_update = Some(now);
        self.screen = screen;
        self.cursor = cursor;
        let glatt = 1.0 - (-dt.as_secs_f32() * FOLLOW_RATE).exp();
        // Nach einem langen Haenger nicht Sekunden an Physik nachholen.
        let sim_dt = dt.as_secs_f32().min(0.1);
        let schritte = (sim_dt / MAX_STEP).ceil().max(1.0) as usize;
        let h = sim_dt / schritte as f32;

        // Ohne Zeiger kein Ziehen: die Karte laeuft an ihren Platz zurueck.
        if cursor.is_none() {
            self.drag = None;
        }
        let mut zug_tempo = (0.0, 0.0);
        if let (Some(drag), Some(c)) = (&mut self.drag, cursor) {
            drag.samples.push((now, c));
            drag.samples.retain(|(t, _)| now.saturating_duration_since(*t) <= VELOCITY_WINDOW);
            if (c.0 - drag.press.0).abs() > CLICK_SLOP || (c.1 - drag.press.1).abs() > CLICK_SLOP {
                drag.moved = true;
            }
            zug_tempo = velocity(&drag.samples);
        }

        let hovered = self.hovered_id();
        let mut top = SCREEN_MARGIN + self.top_inset;
        for t in &mut self.toasts {
            t.height = height_of(&t.note);
            match t.phase {
                Phase::Live => {}
                Phase::Leaving { .. } => continue,
                Phase::Falling { mut center, mut velocity } => {
                    for _ in 0..schritte {
                        velocity.1 += GRAVITY * h;
                        center.0 += velocity.0 * h;
                        center.1 += velocity.1 * h;
                        t.angle += t.omega * h;
                    }
                    t.phase = Phase::Falling { center, velocity };
                    continue;
                }
            }
            let ziel = top;
            top += t.height + CARD_GAP;
            t.y = Some(match t.y {
                None => ziel,
                Some(y) => y + (ziel - y) * glatt,
            });
            let gezogen = self.drag.as_ref().filter(|d| d.id == t.id);
            let ziel_winkel = if let (Some(drag), Some(c)) = (gezogen, cursor) {
                let dx = c.0 - drag.press.0;
                t.offset = (drag.base.0 + dx, drag.base.1 + c.1 - drag.press.1);
                (zug_tempo.0 * TILT_PER_SPEED).clamp(-MAX_TILT, MAX_TILT)
            } else {
                t.offset.0 *= 1.0 - glatt;
                t.offset.1 *= 1.0 - glatt;
                if t.offset.0.abs() < 0.5 && t.offset.1.abs() < 0.5 {
                    t.offset = (0.0, 0.0);
                }
                0.0
            };
            for _ in 0..schritte {
                t.omega += (TILT_STIFFNESS * (ziel_winkel - t.angle) - TILT_DAMPING * t.omega) * h;
                t.angle += t.omega * h;
            }
            if gezogen.is_none() && t.angle.abs() < 1e-4 && t.omega.abs() < 1e-3 {
                t.angle = 0.0;
                t.omega = 0.0;
            }
            if gezogen.is_none() && hovered != Some(t.id) {
                t.shown += dt;
            }
            if t.shown >= t.note.lifetime {
                t.phase = Phase::Leaving { since: now, from_x: t.offset.0, velocity: LEAVE_SPEED };
            }
        }
        let (_, screen_h) = screen;
        self.toasts.retain(|t| match t.phase {
            Phase::Live => true,
            Phase::Leaving { since, .. } => now.saturating_duration_since(since) < LEAVE,
            // Erst weg, wenn auch die gedrehte Karte ganz unter dem Bildrand ist.
            Phase::Falling { center, .. } => center.1 - (CARD_WIDTH + t.height) < screen_h,
        });
    }

    /// Linke Taste gedrueckt. `true` = der Druck gehoert einer Karte und darf nicht weiter
    /// (kein Menueklick, kein Inventarslot darunter).
    pub fn press(&mut self, now: Instant) -> bool {
        self.release_handled = false;
        if self.drag.is_some() {
            return true; // derselbe Druck, zweiter Eingabeweg
        }
        let Some(c) = self.cursor else { return false };
        let Some(id) = self.hovered_id() else { return false };
        let Some(now_frame) = self.last_update else { return false };
        // Der Griffpunkt wird zum Drehpunkt. Schwingt die Karte noch nach, verschiebt ein neuer
        // Drehpunkt bei gleichem Winkel ihre Lage -- der Versatz gleicht das aus, damit sie
        // unter dem Zeiger nicht springt. (Genaugenommen liegt der Griff dann nicht exakt an
        // der ungedrehten Stelle; bei den kleinen Restwinkeln ist das nicht zu sehen.)
        let mut base = (0.0, 0.0);
        let karte = self.cards(now_frame).find(|k| k.id == id).map(|k| (k.pivot, k.x, k.y));
        if let Some((alt, kx, ky)) = karte {
            let neu_lokal = (c.0 - kx, c.1 - ky);
            let neu = c;
            if let Some(t) = self.toasts.iter_mut().find(|t| t.id == id) {
                let zurueck = rotate_about(alt, neu, -t.angle);
                t.offset.0 += zurueck.0 - neu.0 - (alt.0 - neu.0);
                t.offset.1 += zurueck.1 - neu.1 - (alt.1 - neu.1);
                t.pivot = neu_lokal;
                base = t.offset;
            }
        }
        self.drag = Some(Drag { id, press: c, base, moved: false, samples: vec![(now, c)] });
        true
    }

    /// Linke Taste losgelassen. `true` = das Loslassen beendete ein Ziehen auf einer Karte.
    pub fn release(&mut self, now: Instant) -> bool {
        let Some(drag) = self.drag.take() else {
            return std::mem::take(&mut self.release_handled);
        };
        self.release_handled = true;
        let Some(frame) = self.last_update else { return true };
        let Some(card) = self.cards(frame).find(|k| k.id == drag.id) else { return true };
        let (cx, cy, w, h, angle, pivot) = (card.x, card.y, card.w, card.h, card.angle, card.pivot);
        let Some(t) = self.toasts.iter_mut().find(|t| t.id == drag.id && t.phase == Phase::Live) else { return true };
        if !drag.moved {
            // Klick: schliessen, ohne Flug -- sie blendet an Ort und Stelle aus.
            t.phase = Phase::Leaving { since: now, from_x: t.offset.0, velocity: 0.0 };
            return true;
        }
        let v = velocity(&drag.samples);
        if t.offset.0.abs() > CARD_WIDTH * DISMISS_FRACTION || v.0.abs() > FLING_SPEED {
            // Ab jetzt ein freier Koerper um seinen Mittelpunkt. Wo der gerade steht, ergibt
            // sich aus der aktuellen Drehung um den Griffpunkt -- so springt nichts.
            let center = rotate_about((cx + w / 2.0, cy + h / 2.0), pivot, angle);
            // Richtung: die des Wurfs, bei einem langsam weggelegten die des Versatzes.
            let richtung = if v.0.abs() > FLING_SPEED { v.0.signum() } else { t.offset.0.signum() };
            let vx = if richtung < 0.0 { v.0.min(-MIN_THROW_SPEED) } else { v.0.max(MIN_THROW_SPEED) };
            t.omega += vx * SPIN_PER_SPEED;
            t.phase = Phase::Falling { center, velocity: (vx, v.1) };
        }
        // Sonst gleitet sie in update() zurueck und richtet sich auf.
        true
    }

    /// Ob der Zeiger gerade auf einer Karte liegt.
    pub fn hovered_id(&self) -> Option<NotificationId> {
        let (cx, cy) = self.cursor?;
        if let Some(d) = &self.drag {
            return Some(d.id);
        }
        let now = self.last_update?;
        self.cards(now)
            .filter(|c| self.toasts.iter().any(|t| t.id == c.id && t.phase == Phase::Live))
            .find(|c| c.alpha > 0.0 && cx >= c.x && cx <= c.x + c.w && cy >= c.y && cy <= c.y + c.h)
            .map(|c| c.id)
    }

    /// Die Karten dieses Bildes, von oben nach unten.
    pub fn cards(&self, now: Instant) -> impl Iterator<Item = Card<'_>> {
        let rechts = self.screen.0 - SCREEN_MARGIN;
        let hovered = self.drag.as_ref().map(|d| d.id);
        let cursor = self.cursor;
        self.toasts.iter().filter_map(move |t| {
            let (w, h) = (CARD_WIDTH, t.height);
            let seit = now.saturating_duration_since(t.born).as_secs_f32();
            let rein = Easing::OutQuint.apply(seit / ENTER.as_secs_f32());
            // Einfahren: vom rechten Rand bis an den Platz.
            let einfahrt = (1.0 - rein) * (CARD_WIDTH + SCREEN_MARGIN);
            let (x, y, alpha) = match t.phase {
                Phase::Live => (rechts - w + t.offset.0 + einfahrt, t.y? + t.offset.1, rein.min(1.0)),
                Phase::Leaving { since, from_x, velocity } => {
                    let s = now.saturating_duration_since(since).as_secs_f32();
                    let f = (s / LEAVE.as_secs_f32()).clamp(0.0, 1.0);
                    (rechts - w + from_x + einfahrt + velocity * s, t.y? + t.offset.1, rein.min(1.0) * (1.0 - f))
                }
                Phase::Falling { center, .. } => (center.0 - w / 2.0, center.1 - h / 2.0, 1.0),
            };
            let pivot = match t.phase {
                Phase::Falling { center, .. } => center,
                _ => (x + t.pivot.0, y + t.pivot.1),
            };
            let unter_zeiger = cursor.is_some_and(|(cx, cy)| cx >= x && cx <= x + w && cy >= y && cy <= y + h);
            let hovered = t.phase == Phase::Live && (hovered == Some(t.id) || (hovered.is_none() && unter_zeiger));
            let remaining = 1.0 - (t.shown.as_secs_f32() / t.note.lifetime.as_secs_f32().max(1e-3)).min(1.0);
            let falling = matches!(t.phase, Phase::Falling { .. });
            Some(Card { id: t.id, note: &t.note, x, y, w, h, angle: t.angle, pivot, alpha, hovered, falling, remaining })
        })
    }
}

// Mittlere Geschwindigkeit (px/s) ueber die gesammelten Zeigerpositionen.
fn velocity(samples: &[(Instant, (f32, f32))]) -> (f32, f32) {
    let (Some(&(t0, p0)), Some(&(t1, p1))) = (samples.first(), samples.last()) else { return (0.0, 0.0) };
    let dt = t1.saturating_duration_since(t0).as_secs_f32();
    if dt < 1e-3 {
        return (0.0, 0.0);
    }
    ((p1.0 - p0.0) / dt, (p1.1 - p0.1) / dt)
}

#[cfg(test)]
mod tests {
    use super::*;

    const SCREEN_W: f32 = 1920.0;
    const SCREEN_H: f32 = 1080.0;
    const H: f32 = 60.0;

    fn ms(n: u64) -> Duration {
        Duration::from_millis(n)
    }

    // Zeiger bewegen und ein Bild weiterschalten.
    fn frame(n: &mut Notifications, now: Instant, cursor: Option<(f32, f32)>) {
        n.update(now, (SCREEN_W, SCREEN_H), cursor, |_| H);
    }

    fn mitte(n: &Notifications, now: Instant) -> (f32, f32) {
        let c = n.cards(now).next().expect("Karte");
        (c.x + c.w / 2.0, c.y + c.h / 2.0)
    }

    #[test]
    fn faehrt_von_rechts_ein_und_steht_dann_oben_rechts() {
        let t0 = Instant::now();
        let mut n = Notifications::new();
        n.push(Notification::info("Hallo", ""), t0);
        frame(&mut n, t0, None);
        let start = n.cards(t0).next().unwrap();
        assert!(start.x >= SCREEN_W - SCREEN_MARGIN, "startet ausserhalb rechts: {}", start.x);
        let spaeter = t0 + ENTER;
        frame(&mut n, spaeter, None);
        let c = n.cards(spaeter).next().unwrap();
        assert!((c.x - (SCREEN_W - SCREEN_MARGIN - CARD_WIDTH)).abs() < 0.01);
        assert_eq!(c.y, SCREEN_MARGIN);
        assert!((c.alpha - 1.0).abs() < 1e-4);
    }

    #[test]
    fn neue_karte_oben_aeltere_ruecken_nach_unten() {
        let t0 = Instant::now();
        let mut n = Notifications::new();
        let a = n.push(Notification::info("A", ""), t0);
        frame(&mut n, t0, None);
        let b = n.push(Notification::info("B", ""), t0 + ms(10));
        // Lange genug, dass das Nachruecken angekommen ist.
        let mut t = t0 + ms(10);
        for _ in 0..200 {
            t += ms(16);
            frame(&mut n, t, None);
        }
        let karten: Vec<_> = n.cards(t).map(|c| (c.id, c.y)).collect();
        assert_eq!(karten[0].0, b);
        assert_eq!(karten[1].0, a);
        assert!((karten[1].1 - (SCREEN_MARGIN + H + CARD_GAP)).abs() < 0.5, "{karten:?}");
    }

    #[test]
    fn verschwindet_nach_standzeit_aber_nicht_unter_dem_zeiger() {
        let t0 = Instant::now();
        let mut n = Notifications::new();
        n.push(Notification::info("A", "").with_lifetime(ms(1000)), t0);
        frame(&mut n, t0, None);
        let t = t0 + ENTER;
        frame(&mut n, t, None);
        let zeiger = mitte(&n, t);
        // Zwei Sekunden unter dem Zeiger: bleibt.
        let mut t = t;
        for _ in 0..125 {
            t += ms(16);
            frame(&mut n, t, Some(zeiger));
        }
        assert!(n.cards(t).next().is_some_and(|c| c.alpha == 1.0));
        // Zeiger weg: nach Ablauf der Rest-Standzeit plus Ausblenden ist sie fort.
        for _ in 0..120 {
            t += ms(16);
            frame(&mut n, t, None);
        }
        assert!(n.is_empty());
    }

    #[test]
    fn nach_links_ziehen_wirft_weg() {
        let t0 = Instant::now();
        let mut n = Notifications::new();
        n.push(Notification::info("A", ""), t0);
        let mut t = t0 + ENTER;
        frame(&mut n, t, None);
        let (x, y) = mitte(&n, t);
        frame(&mut n, t, Some((x, y)));
        assert!(n.press(t));
        // Langsam (kein Wurf) weit nach links.
        for i in 1..=40 {
            t += ms(16);
            frame(&mut n, t, Some((x - i as f32 * 4.0, y)));
        }
        assert!(n.cards(t).next().unwrap().x < SCREEN_W - SCREEN_MARGIN - CARD_WIDTH - 150.0);
        assert!(n.release(t));
        for _ in 0..120 {
            t += ms(16);
            frame(&mut n, t, Some((x - 160.0, y)));
        }
        assert!(n.is_empty(), "aus dem Bild gefallen");
    }

    #[test]
    fn kurz_ziehen_federt_zurueck() {
        let t0 = Instant::now();
        let mut n = Notifications::new();
        n.push(Notification::info("A", ""), t0);
        let mut t = t0 + ENTER;
        frame(&mut n, t, None);
        let (x, y) = mitte(&n, t);
        frame(&mut n, t, Some((x, y)));
        assert!(n.press(t));
        for i in 1..=10 {
            t += ms(40);
            frame(&mut n, t, Some((x - i as f32 * 5.0, y)));
        }
        assert!(n.release(t));
        for _ in 0..60 {
            t += ms(16);
            frame(&mut n, t, Some((x - 50.0, y)));
        }
        let c = n.cards(t).next().expect("noch da");
        assert!((c.x - (SCREEN_W - SCREEN_MARGIN - CARD_WIDTH)).abs() < 0.01, "zurück an ihrem Platz: {}", c.x);
        assert!(c.angle.abs() < 0.01, "wieder gerade: {}", c.angle);
    }

    #[test]
    fn schneller_wurf_reicht_auch_kurz() {
        let t0 = Instant::now();
        let mut n = Notifications::new();
        n.push(Notification::info("A", ""), t0);
        let mut t = t0 + ENTER;
        frame(&mut n, t, None);
        let (x, y) = mitte(&n, t);
        frame(&mut n, t, Some((x, y)));
        assert!(n.press(t));
        // 60 px in 32 ms = ~1900 px/s nach links.
        for i in 1..=2 {
            t += ms(16);
            frame(&mut n, t, Some((x - i as f32 * 30.0, y)));
        }
        assert!(n.release(t));
        for _ in 0..120 {
            t += ms(16);
            frame(&mut n, t, None);
        }
        assert!(n.is_empty());
    }

    #[test]
    fn klick_schliesst_und_zurueckgezogen_bleibt_sie() {
        let t0 = Instant::now();
        let mut n = Notifications::new();
        n.push(Notification::info("A", ""), t0);
        let t = t0 + ENTER;
        frame(&mut n, t, None);
        let (x, y) = mitte(&n, t);
        frame(&mut n, t, Some((x, y)));
        assert!(n.press(t));
        frame(&mut n, t + ms(16), Some((x + 100.0, y)));
        let c = n.cards(t + ms(16)).next().unwrap();
        assert!((c.x - (SCREEN_W - SCREEN_MARGIN - CARD_WIDTH + 100.0)).abs() < 0.01, "folgt auch nach rechts ohne Widerstand");
        // Zurueck zum Ausgangspunkt und loslassen: kein Wegwerfen.
        frame(&mut n, t + ms(500), Some((x, y)));
        assert!(n.release(t + ms(500)));
        assert!(n.cards(t + ms(500)).next().is_some_and(|c| c.alpha == 1.0));

        // Echter Klick ohne Bewegung: schliesst.
        let t = t + ms(600);
        frame(&mut n, t, Some((x, y)));
        assert!(n.press(t));
        assert!(n.release(t + ms(50)));
        frame(&mut n, t + ms(50) + LEAVE, None);
        assert!(n.is_empty());
    }

    #[test]
    fn klick_daneben_gehoert_nicht_den_karten_und_doppelte_eingabe_wird_geschluckt() {
        let t0 = Instant::now();
        let mut n = Notifications::new();
        n.push(Notification::info("A", ""), t0);
        let t = t0 + ENTER;
        frame(&mut n, t, Some((10.0, 10.0)));
        assert!(!n.press(t));
        assert!(!n.release(t));
        let (x, y) = mitte(&n, t);
        frame(&mut n, t, Some((x, y)));
        // Derselbe Klick ueber zwei Eingabewege: beide Male geschluckt.
        assert!(n.press(t));
        assert!(n.press(t));
        frame(&mut n, t + ms(16), Some((x - 20.0, y)));
        assert!(n.release(t + ms(16)));
        assert!(n.release(t + ms(16)));
        // Danach wieder normal.
        assert!(!n.release(t + ms(20)));
    }

    #[test]
    fn hoechstens_fuenf_stehen_gleichzeitig() {
        let t0 = Instant::now();
        let mut n = Notifications::new();
        for i in 0..7 {
            n.push(Notification::info(format!("{i}"), ""), t0);
        }
        frame(&mut n, t0 + LEAVE, None);
        assert_eq!(n.cards(t0 + LEAVE).count(), MAX_VISIBLE);
    }

    #[test]
    fn kippt_gegen_die_zugrichtung_und_faellt_mit_schwerkraft() {
        let t0 = Instant::now();
        let mut n = Notifications::new();
        n.push(Notification::info("A", ""), t0);
        let mut t = t0 + ENTER;
        frame(&mut n, t, None);
        let (x, y) = mitte(&n, t);
        frame(&mut n, t, Some((x, y)));
        assert!(n.press(t));
        // Zuegig nach links: das untere Ende bleibt rechts zurueck = gegen den Uhrzeigersinn.
        for i in 1..=6 {
            t += ms(16);
            frame(&mut n, t, Some((x - i as f32 * 12.0, y)));
        }
        let c = n.cards(t).next().unwrap();
        assert!(c.angle < -0.05, "kippt: {}", c.angle);
        assert!(c.angle >= -MAX_TILT - 0.2);
        // Der Griffpunkt ist der Drehpunkt und bleibt unter dem Zeiger.
        assert!((c.pivot.0 - (x - 72.0)).abs() < 0.01 && (c.pivot.1 - y).abs() < 0.01, "{:?}", c.pivot);

        // Kraeftig nach links werfen und loslassen.
        for i in 1..=3 {
            t += ms(16);
            frame(&mut n, t, Some((x - 72.0 - i as f32 * 30.0, y)));
        }
        assert!(n.release(t));
        let start = n.cards(t).next().map(|c| (c.x, c.y, c.angle)).unwrap();
        let start = Start { x: start.0, y: start.1, angle: start.2 };
        let mut ys = Vec::new();
        for _ in 0..3 {
            t += ms(100);
            frame(&mut n, t, None);
            let c = n.cards(t).next().unwrap();
            ys.push(c.y);
            assert!(c.x < start.x, "fliegt nach links");
        }
        // Beschleunigt nach unten: die Schritte werden groesser.
        assert!(ys[1] - ys[0] > ys[0] - start.y && ys[2] - ys[1] > ys[1] - ys[0], "{:?}", ys);
        assert!(n.cards(t).next().unwrap().angle < start.angle, "dreht sich weiter");
    }

    struct Start {
        x: f32,
        y: f32,
        angle: f32,
    }

    #[test]
    fn nach_rechts_werfen_geht_auch() {
        let t0 = Instant::now();
        let mut n = Notifications::new();
        n.push(Notification::info("A", ""), t0);
        let mut t = t0 + ENTER;
        frame(&mut n, t, None);
        let (x, y) = mitte(&n, t);
        frame(&mut n, t, Some((x, y)));
        assert!(n.press(t));
        for i in 1..=3 {
            t += ms(16);
            frame(&mut n, t, Some((x + i as f32 * 30.0, y)));
        }
        let kipp = n.cards(t).next().unwrap().angle;
        assert!(kipp > 0.0, "kippt spiegelbildlich: {kipp}");
        assert!(n.release(t));
        let start_x = n.cards(t).next().unwrap().x;
        t += ms(100);
        frame(&mut n, t, None);
        let c = n.cards(t).next().unwrap();
        assert!(c.falling && c.x > start_x, "faellt nach rechts");
        for _ in 0..120 {
            t += ms(16);
            frame(&mut n, t, None);
        }
        assert!(n.is_empty());
    }

    #[test]
    fn drehung_um_den_drehpunkt() {
        let p = rotate_about((10.0, 0.0), (0.0, 0.0), std::f32::consts::FRAC_PI_2);
        // Im Uhrzeigersinn auf dem Bildschirm (y nach unten): rechts -> unten.
        assert!((p.0 - 0.0).abs() < 1e-5 && (p.1 - 10.0).abs() < 1e-5, "{p:?}");
    }

    #[test]
    fn dismiss_von_aussen() {
        let t0 = Instant::now();
        let mut n = Notifications::new();
        let id = n.push(Notification::info("A", ""), t0);
        frame(&mut n, t0, None);
        n.dismiss(id, t0);
        frame(&mut n, t0 + LEAVE, None);
        assert!(n.is_empty());
    }
}
