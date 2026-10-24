// Microsoft-/Minecraft-Authentifizierung, modular und ohne Abhaengigkeit auf net/app.
//
// Wege zum fertigen `Credentials` (Access-Token + Profil-UUID + Name), mit dem man
// Online-Mode-Servern beitreten kann:
//   0. Browser-Login (`start_browser_login` -> Nutzer waehlt im Browser sein Konto ->
//      `finish_browser_login` -> `msa_to_minecraft`): Authorization-Code mit PKCE, Microsoft
//      leitet auf einen lokalen Empfaenger (http://localhost:<port>) zurueck. Zeigt jedes Mal
//      die Kontoauswahl -- der Device-Code-Flow nimmt dagegen stumm das Konto, mit dem der
//      Browser gerade angemeldet ist.
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
const AUTHORIZE_URL: &str = "https://login.microsoftonline.com/consumers/oauth2/v2.0/authorize";
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

// Anfang der Meldungen, an denen Aufrufer zwei Faelle erkennen: der Session-Server lehnt das
// Token ab (403, meist abgelaufen -> erneuern lohnt), und Mojangs Anmeldelimit (429 -> warten).
const JOIN_FORBIDDEN: &str = "Session-Join abgelehnt (403";
// Weitere Versuche nach einer 5xx-Antwort des Session-Servers (s. mojang_join).
const JOIN_RETRIES: u32 = 2;
const LOGIN_LIMIT: &str = "Mojang-Anmeldelimit erreicht";

impl AuthError {
    pub fn join_forbidden(&self) -> bool {
        self.0.starts_with(JOIN_FORBIDDEN)
    }

    pub fn rate_limited(&self) -> bool {
        self.0.starts_with(LOGIN_LIMIT)
    }
}

// Nach einem 429 von login_with_xbox: bis wann keine Anmeldung mehr versucht wird. Jeder
// weitere Versuch vor Ablauf verlaengert bei Mojang nur die Sperre.
static LOGIN_BLOCKED_UNTIL: std::sync::Mutex<Option<std::time::Instant>> = std::sync::Mutex::new(None);
// Ohne Retry-Header so lange warten.
const LOGIN_LIMIT_FALLBACK: Duration = Duration::from_secs(120);

fn login_limit_error(rest: Duration) -> AuthError {
    let s = rest.as_secs().max(1);
    let wann = if s >= 60 { format!("{} min", s.div_ceil(60)) } else { format!("{s} s") };
    err(format!("{LOGIN_LIMIT} -- in {wann} erneut versuchen"))
}

// Noch gesperrt? Dann der Fehler mit der Restzeit.
fn check_login_limit() -> Result<(), AuthError> {
    let mut sperre = LOGIN_BLOCKED_UNTIL.lock().unwrap_or_else(|e| e.into_inner());
    match *sperre {
        Some(bis) if bis > std::time::Instant::now() => Err(login_limit_error(bis - std::time::Instant::now())),
        Some(_) => {
            *sperre = None;
            Ok(())
        }
        None => Ok(()),
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

// --- Browser-Login: Authorization-Code mit PKCE und lokalem Empfaenger ------------------
//
// In der App-Registrierung muss dafuer unter "Mobilgeraet- und Desktopanwendungen" die
// Umleitungs-URI http://localhost stehen; den Port darf der Client frei waehlen.

// Ein begonnener Browser-Login: `url` im Browser oeffnen, dann `finish_browser_login`.
pub struct BrowserLogin {
    pub url: String,
    listeners: Vec<tokio::net::TcpListener>,
    redirect_uri: String,
    verifier: String,
    state: String,
    client_id: String,
}

// Zufallstext aus den in PKCE erlaubten Zeichen (RFC 7636: A-Z a-z 0-9 - . _ ~).
fn random_token(len: usize) -> String {
    use rand::Rng;
    const ZEICHEN: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-._~";
    let mut rng = rand::rngs::OsRng;
    (0..len).map(|_| ZEICHEN[rng.gen_range(0..ZEICHEN.len())] as char).collect()
}

// code_challenge zur Methode S256: base64url(SHA-256(verifier)) ohne Auffuellzeichen.
fn pkce_challenge(verifier: &str) -> String {
    use base64::Engine;
    use sha2::Digest;
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(sha2::Sha256::digest(verifier.as_bytes()))
}

// Empfaenger auf einem freien Port oeffnen und die Anmelde-Adresse bauen. Ein Fehler hier
// (kein Port frei) heisst: auf den Device-Code-Flow ausweichen.
pub async fn start_browser_login(client_id: &str) -> Result<BrowserLogin, AuthError> {
    let v4 = tokio::net::TcpListener::bind(("127.0.0.1", 0)).await.map_err(|e| err(format!("Kein lokaler Port fuer die Anmeldung: {e}")))?;
    let port = v4.local_addr().map_err(|e| err(e.to_string()))?.port();
    let mut listeners = vec![v4];
    // "localhost" loest der Browser oft zuerst zu ::1 auf -- derselbe Port dort auch, wenn
    // es IPv6 gibt (sonst reicht IPv4, die Browser weichen darauf aus).
    if let Ok(v6) = tokio::net::TcpListener::bind(("::1", port)).await {
        listeners.push(v6);
    }
    let redirect_uri = format!("http://localhost:{port}");
    let verifier = random_token(64);
    let state = random_token(32);
    let url = reqwest::Url::parse_with_params(
        AUTHORIZE_URL,
        &[
            ("client_id", client_id),
            ("response_type", "code"),
            ("redirect_uri", redirect_uri.as_str()),
            ("response_mode", "query"),
            ("scope", SCOPE),
            ("state", state.as_str()),
            ("code_challenge", pkce_challenge(&verifier).as_str()),
            ("code_challenge_method", "S256"),
            // Immer die Kontoauswahl zeigen, auch wenn der Browser schon angemeldet ist.
            ("prompt", "select_account"),
        ],
    )
    .map_err(|e| err(e.to_string()))?
    .to_string();
    Ok(BrowserLogin { url, listeners, redirect_uri, verifier, state, client_id: client_id.to_string() })
}

// Farben der Seite nach der Rueckleitung (sRGB 0xRRGGBB) -- der Client gibt sein Theme mit,
// damit sie aussieht wie seine eigenen Fenster.
#[derive(Clone, Copy, Debug)]
pub struct PageTheme {
    pub background: u32,
    pub panel: u32,
    pub border: u32,
    pub text: u32,
    pub text_2: u32,
    pub success: u32,
    pub danger: u32,
}

impl Default for PageTheme {
    fn default() -> Self {
        PageTheme { background: 0x16181d, panel: 0x1e2128, border: 0x2c3039, text: 0xe8e8ea, text_2: 0xa9adb6, success: 0x4cc38a, danger: 0xe5484d }
    }
}

// Seite, die der Browser nach der Rueckleitung zeigt: eine Karte wie die Dialoge des Clients,
// mit Haken bzw. Kreuz in der Erfolgs- bzw. Fehlerfarbe.
fn browser_page(deutsch: bool, ok: bool, theme: &PageTheme, titel: &str, text: &str) -> String {
    let lang = if deutsch { "de" } else { "en" };
    let (zeichen, farbe) = if ok { ("&#10003;", theme.success) } else { ("&#10005;", theme.danger) };
    let PageTheme { background, panel, border, text: text_1, text_2, .. } = *theme;
    format!(
        "<!doctype html><html lang=\"{lang}\"><head><meta charset=\"utf-8\"><title>Coral Client</title>\
         <style>body{{font-family:system-ui,sans-serif;background:#{background:06x};color:#{text_1:06x};display:flex;\
         align-items:center;justify-content:center;height:100vh;margin:0}}\
         .karte{{background:#{panel:06x};border:1px solid #{border:06x};border-radius:8px;padding:32px 40px;\
         max-width:420px;text-align:center;box-shadow:0 12px 32px rgba(0,0,0,.4)}}\
         .zeichen{{width:48px;height:48px;line-height:48px;margin:0 auto 16px;border-radius:50%;\
         border:2px solid #{farbe:06x};color:#{farbe:06x};font-size:24px}}\
         h1{{font-size:22px;margin:0 0 8px}}p{{margin:0;color:#{text_2:06x};line-height:1.5}}</style></head>\
         <body><div class=\"karte\"><div class=\"zeichen\">{zeichen}</div><h1>{titel}</h1><p>{text}</p></div></body></html>"
    )
}

// Die Seite spricht die Sprache des Browsers: Deutsch, wenn er Deutsch an erster Stelle
// nennt (Accept-Language), sonst Englisch.
fn prefers_german(request: &str) -> bool {
    request
        .lines()
        .find_map(|z| z.split_once(':').filter(|(k, _)| k.trim().eq_ignore_ascii_case("accept-language")).map(|(_, v)| v))
        .and_then(|v| v.split(',').next())
        .is_some_and(|erste| erste.trim().to_ascii_lowercase().starts_with("de"))
}

// Warum die Rueckleitung keinen Code brachte.
#[derive(Debug, PartialEq)]
enum RedirectError {
    // Im Browser abgebrochen (access_denied).
    Cancelled,
    // state passt nicht: fremde Rueckleitung (anderer Login, untergeschobener Code).
    ForeignState,
    // Sonstiger Fehler, Microsofts Beschreibung (die kommt schon in der Sprache des Browsers).
    Microsoft(String),
}

impl RedirectError {
    fn text(&self, deutsch: bool) -> String {
        match (self, deutsch) {
            (RedirectError::Cancelled, true) => "Anmeldung im Browser abgebrochen".into(),
            (RedirectError::Cancelled, false) => "Sign-in was cancelled in the browser".into(),
            (RedirectError::ForeignState, true) => "Antwort gehoert nicht zu dieser Anmeldung".into(),
            (RedirectError::ForeignState, false) => "This response does not belong to this sign-in".into(),
            (RedirectError::Microsoft(m), _) => m.clone(),
        }
    }
}

// Was die Rueckleitung mitbringt: Ok(code) oder der Fehler.
// None = keine Rueckleitung (z. B. /favicon.ico), weiter warten.
fn parse_redirect(request_line: &str, state: &str) -> Option<Result<String, RedirectError>> {
    let pfad = request_line.strip_prefix("GET ")?.split(' ').next()?;
    let url = reqwest::Url::parse(&format!("http://localhost{pfad}")).ok()?;
    let mut code = None;
    let (mut fehler, mut beschreibung, mut zurueck) = (None, None, None);
    for (k, v) in url.query_pairs() {
        match k.as_ref() {
            "code" => code = Some(v.into_owned()),
            "error" => fehler = Some(v.into_owned()),
            "error_description" => beschreibung = Some(v.into_owned()),
            "state" => zurueck = Some(v.into_owned()),
            _ => {}
        }
    }
    if code.is_none() && fehler.is_none() {
        return None;
    }
    // Fremde Rueckleitung (anderer Login, untergeschobener Code): nicht annehmen.
    if zurueck.as_deref() != Some(state) {
        return Some(Err(RedirectError::ForeignState));
    }
    match (code, fehler) {
        (Some(c), None) => Some(Ok(c)),
        (_, Some(f)) => Some(Err(match f.as_str() {
            "access_denied" => RedirectError::Cancelled,
            _ => RedirectError::Microsoft(beschreibung.unwrap_or(f)),
        })),
        (None, None) => None,
    }
}

// Auf die Rueckleitung warten (hoechstens 10 Minuten), dem Browser eine Seite in den Farben
// von `theme` zeigen und den Code gegen Tokens tauschen.
pub async fn finish_browser_login(login: BrowserLogin, theme: PageTheme) -> Result<MsaTokens, AuthError> {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let warten = async {
        loop {
            let Ok(mut sock) = accept_any(&login.listeners).await else { continue };
            // Anfragezeile lesen (mehr braucht es nicht; die Rueckleitung ist ein GET).
            let mut buf = Vec::new();
            let mut stueck = [0u8; 2048];
            while !buf.windows(4).any(|w| w == b"\r\n\r\n") && buf.len() < 16 * 1024 {
                match tokio::time::timeout(Duration::from_secs(5), sock.read(&mut stueck)).await {
                    Ok(Ok(n)) if n > 0 => buf.extend_from_slice(&stueck[..n]),
                    _ => break,
                }
            }
            let text = String::from_utf8_lossy(&buf);
            let zeile = text.lines().next().unwrap_or("");
            let ergebnis = parse_redirect(zeile, &login.state);
            let de = prefers_german(&text);
            let (status, seite) = match &ergebnis {
                None => ("404 Not Found", String::new()),
                Some(Ok(_)) if de => ("200 OK", browser_page(de, true, &theme, "Angemeldet", "Du kannst dieses Fenster schlie&szlig;en und zu Coral Client zur&uuml;ckkehren.")),
                Some(Ok(_)) => ("200 OK", browser_page(de, true, &theme, "Signed in", "You can close this window and return to Coral Client.")),
                Some(Err(e)) => {
                    let titel = if de { "Anmeldung fehlgeschlagen" } else { "Sign-in failed" };
                    ("200 OK", browser_page(de, false, &theme, titel, &e.text(de).replace('&', "&amp;").replace('<', "&lt;")))
                }
            };
            let antwort = format!(
                "HTTP/1.1 {status}\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{seite}",
                seite.len()
            );
            let _ = sock.write_all(antwort.as_bytes()).await;
            let _ = sock.shutdown().await;
            // Im Log des Clients deutsch wie die uebrigen Auth-Fehler.
            if let Some(e) = ergebnis {
                return e.map_err(|e| e.text(true));
            }
        }
    };
    let code = match tokio::time::timeout(Duration::from_secs(600), warten).await {
        Ok(Ok(c)) => c,
        Ok(Err(e)) => return Err(err(e)),
        Err(_) => return Err(err("Anmeldung abgelaufen -- bitte neu starten")),
    };
    #[derive(Deserialize)]
    struct Resp {
        access_token: String,
        refresh_token: String,
    }
    let resp = client()?
        .post(TOKEN_URL)
        .form(&[
            ("grant_type", "authorization_code"),
            ("client_id", login.client_id.as_str()),
            ("code", code.as_str()),
            ("redirect_uri", login.redirect_uri.as_str()),
            ("code_verifier", login.verifier.as_str()),
            ("scope", SCOPE),
        ])
        .send()
        .await?;
    if !resp.status().is_success() {
        let body = resp.text().await.unwrap_or_default();
        return Err(err(format!("Code-Tausch abgelehnt: {body}")));
    }
    let r: Resp = resp.json().await.map_err(|e| err(format!("Token-Antwort ungueltig: {e}")))?;
    Ok(MsaTokens { access_token: r.access_token, refresh_token: r.refresh_token })
}

// Die naechste Verbindung auf einem der Empfaenger (IPv4 oder IPv6).
async fn accept_any(listeners: &[tokio::net::TcpListener]) -> std::io::Result<tokio::net::TcpStream> {
    std::future::poll_fn(|cx| {
        for l in listeners {
            if let std::task::Poll::Ready(r) = l.poll_accept(cx) {
                return std::task::Poll::Ready(r.map(|(sock, _)| sock));
            }
        }
        std::task::Poll::Pending
    })
    .await
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
    check_login_limit()?;
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
    if resp.status() == reqwest::StatusCode::TOO_MANY_REQUESTS {
        let warten = resp
            .headers()
            .get(reqwest::header::RETRY_AFTER)
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.trim().parse::<u64>().ok())
            .map_or(LOGIN_LIMIT_FALLBACK, Duration::from_secs);
        *LOGIN_BLOCKED_UNTIL.lock().unwrap_or_else(|e| e.into_inner()) = Some(std::time::Instant::now() + warten);
        return Err(login_limit_error(warten));
    }
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
//
// Mojangs Session-Server antwortet gelegentlich mit 503 (gemessen beim Transfer auf pvphq und
// beim normalen Beitreten auf actest), beim naechsten Versuch geht es. Bei 5xx deshalb noch
// zweimal nach kurzer Pause, bevor der Fehler durchgeht -- der Server wartet auf die
// Encryption-Response bis zu 30 s.
pub async fn mojang_join(access_token: &str, profile_id: &str, server_hash: &str) -> Result<(), AuthError> {
    let body = serde_json::json!({
        "accessToken": access_token,
        "selectedProfile": profile_id,
        "serverId": server_hash,
    });
    let mut versuch = 0;
    let resp = loop {
        let resp = client()?.post(SESSION_JOIN_URL).json(&body).send().await?;
        if !resp.status().is_server_error() || versuch == JOIN_RETRIES {
            break resp;
        }
        versuch += 1;
        tokio::time::sleep(Duration::from_millis(500 * versuch as u64)).await;
    };
    if resp.status() == reqwest::StatusCode::NO_CONTENT || resp.status().is_success() {
        Ok(())
    } else {
        let code = resp.status();
        let text = resp.text().await.unwrap_or_default();
        let text = text.trim();
        if code == reqwest::StatusCode::FORBIDDEN {
            return Err(err(format!("{JOIN_FORBIDDEN} Forbidden): {text}")));
        }
        Err(err(format!("Session-Join abgelehnt ({code}): {text}")))
    }
}

#[cfg(test)]
mod browser_login_tests {
    use super::*;

    #[test]
    fn pkce_challenge_ist_base64url_von_sha256() {
        // Gegenprobe mit Pythons hashlib + base64.urlsafe_b64encode (ohne "=").
        assert_eq!(pkce_challenge("dBjftJeZ4CVP-mJ0kgUEWIq5r8NXZZ3_wwxCqpnh-cO_"), "UjpKb9BMS33vFUyZ7EMdufX64yJLrsmw4oC7FPwRVNc");
        let v = random_token(64);
        assert_eq!(v.len(), 64);
        assert!(v.bytes().all(|b| b.is_ascii_alphanumeric() || b"-._~".contains(&b)));
    }

    #[test]
    fn rueckleitung_wird_gelesen() {
        assert_eq!(parse_redirect("GET /?code=abc&state=xyz HTTP/1.1", "xyz"), Some(Ok("abc".into())));
        assert_eq!(parse_redirect("GET /favicon.ico HTTP/1.1", "xyz"), None);
        assert_eq!(parse_redirect("GET /?code=abc&state=falsch HTTP/1.1", "xyz"), Some(Err(RedirectError::ForeignState)));
        assert_eq!(parse_redirect("GET /?error=access_denied&error_description=x&state=xyz HTTP/1.1", "xyz"), Some(Err(RedirectError::Cancelled)));
    }

    #[test]
    fn seite_spricht_die_sprache_des_browsers() {
        assert!(prefers_german("GET / HTTP/1.1\r\nAccept-Language: de-DE,de;q=0.9,en;q=0.8\r\n"));
        assert!(!prefers_german("GET / HTTP/1.1\r\naccept-language: en-US,de;q=0.5\r\n"));
        assert!(!prefers_german("GET / HTTP/1.1\r\nHost: localhost\r\n"));
    }

    // Der Empfaenger ueber echtes TCP, wie ein Browser: erst /favicon.ico (wird ignoriert),
    // dann die Rueckleitung mit Abbruch -- ueber "localhost", also IPv6 oder IPv4.
    #[tokio::test]
    async fn empfaenger_beantwortet_den_browser() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let login = start_browser_login("id-123").await.expect("Port");
        let port = login.redirect_uri.rsplit(':').next().unwrap().to_string();
        let state = login.state.clone();
        let warten = tokio::spawn(finish_browser_login(login, PageTheme::default()));
        for pfad in ["/favicon.ico".to_string(), format!("/?error=access_denied&state={state}")] {
            let mut sock = tokio::net::TcpStream::connect(format!("localhost:{port}")).await.expect("verbinden");
            sock.write_all(format!("GET {pfad} HTTP/1.1\r\nHost: localhost\r\n\r\n").as_bytes()).await.unwrap();
            let mut antwort = String::new();
            sock.read_to_string(&mut antwort).await.unwrap();
            assert!(antwort.starts_with("HTTP/1.1 "), "{antwort}");
        }
        let e = warten.await.unwrap().expect_err("Abbruch");
        assert!(e.0.contains("abgebrochen"), "{e}");
    }

    #[tokio::test]
    async fn anmelde_adresse_zeigt_die_kontoauswahl() {
        let login = start_browser_login("id-123").await.expect("Port");
        let url = reqwest::Url::parse(&login.url).unwrap();
        let q: std::collections::HashMap<_, _> = url.query_pairs().into_owned().collect();
        assert_eq!(q["prompt"], "select_account");
        assert_eq!(q["code_challenge"], pkce_challenge(&login.verifier));
        assert_eq!(q["redirect_uri"], login.redirect_uri);
        assert!(login.redirect_uri.starts_with("http://localhost:"));
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
    use base64::engine::{DecodePaddingMode, GeneralPurpose, GeneralPurposeConfig};
    use base64::Engine;
    // Mit oder ohne "="-Auffuellung, wie Javas Base64-Decoder in Vanilla: Server-Koepfe (etwa
    // der Golden Head auf pvphq) schicken die Property oft ohne -- streng dekodiert blieb der
    // Kopf fuer immer ohne Skin.
    const LOCKER: GeneralPurpose =
        GeneralPurpose::new(&base64::alphabet::STANDARD, GeneralPurposeConfig::new().with_decode_padding_mode(DecodePaddingMode::Indifferent));
    let json = LOCKER.decode(property.trim()).ok()?;
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

    // Ohne "="-Auffuellung, wie Javas Decoder: die Property des Golden Heads auf pvphq.
    #[test]
    fn property_ohne_auffuellung() {
        let p = "eyJ0ZXh0dXJlcyI6eyJTS0lOIjp7InVybCI6Imh0dHA6Ly90ZXh0dXJlcy5taW5lY3JhZnQubmV0L3RleHR1cmUvNzU2NmYyZjg3Y2NkYzU4ZjY3ODI5ZDQ2Njc3ZjIyYmUyNWRmMTdjMDFlMmQ5NDRlZmE3ZWJlNjI2MjI5ZjgyZSJ9fX0";
        let skin = parse_textures_property(p).expect("ohne Auffuellung lesbar");
        assert_eq!(skin.url, "http://textures.minecraft.net/texture/7566f2f87ccdc58f67829d46677f22be25df17c01e2d944efa7ebe626229f82e");
        assert!(parse_textures_property(&format!("{p}=")).is_some(), "mit Auffuellung ebenso");
    }

    // Winziger Blocking-Helfer, damit der Host-Test ohne Tokio-Runtime laeuft.
    fn pollster_block<F: std::future::Future>(future: F) -> F::Output {
        let runtime = tokio::runtime::Builder::new_current_thread().enable_all().build().expect("Runtime");
        runtime.block_on(future)
    }
}
