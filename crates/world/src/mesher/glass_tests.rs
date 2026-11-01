use super::*;

// Vanillas BlockRenderLayers legt ALLE Glasarten in dieselbe TRANSLUCENT-Schicht --
// inklusive KLAREM Glas und klaren Scheiben. Die naheliegende Annahme, nur gefaerbtes
// Glas sei echt transparent, ist falsch.
#[test]
fn alle_glasarten_zaehlen_als_glas() {
    for name in [
        "glass",
        "glass_pane",
        "tinted_glass",
        "white_stained_glass",
        "black_stained_glass",
        "light_blue_stained_glass",
        "red_stained_glass_pane",
        "lime_stained_glass_pane",
    ] {
        assert!(is_glass(name), "{name} muss als Glas gelten");
    }
}

// Baut eine Spalte mit zwei Bloecken uebereinander bei (0,0,0) und (0,1,0).
fn welt_mit_zwei(unten: u32, oben: u32) -> World {
    let mut w = World::new();
    let mut sections = Vec::new();
    for i in 0..24 {
        let mut states = vec![0u32; crate::chunk::SECTION_VOLUME];
        if i == 4 {
            states[0] = unten; // lokal (0,0,0) -> Welt-y=0
            states[16 * 16] = oben; // lokal (0,1,0) -> Welt-y=1
        }
        sections.push(crate::chunk::ChunkSection::from_states(states));
    }
    w.insert_column(0, 0, sections);
    w
}

fn state_von(block: &str) -> u32 {
    (0..crate::block_registry::state_count() as u32)
        .find(|&s| block_name(s) == Some(block))
        .unwrap_or_else(|| panic!("{block}-State nicht gefunden"))
}

// Man darf nicht durch das Gras gucken koennen: unter einer Glasscheibe muss
// die Oberseite des Blocks darunter sichtbar bleiben. Glas ist zwar `is_solid` (= nicht
// Luft), verdeckt aber nichts -- vor dem Fix wurde die Flaeche weggeculled und man sah
// durch den Grasblock hindurch ins Nichts.
#[test]
fn block_unter_glas_behaelt_seine_oberseite() {
    let stein = state_von("stone");
    let glas = state_von("glass");
    let mit_glas = welt_mit_zwei(stein, glas);
    let mit_luft = welt_mit_zwei(stein, 0);

    let flaechen = |w: &World| {
        let (opaque, _) = mesh_column(
            w,
            0,
            0,
            &BlockUvs::single_tile(),
            &BlockShapes::all_full_cubes(),
            &BiomePalette::fallback(),
            &CrossBlocks::none(),
            &BlockShapes::all_full_cubes(),
        );
        opaque.len()
    };
    assert_eq!(
        flaechen(&mit_glas),
        flaechen(&mit_luft),
        "der Stein unter Glas muss genauso viele Flaechen haben wie unter freiem Himmel -- Glas verdeckt nichts"
    );
}

// Auch Wasser wird von Glas nicht verdeckt -- der Weg dorthin ist in Vanilla aber ein
// anderer als bei Bloecken, und die naheliegende Herleitung fuehrt in die Irre:
// FluidRenderer.isSideCovered prueft die FORM, nicht die Deckkraft, und Glas sieht wie
// ein Vollwuerfel aus. Die Form wird jedoch erst gar nicht gefuellt, wenn ein Block nicht
// opak ist (AbstractBlock: `cullingShape = opaque ? ... : empty()`), und bei leerer Form
// steigt isSideCovered sofort mit false aus.
#[test]
fn wasser_neben_glas_behaelt_seine_seitenflaeche() {
    let wasser = state_von("water");
    let glas = state_von("glass");

    // Wasser bei (0,0,0), Nachbar in +X bei (1,0,0).
    let welt_mit_nachbar = |nachbar: u32| {
        let mut w = World::new();
        let mut sections = Vec::new();
        for i in 0..24 {
            let mut states = vec![0u32; crate::chunk::SECTION_VOLUME];
            if i == 4 {
                states[0] = wasser;
                states[1] = nachbar; // lokal (1,0,0)
            }
            sections.push(crate::chunk::ChunkSection::from_states(states));
        }
        w.insert_column(0, 0, sections);
        w
    };
    let wasserflaechen = |w: &World| {
        let (_, translucent) = mesh_column(
            w,
            0,
            0,
            &BlockUvs::single_tile(),
            &BlockShapes::all_full_cubes(),
            &BiomePalette::fallback(),
            &CrossBlocks::none(),
            &BlockShapes::all_full_cubes(),
        );
        translucent.len()
    };
    // ACHTUNG: Glas liegt seit dem Wechsel ins echte Alpha-Blending SELBST im
    // Translucent-Ergebnis -- ein blosser Vergleich der Gesamtzahl misst also Wasser UND
    // Glas. Deshalb wird der Eigenanteil des Glases abgezogen, statt eine Zahl zu raten:
    // was uebrigbleibt, muss exakt dem Wasser neben Luft entsprechen.
    let nur_glas = {
        let mut w = World::new();
        let mut sections = Vec::new();
        for i in 0..24 {
            let mut states = vec![0u32; crate::chunk::SECTION_VOLUME];
            if i == 4 {
                states[1] = glas;
            }
            sections.push(crate::chunk::ChunkSection::from_states(states));
        }
        w.insert_column(0, 0, sections);
        wasserflaechen(&w)
    };
    let wasser_neben_glas = wasserflaechen(&welt_mit_nachbar(glas)) - nur_glas;
    let wasser_neben_luft = wasserflaechen(&welt_mit_nachbar(0));
    let wasser_neben_stein = wasserflaechen(&welt_mit_nachbar(state_von("stone")));

    // Gleiche ANZAHL Flaechen wie neben Luft, aber 6 Vertices weniger: die Flaeche zum Glas
    // traegt das water_overlay und ist als einzige NICHT doppelseitig (s. push_rueckseiten
    // -- wie in Vanilla). Entscheidend fuer diesen Test
    // ist, dass die Flaeche ueberhaupt noch da ist: Glas hat keine Culling-Form.
    assert_eq!(
        wasser_neben_glas,
        wasser_neben_luft - 6,
        "Wasser neben Glas muss dieselbe Flaeche zeigen wie neben Luft (nur einseitig) -- Glas hat keine Culling-Form"
    );
    // Gegenprobe, damit der Test nicht bloss "nichts wird je geculled" bestaetigt:
    // gegen einen echten opaken Vollwuerfel faellt die gemeinsame Flaeche weiterhin weg.
    assert!(wasser_neben_stein < wasser_neben_luft, "gegen Stein muss die Wasserflaeche weiterhin weggeculled werden");
}

// Gegenprobe zum Fix: GLEICHES Glas an gleichem Glas culled die gemeinsame Flaeche sehr
// wohl (Vanillas skipRendering) -- sonst saehe man in einer Glaswand jede innere Naht.
#[test]
fn glas_an_gleichem_glas_culled_die_naht() {
    let glas = state_von("glass");
    let (_, zwei_glas) = mesh_column(
        &welt_mit_zwei(glas, glas),
        0,
        0,
        &BlockUvs::single_tile(),
        &BlockShapes::all_full_cubes(),
        &BiomePalette::fallback(),
        &CrossBlocks::none(),
        &BlockShapes::all_full_cubes(),
    );
    let (_, ein_glas) = mesh_column(
        &welt_mit_zwei(glas, 0),
        0,
        0,
        &BlockUvs::single_tile(),
        &BlockShapes::all_full_cubes(),
        &BiomePalette::fallback(),
        &CrossBlocks::none(),
        &BlockShapes::all_full_cubes(),
    );
    // Zwei gestapelte Glasbloecke: 12 Flaechen minus die 2 an der gemeinsamen Naht = 10,
    // also weniger als das Doppelte eines einzelnen (6).
    assert!(
        zwei_glas.len() < ein_glas.len() * 2,
        "die Naht zwischen zwei gleichen Glasbloecken muss wegfallen (zwei: {}, einzeln: {})",
        zwei_glas.len(),
        ein_glas.len()
    );
    assert!(zwei_glas.len() > ein_glas.len(), "zwei Glasbloecke muessen trotzdem mehr Flaechen haben als einer");
}

// DER Punkt, an dem ich dreimal danebenlag: warum Wasser hinter Glas in Vanilla "nur
// blau" ist. Nicht Culling, nicht Doppelseitigkeit -- eine EIGENE, flachere Textur
// (water_overlay), die Vanilla nimmt, sobald der Nachbar Glas oder Laub ist.
#[test]
fn wasserseite_an_glas_und_laub_nutzt_die_overlay_textur() {
    const STILL: [f32; 4] = [0.0, 0.0, 0.25, 0.25];
    const OVERLAY: [f32; 4] = [0.5, 0.5, 0.75, 0.75];
    let uvs = BlockUvs::single_tile().with_water_overlay(Some(OVERLAY));

    // Die Auswahllogik selbst -- dieselbe Bedingung wie in push_fluid_quads.
    let waehle = |name: &str| if is_glass(name) || is_leaves(name) { uvs.water_overlay_rect().unwrap_or(STILL) } else { STILL };

    for name in ["glass", "white_stained_glass", "tinted_glass", "oak_leaves", "birch_leaves"] {
        assert_eq!(waehle(name), OVERLAY, "{name}: Wasser daneben muss die Overlay-Textur bekommen");
    }
    for name in ["stone", "dirt", "sand", "water"] {
        assert_eq!(waehle(name), STILL, "{name}: Wasser daneben muss die normale Textur behalten");
    }
}

// Fehlt water_overlay im Atlas (Resourcepack ohne die Datei), muss sauber die
// Standtextur herauskommen statt eines Griffs in eine fremde Kachel.
#[test]
fn ohne_overlay_kachel_bleibt_die_standtextur() {
    const STILL: [f32; 4] = [0.0, 0.0, 0.25, 0.25];
    let uvs = BlockUvs::single_tile(); // kein with_water_overlay
    assert_eq!(uvs.water_overlay_rect().unwrap_or(STILL), STILL);
}

// Gegenprobe: nichts anderes darf hineinrutschen. Besonders Laub nicht -- dessen Alpha
// wird beim Atlas-Bau bewusst auf voll gesetzt (s. assets::is_leaves), es gehoert in den
// Opak-Pass. Und kein Block, der bloss "glass" im Namen traegt.
#[test]
fn andere_bloecke_sind_kein_glas() {
    for name in ["stone", "oak_leaves", "water", "glowstone", "glass_bottle", "ice", "sea_lantern"] {
        assert!(!is_glass(name), "{name} darf NICHT als Glas gelten");
    }
}
