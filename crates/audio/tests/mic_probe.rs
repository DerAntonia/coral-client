// Oeffnet das Standard-Mikrofon fuer eine Sekunde und zaehlt die Rahmen. Braucht echte
// Hardware, daher #[ignore]: cargo test -p audio --test mic_probe -- --ignored --nocapture

#[test]
#[ignore]
fn mikrofon_liefert_rahmen() {
    println!("Eingabegeraete: {:?}", audio::mic::list_input_devices());
    let (tx, rx) = std::sync::mpsc::channel();
    let mic = audio::mic::Microphone::open(None, move |r: &[i16]| {
        let _ = tx.send(r.iter().map(|s| s.unsigned_abs()).max().unwrap_or(0));
    })
    .expect("Mikrofon");
    println!("Geraet: {}", mic.device_name);
    std::thread::sleep(std::time::Duration::from_secs(1));
    let spitzen: Vec<u16> = rx.try_iter().collect();
    println!("{} Rahmen in 1 s, Spitzen {:?}", spitzen.len(), &spitzen[..spitzen.len().min(10)]);
    assert!((45..=55).contains(&spitzen.len()), "{} Rahmen", spitzen.len());
}
