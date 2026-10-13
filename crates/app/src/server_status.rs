// Status der gespeicherten Server fuer die Server-Liste: MOTD, Server-Icon und ob er
// ueberhaupt antwortet. Kommt aus dem Status-Ping (net::ping), der als JSON antwortet; das Icon
// steckt darin als "data:image/png;base64,..." (Vanilla: 64x64).

use std::time::{Duration, Instant};

// Wie lange ein Ergebnis gilt, bevor die Liste neu pingt.
pub const REFRESH: Duration = Duration::from_secs(30);
pub const TIMEOUT: Duration = Duration::from_secs(5);
pub const ICON_SIZE: u32 = 64;

#[derive(Clone, Debug, Default)]
pub struct Status {
    // Zeilen der MOTD (Vanilla: zwei), je als farbige Abschnitte wie im Chat.
    pub motd: Vec<Vec<protocol::nbt::TextSpan>>,
    // 64x64 RGBA, falls der Server eins schickt.
    pub icon: Option<Vec<u8>>,
}

#[derive(Clone, Debug)]
pub enum State {
    Pending,
    Ok(Status),
    Failed,
}

pub struct Entry {
    pub state: State,
    pub since: Instant,
    // Platz im Icon-Atlas, sobald hochgeladen.
    pub icon_slot: Option<usize>,
}

// Status-JSON lesen. Fehlende Teile bleiben leer statt den ganzen Eintrag zu verwerfen --
// manche Server lassen die MOTD oder das Icon weg.
pub fn parse(json: &str) -> Option<Status> {
    let v: serde_json::Value = serde_json::from_str(json).ok()?;
    let motd = v.get("description").map(|d| motd_lines(&protocol::nbt::json_text_to_spans(&d.to_string()))).unwrap_or_default();
    let icon = v.get("favicon").and_then(|f| f.as_str()).and_then(decode_icon);
    Some(Status { motd, icon })
}

// Abschnitte an den Zeilenumbruechen aufteilen; leere Zeilen am Ende fallen weg.
fn motd_lines(spans: &[protocol::nbt::TextSpan]) -> Vec<Vec<protocol::nbt::TextSpan>> {
    let mut zeilen = vec![Vec::new()];
    for span in spans {
        for (i, teil) in span.text.split('\n').enumerate() {
            if i > 0 {
                zeilen.push(Vec::new());
            }
            if !teil.is_empty() {
                zeilen.last_mut().expect("mindestens eine Zeile").push(protocol::nbt::TextSpan { text: teil.to_string(), ..span.clone() });
            }
        }
    }
    while zeilen.last().is_some_and(|z| z.iter().all(|s| s.text.trim().is_empty())) {
        zeilen.pop();
    }
    zeilen
}

fn decode_icon(data: &str) -> Option<Vec<u8>> {
    use base64::Engine;
    let b64 = data.strip_prefix("data:image/png;base64,")?;
    let bytes = base64::engine::general_purpose::STANDARD.decode(b64.trim().replace(['\n', '\r'], "")).ok()?;
    let img = image::load_from_memory(&bytes).ok()?.to_rgba8();
    fit_icon(img.width(), img.height(), img.into_raw())
}

// RGBA-Bild beliebiger Groesse auf ICON_SIZE zum Quadrat bringen (Vanillas unknown_server.png
// ist 128 gross, Server-Favicons sollen 64 sein, halten sich aber nicht immer daran).
pub fn fit_icon(width: u32, height: u32, rgba: Vec<u8>) -> Option<Vec<u8>> {
    let img = image::RgbaImage::from_raw(width, height, rgba)?;
    let img = if img.width() == ICON_SIZE && img.height() == ICON_SIZE {
        img
    } else {
        image::imageops::resize(&img, ICON_SIZE, ICON_SIZE, image::imageops::FilterType::Triangle)
    };
    Some(img.into_raw())
}

#[cfg(test)]
mod tests {
    use super::*;

    // Vanillas unknown_server.png ist 128 gross -- der Atlas-Platz hat 64.
    #[test]
    fn icon_wird_auf_atlasgroesse_gebracht() {
        let gross = fit_icon(128, 128, vec![200; 128 * 128 * 4]).unwrap();
        assert_eq!(gross.len(), (ICON_SIZE * ICON_SIZE * 4) as usize);
        assert!(fit_icon(128, 128, vec![0; 5]).is_none(), "zu wenige Bytes fuer die Groesse");
    }

    #[test]
    fn liest_motd_und_laesst_fehlendes_leer() {
        let s = parse(r#"{"players":{"online":12,"max":100},"description":{"text":"§aHallo","extra":[{"text":"\nWelt"}]}}"#).unwrap();
        // Zwei Zeilen; der alte Farbcode wird zur Farbe des Abschnitts, nicht zu Text.
        let text = |z: &Vec<protocol::nbt::TextSpan>| z.iter().map(|s| s.text.as_str()).collect::<String>();
        assert_eq!(s.motd.iter().map(text).collect::<Vec<_>>(), ["Hallo", "Welt"]);
        assert_eq!(s.motd[0][0].color, Some(0x55FF55));
        assert!(s.icon.is_none());
        let leer = parse(r#"{"description":"x\n"}"#).unwrap();
        assert_eq!(leer.motd.len(), 1);
        assert!(parse("kein json").is_none());
    }
}
