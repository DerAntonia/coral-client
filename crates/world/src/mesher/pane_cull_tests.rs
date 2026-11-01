use super::*;

fn scheibe(block: &str, verbunden: &[&str]) -> u32 {
    (0..crate::block_registry::state_count() as u32)
        .find(|&s| {
            block_name(s) == Some(block)
                && ["north", "south", "east", "west"].iter().all(|r| (property(s, r) == Some("true")) == verbunden.contains(r))
                && property(s, "waterlogged") == Some("false")
        })
        .unwrap_or_else(|| panic!("{block} {verbunden:?} nicht gefunden"))
}

#[test]
// IronBarsBlock.skipRendering: zur Seite nur, wenn beide zueinander verbunden sind, nach
// oben und unten gegen denselben Block immer.
fn verbundene_scheiben_verdecken_ihre_gemeinsame_flaeche() {
    let a = scheibe("glass_pane", &["east", "west"]);
    let b = scheibe("glass_pane", &["east", "west"]);
    assert!(pane_skips_face(a, b, [1, 0, 0]));
    assert!(pane_skips_face(a, b, [0, 1, 0]));
    let einzeln = scheibe("glass_pane", &[]);
    assert!(!pane_skips_face(einzeln, b, [1, 0, 0]), "ohne Verbindung bleibt die Flaeche");
    let rot = scheibe("red_stained_glass_pane", &["east", "west"]);
    assert!(!pane_skips_face(a, rot, [1, 0, 0]), "andere Scheibe: Flaeche bleibt wie in Vanilla");
    assert!(!pane_skips_face(a, rot, [0, 1, 0]));
    let gitter = scheibe("iron_bars", &["east", "west"]);
    let kupfer = scheibe("copper_bars", &["east", "west"]);
    assert!(pane_skips_face(gitter, kupfer, [1, 0, 0]), "Gitter untereinander (BlockTags.BARS)");
}
