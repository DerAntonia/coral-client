use super::*;
use crate::block_registry::state_count;

const FLOW: [f32; 4] = [0.5, 0.5, 0.75, 0.75];
const OVERLAY: [f32; 4] = [0.25, 0.25, 0.5, 0.5];

fn state_von(block: &str) -> u32 {
    (0..state_count() as u32)
        .find(|&s| block_name(s) == Some(block) && crate::state_props::property(s, "level").is_none_or(|l| l == "0"))
        .expect("State nicht gefunden")
}

// Eine Spalte mit einem Block bei Welt (0,0,0) und optional einem Nachbarn bei (1,0,0).
fn welt(block: u32, nachbar: Option<u32>) -> World {
    let mut w = World::new();
    let mut sections = Vec::new();
    for i in 0..24 {
        let mut states = vec![0u32; crate::chunk::SECTION_VOLUME];
        if i == 4 {
            // Section 4 beginnt bei MIN_Y + 4*16 = 0 -- lokal (0,0,0) ist Welt-y=0.
            states[0] = block;
            if let Some(n) = nachbar {
                states[1] = n; // lokal (1,0,0)
            }
        }
        sections.push(crate::chunk::ChunkSection::from_states(states));
    }
    w.insert_column(0, 0, sections);
    w
}

fn mesh(w: &World, uvs: &BlockUvs) -> (Vec<BlockVertex>, Vec<BlockVertex>) {
    mesh_column(
        w,
        0,
        0,
        uvs,
        &BlockShapes::all_full_cubes(),
        &BiomePalette::fallback(),
        &CrossBlocks::none(),
        &BlockShapes::all_full_cubes(),
    )
}

// DER Kern. Eine freistehende Lavaquelle darf NICHT bis zur Zellkante reichen:
// Vanillas Quellhoehe ist 8/9, und weil die vier Ecken gegen die Luftnachbarn gemittelt
// werden (FluidRenderer.calculateFluidHeight, Gewicht 10 fuer die Quelle gegen je 1 fuer
// die beiden Luftseiten), sackt sie auf (10*8/9)/12 ab -- exakt dieselbe Zahl, die
// fluid::tests::isolierte_quelle_sackt_an_den_ecken_ab fuer Wasser festhaelt.
#[test]
fn lavaquelle_reicht_nicht_bis_zur_zellkante() {
    let lava = state_von("lava");
    let (opaque, _) = mesh(&welt(lava, None), &BlockUvs::single_tile());
    assert!(!opaque.is_empty(), "eine Lavaquelle muss Geometrie liefern");

    let min_y = opaque.iter().fold(f32::MAX, |a, v| a.min(v.position[1]));
    let max_y = opaque.iter().fold(f32::MIN, |a, v| a.max(v.position[1]));
    // Unterseite liegt um EPS hoch, Oberseite um EPS tief (beides Vanillas 0.001, s.
    // push_fluid_quads) -- die sichtbare Spanne ist also Eckhoehe minus 2*EPS.
    let erwartet = crate::fluid::calculate_corner_height(8.0 / 9.0, 0.0, 0.0, 0.0) - 0.002;
    assert!(
        (max_y - min_y - erwartet).abs() < 1e-4,
        "Lava-Spanne war {}, erwartet {erwartet} (vorher stand sie als Vollwuerfel bei 1.0)",
        max_y - min_y
    );
    assert!(max_y - min_y < 0.9, "Lava darf keinesfalls einen ganzen Block hoch sein");
}

// Vanilla legt nur FluidTags.WATER in die durchsichtige Schicht (RenderLayers.
// getFluidLayer) -- Lava ist blickdicht und gehoert in den Opak-Pass. Gegenstueck zu
// wasser_landet_im_translucent_pass.
#[test]
fn lava_landet_im_opaken_pass() {
    let lava = state_von("lava");
    let (opaque, translucent) = mesh(&welt(lava, None), &BlockUvs::single_tile());
    assert!(!opaque.is_empty(), "ein Lavablock muss Opak-Vertices liefern");
    assert!(translucent.is_empty(), "Lava darf nichts in den Translucent-Pass legen");
}

// Die Seitenflaechen tragen lava_flow, nicht lava_still -- dieselbe Halbkachel-UV wie
// bei Wasser (Vanillas getFrameU(0.0)..getFrameU(0.5)).
#[test]
fn lava_seitenflaechen_kommen_aus_der_fliesskachel() {
    let lava = state_von("lava");
    let uvs = BlockUvs::single_tile().with_lava_flow(Some(FLOW));
    let (opaque, _) = mesh(&welt(lava, None), &uvs);

    let aus_fliesskachel = |v: &BlockVertex| {
        v.uv[0] >= FLOW[0] - 1e-5 && v.uv[0] <= FLOW[2] + 1e-5 && v.uv[1] >= FLOW[1] - 1e-5 && v.uv[1] <= FLOW[3] + 1e-5
    };
    // 4 Seiten * 6 Vertices, doppelseitig (s. push_rueckseiten) = 48. Die Standkachel
    // liegt bei [0,0,1,1] und umschliesst die Fliesskachel; Ober-/Unterseite nutzen davon
    // aber nur die Eckpunkte 0 und 1, fallen also nicht in den Bereich 0.5..0.75.
    assert_eq!(
        opaque.iter().filter(|v| aus_fliesskachel(v)).count(),
        48,
        "vier doppelseitige Lava-Seitenflaechen aus der Fliesskachel erwartet"
    );
}

// Der water_overlay-Zweig haengt in Vanilla an `if (!bl)` -- Lava an Glas/Laub bekommt
// ihr ganz normales Fliess-Sprite. Ohne die kind-Abfrage in push_fluid_quads::seite
// saehe man an jeder Glasscheibe neben Lava eine WASSER-Textur.
#[test]
fn lava_neben_glas_nimmt_kein_wasser_overlay() {
    let lava = state_von("lava");
    let glas = state_von("glass");
    let uvs = BlockUvs::single_tile().with_lava_flow(Some(FLOW)).with_water_overlay(Some(OVERLAY));
    let (opaque, _) = mesh(&welt(lava, Some(glas)), &uvs);

    let in_kachel = |v: &BlockVertex, r: [f32; 4]| {
        v.uv[0] >= r[0] - 1e-5 && v.uv[0] <= r[2] + 1e-5 && v.uv[1] >= r[1] - 1e-5 && v.uv[1] <= r[3] + 1e-5
    };
    // OVERLAY (0.25..0.5) und FLOW (0.5..0.75) beruehren sich bei 0.5; nur Vertices
    // ECHT innerhalb der Overlay-Kachel zaehlen als Treffer.
    let overlay_treffer = opaque.iter().filter(|v| in_kachel(v, OVERLAY) && !in_kachel(v, FLOW)).count();
    assert_eq!(overlay_treffer, 0, "Lava darf nie die water_overlay-Kachel sampeln");
    assert!(opaque.iter().any(|v| in_kachel(v, FLOW)), "die Lavaseite zum Glas muss die Fliesskachel tragen");
}

// Zwei Lavazellen nebeneinander duerfen ihre gemeinsame Flaeche nicht zeichnen. Vor dem
// Umbau lief Lava ueber die Box-Schleife, und weil has_resolved_shape("lava") false ist
// (lava.json hat keine "elements"), galt der Lava-Nachbar dort als "verdeckt nichts" --
// eine Lavasee zeichnete JEDE innere Flaeche mit.
#[test]
fn lava_neben_lava_culled_die_gemeinsame_flaeche() {
    let lava = state_von("lava");
    let (allein, _) = mesh(&welt(lava, None), &BlockUvs::single_tile());
    let (zu_zweit, _) = mesh(&welt(lava, Some(lava)), &BlockUvs::single_tile());
    // Ohne Culling waeren es glatt 2 * allein. Genau die zwei gemeinsamen Flaechen
    // (je doppelseitig, also 2 * 12 Vertices) muessen fehlen.
    assert_eq!(zu_zweit.len(), 2 * allein.len() - 2 * 12, "die Naht zwischen zwei Lavazellen muss verschwinden");
}
