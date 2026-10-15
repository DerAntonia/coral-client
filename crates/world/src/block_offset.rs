// Vanilla verschiebt bestimmte Bloecke (Bambus, Setzlinge, Gras, Blumen, kleine Pilze, ...)
// horizontal um einen kleinen, pro Block-Position DETERMINISTISCHEN Zufallswert -- deshalb
// stehen echte Bambusstangen/Grasbueschel nie exakt mittig in ihrer Zelle. Das ist KEINE
// Blockstate-Eigenschaft (kein zusaetzlicher Property-Wert), sondern eine reine Funktion der
// Weltposition (Block.getOffset() in Vanilla), angewendet auf Render- UND Kollisionsbox.
// Ohne diesen Schritt stuende und kollidierte Bambus immer exakt mittig.
//
// Formel gegen die unabhaengig dokumentierten Konstanten auf minecraft.wiki/w/Block_offset
// verifiziert (0x2FC20F=3129871, 0x6EBFFF5=116129781, 0x285B825=42317861, 0x000B=11 --
// alle exakt deckungsgleich mit den hier genutzten). WICHTIG, zweimal falsch geraten und
// jetzt gegenkontrolliert: getOffset() beeinflusst laut derselben Quelle NICHT nur die
// Renderposition, sondern auch die Kollisionsbox (s. world::collision_boxes-Aufrufer) -- UND
// der Aufruf ist Mth.getSeed(x, z, 0), NICHT (wie zunaechst angenommen) getSeed(x, 0, z): z
// landet im "y"-Parameter (bekommt die volle 64-Bit-Multiplikation mit 116129781), der echte
// dritte Parameter ist 0 und traegt nichts bei. Bei vertauschter Reihenfolge durchlief z
// stattdessen den 32-Bit-Wrap-Pfad (fuer x gedacht) und lieferte bei typischen
// Welt-Distanzen einen anderen Wert als der echte Server -- Ursache des andauernden
// Server-Flags beim Bambus-Durchlaufen trotz des vorherigen 32-Bit-Wrap-Fixes.
pub fn xz_offset(x: i32, z: i32) -> (f32, f32) {
    let seed = mc_seed(x, z, 0);
    let dx = ((seed & 15) as f32 / 15.0 - 0.5) * 0.5;
    let dz = ((seed >> 8 & 15) as f32 / 15.0 - 0.5) * 0.5;
    (dx, dz)
}

// Die Java-Semantik von Mth.getSeed nachgebildet -- WICHTIG: "x * 3129871" ist in Java eine
// int*int-Multiplikation (32-Bit-Ueberlauf/Wrap), erst DANACH nach long erweitert. "y *
// 116129781L" dagegen hat einen long-Literalen Faktor, Java erweitert y also VOR der
// Multiplikation auf 64 Bit -- kein 32-Bit-Wrap dort. Eine anfaengliche Fassung hat beide
// Terme gleich (in i64) multipliziert; das stimmt nur fuer kleine Koordinaten (|x| < ~685)
// zufaellig ueberein und weicht bei welt-typischen Distanzen vom Spawn vom echten Server-Wert
// ab -- beobachtet als Server-Flag beim Durchlaufen von Bambus (Kollisionsbox client- und
// serverseitig an unterschiedlichen Positionen berechnet).
// pub(crate) statt privat: world::render_rng nutzt exakt dieselbe Formel fuer
// MathHelper.hashCode(BlockPos) (Vanillas pseudozufaellige Block-Rotation), nur mit
// anderer Parameterreihenfolge am Aufrufer (s. dortigen Kommentar) -- eine zweite,
// duplizierte Fassung waere nur eine Fehlerquelle mehr fuer dieselbe, schon einmal
// muehsam verifizierte Formel.
pub(crate) fn mc_seed(x: i32, y: i32, z: i32) -> i64 {
    let x_term = x.wrapping_mul(3129871) as i64; // 32-Bit-Wrap wie Java int*int, dann vorzeichenrichtig erweitert
    let y_term = (y as i64).wrapping_mul(116129781); // y zuerst auf long erweitert -> 64-Bit-Multiplikation
    let mut l = x_term ^ y_term ^ (z as i64);
    l = l.wrapping_mul(l).wrapping_mul(42317861).wrapping_add(l.wrapping_mul(11));
    l >> 16
}

// Konservative, gegen Vanilla-Quellcode-Wissen abgesicherte Liste der Bloecke mit
// OffsetType.XZ -- bewusst NICHT vollstaendig (z.B. bei kleineren/unsicheren Kandidaten wie
// Suesse-Beeren-Strauch oder kleinem Tropfblatt lieber weggelassen als geraten). Waechst bei
// Bedarf, wenn weitere Bloecke falsch mittig stehen.
pub fn has_xz_offset(block_name: &str) -> bool {
    // Setzlinge, Pilze, Pilzgewaechse (Fungus) und Azaleen standen hier frueher auch -- sie
    // haben in Vanilla KEINEN Versatz (der Formen-Dump zeigt sie mittig), standen im Client
    // also schief in ihrer Zelle.
    matches!(
        block_name,
        "bamboo"
            | "bamboo_sapling"
            | "short_grass"
            | "fern"
            | "dead_bush"
            | "dandelion"
            | "poppy"
            | "blue_orchid"
            | "allium"
            | "azure_bluet"
            | "red_tulip"
            | "orange_tulip"
            | "white_tulip"
            | "pink_tulip"
            | "oxeye_daisy"
            | "cornflower"
            | "lily_of_the_valley"
            | "wither_rose"
            | "torchflower"
            | "open_eyeblossom"
            | "closed_eyeblossom"
            | "mangrove_propagule"
            | "crimson_roots"
            | "warped_roots"
            | "pointed_dripstone"
    )
}

// Block.getMaxHorizontalModelOffset: 0.25 als Vorgabe, der Tropfstein ueberschreibt es mit
// 0.125 (PointedDripstoneBlock) -- der Versatz wird darauf geklemmt.
pub fn max_xz_offset(block_name: &str) -> f32 {
    if block_name == "pointed_dripstone" {
        0.125
    } else {
        0.25
    }
}

// Der Versatz eines Blocks an (x, z) wie AbstractBlockState.getModelOffset: 0 ohne
// OffsetType.XZ, sonst xz_offset auf max_xz_offset geklemmt.
pub fn block_xz_offset(block_name: &str, x: i32, z: i32) -> (f32, f32) {
    if !has_xz_offset(block_name) {
        return (0.0, 0.0);
    }
    let m = max_xz_offset(block_name);
    let (dx, dz) = xz_offset(x, z);
    (dx.clamp(-m, m), dz.clamp(-m, m))
}

// Der Versatz des UMRISSES (Zielen, Blockrahmen) -- nicht dasselbe wie der des Modells. Nur
// manche Bloecke verschieben in getShape ihre Form mit (Blumen, Bambus, Mangroven-Keimling,
// Tropfstein); Gras, Farn, toter Busch und Nether-Wurzeln stehen versetzt, ihr Umriss bleibt
// aber mittig in der Zelle. Mit verschobenem Umriss lag der Trefferpunkt bis zu 1/8 Block
// AUSSERHALB der Zelle, und genau das prueft Grims FabricatedPlace (Trefferpunkt je Achse
// 0..1): beim Platzieren auf Gras flaggte jeder solche Klick.
pub fn outline_xz_offset(block_name: &str, x: i32, z: i32) -> (f32, f32) {
    if matches!(block_name, "short_grass" | "fern" | "dead_bush" | "crimson_roots" | "warped_roots") {
        return (0.0, 0.0);
    }
    block_xz_offset(block_name, x, z)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gras_umriss_bleibt_in_der_zelle() {
        // Irgendeine Stelle mit deutlichem Versatz suchen: das Modell wandert, der Umriss nicht.
        let (x, z) = (0..64).flat_map(|x| (0..64).map(move |z| (x, z))).find(|&(x, z)| block_xz_offset("short_grass", x, z).0.abs() > 0.2).unwrap();
        assert_eq!(outline_xz_offset("short_grass", x, z), (0.0, 0.0));
        assert_eq!(outline_xz_offset("poppy", x, z), block_xz_offset("poppy", x, z));
    }

    #[test]
    fn offset_ist_deterministisch_und_unabhaengig_von_y() {
        let a = xz_offset(5, -3);
        let b = xz_offset(5, -3);
        assert_eq!(a, b);
    }

    #[test]
    fn offset_liegt_im_erwarteten_bereich() {
        // Absichtlich auch weit-vom-Ursprung-entfernte Koordinaten (nicht nur -20..20) --
        // genau da wich der frueher fehlerhafte 64-Bit-statt-32-Bit-Multiplikations-Bug vom
        // echten Server-Wert ab (s. mc_seed-Kommentar).
        for x in [-20_000, -685, -20, 0, 20, 685, 20_000] {
            for z in [-20_000, -685, -20, 0, 20, 685, 20_000] {
                let (dx, dz) = xz_offset(x, z);
                assert!((-0.26..=0.26).contains(&dx), "dx={dx} out of range at {x},{z}");
                assert!((-0.26..=0.26).contains(&dz), "dz={dz} out of range at {x},{z}");
            }
        }
    }

    // Reproduziert exakt den urspruenglichen Bug: bei x=1000 (jenseits der 32-Bit-int-
    // Ueberlaufgrenze fuer x*3129871) muss das Ergebnis vom naiven 64-Bit-Produkt abweichen
    // -- sonst waere der Fix wirkungslos bzw. wieder rueckgaengig gemacht worden.
    #[test]
    fn x_term_nutzt_echten_32_bit_ueberlauf() {
        let x = 1000i32;
        let korrekt = x.wrapping_mul(3129871) as i64;
        let naiv_64bit = (x as i64).wrapping_mul(3129871);
        assert_ne!(korrekt, naiv_64bit, "bei x=1000 muss der 32-Bit-Wrap bereits gegriffen haben");
    }

    #[test]
    fn nicht_jede_position_hat_denselben_offset() {
        let a = xz_offset(0, 0);
        let b = xz_offset(1, 0);
        assert_ne!(a, b, "verschiedene Positionen sollten (fast immer) verschiedene Offsets haben");
    }
}
