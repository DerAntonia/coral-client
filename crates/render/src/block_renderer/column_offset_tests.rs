use super::*;

// Was block.wgsl::column_relative rechnet, in f32.
fn im_shader(lokal: f32, o: [f32; 6], achse: usize) -> f32 {
    (lokal + o[achse]) - o[achse + 3]
}

// Die gemeinsame Kante zweier Nachbarspalten (x = 16 in der einen, x = 0 in der anderen)
// muss auf beiden Seiten bitgleich herauskommen, sonst bleibt beim Rastern eine Pixelluecke.
// Mit einem fertig gerundeten Versatz je Spalte (vorher) traf das fuer einen Teil der
// Augenpositionen nicht zu -- s. update_column_offsets.
#[test]
fn nachbarspalten_teilen_ihre_kante_bitgleich() {
    let mut alt_daneben = 0;
    let mut zufall = 0x2545_F491_4F6C_DD1Du64;
    for _ in 0..20_000 {
        zufall ^= zufall << 13;
        zufall ^= zufall >> 7;
        zufall ^= zufall << 17;
        let eye = DVec3::new((zufall % 6_000_000) as f64 / 1000.0 - 3000.0, 70.3, 12.5);
        let cx = (eye.x / 16.0).floor() as i32 + (zufall >> 40) as i32 % 33 - 16;
        let (a, b) = (column_offset((cx, 0), -64, eye), column_offset((cx + 1, 0), -64, eye));
        assert_eq!(im_shader(16.0, a, 0).to_bits(), im_shader(0.0, b, 0).to_bits(), "Auge {eye}, Spalte {cx}");
        let alt = |key: i32| (f64::from(key * 16) - eye.x) as f32;
        alt_daneben += usize::from(16.0 + alt(cx) != alt(cx + 1));
    }
    println!("alte Rechnung daneben: {alt_daneben} von 20000");
    // Gegenprobe: die alte Rechnung lag tatsaechlich daneben.
    assert!(alt_daneben > 100, "{alt_daneben}");
}
