use super::*;

// Diamantschwert: +6 Schaden und -2,4 Tempo auf den Grundwert des Spielers (1 und 4) --
// 7 Schaden, 1,6 Schlaege/s. Multiplikatoren wie AttributeInstance.
#[test]
fn attributwerte_eines_items() {
    use protocol::item::{ItemAttributeModifier, ATTRIBUTE_ATTACK_DAMAGE as SCHADEN, ATTRIBUTE_ATTACK_SPEED as TEMPO, SLOT_GROUP_MAINHAND};
    let m = |attribute, amount, operation, slot| ItemAttributeModifier { attribute, id: String::new(), amount, operation, slot, hidden: false };
    let hand = |s: i32| s == SLOT_GROUP_MAINHAND;
    let schwert = [m(SCHADEN, 6.0, 0, SLOT_GROUP_MAINHAND), m(TEMPO, -2.4, 0, SLOT_GROUP_MAINHAND)];
    assert_eq!(item_attribute_value(&schwert, SCHADEN, 1.0, &hand), Some(7.0));
    assert_eq!(decimal_de(item_attribute_value(&schwert, TEMPO, 4.0, &hand).unwrap()), "1,6");
    assert_eq!(item_attribute_value(&schwert, SCHADEN, 1.0, &|s| s == 4), None, "andere Slots zaehlen nicht");
    let mal = [m(SCHADEN, 4.0, 0, SLOT_GROUP_MAINHAND), m(SCHADEN, 0.5, 1, SLOT_GROUP_MAINHAND), m(SCHADEN, 1.0, 2, SLOT_GROUP_MAINHAND)];
    assert_eq!(item_attribute_value(&mal, SCHADEN, 1.0, &hand), Some(15.0), "(1+4) * 1,5 * 2");
    assert_eq!(decimal_de(13.5), "13,5");
    assert_eq!(decimal_de(10.0), "10");
}

#[test]
fn haltbarkeit_wie_vanilla() {
    assert_eq!(default_max_damage("diamond_pickaxe"), Some(1561));
    assert_eq!(default_max_damage("netherite_chestplate"), Some(592));
    assert_eq!(default_max_damage("leather_boots"), Some(65));
    assert_eq!(default_max_damage("elytra"), Some(432));
    assert_eq!(default_max_damage("golden_apple"), None);
    assert_eq!(default_max_damage("iron_horse_armor"), None);
    let mut stack = protocol::item::ItemStack::new(1, 1);
    assert!(item_bar(&stack, "diamond_sword").is_none(), "unbeschädigt: kein Balken");
    // Durchdruecken: Apfel (32 Ticks) nach 30 Ticks -- 100 ms fehlen.
    assert!(eat_rest_within(32, 30, 100));
    assert!(!eat_rest_within(32, 29, 100));
    assert!(eat_rest_within(32, 40, 0));
    // Kamera-Empfindlichkeit: ohne Trennung gilt X fuer beide Achsen, mit Trennung eigenes Y.
    let mut set = Settings {
        mouse_sensitivity: 0.002,
        mouse_sensitivity_y: 0.004,
        ..Default::default()
    };
    assert_eq!(camera_sensitivities(&set), (0.002, 0.002));
    set.mouse_sens_split = true;
    assert_eq!(camera_sensitivities(&set), (0.002, 0.004));
    // Tooltip nur, wenn das Bild nicht reicht: blankes Schwert nein, verzaubertes ja.
    assert!(!item_needs_tooltip(&stack));
    let mut verzaubert = stack.clone();
    verzaubert.enchantments.push(protocol::item::Enchantment { id: 1, level: 5 });
    assert!(item_needs_tooltip(&verzaubert));
    // Haltbarkeit ab (Hotbar): ein Schwert mit 1561 Punkten und 1171 Schaden hat noch 25 %.
    let mut halb = stack.clone();
    halb.damage = Some(1171);
    assert!((durability_rest(&halb, "diamond_sword").unwrap() - 0.25).abs() < 0.01);
    assert!(durability_rest(&halb, "stick").is_none());
    stack.damage = Some(1561 / 2);
    let (schritt, farbe) = item_bar(&stack, "diamond_sword").unwrap();
    assert_eq!(schritt, 7.0);
    assert!(farbe[0] > 0.9 && farbe[1] > 0.9, "halb abgenutzt ist gelb: {farbe:?}");
}
