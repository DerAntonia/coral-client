use super::*;

#[test]
fn alte_einstellung_mit_einem_pack_bleibt_lesbar() {
    assert_eq!(parse_skin_stack("CPVP"), vec!["CPVP".to_string()]);
    assert!(parse_skin_stack("").is_empty());
    assert_eq!(parse_skin_stack("A|B| |A"), vec!["A".to_string(), "B".to_string()]);
}

#[test]
fn stapel_uebersteht_speichern_und_lesen() {
    assert_eq!(parse_skin_stack(&["X".to_string(), "Y".to_string()].join(SKIN_STACK_SEP)), vec!["X".to_string(), "Y".to_string()]);
}
