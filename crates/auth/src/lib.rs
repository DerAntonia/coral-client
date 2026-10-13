// Microsoft-/Minecraft-Authentifizierung, modular und ohne Abhaengigkeit auf net/app.
//
// Zwei Wege zum fertigen `Credentials` (Access-Token + Profil-UUID + Name), mit dem man
// Online-Mode-Servern beitreten kann:
//   1. Device-Code-Flow (`request_device_code` -> Nutzer meldet sich im Browser an ->
//      `poll_for_msa` -> `msa_to_minecraft`), inkl. Refresh-Token zum Eingeloggt-Bleiben
//      (`refresh_msa`).
//   2. Manuelles Minecraft-Access-Token (`credentials_from_token`) -- fuer das
//      Token-Eingabefenster als Fallback ohne Microsoft-Anmeldung.
//
// Dazu der Server-Handshake-Baustein `mojang_join` (Session-Server-POST mit dem
// Server-Hash), den net waehrend des Verschluesselungs-Handshakes aufruft.
//
// Kette (Device-Code): MSA-Token -> Xbox Live (XBL) -> XSTS -> Minecraft-Services-Login
// -> Profil. Alle HTTP-Aufrufe leben hier, damit net/app nichts von reqwest wissen muessen.

use serde::Deserialize;
use std::path::PathBuf;
use std::time::Duration;

// OAuth2-Endpunkte (consumers-Tenant = private Microsoft-Konten).
const DEVICE_CODE_URL: &str = "https://login.microsoftonline.com/consumers/oauth2/v2.0/devicecode";
const TOKEN_URL: &str = "https://login.microsoftonline.com/consumers/oauth2/v2.0/token";
const SCOPE: &str = "XboxLive.signin offline_access";

const XBL_URL: &str = "https://user.auth.xboxlive.com/user/authenticate";
const XSTS_URL: &str = "https://xsts.auth.xboxlive.com/xsts/authorize";
const MC_LOGIN_URL: &str = "https://api.minecraftservices.com/authentication/login_with_xbox";
const MC_PROFILE_URL: &str = "https://api.minecraftservices.com/minecraft/profile";
const SESSION_JOIN_URL: &str = "https://sessionserver.mojang.com/session/minecraft/join";
// Profilschluessel fuer signierten Chat (s. fetch_profile_key).
const PROFILE_KEY_URL: &str = "https://api.minecraftservices.com/player/certificates";

#[derive(Debug, Clone)]
pub struct AuthError(pub String);

impl std::fmt::Display for AuthError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Auth-Fehler: {}", self.0)
    }
}
impl std::error::Error for AuthError {}

fn err(msg: impl Into<String>) -> AuthError {
    AuthError(msg.into())
}
impl From<reqwest::Error> for AuthError {
    fn from(e: reqwest::Error) -> Self {
        AuthError(e.to_string())
    }
}

// Fertige Minecraft-Anmeldedaten. profile_id ist die UUID als 32-stelliger Hex-String
// OHNE Bindestriche (so wie Mojang sie liefert und der Session-Join sie erwartet).
#[derive(Debug, Clone)]
pub struct Credentials {
    pub access_token: String,
    pub profile_id: String,
    pub username: String,
}

// Rohe Microsoft-Tokens. refresh_token wird gespeichert, um beim naechsten Start ohne
// erneute Browser-Anmeldung wieder einzusteigen.
#[derive(Debug, Clone)]
pub struct MsaTokens {
    pub access_token: String,
    pub refresh_token: String,
}

// Was dem Nutzer angezeigt wird, damit er sich im Browser anmelden kann.
#[derive(Debug, Clone)]
pub struct DeviceCode {
    pub user_code: String,
    pub verification_uri: String,
    pub message: String,
    // intern fuer poll_for_msa:
    pub device_code: String,
    pub interval: u64,
    pub expires_in: u64,
}

fn client() -> Result<reqwest::Client, AuthError> {
    reqwest::Client::builder()
        .timeout(Duration::from_secs(30))
        // api.minecraftservices.com haengt hinter Cloudflare und weist POSTs OHNE User-Agent
        // mit 403 Forbidden ab (WAF-Regel). reqwest schickt per Default keinen -- deshalb hier
        // explizit setzen, sonst schlaegt login_with_xbox fehl.
        .user_agent(concat!("coral-client/", env!("CARGO_PKG_VERSION")))
        .build()
        .map_err(AuthError::from)
}

// --- Schritt 1: Device-Code anfordern ---------------------------------------------------

pub async fn request_device_code(client_id: &str) -> Result<DeviceCode, AuthError> {
    #[derive(Deserialize)]
    struct Resp {
        device_code: String,
        user_code: String,
        verification_uri: String,
        expires_in: u64,
        interval: u64,
        message: String,
    }
    let resp = client()?.post(DEVICE_CODE_URL).form(&[("client_id", client_id), ("scope", SCOPE)]).send().await?;
    if !resp.status().is_success() {
        let body = resp.text().await.unwrap_or_default();
        return Err(err(format!("Device-Code abgelehnt: {body}")));
    }
    let r: Resp = resp.json().await.map_err(|e| err(format!("Device-Code-Antwort ungueltig: {e}")))?;
    Ok(DeviceCode {
        user_code: r.user_code,
        verification_uri: r.verification_uri,
        message: r.message,
        device_code: r.device_code,
        interval: r.interval.max(1),
        expires_in: r.expires_in,
    })
}

// --- Schritt 2: auf die Browser-Anmeldung pollen ---------------------------------------

// Pollt den Token-Endpunkt, bis der Nutzer sich angemeldet hat (oder der Code ablaeuft).
// Blockiert nicht den Aufrufer-Thread -- laeuft in einem Async-Task.
pub async fn poll_for_msa(client_id: &str, dc: &DeviceCode) -> Result<MsaTokens, AuthError> {
    #[derive(Deserialize)]
    struct Ok_ {
        access_token: String,
        refresh_token: String,
    }
    #[derive(Deserialize)]
    struct Err_ {
        error: String,
    }
    let http = client()?;
    let deadline = std::time::Instant::now() + Duration::from_secs(dc.expires_in);
    let mut interval = dc.interval;
    loop {
        tokio::time::sleep(Duration::from_secs(interval)).await;
        if std::time::Instant::now() > deadline {
            return Err(err("Anmeldung abgelaufen -- bitte neu starten"));
        }
        let resp = http
            .post(TOKEN_URL)
            .form(&[
                ("grant_type", "urn:ietf:params:oauth:grant-type:device_code"),
                ("client_id", client_id),
                ("device_code", &dc.device_code),
            ])
            .send()
            .await?;
        if resp.status().is_success() {
            let ok: Ok_ = resp.json().await.map_err(|e| err(format!("Token-Antwort ungueltig: {e}")))?;
            return Ok(MsaTokens { access_token: ok.access_token, refresh_token: ok.refresh_token });
        }
        let e: Err_ = resp.json().await.unwrap_or(Err_ { error: "unknown".into() });
        match e.error.as_str() {
            // Nutzer hat noch nicht bestaetigt -- weiter pollen.
            "authorization_pending" => continue,
            // Server bittet um laengeres Intervall.
            "slow_down" => {
                interval += 5;
                continue;
            }
            "expired_token" => return Err(err("Anmeldung abgelaufen -- bitte neu starten")),
            "authorization_declined" => return Err(err("Anmeldung im Browser abgelehnt")),
            other => return Err(err(format!("Microsoft-Login fehlgeschlagen: {other}"))),
        }
    }
}

// --- Refresh: gespeichertes refresh_token gegen frische Tokens tauschen -----------------

pub async fn refresh_msa(client_id: &str, refresh_token: &str) -> Result<MsaTokens, AuthError> {
    #[derive(Deserialize)]
    struct Resp {
        access_token: String,
        refresh_token: String,
    }
    let resp = client()?
        .post(TOKEN_URL)
        .form(&[("grant_type", "refresh_token"), ("client_id", client_id), ("refresh_token", refresh_token), ("scope", SCOPE)])
        .send()
        .await?;
    if !resp.status().is_success() {
        let body = resp.text().await.unwrap_or_default();
        return Err(err(format!("Refresh abgelehnt (neu anmelden noetig): {body}")));
    }
    let r: Resp = resp.json().await.map_err(|e| err(format!("Refresh-Antwort ungueltig: {e}")))?;
    Ok(MsaTokens { access_token: r.access_token, refresh_token: r.refresh_token })
}

// --- Schritt 3-6: MSA-Access-Token -> fertige Minecraft-Credentials ---------------------

pub async fn msa_to_minecraft(msa_access_token: &str) -> Result<Credentials, AuthError> {
    let http = client()?;
    let (xbl_token, _uhs) = xbox_live_auth(&http, msa_access_token).await?;
    let (xsts_token, uhs) = xsts_auth(&http, &xbl_token).await?;
    let mc_token = minecraft_login(&http, &uhs, &xsts_token).await?;
    let (id, name) = minecraft_profile(&http, &mc_token).await?;
    Ok(Credentials { access_token: mc_token, profile_id: id, username: name })
}

async fn xbox_live_auth(http: &reqwest::Client, msa_access_token: &str) -> Result<(String, String), AuthError> {
    let body = serde_json::json!({
        "Properties": {
            "AuthMethod": "RPS",
            "SiteName": "user.auth.xboxlive.com",
            "RpsTicket": format!("d={msa_access_token}"),
        },
        "RelyingParty": "http://auth.xboxlive.com",
        "TokenType": "JWT",
    });
    let resp = http.post(XBL_URL).json(&body).send().await?;
    if !resp.status().is_success() {
        return Err(err(format!("Xbox-Live-Login fehlgeschlagen: {}", resp.status())));
    }
    parse_xbox_token(resp).await
}

async fn xsts_auth(http: &reqwest::Client, xbl_token: &str) -> Result<(String, String), AuthError> {
    let body = serde_json::json!({
        "Properties": {
            "SandboxId": "RETAIL",
            "UserTokens": [xbl_token],
        },
        "RelyingParty": "rp://api.minecraftservices.com/",
        "TokenType": "JWT",
    });
    let resp = http.post(XSTS_URL).json(&body).send().await?;
    if resp.status() == reqwest::StatusCode::UNAUTHORIZED {
        // XSTS meldet fehlende/gesperrte Xbox-Konten ueber XErr-Codes.
        #[derive(Deserialize)]
        struct XErr {
            #[serde(rename = "XErr")]
            xerr: Option<u64>,
        }
        let x: XErr = resp.json().await.unwrap_or(XErr { xerr: None });
        let msg = match x.xerr {
            Some(2148916233) => "Dieses Microsoft-Konto hat kein Xbox-Profil (einmal xbox.com oeffnen)",
            Some(2148916235) => "Xbox Live ist in dieser Region nicht verfuegbar",
            Some(2148916238) => "Kinderkonto -- muss einer Familie hinzugefuegt werden",
            _ => "XSTS-Autorisierung abgelehnt",
        };
        return Err(err(msg.to_string()));
    }
    if !resp.status().is_success() {
        return Err(err(format!("XSTS-Autorisierung fehlgeschlagen: {}", resp.status())));
    }
    parse_xbox_token(resp).await
}

// XBL- und XSTS-Antwort haben dasselbe Format: {Token, DisplayClaims:{xui:[{uhs}]}}.
async fn parse_xbox_token(resp: reqwest::Response) -> Result<(String, String), AuthError> {
    #[derive(Deserialize)]
    struct Xui {
        uhs: String,
    }
    #[derive(Deserialize)]
    struct Claims {
        xui: Vec<Xui>,
    }
    #[derive(Deserialize)]
    struct Resp {
        #[serde(rename = "Token")]
        token: String,
        #[serde(rename = "DisplayClaims")]
        display_claims: Claims,
    }
    let r: Resp = resp.json().await.map_err(|e| err(format!("Xbox-Antwort ungueltig: {e}")))?;
    let uhs = r.display_claims.xui.into_iter().next().ok_or_else(|| err("Xbox-Antwort ohne uhs"))?.uhs;
    Ok((r.token, uhs))
}

async fn minecraft_login(http: &reqwest::Client, uhs: &str, xsts_token: &str) -> Result<String, AuthError> {
    #[derive(Deserialize)]
    struct Resp {
        access_token: String,
    }
    let body = serde_json::json!({ "identityToken": format!("XBL3.0 x={uhs};{xsts_token}") });
    // Accept explizit setzen -- manche Cloudflare-WAF-Regeln erwarten es.
    let resp = http.post(MC_LOGIN_URL).header("Accept", "application/json").json(&body).send().await?;
    if !resp.status().is_success() {
        let code = resp.status();
        // Body mitgeben: Cloudflare-403 liefert HTML, ein echter Mojang-Fehler JSON -- so laesst
        // sich WAF-Block von Konto-/Token-Problem unterscheiden, ohne zu raten.
        let text = resp.text().await.unwrap_or_default();
        let snippet: String = text.chars().take(200).collect();
        return Err(err(format!("Minecraft-Services-Login fehlgeschlagen: {code} -- {snippet}")));
    }
    let r: Resp = resp.json().await.map_err(|e| err(format!("MC-Login-Antwort ungueltig: {e}")))?;
    Ok(r.access_token)
}

async fn minecraft_profile(http: &reqwest::Client, mc_token: &str) -> Result<(String, String), AuthError> {
    #[derive(Deserialize)]
    struct Resp {
        id: String,
        name: String,
    }
    let resp = http.get(MC_PROFILE_URL).bearer_auth(mc_token).send().await?;
    if resp.status() == reqwest::StatusCode::UNAUTHORIZED {
        return Err(err("Token ungueltig/abgelaufen"));
    }
    if resp.status() == reqwest::StatusCode::NOT_FOUND {
        return Err(err("Dieses Konto besitzt Minecraft (Java Edition) nicht"));
    }
    if !resp.status().is_success() {
        return Err(err(format!("Profil-Abruf fehlgeschlagen: {}", resp.status())));
    }
    let r: Resp = resp.json().await.map_err(|e| err(format!("Profil-Antwort ungueltig: {e}")))?;
    Ok((r.id, r.name))
}

// --- Profilschluessel fuer signierten Chat ----------------------------------------------

// Was Mojang unter /player/certificates ausgibt: ein RSA-Schluesselpaar, das zum Konto
// gehoert, plus Mojangs eigene Signatur ueber den oeffentlichen Teil. Server mit
// enforce-secure-profile=true verlangen genau das -- ohne diesen Schluessel lehnen sie
// jede Chatnachricht mit "chat.disabled.missingProfileKey" ab.
//
// Der oeffentliche Schluessel geht als DER (X.509 SubjectPublicKeyInfo) ins Player-Session-
// Paket, der private bleibt hier und signiert jede Nachricht (s. net::chat_signing).
#[derive(Debug, Clone)]
pub struct ProfileKey {
    // X.509-DER, genau so wie es ins Paket geschrieben wird.
    pub public_key_der: Vec<u8>,
    // Mojangs Signatur ueber (UUID + Ablaufzeit + oeffentlicher Schluessel), Feld
    // "publicKeySignatureV2" -- der Server prueft damit, dass der Schluessel echt ist.
    pub key_signature: Vec<u8>,
    // PKCS#8-DER des privaten Schluessels (Mojang liefert ihn im PEM trotz des Headers
    // "RSA PRIVATE KEY" als PKCS#8).
    pub private_key_der: Vec<u8>,
    pub expires_at_millis: i64,
}

pub async fn fetch_profile_key(mc_access_token: &str) -> Result<ProfileKey, AuthError> {
    #[derive(Deserialize)]
    struct KeyPair {
        #[serde(rename = "privateKey")]
        private_key: String,
        #[serde(rename = "publicKey")]
        public_key: String,
    }
    #[derive(Deserialize)]
    struct Resp {
        #[serde(rename = "keyPair")]
        key_pair: KeyPair,
        #[serde(rename = "publicKeySignatureV2")]
        public_key_signature_v2: String,
        #[serde(rename = "expiresAt")]
        expires_at: String,
    }

    let http = client()?;
    let resp = http.post(PROFILE_KEY_URL).bearer_auth(mc_access_token).header("Content-Length", "0").send().await?;
    if resp.status() == reqwest::StatusCode::UNAUTHORIZED {
        return Err(err("Token ungueltig/abgelaufen -- Profilschluessel nicht abrufbar"));
    }
    if !resp.status().is_success() {
        return Err(err(format!("Profilschluessel-Abruf fehlgeschlagen: {}", resp.status())));
    }
    let r: Resp = resp.json().await.map_err(|e| err(format!("Profilschluessel-Antwort ungueltig: {e}")))?;

    Ok(ProfileKey {
        public_key_der: decode_pem(&r.key_pair.public_key)?,
        key_signature: decode_base64(r.public_key_signature_v2.trim())?,
        private_key_der: decode_pem(&r.key_pair.private_key)?,
        expires_at_millis: parse_iso8601_millis(&r.expires_at)?,
    })
}

// PEM ohne Ruecksicht auf das Label entpacken: Mojang beschriftet den privaten Schluessel
// als "RSA PRIVATE KEY", der Inhalt ist aber PKCS#8 -- ein Parser, der aufs Label achtet,
// wuerde ihn ablehnen.
fn decode_pem(pem: &str) -> Result<Vec<u8>, AuthError> {
    let body: String = pem.lines().filter(|line| !line.starts_with("-----")).collect::<Vec<_>>().join("");
    decode_base64(body.trim())
}

fn decode_base64(text: &str) -> Result<Vec<u8>, AuthError> {
    use base64::Engine;
    base64::engine::general_purpose::STANDARD.decode(text).map_err(|e| err(format!("Base64 im Profilschluessel ungueltig: {e}")))
}

// ISO-8601-Zeitstempel wie "2026-08-01T09:12:33.123456789Z" in Epoch-Millis. Bewusst von
// Hand statt einer Datums-Bibliothek: es ist genau EIN Format (Mojang liefert immer UTC mit
// Z), und die Umrechnung ist der bekannte Tage-seit-Epoche-Algorithmus.
fn parse_iso8601_millis(text: &str) -> Result<i64, AuthError> {
    let bad = || err(format!("Ablaufzeit '{text}' nicht lesbar"));
    let (date, rest) = text.split_once('T').ok_or_else(bad)?;
    let time = rest.trim_end_matches('Z');
    let mut date_parts = date.split('-');
    let year: i64 = date_parts.next().ok_or_else(bad)?.parse().map_err(|_| bad())?;
    let month: i64 = date_parts.next().ok_or_else(bad)?.parse().map_err(|_| bad())?;
    let day: i64 = date_parts.next().ok_or_else(bad)?.parse().map_err(|_| bad())?;

    let (hms, fraction) = time.split_once('.').unwrap_or((time, "0"));
    let mut time_parts = hms.split(':');
    let hour: i64 = time_parts.next().ok_or_else(bad)?.parse().map_err(|_| bad())?;
    let minute: i64 = time_parts.next().ok_or_else(bad)?.parse().map_err(|_| bad())?;
    let second: i64 = time_parts.next().unwrap_or("0").parse().map_err(|_| bad())?;
    // Nur die ersten drei Nachkommastellen sind Millisekunden; Mojang haengt Nanosekunden an.
    let millis: i64 = format!("{fraction:0<3}")[..3].parse().unwrap_or(0);

    Ok((days_from_civil(year, month, day) * 86_400 + hour * 3600 + minute * 60 + second) * 1000 + millis)
}

// Tage seit 1970-01-01 (Howard Hinnants days_from_civil) -- schaltjahrfest ohne Tabelle.
fn days_from_civil(year: i64, month: i64, day: i64) -> i64 {
    let year = if month <= 2 { year - 1 } else { year };
    let era = if year >= 0 { year } else { year - 399 } / 400;
    let year_of_era = year - era * 400;
    let day_of_year = (153 * (if month > 2 { month - 3 } else { month + 9 }) + 2) / 5 + day - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    era * 146_097 + day_of_era - 719_468
}

// --- Manueller Weg: fertiges Minecraft-Access-Token -> Credentials ----------------------

// Fuer das Token-Eingabefenster: der Nutzer fuegt ein bereits gueltiges
// Minecraft-Access-Token ein, wir holen nur noch UUID + Name dazu.
pub async fn credentials_from_token(mc_access_token: &str) -> Result<Credentials, AuthError> {
    let http = client()?;
    let (id, name) = minecraft_profile(&http, mc_access_token).await?;
    Ok(Credentials { access_token: mc_access_token.to_string(), profile_id: id, username: name })
}

// --- Prism/PolyMC/MultiMC: gespeicherte Konten auslesen ---------------------------------

// Ein in einem Launcher gespeichertes Konto: Name + Minecraft-Access-Token (ygg.token).
// Das Token kann abgelaufen sein (~24h) -- vor Gebrauch validiert credentials_from_token es.
#[derive(Debug, Clone)]
pub struct LauncherAccount {
    pub name: String,
    pub uuid: String,
    pub token: String,
}

// Sucht in den ueblichen Prism-/PolyMC-/MultiMC-Pfaden nach accounts.json und liefert die
// darin gespeicherten Konten (Name + Token). Leere Liste = nichts gefunden. Rein lokal, kein
// Netzwerk -- die Gueltigkeit des Tokens prueft erst credentials_from_token.
pub fn launcher_accounts() -> Vec<LauncherAccount> {
    // Windows: Prism legt die Konten unter %APPDATA%\PrismLauncher ab.
    if let Some(appdata) = std::env::var_os("APPDATA").map(PathBuf::from) {
        for path in [appdata.join("PrismLauncher").join("accounts.json"), appdata.join("PolyMC").join("accounts.json")] {
            if let Ok(bytes) = std::fs::read(&path) {
                let accounts = parse_launcher_accounts(&bytes);
                if !accounts.is_empty() {
                    return accounts;
                }
            }
        }
    }
    let Some(home) = std::env::var_os("HOME").map(PathBuf::from) else {
        return Vec::new();
    };
    let candidates = [
        // Prism (Flatpak)
        home.join(".var/app/org.prismlauncher.PrismLauncher/data/PrismLauncher/accounts.json"),
        // Prism (nativ)
        home.join(".local/share/PrismLauncher/accounts.json"),
        // PolyMC / MultiMC (Fallback)
        home.join(".var/app/org.polymc.PolyMC/data/PolyMC/accounts.json"),
        home.join(".local/share/PolyMC/accounts.json"),
        home.join(".local/share/multimc/accounts.json"),
    ];
    for path in candidates {
        if let Ok(bytes) = std::fs::read(&path) {
            let accounts = parse_launcher_accounts(&bytes);
            if !accounts.is_empty() {
                return accounts;
            }
        }
    }
    Vec::new()
}

fn parse_launcher_accounts(bytes: &[u8]) -> Vec<LauncherAccount> {
    let Ok(v) = serde_json::from_slice::<serde_json::Value>(bytes) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    if let Some(arr) = v.get("accounts").and_then(|a| a.as_array()) {
        for acc in arr {
            // Prism-Schema: MC-Token unter ygg.token, Profil unter profile.{name,id}.
            let token = acc.pointer("/ygg/token").and_then(|t| t.as_str()).unwrap_or_default();
            let name = acc.pointer("/profile/name").and_then(|t| t.as_str()).unwrap_or_default();
            let uuid = acc.pointer("/profile/id").and_then(|t| t.as_str()).unwrap_or_default();
            if !token.is_empty() && !name.is_empty() {
                out.push(LauncherAccount { name: name.to_string(), uuid: uuid.to_string(), token: token.to_string() });
            }
        }
    }
    out
}

// --- Server-Handshake: Session-Join -----------------------------------------------------

// Teilt dem Mojang-Session-Server mit, dass wir gerade diesem Server (identifiziert ueber
// den Server-Hash aus SHA-1(serverId + shared_secret + public_key)) beitreten. Der Server
// fragt anschliessend bei Mojang nach und laesst uns nur rein, wenn dieser Eintrag da ist.
// Erwartet 204 No Content.
pub async fn mojang_join(access_token: &str, profile_id: &str, server_hash: &str) -> Result<(), AuthError> {
    let body = serde_json::json!({
        "accessToken": access_token,
        "selectedProfile": profile_id,
        "serverId": server_hash,
    });
    let resp = client()?.post(SESSION_JOIN_URL).json(&body).send().await?;
    if resp.status() == reqwest::StatusCode::NO_CONTENT || resp.status().is_success() {
        Ok(())
    } else {
        let code = resp.status();
        let text = resp.text().await.unwrap_or_default();
        Err(err(format!("Session-Join abgelehnt ({code}): {text}")))
    }
}

#[cfg(test)]
mod profile_key_tests {
    use super::*;

    // Die Ablaufzeit kommt als ISO-8601 mit Nanosekunden. Falsch geparst wuerde der Server
    // den Schluessel als abgelaufen ablehnen ("expired_public_key"), deshalb hier gegen
    // bekannte Werte gepruefen.
    #[test]
    fn iso8601_wird_zu_epoch_millis() {
        assert_eq!(parse_iso8601_millis("1970-01-01T00:00:00Z").unwrap(), 0);
        assert_eq!(parse_iso8601_millis("2000-01-01T00:00:00Z").unwrap(), 946_684_800_000);
        // Schaltjahr-Grenze und Nanosekunden-Anhang (nur die ersten drei Stellen zaehlen)
        assert_eq!(parse_iso8601_millis("2024-02-29T12:34:56.789123456Z").unwrap(), 1_709_210_096_789);
    }

    // PEM-Label ignorieren: Mojang beschriftet PKCS#8 als "RSA PRIVATE KEY".
    #[test]
    fn pem_wird_unabhaengig_vom_label_entpackt() {
        let pem = "-----BEGIN RSA PRIVATE KEY-----\nAAECAwQ=\n-----END RSA PRIVATE KEY-----\n";
        assert_eq!(decode_pem(pem).unwrap(), vec![0, 1, 2, 3, 4]);
    }
}

// --- Spieler-Skins ---
//
// Der Server schickt in der Spielerliste eine "textures"-Property: Base64 mit JSON darin,
// darin die Skin-URL auf textures.minecraft.net und (nur bei Alex-Modellen) ein
// metadata.model = "slim". Struktur aus einem echten Paket:
//
//   {"timestamp":..,"profileId":"..","profileName":"..","signatureRequired":true,
//    "textures":{"SKIN":{"url":"https://textures.minecraft.net/texture/<hash>",
//                        "metadata":{"model":"slim"}},
//                "CAPE":{"url":".."}}}
//
// Der Wert ist SIGNIERT (die Signatur steht daneben im Paket) -- wir pruefen sie nicht:
// eine gefaelschte Property kann hier nur ein falsches Bild anzeigen, und die URL zeigt
// ohnehin fest auf Mojangs Textur-Host (s. is_mojang_texture_url).

#[derive(Debug, Clone, PartialEq)]
pub struct SkinRef {
    pub url: String,
    // true = Alex-Proportionen (3 Pixel breite Arme), false = Steve.
    pub slim: bool,
    // Umhang, falls das Konto einen hat -- derselbe Host-Filter wie beim Skin. Die meisten
    // Konten haben keinen, deshalb Option und kein eigener Rueckgabetyp.
    pub cape_url: Option<String>,
}

// Die Skin-Referenz aus der Base64-Property herausziehen. None, wenn die Property kein
// gueltiges Base64/JSON ist oder gar keinen SKIN-Eintrag hat (z.B. ein Spieler ohne Skin).
pub fn parse_textures_property(property: &str) -> Option<SkinRef> {
    use base64::Engine;
    let json = base64::engine::general_purpose::STANDARD.decode(property.trim()).ok()?;
    let value: serde_json::Value = serde_json::from_slice(&json).ok()?;
    let skin = value.get("textures")?.get("SKIN")?;
    let url = skin.get("url")?.as_str()?.to_string();
    if !is_mojang_texture_url(&url) {
        return None;
    }
    let slim = skin.get("metadata").and_then(|m| m.get("model")).and_then(|m| m.as_str()) == Some("slim");
    // Ein Umhang ist optional: fehlt er oder zeigt er auf einen fremden Host, bleibt es beim
    // Skin. Ein kaputter CAPE-Eintrag darf nicht den ganzen Skin verwerfen.
    let cape_url = value
        .get("textures")
        .and_then(|t| t.get("CAPE"))
        .and_then(|c| c.get("url"))
        .and_then(|u| u.as_str())
        .filter(|u| is_mojang_texture_url(u))
        .map(|u| u.to_string());
    Some(SkinRef { url, slim, cape_url })
}

// Nur Mojangs Textur-Host. Ohne diese Pruefung waere die Skin-URL eine vom Server frei
// waehlbare Adresse, die dieser Client dann brav abruft -- ein bequemer Weg, die IP eines
// Spielers an einen beliebigen Server zu schicken oder ihn als Lastgenerator zu benutzen.
fn is_mojang_texture_url(url: &str) -> bool {
    let rest = url.strip_prefix("https://").or_else(|| url.strip_prefix("http://"));
    let Some(rest) = rest else { return false };
    let host = rest.split('/').next().unwrap_or("");
    host == "textures.minecraft.net"
}

// Skin-PNG laden. Der Aufrufer dekodiert das Bild (s. assets) -- hier nur der Abruf, damit
// dieses Crate keine Bildbibliothek braucht.
pub async fn fetch_skin_png(url: &str) -> Result<Vec<u8>, String> {
    if !is_mojang_texture_url(url) {
        return Err(format!("unerlaubter Skin-Host: {url}"));
    }
    let response = reqwest::get(url).await.map_err(|e| format!("Skin-Abruf: {e}"))?;
    if !response.status().is_success() {
        return Err(format!("Skin-Abruf: HTTP {}", response.status()));
    }
    let bytes = response.bytes().await.map_err(|e| format!("Skin-Daten: {e}"))?;
    // Ein Vanilla-Skin ist 64x64 RGBA, also gut 16 KB als PNG. Eine Obergrenze verhindert,
    // dass ein manipulierter Host beliebig viel Speicher belegt.
    if bytes.len() > 1024 * 1024 {
        return Err(format!("Skin zu gross: {} Byte", bytes.len()));
    }
    Ok(bytes.to_vec())
}

// Obergrenze fuer ein Server-Resourcepack, dieselbe wie bei Vanilla (250 MiB).
pub const MAX_RESOURCE_PACK_BYTES: usize = 250 * 1024 * 1024;

/// Server-Resourcepack laden. Die Kopfzeilen sind die, die Vanilla mitschickt -- manche
/// Pack-Hosts liefern nur an Minecraft-Clients aus oder waehlen das Pack nach Version/Format.
/// Rueckgabe: die Bytes und ihr SHA-1 als Hex (kleingeschrieben).
pub async fn fetch_resource_pack(url: &str, username: &str, uuid_hex: &str, version: &str, protocol: i32, pack_format: u32) -> Result<(Vec<u8>, String), String> {
    use sha1::{Digest, Sha1};
    let http = reqwest::Client::builder()
        .timeout(Duration::from_secs(300))
        .user_agent(format!("Minecraft Java/{version}"))
        .build()
        .map_err(|e| format!("HTTP: {e}"))?;
    let mut response = http
        .get(url)
        .header("X-Minecraft-Username", username)
        .header("X-Minecraft-UUID", uuid_hex)
        .header("X-Minecraft-Version", version)
        .header("X-Minecraft-Version-ID", version)
        .header("X-Minecraft-Pack-Format", pack_format.to_string())
        .header("X-Minecraft-Protocol", protocol.to_string())
        .send()
        .await
        .map_err(|e| format!("Abruf: {e}"))?;
    if !response.status().is_success() {
        return Err(format!("HTTP {}", response.status()));
    }
    if response.content_length().is_some_and(|l| l as usize > MAX_RESOURCE_PACK_BYTES) {
        return Err("Pack zu gross".into());
    }
    let mut bytes = Vec::new();
    while let Some(chunk) = response.chunk().await.map_err(|e| format!("Daten: {e}"))? {
        bytes.extend_from_slice(&chunk);
        if bytes.len() > MAX_RESOURCE_PACK_BYTES {
            return Err("Pack zu gross".into());
        }
    }
    let hash = Sha1::digest(&bytes).iter().map(|b| format!("{b:02x}")).collect();
    Ok((bytes, hash))
}

/// Die Textur-Property eines Profils nachschlagen, das nur Name oder UUID traegt -- wie
/// Vanillas `ProfileResolver` fuer einen Spielerkopf ohne Skin-Daten (ResolvableProfile.Dynamic):
/// zum Namen erst die UUID (api.mojang.com), dann das Profil mit Properties vom Session-Server.
///
/// `Ok(None)`: das Profil gibt es nicht oder es hat keine Textur -- kein Fehler, der Kopf zeigt
/// dann wie in Vanilla den Standard-Skin.
pub async fn resolve_profile_textures(uuid: Option<[u8; 16]>, name: Option<String>) -> Result<Option<String>, String> {
    let hex = match (uuid, name) {
        (Some(u), _) => u.iter().map(|b| format!("{b:02x}")).collect::<String>(),
        (None, Some(name)) => {
            // Nur gueltige Spielernamen -- der Name kommt vom Server und landet in einer URL.
            if name.is_empty() || name.len() > 16 || !name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
                return Ok(None);
            }
            let antwort = reqwest::get(format!("https://api.mojang.com/users/profiles/minecraft/{name}")).await.map_err(|e| format!("Profil-Suche: {e}"))?;
            if antwort.status() == reqwest::StatusCode::NOT_FOUND || antwort.status() == reqwest::StatusCode::NO_CONTENT {
                return Ok(None);
            }
            if !antwort.status().is_success() {
                return Err(format!("Profil-Suche: HTTP {}", antwort.status()));
            }
            let wert: serde_json::Value = antwort.json().await.map_err(|e| format!("Profil-Suche: {e}"))?;
            match wert.get("id").and_then(|v| v.as_str()) {
                Some(id) if id.len() == 32 && id.chars().all(|c| c.is_ascii_hexdigit()) => id.to_string(),
                _ => return Ok(None),
            }
        }
        (None, None) => return Ok(None),
    };
    let antwort = reqwest::get(format!("https://sessionserver.mojang.com/session/minecraft/profile/{hex}")).await.map_err(|e| format!("Profil-Abruf: {e}"))?;
    if antwort.status() == reqwest::StatusCode::NOT_FOUND || antwort.status() == reqwest::StatusCode::NO_CONTENT {
        return Ok(None);
    }
    if !antwort.status().is_success() {
        return Err(format!("Profil-Abruf: HTTP {}", antwort.status()));
    }
    let wert: serde_json::Value = antwort.json().await.map_err(|e| format!("Profil-Abruf: {e}"))?;
    Ok(textures_from_profile_json(&wert))
}

// Die "textures"-Property aus einer Profil-Antwort des Session-Servers.
fn textures_from_profile_json(wert: &serde_json::Value) -> Option<String> {
    wert.get("properties")?
        .as_array()?
        .iter()
        .find(|p| p.get("name").and_then(|n| n.as_str()) == Some("textures"))
        .and_then(|p| p.get("value")?.as_str())
        .map(str::to_string)
}

#[cfg(test)]
mod skin_tests {
    use super::*;
    use base64::Engine;

    fn property(json: &str) -> String {
        base64::engine::general_purpose::STANDARD.encode(json)
    }

    #[test]
    fn textur_property_aus_der_profil_antwort() {
        let wert: serde_json::Value = serde_json::from_str(
            r#"{"id":"069a79f444e94726a5befca90e38aaf5","name":"Notch","properties":[{"name":"textures","value":"abc"}]}"#,
        )
        .unwrap();
        assert_eq!(textures_from_profile_json(&wert).as_deref(), Some("abc"));
        let ohne: serde_json::Value = serde_json::from_str(r#"{"id":"x","name":"y","properties":[]}"#).unwrap();
        assert_eq!(textures_from_profile_json(&ohne), None);
    }

    #[test]
    fn liest_url_und_classic_modell() {
        let p = property(r#"{"textures":{"SKIN":{"url":"https://textures.minecraft.net/texture/abc123"}}}"#);
        let skin = parse_textures_property(&p).expect("muss lesbar sein");
        assert_eq!(skin.url, "https://textures.minecraft.net/texture/abc123");
        assert!(!skin.slim, "ohne metadata.model ist es das Steve-Modell");
    }

    #[test]
    fn erkennt_slim_modell() {
        let p = property(r#"{"textures":{"SKIN":{"url":"https://textures.minecraft.net/texture/x","metadata":{"model":"slim"}}}}"#);
        assert!(parse_textures_property(&p).unwrap().slim);
    }

    // Die Struktur, die der Session-Server WIRKLICH liefert -- abgeglichen mit einem echten
    // Abruf von sessionserver.mojang.com. Zwei Dinge weichen von dem ab, was man annehmen
    // wuerde und was die Tests darueber pruefen:
    //   * die URL kommt als "http://", nicht https,
    //   * neben SKIN steht ein CAPE-Eintrag, der nicht dazwischenkommen darf.
    #[test]
    fn echte_session_server_struktur() {
        let p = property(
            r#"{
              "timestamp": 1785404472388,
              "profileId": "00000000000000000000000000000000",
              "profileName": "Spieler",
              "textures": {
                "SKIN": {
                  "url": "http://textures.minecraft.net/texture/e82f819c55308052",
                  "metadata": { "model": "slim" }
                },
                "CAPE": { "url": "http://textures.minecraft.net/texture/28de4a81688ad18b" }
              }
            }"#,
        );
        let skin = parse_textures_property(&p).expect("echte Struktur muss lesbar sein");
        assert_eq!(skin.url, "http://textures.minecraft.net/texture/e82f819c55308052");
        assert!(skin.slim, "metadata.model=slim muss das schmale Modell ergeben");
        assert_eq!(skin.cape_url.as_deref(), Some("http://textures.minecraft.net/texture/28de4a81688ad18b"));
    }

    // Ohne CAPE-Eintrag gibt es keinen Umhang -- und das ist der Normalfall, kein Fehler.
    #[test]
    fn ohne_cape_eintrag_kein_umhang() {
        let p = property(r#"{"textures":{"SKIN":{"url":"http://textures.minecraft.net/texture/x"}}}"#);
        assert_eq!(parse_textures_property(&p).unwrap().cape_url, None);
    }

    // Ein CAPE auf einem fremden Host wird verworfen -- aber NUR der Umhang. Der Skin muss
    // trotzdem ankommen, sonst koennte ein Server mit einem manipulierten CAPE-Eintrag jedem
    // Spieler den Skin abschalten.
    #[test]
    fn fremder_cape_host_verwirft_nur_den_umhang() {
        let p = property(
            r#"{"textures":{"SKIN":{"url":"http://textures.minecraft.net/texture/x"},
                            "CAPE":{"url":"https://example.com/cape.png"}}}"#,
        );
        let skin = parse_textures_property(&p).expect("der Skin muss erhalten bleiben");
        assert_eq!(skin.url, "http://textures.minecraft.net/texture/x");
        assert_eq!(skin.cape_url, None, "fremder Umhang-Host muss weg sein");
    }

    // Ein Spieler ohne Skin hat keinen SKIN-Eintrag -- das ist kein Fehler.
    #[test]
    fn ohne_skin_eintrag_none() {
        let p = property(r#"{"textures":{"CAPE":{"url":"https://textures.minecraft.net/texture/y"}}}"#);
        assert!(parse_textures_property(&p).is_none());
    }

    // Fremde Hosts werden abgelehnt -- sonst koennte ein Server diesen Client auf eine
    // beliebige Adresse schicken (s. is_mojang_texture_url).
    #[test]
    fn fremder_host_wird_abgelehnt() {
        for url in [
            "https://example.com/skin.png",
            "https://textures.minecraft.net.example.com/x",
            "http://evil/textures.minecraft.net/x",
            "file:///etc/passwd",
        ] {
            let p = property(&format!(r#"{{"textures":{{"SKIN":{{"url":"{url}"}}}}}}"#));
            assert!(parse_textures_property(&p).is_none(), "'{url}' haette abgelehnt werden muessen");
        }
        // Und der Abruf selbst prueft nochmal, falls die URL anders hereinkommt.
        let err = pollster_block(fetch_skin_png("https://example.com/x"));
        assert!(err.is_err());
    }

    // Kaputtes Base64 oder kaputtes JSON: None statt Panik.
    #[test]
    fn unlesbare_property_gibt_none() {
        assert!(parse_textures_property("das ist kein base64!!!").is_none());
        assert!(parse_textures_property(&property("{kein json")).is_none());
        assert!(parse_textures_property("").is_none());
    }

    // Winziger Blocking-Helfer, damit der Host-Test ohne Tokio-Runtime laeuft.
    fn pollster_block<F: std::future::Future>(future: F) -> F::Output {
        let runtime = tokio::runtime::Builder::new_current_thread().enable_all().build().expect("Runtime");
        runtime.block_on(future)
    }
}
