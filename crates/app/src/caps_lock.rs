// Feststelltaste im Spiel sperren: die Taste kommt weiter als ganz normale Taste an (belegbar,
// z.B. Push-to-Talk), schaltet aber die Grossschreibung nicht mehr ein und die Lampe bleibt
// aus. Dafuer wird die Feststell-Sperre des Systems gleich wieder geloest -- kein Eingriff in
// Kernel oder Tastaturbelegung, nur der Zustand, den auch die Taste selbst umschaltet.
//
// X11: ueber die XKB-Erweiterung (LatchLockState) auf einer eigenen Verbindung, die beim
// ersten Gebrauch aufgebaut wird; schon beim Druecken, damit die Lampe auch beim Halten aus
// bleibt. Geloest wird nicht nur Lock: mit "caps:shiftlock" rastet die Taste Shift ein, eigene
// Belegungen auch Mod3/Mod5 (s. RASTEN). NumLock (Mod2) bleibt, wie es ist. Unter Wayland wirkt das nur auf XWayland, nicht auf den Compositor.
// Windows: erst nach dem Loslassen wird die Taste ein zweites Mal getippt, wenn die Sperre an
// ist -- beim Druecken ginge das nicht, das kuenstliche Loslassen beendete den gehaltenen Bind.

// Was eine Feststelltaste einrasten kann.
#[cfg(target_os = "linux")]
fn rasten() -> x11rb::protocol::xproto::ModMask {
    use x11rb::protocol::xproto::ModMask;
    ModMask::LOCK | ModMask::SHIFT | ModMask::M3 | ModMask::M5
}

#[derive(Default)]
pub struct CapsLockGuard {
    #[cfg(target_os = "linux")]
    x11: Option<x11rb::rust_connection::RustConnection>,
    #[cfg(target_os = "linux")]
    versucht: bool,
}

impl CapsLockGuard {
    // Die Feststelltaste wurde im Spiel gedrueckt oder losgelassen.
    pub fn on_key(&mut self, pressed: bool) {
        if cfg!(target_os = "linux") || !pressed {
            self.release();
        }
    }

    // Nachkontrolle kurz nach dem Druck (s. App::track_bind); nur unter X11 noetig.
    pub fn recheck(&mut self) {
        if cfg!(target_os = "linux") {
            self.release();
        }
    }

    // Feststell-Sperre aus. Fehler (kein X-Server, keine XKB-Erweiterung) bleiben stumm: dann
    // verhaelt sich die Taste eben wie ohne die Einstellung.
    #[cfg(target_os = "linux")]
    fn release(&mut self) {
        use x11rb::connection::Connection;
        use x11rb::protocol::xkb::{self, ConnectionExt as _};
        use x11rb::protocol::xproto::ModMask;
        if !self.versucht {
            self.versucht = true;
            self.x11 = x11rb::connect(None)
                .ok()
                .map(|(conn, _)| conn)
                .filter(|conn| conn.xkb_use_extension(1, 0).ok().and_then(|c| c.reply().ok()).is_some_and(|r| r.supported));
        }
        let Some(conn) = &self.x11 else { return };
        let keine = ModMask::from(0u16);
        let _ = conn.xkb_latch_lock_state(xkb::ID::USE_CORE_KBD.into(), rasten(), keine, false, xkb::Group::M1, keine, false, 0);
        let _ = conn.flush();
    }

    #[cfg(windows)]
    fn release(&mut self) {
        const VK_CAPITAL: i32 = 0x14;
        const KEYEVENTF_KEYUP: u32 = 0x2;
        #[link(name = "user32")]
        extern "system" {
            fn GetKeyState(virt_key: i32) -> i16;
            fn keybd_event(vk: u8, scan: u8, flags: u32, extra: usize);
        }
        // SAFETY: reine Win32-Aufrufe ohne Zeiger; Zustand abfragen, dann einmal tippen.
        unsafe {
            if GetKeyState(VK_CAPITAL) & 1 != 0 {
                keybd_event(VK_CAPITAL as u8, 0x3A, 0, 0);
                keybd_event(VK_CAPITAL as u8, 0x3A, KEYEVENTF_KEYUP, 0);
            }
        }
    }

    #[cfg(not(any(target_os = "linux", windows)))]
    fn release(&mut self) {}
}

#[cfg(all(test, target_os = "linux"))]
mod tests {
    use super::*;

    // Am echten X-Server: Sperre setzen, loesen, nachsehen. Nur von Hand (veraendert kurz den
    // Feststell-Zustand des Desktops).
    #[test]
    #[ignore]
    fn loest_die_sperre_am_x_server() {
        use x11rb::protocol::xkb::{self, ConnectionExt as _};
        use x11rb::protocol::xproto::ModMask;
        use x11rb::connection::Connection;
        let (conn, _) = x11rb::connect(None).expect("kein X-Server");
        let r = conn.xkb_use_extension(1, 0).unwrap().reply().unwrap();
        eprintln!("xkb supported={} {}.{}", r.supported, r.server_major, r.server_minor);
        let gesperrt = |c: &x11rb::rust_connection::RustConnection| c.xkb_get_state(xkb::ID::USE_CORE_KBD.into()).unwrap().reply().unwrap().locked_mods;
        eprintln!("vorher: {:?}", gesperrt(&conn));
        let keine = ModMask::from(0u16);
        // Wie caps:shiftlock mit eigener Belegung: Shift und Mod3 eingerastet.
        let gesetzt = ModMask::SHIFT | ModMask::M3;
        conn.xkb_latch_lock_state(xkb::ID::USE_CORE_KBD.into(), gesetzt, gesetzt, false, xkb::Group::M1, keine, false, 0).unwrap();
        conn.flush().unwrap();
        eprintln!("gesetzt: {:?}", gesperrt(&conn));
        let mut g = CapsLockGuard::default();
        g.release();
        eprintln!("geloest (eigene Verbindung): {:?}", gesperrt(&conn));
        assert_eq!(u16::from(gesperrt(&conn)) & u16::from(rasten()), 0);
    }
}
