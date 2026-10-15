// Best-effort Latenz-Tunings, die ueber das reine Rendering hinausgehen: erhoehte
// Scheduler-Prioritaet. Alles hier ist rein additiv/optional -- schlaegt ein Tuning
// fehl, laeuft der Client trotzdem normal weiter, nur eben mit Standard-Verhalten des OS.

#[cfg(target_os = "linux")]
pub fn raise_process_priority() {
    let current = unsafe { libc::getpriority(libc::PRIO_PROCESS, 0) };
    let result = unsafe { libc::setpriority(libc::PRIO_PROCESS, 0, -10) };
    if result == 0 {
        println!("[latency] Prozess-Priorität erhöht (nice -10)");
    } else {
        println!(
            "[latency] Prozess-Prioritaet konnte nicht erhoeht werden (aktuell nice={current}). \
             Fuer weniger Scheduler-Jitter: nice-Limit fuer deinen User erhoehen, z.B. Zeile \
             '<user> - nice -10' in /etc/security/limits.d/ eintragen und neu einloggen."
        );
    }
}

// Windows: Prioritaetsklasse "Hoch" (nicht "Echtzeit" -- die kann Maus und Tastatur des
// ganzen Systems aushungern).
#[cfg(windows)]
pub fn raise_process_priority() {
    use windows_sys::Win32::System::Threading::{GetCurrentProcess, SetPriorityClass, HIGH_PRIORITY_CLASS};
    // SAFETY: Pseudo-Handle des eigenen Prozesses.
    if unsafe { SetPriorityClass(GetCurrentProcess(), HIGH_PRIORITY_CLASS) } != 0 {
        println!("[latency] Prozess-Priorität erhöht (Prioritätsklasse Hoch)");
    } else {
        println!("[latency] Prozess-Priorität konnte nicht erhöht werden: {}", std::io::Error::last_os_error());
    }
}

#[cfg(not(any(target_os = "linux", windows)))]
pub fn raise_process_priority() {}

// Frueher stand hier ein expliziter _NET_WM_BYPASS_COMPOSITOR-Hint. Empirisch entfernt:
// er kollidierte mit Muffins eigener "Komposit fuer Vollbildfenster deaktivieren"-
// Heuristik und verhinderte dadurch einen sauberen Hardware-Flip -- mit Hint riss Fifo
// trotz exakt 120 FPS, ohne Hint (Muffins Heuristik allein) nicht mehr. Ohne eigenen
// Hint entscheidet der Fenstermanager, was in aller Regel die bessere Wahl ist.

// TearFree ist eine RandR-Output-Eigenschaft, die eine zusaetzliche Scanout-Pufferstufe
// erzwingt, um Tearing bei fehlendem Hardware-Flip zu verhindern -- empirisch verifiziert,
// kostet aber messbar Latenz.
//
// Bewusst NICHT automatisch an den Present-Modus gekoppelt -- das hatten wir gebaut und
// wieder entfernt, weil eine globale Desktop-Einstellung bei jedem Present-Modus-Klick im
// Hintergrund umzuschalten die Zustaendigkeit des Clients ueberschreitet. Die Funktionen
// hier sind reine Werkzeuge fuer einen EXPLIZITEN, vom Nutzer selbst gedrueckten Schalter
// in den Einstellungen -- keine automatische Kopplung an irgendein Ereignis.
//
// Der Schalter selbst wirkt aber nur, waehrend Coral Client laeuft und im Vordergrund ist
// (s. main.rs: tearfree_original_value/tearfree_user_choice, Focused-Handler, exiting()) --
// bei Alt-Tab oder Beenden springt der Systemwert automatisch auf den Wert von vor dem Start
// zurueck, statt den Desktop dauerhaft umzustellen.
//
// WELCHER TREIBER sie hat, entscheidet der Treiber, nicht der GPU-Hersteller: amdgpu und
// radeon (xf86-video-ati) haben sie; der modesetting-Treiber erst in neueren Xorg-Versionen;
// Nvidias Treiber gar nicht (dort heisst das Gegenstueck ForceFullCompositionPipeline und
// laeuft ueber nvidia-settings); Intels xf86-video-intel nur als xorg.conf-Option, nicht zur
// Laufzeit. Deshalb wird hier NACHGESEHEN, ob der Ausgang die Eigenschaft hat -- der
// Schalter erscheint genau dann (s. main.rs), statt an "ist AMD" zu haengen.

// Der Ausgang, um den es geht: der primaere, sonst der erste angeschlossene. Frueher immer
// der erste angeschlossene -- bei zwei Monitoren war das nicht zwingend der, auf dem das
// Spiel laeuft.
pub fn pick_output(xrandr: &str) -> Option<String> {
    let angeschlossen = |l: &&str| !l.starts_with([' ', '\t']) && l.split_whitespace().nth(1) == Some("connected");
    let name = |l: &str| l.split_whitespace().next().map(str::to_string);
    xrandr
        .lines()
        .filter(angeschlossen)
        .find(|l| l.split_whitespace().nth(2) == Some("primary"))
        .or_else(|| xrandr.lines().find(angeschlossen))
        .and_then(name)
}

// Der Wert der TearFree-Eigenschaft von `output` in der Ausgabe von `xrandr --props` -- roh,
// also "on", "off" oder "auto". None = der Ausgang hat die Eigenschaft nicht.
pub fn parse_tearfree(props: &str, output: &str) -> Option<String> {
    let mut im_ausgang = false;
    for line in props.lines() {
        // Ausgangs-Kopfzeilen stehen ohne Einrueckung, alle Eigenschaften darunter mit --
        // das trennt einen Ausgangsblock vom naechsten.
        if !line.starts_with([' ', '\t']) {
            im_ausgang = line.split_whitespace().next() == Some(output);
            continue;
        }
        if im_ausgang {
            if let Some(value) = line.trim().strip_prefix("TearFree:") {
                return Some(value.trim().to_string());
            }
        }
    }
    None
}

#[cfg(target_os = "linux")]
fn xrandr(args: &[&str]) -> Option<String> {
    let out = std::process::Command::new("xrandr").args(args).output().ok()?;
    Some(String::from_utf8_lossy(&out.stdout).into_owned())
}

// Rohwert ("on"/"off"/"auto") des Spiel-Ausgangs; None = Treiber hat kein TearFree.
#[cfg(target_os = "linux")]
pub fn tearfree_value() -> Option<String> {
    let output = pick_output(&xrandr(&[])?)?;
    parse_tearfree(&xrandr(&["--props"])?, &output)
}

#[cfg(target_os = "linux")]
pub fn tearfree_enabled() -> Option<bool> {
    tearfree_value().map(|v| v == "on")
}

#[cfg(target_os = "linux")]
pub fn set_tearfree_value(value: &str) -> Result<(), String> {
    let output = xrandr(&[]).and_then(|t| pick_output(&t)).ok_or("kein verbundener Ausgang gefunden")?;
    let result = std::process::Command::new("xrandr")
        .args(["--output", &output, "--set", "TearFree", value])
        .output()
        .map_err(|e| format!("xrandr nicht ausfuehrbar: {e}"))?;
    if result.status.success() {
        println!("[latency] TearFree für {output} auf '{value}' gesetzt");
        Ok(())
    } else {
        Err(String::from_utf8_lossy(&result.stderr).trim().to_string())
    }
}

// "auto" statt "off" als Gegenstueck zu "on": das ist bei amdgpu der eigentliche
// Standardwert (schaltet sich z.B. bei aktivem VRR selbst ein). Kennt ein Treiber kein
// "auto", bleibt "off".
#[cfg(target_os = "linux")]
pub fn set_tearfree(enabled: bool) -> Result<(), String> {
    if enabled {
        return set_tearfree_value("on");
    }
    set_tearfree_value("auto").or_else(|_| set_tearfree_value("off"))
}

#[cfg(not(target_os = "linux"))]
pub fn tearfree_value() -> Option<String> {
    None
}

#[cfg(not(target_os = "linux"))]
pub fn set_tearfree_value(_value: &str) -> Result<(), String> {
    Err("TearFree gibt es nur unter X11".to_string())
}

#[cfg(not(target_os = "linux"))]
pub fn tearfree_enabled() -> Option<bool> {
    None
}

#[cfg(not(target_os = "linux"))]
pub fn set_tearfree(_enabled: bool) -> Result<(), String> {
    Err("TearFree gibt es nur unter X11".to_string())
}

#[cfg(test)]
mod tearfree_tests {
    use super::*;

    // Gekuerzte echte Ausgabe dieses Rechners (amdgpu, ein Monitor an HDMI, zwei freie DP).
    const AMDGPU: &str = "Screen 0: minimum 320 x 200, current 1920 x 1080, maximum 16384 x 16384
DisplayPort-0 disconnected (normal left inverted right x axis y axis)
\tTearFree: auto 
\t\tsupported: off, on, auto
HDMI-A-0 connected primary 1920x1080+0+0 (normal left inverted right x axis y axis) 598mm x 336mm
\tEDID: 
\t\t00ffffffffffff00
\tTearFree: on 
\t\tsupported: off, on, auto
   1920x1080    120.00*+
";

    // Nachgestellt: ein Treiber ohne die Eigenschaft (Nvidia, aeltere modesetting).
    const OHNE: &str = "Screen 0: minimum 8 x 8, current 2560 x 1440, maximum 32767 x 32767
DP-0 connected primary 2560x1440+0+0 (normal left inverted right x axis y axis) 597mm x 336mm
\tEDID: 
\t\t00ffffffffffff00
   2560x1440    165.00*+
";

    // Zwei Monitore, der primaere ist NICHT der erste.
    const ZWEI: &str = "DP-1 connected 1920x1080+2560+0 (normal) 0mm x 0mm
\tTearFree: off 
DP-2 connected primary 2560x1440+0+0 (normal) 0mm x 0mm
\tTearFree: auto 
";

    #[test]
    fn liest_den_wert_des_richtigen_ausgangs() {
        assert_eq!(pick_output(AMDGPU).as_deref(), Some("HDMI-A-0"));
        assert_eq!(parse_tearfree(AMDGPU, "HDMI-A-0").as_deref(), Some("on"));
        assert_eq!(parse_tearfree(AMDGPU, "DisplayPort-0").as_deref(), Some("auto"));
    }

    #[test]
    fn ohne_eigenschaft_gibt_es_keinen_wert() {
        assert_eq!(pick_output(OHNE).as_deref(), Some("DP-0"));
        assert_eq!(parse_tearfree(OHNE, "DP-0"), None);
    }

    #[test]
    fn nimmt_den_primaeren_monitor() {
        assert_eq!(pick_output(ZWEI).as_deref(), Some("DP-2"));
        assert_eq!(parse_tearfree(ZWEI, "DP-2").as_deref(), Some("auto"));
        // Ohne "primary" der erste angeschlossene.
        assert_eq!(pick_output(&ZWEI.replace(" primary", "")).as_deref(), Some("DP-1"));
    }
}
