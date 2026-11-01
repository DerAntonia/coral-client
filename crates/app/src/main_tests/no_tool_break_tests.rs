use super::*;

#[test]
fn nur_diamant_und_netherit_werkzeuge() {
    assert_eq!(protected_tool_durability("diamond_pickaxe"), Some(1561));
    assert_eq!(protected_tool_durability("netherite_axe"), Some(2031));
    assert_eq!(protected_tool_durability("netherite_sword"), Some(2031));
    assert_eq!(protected_tool_durability("iron_pickaxe"), None);
    assert_eq!(protected_tool_durability("diamond_chestplate"), None);
    assert_eq!(protected_tool_durability("diamond"), None);
    // Kein Werkzeug, auch wenn der Name passt wie "diamond_block".
    assert_eq!(protected_tool_durability("diamond_block"), None);
}

#[test]
fn lesbarer_name_oder_der_eigene() {
    let mut st = protocol::item::ItemStack::new(1, 1);
    assert_eq!(tool_display_name(&st, "netherite_pickaxe"), "Netheritspitzhacke");
    st.custom_name = Some("Bernd".into());
    assert_eq!(tool_display_name(&st, "netherite_pickaxe"), "Bernd");
}
