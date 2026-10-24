// Modul "Delay": kuenstliche Verzoegerung auf der Leitung, zum Testen mit hohem Ping. Sitzt
// direkt am Socket, unter Verschluesselung und Framing -- jedes Byte kommt so viel spaeter an
// und geht so viel spaeter raus, auch die Antworten des Netz-Tasks (Pong, KeepAlive). Nur fuer
// Server auf diesem Rechner und anticheat-test.com, nicht fuer echte Server.
//
// Die Schicht wird beim Verbinden eingesetzt, sobald der Server erlaubt ist; die Verzoegerung
// selbst liest sie je Schwung Bytes aus LAG_MS, das Modul laesst sich also mitten in der
// Verbindung an- und abschalten.

use std::sync::atomic::{AtomicU32, Ordering};
use std::time::Duration;

use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt, DuplexStream};
use tokio::net::TcpStream;
use tokio::sync::mpsc;
use tokio::time::Instant;

// Verzoegerung je Richtung in ms, 0 = aus. Setzt die App (s. set_delay).
static LAG_MS: AtomicU32 = AtomicU32::new(0);

pub fn set_delay(ms: u32) {
    LAG_MS.store(ms, Ordering::Relaxed);
}

fn delay() -> Duration {
    Duration::from_millis(LAG_MS.load(Ordering::Relaxed) as u64)
}

pub(crate) fn allowed(host: &str) -> bool {
    let host = host.trim_end_matches('.').to_ascii_lowercase();
    crate::is_local_address(&host) || host == "anticheat-test.com" || host.ends_with(".anticheat-test.com")
}

// Was Connection als Stream haelt: der Socket selbst oder die Verzoegerungsschicht davor.
pub(crate) trait Io: AsyncRead + AsyncWrite + Unpin + Send {}
impl<T: AsyncRead + AsyncWrite + Unpin + Send> Io for T {}

// Schiebt die Verzoegerungsschicht zwischen Socket und Verbindung: zwei Richtungen, je ein
// Leser, der die Bytes mit Faelligkeit einreiht, und ein Schreiber, der sie in Reihenfolge
// erst dann weitergibt. Endet eine Seite, enden alle vier Tasks.
pub(crate) fn wrap(stream: TcpStream) -> DuplexStream {
    let (innen, aussen) = tokio::io::duplex(1 << 20);
    let (sock_r, sock_w) = stream.into_split();
    let (app_r, app_w) = tokio::io::split(aussen);
    richtung(sock_r, app_w);
    richtung(app_r, sock_w);
    innen
}

fn richtung<R, W>(mut von: R, mut nach: W)
where
    R: AsyncRead + Unpin + Send + 'static,
    W: AsyncWrite + Unpin + Send + 'static,
{
    let (tx, mut rx) = mpsc::unbounded_channel::<(Instant, Vec<u8>)>();
    tokio::spawn(async move {
        let mut buf = vec![0u8; 16384];
        loop {
            match von.read(&mut buf).await {
                Ok(0) | Err(_) => break,
                Ok(n) => {
                    if tx.send((Instant::now() + delay(), buf[..n].to_vec())).is_err() {
                        break;
                    }
                }
            }
        }
    });
    tokio::spawn(async move {
        while let Some((faellig, bytes)) = rx.recv().await {
            tokio::time::sleep_until(faellig).await;
            if nach.write_all(&bytes).await.is_err() {
                break;
            }
        }
        let _ = nach.shutdown().await;
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nur_lokal_und_actest() {
        assert!(allowed("localhost"));
        assert!(allowed("127.0.0.1"));
        assert!(allowed("anticheat-test.com"));
        assert!(allowed("modern.anticheat-test.com"));
        assert!(!allowed("play.pvphq.com"));
        assert!(!allowed("fakeanticheat-test.com"));
    }

    // Echo-Server ueber die Schicht: die Antwort kommt erst nach Hin- und Rueckweg, also nach
    // mindestens der doppelten Verzoegerung, und die Bytes kommen unveraendert in Reihenfolge an.
    #[tokio::test]
    async fn verzoegert_beide_richtungen() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            let (mut s, _) = listener.accept().await.unwrap();
            let mut buf = [0u8; 64];
            loop {
                let n = s.read(&mut buf).await.unwrap();
                if n == 0 {
                    break;
                }
                s.write_all(&buf[..n]).await.unwrap();
            }
        });
        set_delay(100);
        let mut s = wrap(TcpStream::connect(addr).await.unwrap());
        let start = std::time::Instant::now();
        s.write_all(b"hallo").await.unwrap();
        s.write_all(b" welt").await.unwrap();
        let mut got = [0u8; 10];
        s.read_exact(&mut got).await.unwrap();
        let rtt = start.elapsed();
        set_delay(0);
        assert_eq!(&got, b"hallo welt");
        assert!(rtt >= Duration::from_millis(200), "{rtt:?}");
        assert!(rtt < Duration::from_millis(400), "{rtt:?}");
    }
}
