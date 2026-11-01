use super::*;

#[test]
fn namen_bekommen_laengen_anderes_bleibt() {
    let mut t: std::collections::HashMap<String, String> = [
        ("item.minecraft.totem_of_undying", "Simulacrum immortalitatis"),
        ("block.minecraft.obsidian", "Lapis obsidianus"),
        ("death.attack.generic", "Simulacrum immortalitatis"),
    ]
    .into_iter()
    .map(|(k, v)| (k.to_string(), v.to_string()))
    .collect();
    add_latin_macrons(&mut t);
    assert_eq!(t["item.minecraft.totem_of_undying"], "Simulācrum immortālitātis");
    assert_eq!(t["block.minecraft.obsidian"], "Lapis obsidiānus");
    assert_eq!(t["death.attack.generic"], "Simulacrum immortalitatis");
}

// Jedes Wort der Liste muss Laengen tragen -- ein Eintrag ohne waere ein Tippfehler.
#[test]
fn jedes_listenwort_hat_eine_laenge() {
    for w in include_str!("../la_macrons.txt").split_whitespace() {
        assert_ne!(strip_macrons(w), w, "{w} ohne Laenge");
    }
}
