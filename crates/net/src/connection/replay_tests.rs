use super::*;

#[test]
#[ignore]
fn wiedergabe_einer_echten_aufnahme() {
    let Some(pfad) = std::env::var_os("CORAL_REPLAY_TEST") else { return };
    let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
    let (tx, rx) = std::sync::mpsc::channel();
    let (_out_tx, out_rx) = tokio::sync::mpsc::unbounded_channel();
    let uhr = std::sync::Arc::new(record::ReplayClock::new(1e12));
    let rotation = std::sync::Arc::new(SharedLook::default());
    let lauf = replay(pfad.into(), uhr, Events::new(tx), out_rx, rotation);
    let _ = rt.block_on(async { tokio::time::timeout(Duration::from_secs(20), lauf).await });
    let (mut chunks, mut verbunden, mut alle) = (0, false, 0);
    let mut fehler = Vec::new();
    while let Ok(e) = rx.try_recv() {
        alle += 1;
        match e {
            ClientEvent::Chunk(_) => chunks += 1,
            ClientEvent::Connected => verbunden = true,
            ClientEvent::Disconnected(g) => fehler.push(g),
            ClientEvent::Log(l) if l.contains("FEHLER") || l.contains("nicht") => fehler.push(l),
            _ => {}
        }
    }
    println!("{alle} Events, {chunks} Chunks, verbunden {verbunden}, Meldungen: {fehler:#?}");
    assert!(verbunden && chunks > 0);
}
