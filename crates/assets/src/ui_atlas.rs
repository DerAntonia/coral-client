// Ein einziger RGBA-Atlas fuer alles, was die 2D-Oberflaeche an Vanilla-Bildern braucht:
// die GUI-Sheets (Hotbar, Inventar-Hintergrund, Kisten) und die flachen Item-Sprites.
//
// Warum ein gemeinsamer Atlas: Hotbar und Inventar zeichnen in EINEM Zug Hintergrund UND
// Icons; laegen die in getrennten Texturen, braeuchte jede Lage einen eigenen Draw-Call mit
// Bind-Group-Wechsel. Die Bilder haben sehr unterschiedliche Groessen (256x256-Sheet neben
// 16x16-Item), deshalb hier ein einfacher Regal-Packer statt des festen Kachelrasters aus
// blocks.rs.
//
// Nichts davon wird mitgeliefert: alle Bilder kommen zur Laufzeit aus der lokalen
// .minecraft-Installation (bzw. dem gesetzten Resourcepack), genau wie die Blocktexturen.
//
// Bloecke im Inventar sind KEINE Sprites: fuer sie liefert resolve_item_icons die sechs
// Flaechen-UVs aus dem bereits gebauten Block-Atlas, die App zeichnet daraus einen
// isometrischen Wuerfel (s. app::item_icons).

use std::collections::HashMap;

use serde::Deserialize;

use crate::blocks::{load_model_chain, resolve_var, strip_ns, BlockAtlas, UvRect};
use crate::installation::MinecraftInstallation;

// Obergrenze der Atlaskante. 4096 ist auf jeder halbwegs aktuellen GPU sicher; mehr braucht
// es selbst mit einem hochaufgeloesten Resourcepack fuer ~1500 Item-Sprites nicht.
const MAX_ATLAS_SIZE: u32 = 4096;
// Ein Pixel Rand um jedes Bild, damit die Nachbarkachel beim Sampeln nicht durchblutet.
const PADDING: u32 = 1;

// Die GUI-Sheets, die die Oberflaeche kennt. Schluessel = der Name, unter dem die App das
// Rechteck abfragt (s. UiAtlas::rect), Wert = Pfad in der Installation.
pub const GUI_SHEETS: [(&str, &str); 151] = [
    ("hud/hotbar", "minecraft/textures/gui/sprites/hud/hotbar.png"),
    ("hud/hotbar_selection", "minecraft/textures/gui/sprites/hud/hotbar_selection.png"),
    ("hud/hotbar_offhand_left", "minecraft/textures/gui/sprites/hud/hotbar_offhand_left.png"),
    ("container/inventory", "minecraft/textures/gui/container/inventory.png"),
    ("container/generic_54", "minecraft/textures/gui/container/generic_54.png"),
    // Statusleisten (Leben, Ruestung, Nahrung, Luft, Erfahrung). Seit 1.20.2 liegt jedes
    // dieser Bilder als EIGENE Datei unter gui/sprites/hud/ -- frueher waren sie Ausschnitte
    // aus einem gemeinsamen icons.png, weshalb aeltere Anleitungen hier Pixelkoordinaten
    // nennen. Alle sind 9x9 ausser der Erfahrungsleiste (182x5).
    ("hud/heart/container", "minecraft/textures/gui/sprites/hud/heart/container.png"),
    ("hud/heart/full", "minecraft/textures/gui/sprites/hud/heart/full.png"),
    ("hud/heart/half", "minecraft/textures/gui/sprites/hud/heart/half.png"),
    ("hud/heart/poisoned_full", "minecraft/textures/gui/sprites/hud/heart/poisoned_full.png"),
    ("hud/heart/poisoned_half", "minecraft/textures/gui/sprites/hud/heart/poisoned_half.png"),
    ("hud/heart/withered_full", "minecraft/textures/gui/sprites/hud/heart/withered_full.png"),
    ("hud/heart/withered_half", "minecraft/textures/gui/sprites/hud/heart/withered_half.png"),
    ("hud/heart/absorbing_full", "minecraft/textures/gui/sprites/hud/heart/absorbing_full.png"),
    ("hud/heart/absorbing_half", "minecraft/textures/gui/sprites/hud/heart/absorbing_half.png"),
    ("hud/armor_empty", "minecraft/textures/gui/sprites/hud/armor_empty.png"),
    ("hud/armor_half", "minecraft/textures/gui/sprites/hud/armor_half.png"),
    ("hud/armor_full", "minecraft/textures/gui/sprites/hud/armor_full.png"),
    ("hud/food_empty", "minecraft/textures/gui/sprites/hud/food_empty.png"),
    ("hud/food_half", "minecraft/textures/gui/sprites/hud/food_half.png"),
    ("hud/food_full", "minecraft/textures/gui/sprites/hud/food_full.png"),
    ("hud/air", "minecraft/textures/gui/sprites/hud/air.png"),
    ("hud/air_bursting", "minecraft/textures/gui/sprites/hud/air_bursting.png"),
    ("hud/air_empty", "minecraft/textures/gui/sprites/hud/air_empty.png"),
    ("hud/experience_bar_background", "minecraft/textures/gui/sprites/hud/experience_bar_background.png"),
    ("hud/experience_bar_progress", "minecraft/textures/gui/sprites/hud/experience_bar_progress.png"),
    // Bossbar (BossBarHud): je Farbe Hintergrund und Fuellung, je Kerbenstil eine Lage darueber.
    ("boss_bar/pink_background", "minecraft/textures/gui/sprites/boss_bar/pink_background.png"),
    ("boss_bar/pink_progress", "minecraft/textures/gui/sprites/boss_bar/pink_progress.png"),
    ("boss_bar/blue_background", "minecraft/textures/gui/sprites/boss_bar/blue_background.png"),
    ("boss_bar/blue_progress", "minecraft/textures/gui/sprites/boss_bar/blue_progress.png"),
    ("boss_bar/red_background", "minecraft/textures/gui/sprites/boss_bar/red_background.png"),
    ("boss_bar/red_progress", "minecraft/textures/gui/sprites/boss_bar/red_progress.png"),
    ("boss_bar/green_background", "minecraft/textures/gui/sprites/boss_bar/green_background.png"),
    ("boss_bar/green_progress", "minecraft/textures/gui/sprites/boss_bar/green_progress.png"),
    ("boss_bar/yellow_background", "minecraft/textures/gui/sprites/boss_bar/yellow_background.png"),
    ("boss_bar/yellow_progress", "minecraft/textures/gui/sprites/boss_bar/yellow_progress.png"),
    ("boss_bar/purple_background", "minecraft/textures/gui/sprites/boss_bar/purple_background.png"),
    ("boss_bar/purple_progress", "minecraft/textures/gui/sprites/boss_bar/purple_progress.png"),
    ("boss_bar/white_background", "minecraft/textures/gui/sprites/boss_bar/white_background.png"),
    ("boss_bar/white_progress", "minecraft/textures/gui/sprites/boss_bar/white_progress.png"),
    ("boss_bar/notched_6_background", "minecraft/textures/gui/sprites/boss_bar/notched_6_background.png"),
    ("boss_bar/notched_6_progress", "minecraft/textures/gui/sprites/boss_bar/notched_6_progress.png"),
    ("boss_bar/notched_10_background", "minecraft/textures/gui/sprites/boss_bar/notched_10_background.png"),
    ("boss_bar/notched_10_progress", "minecraft/textures/gui/sprites/boss_bar/notched_10_progress.png"),
    ("boss_bar/notched_12_background", "minecraft/textures/gui/sprites/boss_bar/notched_12_background.png"),
    ("boss_bar/notched_12_progress", "minecraft/textures/gui/sprites/boss_bar/notched_12_progress.png"),
    ("boss_bar/notched_20_background", "minecraft/textures/gui/sprites/boss_bar/notched_20_background.png"),
    ("boss_bar/notched_20_progress", "minecraft/textures/gui/sprites/boss_bar/notched_20_progress.png"),
    // Ping-Balken der Tab-Liste (PlayerListHud.renderLatencyIcon), je 10x8.
    ("icon/ping_1", "minecraft/textures/gui/sprites/icon/ping_1.png"),
    ("icon/ping_2", "minecraft/textures/gui/sprites/icon/ping_2.png"),
    ("icon/ping_3", "minecraft/textures/gui/sprites/icon/ping_3.png"),
    ("icon/ping_4", "minecraft/textures/gui/sprites/icon/ping_4.png"),
    ("icon/ping_5", "minecraft/textures/gui/sprites/icon/ping_5.png"),
    ("icon/ping_unknown", "minecraft/textures/gui/sprites/icon/ping_unknown.png"),
    // Zaubertisch, Amboss, Schleifstein: Sheets und ihre Sprites (EnchantmentScreen,
    // AnvilScreen, GrindstoneScreen).
    ("container/enchanting_table", "minecraft/textures/gui/container/enchanting_table.png"),
    ("container/anvil", "minecraft/textures/gui/container/anvil.png"),
    ("container/grindstone", "minecraft/textures/gui/container/grindstone.png"),
    // Werkbank und die drei Oefen (CraftingScreen, AbstractFurnaceScreen) samt Flamme und Pfeil.
    ("container/crafting_table", "minecraft/textures/gui/container/crafting_table.png"),
    ("container/furnace", "minecraft/textures/gui/container/furnace.png"),
    ("container/blast_furnace", "minecraft/textures/gui/container/blast_furnace.png"),
    ("container/smoker", "minecraft/textures/gui/container/smoker.png"),
    ("container/furnace/lit_progress", "minecraft/textures/gui/sprites/container/furnace/lit_progress.png"),
    ("container/furnace/burn_progress", "minecraft/textures/gui/sprites/container/furnace/burn_progress.png"),
    ("container/blast_furnace/lit_progress", "minecraft/textures/gui/sprites/container/blast_furnace/lit_progress.png"),
    ("container/blast_furnace/burn_progress", "minecraft/textures/gui/sprites/container/blast_furnace/burn_progress.png"),
    ("container/smoker/lit_progress", "minecraft/textures/gui/sprites/container/smoker/lit_progress.png"),
    ("container/smoker/burn_progress", "minecraft/textures/gui/sprites/container/smoker/burn_progress.png"),
    ("container/enchanting_table/enchantment_slot", "minecraft/textures/gui/sprites/container/enchanting_table/enchantment_slot.png"),
    ("container/enchanting_table/enchantment_slot_disabled", "minecraft/textures/gui/sprites/container/enchanting_table/enchantment_slot_disabled.png"),
    ("container/enchanting_table/enchantment_slot_highlighted", "minecraft/textures/gui/sprites/container/enchanting_table/enchantment_slot_highlighted.png"),
    ("container/enchanting_table/level_1", "minecraft/textures/gui/sprites/container/enchanting_table/level_1.png"),
    ("container/enchanting_table/level_2", "minecraft/textures/gui/sprites/container/enchanting_table/level_2.png"),
    ("container/enchanting_table/level_3", "minecraft/textures/gui/sprites/container/enchanting_table/level_3.png"),
    ("container/enchanting_table/level_1_disabled", "minecraft/textures/gui/sprites/container/enchanting_table/level_1_disabled.png"),
    ("container/enchanting_table/level_2_disabled", "minecraft/textures/gui/sprites/container/enchanting_table/level_2_disabled.png"),
    ("container/enchanting_table/level_3_disabled", "minecraft/textures/gui/sprites/container/enchanting_table/level_3_disabled.png"),
    ("container/anvil/text_field", "minecraft/textures/gui/sprites/container/anvil/text_field.png"),
    ("container/anvil/text_field_disabled", "minecraft/textures/gui/sprites/container/anvil/text_field_disabled.png"),
    ("container/anvil/error", "minecraft/textures/gui/sprites/container/anvil/error.png"),
    ("container/grindstone/error", "minecraft/textures/gui/sprites/container/grindstone/error.png"),
    // Handelsbildschirm (MerchantScreen): 512x256-Sheet, davon 276x166 benutzt, samt Sprites
    // und dem Knopf der Angebotsliste.
    ("container/villager", "minecraft/textures/gui/container/villager.png"),
    ("container/villager/out_of_stock", "minecraft/textures/gui/sprites/container/villager/out_of_stock.png"),
    ("container/villager/experience_bar_background", "minecraft/textures/gui/sprites/container/villager/experience_bar_background.png"),
    ("container/villager/experience_bar_current", "minecraft/textures/gui/sprites/container/villager/experience_bar_current.png"),
    ("container/villager/experience_bar_result", "minecraft/textures/gui/sprites/container/villager/experience_bar_result.png"),
    ("container/villager/scroller", "minecraft/textures/gui/sprites/container/villager/scroller.png"),
    ("container/villager/scroller_disabled", "minecraft/textures/gui/sprites/container/villager/scroller_disabled.png"),
    ("container/villager/trade_arrow", "minecraft/textures/gui/sprites/container/villager/trade_arrow.png"),
    ("container/villager/trade_arrow_out_of_stock", "minecraft/textures/gui/sprites/container/villager/trade_arrow_out_of_stock.png"),
    // Rezeptbuch: Knopf im Inventar und der Schalter "Craftbares zuerst".
    ("recipe_book/button", "minecraft/textures/gui/sprites/recipe_book/button.png"),
    ("recipe_book/button_highlighted", "minecraft/textures/gui/sprites/recipe_book/button_highlighted.png"),
    ("recipe_book/filter_enabled", "minecraft/textures/gui/sprites/recipe_book/filter_enabled.png"),
    ("recipe_book/filter_disabled", "minecraft/textures/gui/sprites/recipe_book/filter_disabled.png"),
    ("recipe_book/filter_enabled_highlighted", "minecraft/textures/gui/sprites/recipe_book/filter_enabled_highlighted.png"),
    ("recipe_book/filter_disabled_highlighted", "minecraft/textures/gui/sprites/recipe_book/filter_disabled_highlighted.png"),
    ("widget/button", "minecraft/textures/gui/sprites/widget/button.png"),
    ("widget/button_highlighted", "minecraft/textures/gui/sprites/widget/button_highlighted.png"),
    ("widget/button_disabled", "minecraft/textures/gui/sprites/widget/button_disabled.png"),
    // Die Zauberschrift des Zaubertischs (font/alt.json) als Sheet; die Zeichen schneidet
    // App::push_alt_text heraus.
    ("font/ascii_sga", "minecraft/textures/font/ascii_sga.png"),
    // Statuseffekte: 24x24-Rahmen der HUD-Anzeige oben rechts, und der Neun-Teile-Rahmen
    // (32x32, Rand 4) der Liste neben dem Inventar.
    ("hud/effect_background", "minecraft/textures/gui/sprites/hud/effect_background.png"),
    ("hud/effect_background_ambient", "minecraft/textures/gui/sprites/hud/effect_background_ambient.png"),
    ("container/inventory/effect_background", "minecraft/textures/gui/sprites/container/inventory/effect_background.png"),
    ("container/inventory/effect_background_ambient", "minecraft/textures/gui/sprites/container/inventory/effect_background_ambient.png"),    // Kreativinventar: die drei Hintergruende (195x136 oben links in 256x256), die Reiter je
    // Reihe, Spalte und Zustand (26x32) und der Schieber (12x15).
    ("container/creative_inventory/tab_items", "minecraft/textures/gui/container/creative_inventory/tab_items.png"),
    ("container/creative_inventory/tab_item_search", "minecraft/textures/gui/container/creative_inventory/tab_item_search.png"),
    ("container/creative_inventory/tab_inventory", "minecraft/textures/gui/container/creative_inventory/tab_inventory.png"),
    ("container/creative_inventory/tab_top_selected_1", "minecraft/textures/gui/sprites/container/creative_inventory/tab_top_selected_1.png"),
    ("container/creative_inventory/tab_top_selected_2", "minecraft/textures/gui/sprites/container/creative_inventory/tab_top_selected_2.png"),
    ("container/creative_inventory/tab_top_selected_3", "minecraft/textures/gui/sprites/container/creative_inventory/tab_top_selected_3.png"),
    ("container/creative_inventory/tab_top_selected_4", "minecraft/textures/gui/sprites/container/creative_inventory/tab_top_selected_4.png"),
    ("container/creative_inventory/tab_top_selected_5", "minecraft/textures/gui/sprites/container/creative_inventory/tab_top_selected_5.png"),
    ("container/creative_inventory/tab_top_selected_6", "minecraft/textures/gui/sprites/container/creative_inventory/tab_top_selected_6.png"),
    ("container/creative_inventory/tab_top_selected_7", "minecraft/textures/gui/sprites/container/creative_inventory/tab_top_selected_7.png"),
    ("container/creative_inventory/tab_top_unselected_1", "minecraft/textures/gui/sprites/container/creative_inventory/tab_top_unselected_1.png"),
    ("container/creative_inventory/tab_top_unselected_2", "minecraft/textures/gui/sprites/container/creative_inventory/tab_top_unselected_2.png"),
    ("container/creative_inventory/tab_top_unselected_3", "minecraft/textures/gui/sprites/container/creative_inventory/tab_top_unselected_3.png"),
    ("container/creative_inventory/tab_top_unselected_4", "minecraft/textures/gui/sprites/container/creative_inventory/tab_top_unselected_4.png"),
    ("container/creative_inventory/tab_top_unselected_5", "minecraft/textures/gui/sprites/container/creative_inventory/tab_top_unselected_5.png"),
    ("container/creative_inventory/tab_top_unselected_6", "minecraft/textures/gui/sprites/container/creative_inventory/tab_top_unselected_6.png"),
    ("container/creative_inventory/tab_top_unselected_7", "minecraft/textures/gui/sprites/container/creative_inventory/tab_top_unselected_7.png"),
    ("container/creative_inventory/tab_bottom_selected_1", "minecraft/textures/gui/sprites/container/creative_inventory/tab_bottom_selected_1.png"),
    ("container/creative_inventory/tab_bottom_selected_2", "minecraft/textures/gui/sprites/container/creative_inventory/tab_bottom_selected_2.png"),
    ("container/creative_inventory/tab_bottom_selected_3", "minecraft/textures/gui/sprites/container/creative_inventory/tab_bottom_selected_3.png"),
    ("container/creative_inventory/tab_bottom_selected_4", "minecraft/textures/gui/sprites/container/creative_inventory/tab_bottom_selected_4.png"),
    ("container/creative_inventory/tab_bottom_selected_5", "minecraft/textures/gui/sprites/container/creative_inventory/tab_bottom_selected_5.png"),
    ("container/creative_inventory/tab_bottom_selected_6", "minecraft/textures/gui/sprites/container/creative_inventory/tab_bottom_selected_6.png"),
    ("container/creative_inventory/tab_bottom_selected_7", "minecraft/textures/gui/sprites/container/creative_inventory/tab_bottom_selected_7.png"),
    ("container/creative_inventory/tab_bottom_unselected_1", "minecraft/textures/gui/sprites/container/creative_inventory/tab_bottom_unselected_1.png"),
    ("container/creative_inventory/tab_bottom_unselected_2", "minecraft/textures/gui/sprites/container/creative_inventory/tab_bottom_unselected_2.png"),
    ("container/creative_inventory/tab_bottom_unselected_3", "minecraft/textures/gui/sprites/container/creative_inventory/tab_bottom_unselected_3.png"),
    ("container/creative_inventory/tab_bottom_unselected_4", "minecraft/textures/gui/sprites/container/creative_inventory/tab_bottom_unselected_4.png"),
    ("container/creative_inventory/tab_bottom_unselected_5", "minecraft/textures/gui/sprites/container/creative_inventory/tab_bottom_unselected_5.png"),
    ("container/creative_inventory/tab_bottom_unselected_6", "minecraft/textures/gui/sprites/container/creative_inventory/tab_bottom_unselected_6.png"),
    ("container/creative_inventory/tab_bottom_unselected_7", "minecraft/textures/gui/sprites/container/creative_inventory/tab_bottom_unselected_7.png"),
    ("container/creative_inventory/scroller", "minecraft/textures/gui/sprites/container/creative_inventory/scroller.png"),
    ("container/creative_inventory/scroller_disabled", "minecraft/textures/gui/sprites/container/creative_inventory/scroller_disabled.png"),
    // Zuschauer-Menue (SpectatorGui): Symbole der Kategorien und der Blaetter-/Schliessen-Plaetze.
    ("spectator/teleport_to_player", "minecraft/textures/gui/sprites/spectator/teleport_to_player.png"),
    ("spectator/teleport_to_team", "minecraft/textures/gui/sprites/spectator/teleport_to_team.png"),
    ("spectator/close", "minecraft/textures/gui/sprites/spectator/close.png"),
    ("spectator/scroll_left", "minecraft/textures/gui/sprites/spectator/scroll_left.png"),
    ("spectator/scroll_right", "minecraft/textures/gui/sprites/spectator/scroll_right.png"),
    // Buendel-Tooltip (ClientBundleTooltip): Platz je Item, gewaehlter Platz (hinter und vor
    // dem Item) und der Fuellbalken.
    ("container/bundle/slot_background", "minecraft/textures/gui/sprites/container/bundle/slot_background.png"),
    ("container/bundle/slot_highlight_back", "minecraft/textures/gui/sprites/container/bundle/slot_highlight_back.png"),
    ("container/bundle/slot_highlight_front", "minecraft/textures/gui/sprites/container/bundle/slot_highlight_front.png"),
    ("container/bundle/bundle_progressbar_border", "minecraft/textures/gui/sprites/container/bundle/bundle_progressbar_border.png"),
    ("container/bundle/bundle_progressbar_fill", "minecraft/textures/gui/sprites/container/bundle/bundle_progressbar_fill.png"),
    ("container/bundle/bundle_progressbar_full", "minecraft/textures/gui/sprites/container/bundle/bundle_progressbar_full.png"),
    // Buch (BookViewScreen): das 256er-Sheet (192x192 benutzt) und die Blaetter-Knoepfe.
    ("book", "minecraft/textures/gui/book.png"),
    ("widget/page_forward", "minecraft/textures/gui/sprites/widget/page_forward.png"),
    ("widget/page_forward_highlighted", "minecraft/textures/gui/sprites/widget/page_forward_highlighted.png"),
    ("widget/page_backward", "minecraft/textures/gui/sprites/widget/page_backward.png"),
    ("widget/page_backward_highlighted", "minecraft/textures/gui/sprites/widget/page_backward_highlighted.png"),
];

// Die Symbole der Statuseffekte (textures/mob_effect/<name>.png), im Atlas als
// "mob_effect/<name>" -- Reihenfolge und Namen wie protocol::packets::MOB_EFFECTS.
pub const MOB_EFFECT_SPRITES: [&str; 40] = [
    "speed", "slowness", "haste", "mining_fatigue", "strength", "instant_health", "instant_damage", "jump_boost", "nausea",
    "regeneration", "resistance", "fire_resistance", "water_breathing", "invisibility", "blindness", "night_vision", "hunger",
    "weakness", "poison", "wither", "health_boost", "absorption", "saturation", "glowing", "levitation", "luck", "unluck",
    "slow_falling", "conduit_power", "dolphins_grace", "bad_omen", "hero_of_the_village", "darkness", "trial_omen", "raid_omen",
    "wind_charged", "weaving", "oozing", "infested", "breath_of_the_nautilus",
];

// Name der rein weissen Flaeche im UI-Atlas (s. build). Zum Sampeln nur die Mitte nehmen
// (`sub(1, 1, 2, 2)`), damit der Rand nicht mit dem Nachbarn verschwimmt.
pub const UI_WHITE: &str = "white";

// Ein Bild im Atlas: normierte UVs plus seine Groesse in Vanilla-Pixeln. Bringt ein Pack
// ein feiner gerastertes Bild (inventory.png in 512x512 statt 256x256), bleibt die Groesse
// die von Vanilla -- wie in Vanilla, das GUI-Bilder immer ueber die Vanilla-Masse adressiert
// und das Pack-Bild nur dichter abtastet.
#[derive(Clone, Copy, Debug)]
pub struct AtlasRect {
    pub uv: UvRect,
    pub width: u32,
    pub height: u32,
}

impl AtlasRect {
    // Teilrechteck eines Sheets (Vanilla-GUI-Sheets enthalten mehrere Elemente in einer
    // Datei, z.B. inventory.png: 176x166 Hintergrund oben links in einer 256x256-Datei).
    // x/y/w/h in Vanilla-Pixeln des Sheets, auch bei einem hochaufgeloesten Pack-Bild.
    pub fn sub(&self, x: u32, y: u32, w: u32, h: u32) -> UvRect {
        let [u0, v0, u1, v1] = self.uv;
        let du = (u1 - u0) / self.width as f32;
        let dv = (v1 - v0) / self.height as f32;
        [u0 + x as f32 * du, v0 + y as f32 * dv, u0 + (x + w) as f32 * du, v0 + (y + h) as f32 * dv]
    }
}

pub struct UiAtlas {
    // RGBA8, Zeile fuer Zeile von oben.
    pub rgba: Vec<u8>,
    pub size: u32,
    rects: HashMap<String, AtlasRect>,
    // Item-Texturen mit .mcmeta-Animation: im Atlas liegt der erste Frame, die App laedt im
    // Spieltakt den jeweils aktuellen hoch (wie bei Bloecken und Items in der Hand).
    pub animations: Vec<crate::blocks::TileAnimation>,
}

impl UiAtlas {
    pub fn rect(&self, name: &str) -> Option<&AtlasRect> {
        self.rects.get(name)
    }
}

// Eine Lage/Flaeche eines Icons: Atlas-Ausschnitt plus die Farbe, mit der sie multipliziert
// wird. Ohne Toenung ist die Farbe Weiss.
#[derive(Clone, Copy, Debug)]
pub struct IconLayer {
    pub uv: UvRect,
    pub tint: [f32; 3],
    // Toenungsquelle "minecraft:potion": die Farbe haengt am Stack (potion_contents), `tint`
    // ist nur der "default" fuer einen Stack ohne diese Component. Der Zeichner ersetzt sie.
    pub potion_tint: bool,
}

impl IconLayer {
    pub fn untinted(uv: UvRect) -> Self {
        Self { uv, tint: [1.0, 1.0, 1.0], potion_tint: false }
    }
}

// Eine Flaeche eines 3D-Item-Modells fuers Inventar (s. ItemIcon::Model): Ecken im
// Modellraum (Bloecke, 0..1), Normale, Atlas-Ausschnitt im UI-Atlas und die Flaechenrichtung
// vor der Elementdrehung.
#[derive(Clone, Copy, Debug)]
pub struct IconQuad {
    pub corners: [[f32; 3]; 4],
    pub normal: [f32; 3],
    pub uv: UvRect,
    pub face: usize,
}

// Wie ein Item im Inventar dargestellt wird.
#[derive(Clone, Debug)]
pub enum ItemIcon {
    // Eigenes 3D-Modell aus Elementen (Resourcepacks, s. model_quads): die Flaechen plus der
    // "gui"-Eintrag des Modells, mit dem Vanilla es im Inventar dreht und skaliert.
    Model { quads: Vec<IconQuad>, gui: ItemDisplay },
    // Flache Sprites aus dem UI-Atlas -- layer0 zuerst, weitere Lagen darueber (z.B.
    // Trank-Fuellung unter der Glasflasche). Der "gui"-Eintrag gilt auch fuer sie: Vanilla
    // zeichnet ein flaches Item im Inventar ebenso durch seine gui-Transformation wie ein
    // 3D-Modell (Packs ruecken mit der translation z.B. Schwerter
    // um einen Pixel).
    Sprite { layers: Vec<IconLayer>, gui: ItemDisplay },
    // Blockitem: die sechs Flaechen aus dem BLOCK-Atlas (Reihenfolge FACE_*), aus denen die
    // App einen isometrischen Wuerfel baut.
    Block {
        faces: Box<[IconLayer; 6]>,
        // Zweite, getoente Lage auf derselben Flaeche -- in Vanilla die Grasblock-Seiten:
        // Basistextur (Erde) ungetoent, darueber ein getoentes Overlay. Ohne das waere ein
        // Grasblock im Inventar oben gruen und an den Seiten reine Erde.
        overlay: Box<[Option<IconLayer>; 6]>,
    },
}

// items/<name>.json (Item-Modell-Definition seit 1.21.4). Uns interessiert das Modell und
// die Toenungsliste -- die Varianten-Logik (zustandsabhaengige Modelle wie beim Kompass)
// nehmen wir jeweils beim ersten Eintrag ab.
#[derive(Deserialize)]
struct ItemDefinitionJson {
    model: Option<serde_json::Value>,
}

// Ein Eintrag aus "tints": Vanillas Farbquelle fuer den jeweiligen tintindex.
//
// Umgesetzt sind die beiden Sorten, die konstant sind und deshalb ohne Spielzustand
// auskommen:
//   * minecraft:grass -- Stichprobe aus colormap/grass.png an fest im Modell stehenden
//     Klima-Werten (Grasblock/Gras/Farn/Busch: temperature 0.5, downfall 1.0). Genau
//     deshalb ist ein Grasblock im Inventar IMMER gleich gruen, unabhaengig vom Biom, in
//     dem man gerade steht.
//   * minecraft:constant -- fester ARGB-Wert (Blaetter, Seerosenblatt).
// Alle anderen (dye, potion, firework, map_color, ...) haengen an Item-Komponenten des
// konkreten Stacks; wo das JSON einen "default" mitgibt, nehmen wir den -- das ist genau
// die Farbe, die Vanilla ohne die jeweilige Komponente zeigt. Sonst bleibt es ungetoent.
fn resolve_tint(entry: &serde_json::Value, grass: Option<&crate::Colormap>) -> Option<[f32; 3]> {
    let obj = entry.as_object()?;
    let kind = obj.get("type").and_then(|t| t.as_str()).map(strip_ns).unwrap_or("");
    let from_argb = |v: i64| {
        let v = v as u32;
        [((v >> 16) & 0xff) as f32 / 255.0, ((v >> 8) & 0xff) as f32 / 255.0, (v & 0xff) as f32 / 255.0]
    };
    match kind {
        "grass" => {
            let temperature = obj.get("temperature").and_then(|v| v.as_f64()).unwrap_or(0.5) as f32;
            let downfall = obj.get("downfall").and_then(|v| v.as_f64()).unwrap_or(1.0) as f32;
            grass.map(|map| map.sample(temperature, downfall))
        }
        _ => obj.get("value").or_else(|| obj.get("default")).and_then(|v| v.as_i64()).map(from_argb),
    }
}

// Bei `select` auf `minecraft:display_context` der Fall, der fuer diesen Anzeigeort gilt
// ("gui", "ground", "thirdperson_righthand", ... -- dieselben Namen wie die display-Eintraege
// der Modelle). Ohne das zeigte der Speer im Inventar sein langes Hand-Modell -- dessen
// items/*.json waehlt fuer gui/ground/fixed ein eigenes Bild, sonst (fallback) das Hand-Modell.
fn display_context_case<'a>(obj: &'a serde_json::Map<String, serde_json::Value>, ctx: Option<&str>) -> Option<&'a serde_json::Value> {
    let ctx = ctx?;
    if obj.get("property").and_then(|p| p.as_str()).map(strip_ns) != Some("display_context") {
        return None;
    }
    obj.get("cases")?.as_array()?.iter().find_map(|case| {
        let passt = match case.get("when")? {
            serde_json::Value::String(w) => w == ctx,
            serde_json::Value::Array(ws) => ws.iter().any(|w| w.as_str() == Some(ctx)),
            _ => false,
        };
        passt.then(|| case.get("model")).flatten()
    })
}

const HAND_CONTEXT: &str = "thirdperson_righthand";

// Aus dem verschachtelten "model"-Objekt den ersten konkreten Modellpfad ziehen. `ctx` ist der
// Anzeigeort (s. display_context_case), None = egal.
fn first_model_ref(value: &serde_json::Value, depth: u32, ctx: Option<&str>) -> Option<String> {
    if depth > 8 {
        return None;
    }
    let obj = value.as_object()?;
    // type: minecraft:model -> direkter Modellverweis
    if let Some(model) = obj.get("model").and_then(|m| m.as_str()) {
        return Some(strip_ns(model).to_string());
    }
    // special: "base" ist der Modellpfad selbst (Schild).
    if let Some(base) = obj.get("base").and_then(|m| m.as_str()) {
        return Some(strip_ns(base).to_string());
    }
    if let Some(fall) = display_context_case(obj, ctx) {
        return first_model_ref(fall, depth + 1, ctx);
    }
    // composite/select/range_dispatch/condition/special: den Zweig fuer den RUHEZUSTAND --
    // on_false vor on_true, fallback vor den Faellen. ECHTER BUG gewesen: on_true kam zuerst,
    // damit zeigte das Inventar den aktiven Zustand (Bogen/Armbrust gespannt, Angel
    // ausgeworfen, Elytra kaputt).
    for key in ["model", "base", "on_false", "fallback", "on_true"] {
        if let Some(nested) = obj.get(key) {
            if let Some(found) = first_model_ref(nested, depth + 1, ctx) {
                return Some(found);
            }
        }
    }
    for key in ["models", "cases", "entries"] {
        if let Some(array) = obj.get(key).and_then(|a| a.as_array()) {
            for entry in array {
                let candidate = entry.get("model").unwrap_or(entry);
                if let Some(found) = first_model_ref(candidate, depth + 1, ctx) {
                    return Some(found);
                }
            }
        }
    }
    None
}

// Modellverweis UND Toenungsliste eines Items. Die Liste ist nach tintindex geordnet:
// Eintrag i faerbt jede Modellflaeche bzw. Sprite-Lage mit "tintindex": i.
// Dazu je tintindex, ob die Quelle "minecraft:potion" ist (s. IconLayer::potion_tint).
fn item_model_ref(
    mc: &mut MinecraftInstallation,
    item: &str,
    grass: Option<&crate::Colormap>,
    ctx: Option<&str>,
) -> (String, Vec<Option<[f32; 3]>>, Vec<bool>) {
    // `item` ist ein Vanilla-Itemname ("stone") oder die ID einer Item-Definition aus einem
    // Resourcepack ("uhc:storm_wall", s. world::custom_model_id).
    let fallback = match item.split_once(':') {
        Some((ns, pfad)) => format!("{ns}:item/{pfad}"),
        None => format!("item/{item}"),
    };
    let Ok(bytes) = mc.load_bytes(&crate::blocks::resource_path("items", item, ".json")) else {
        return (fallback, Vec::new(), Vec::new());
    };
    let Ok(def) = serde_json::from_slice::<ItemDefinitionJson>(&bytes) else {
        return (fallback, Vec::new(), Vec::new());
    };
    let model = def.model.as_ref().and_then(|m| first_model_ref(m, 0, ctx)).unwrap_or(fallback);
    let list = def.model.as_ref().and_then(|m| first_tints(m, 0, ctx)).unwrap_or_default();
    let tints = list.iter().map(|e| resolve_tint(e, grass)).collect();
    let potion = list
        .iter()
        .map(|e| e.get("type").and_then(|t| t.as_str()).map(strip_ns) == Some("potion"))
        .collect();
    (model, tints, potion)
}

// Die Toenungen einer Item-Definition fuer die Welt (Hand-Kontext), je tintindex: eine feste
// Farbe (constant, Vorgabewert anderer Quellen), und ob die Lage die Trankfarbe traegt.
pub fn item_tints(mc: &mut MinecraftInstallation, item: &str) -> Vec<(Option<[f32; 3]>, bool)> {
    let (_model, tints, potion) = item_model_ref(mc, item, None, Some(HAND_CONTEXT));
    tints.into_iter().enumerate().map(|(i, t)| (t, potion.get(i).copied().unwrap_or(false))).collect()
}

// Die "tints"-Liste zum ersten konkreten Modell -- dieselbe Zweigwahl wie first_model_ref,
// damit Modell und Toenung garantiert vom selben Eintrag stammen.
fn first_tints(value: &serde_json::Value, depth: u32, ctx: Option<&str>) -> Option<Vec<serde_json::Value>> {
    if depth > 8 {
        return None;
    }
    let obj = value.as_object()?;
    if let Some(fall) = display_context_case(obj, ctx) {
        return first_tints(fall, depth + 1, ctx);
    }
    if obj.get("model").and_then(|m| m.as_str()).is_some() {
        return Some(obj.get("tints").and_then(|t| t.as_array()).cloned().unwrap_or_default());
    }
    if obj.get("base").and_then(|m| m.as_str()).is_some() {
        return Some(obj.get("tints").and_then(|t| t.as_array()).cloned().unwrap_or_default());
    }
    for key in ["model", "base", "on_false", "fallback", "on_true"] {
        if let Some(found) = obj.get(key).and_then(|nested| first_tints(nested, depth + 1, ctx)) {
            return Some(found);
        }
    }
    for key in ["models", "cases", "entries"] {
        if let Some(array) = obj.get(key).and_then(|a| a.as_array()) {
            for entry in array {
                let candidate = entry.get("model").unwrap_or(entry);
                if let Some(found) = first_tints(candidate, depth + 1, ctx) {
                    return Some(found);
                }
            }
        }
    }
    None
}

// Ein Bild fuer den Packer.
struct Pending {
    name: String,
    width: u32,
    height: u32,
    rgba: Vec<u8>,
    // Groesse in Vanilla-Pixeln (s. AtlasRect); ohne Pack-Bild gleich width/height.
    logical: (u32, u32),
}

fn load_image(mc: &mut MinecraftInstallation, path: &str) -> Option<Pending> {
    let tex = mc.load_texture(path).ok()?;
    if tex.width == 0 || tex.height == 0 {
        return None;
    }
    // Animierte Item-Texturen sind vertikale Streifen -- nur der erste Frame (wie bei den
    // Blocktexturen, s. blocks::load_tile).
    let height = if tex.height > tex.width && tex.height % tex.width == 0 { tex.width } else { tex.height };
    let rgba = tex.rgba[..(tex.width * height * 4) as usize].to_vec();
    Some(Pending { name: String::new(), width: tex.width, height, rgba, logical: (tex.width, height) })
}

// Ein GUI-Bild: wie intern, nur mit der Vanilla-Groesse als logischem Raster, wenn ein Pack
// es in anderer Aufloesung bringt.
fn intern_gui(mc: &mut MinecraftInstallation, name: &str, path: &str, pending: &mut Vec<Pending>, by_name: &mut HashMap<String, usize>) {
    if !intern(mc, name.to_string(), path.to_string(), pending, by_name) {
        return;
    }
    let Some(&i) = by_name.get(name) else { return };
    let image = &mut pending[i];
    if let Some((vw, _)) = mc.vanilla_image_size(path) {
        image.logical = logical_size(image.width, image.height, vw);
    }
}

// Vanilla-Masse eines Bildes, das ein Pack in Breite `breite` statt `vanilla_breite` bringt.
// Die Hoehe folgt demselben Faktor (das Pack-Bild kann auch nur den ersten Frame eines
// Streifens enthalten, s. load_image).
fn logical_size(breite: u32, hoehe: u32, vanilla_breite: u32) -> (u32, u32) {
    if vanilla_breite == 0 || breite == vanilla_breite {
        return (breite, hoehe);
    }
    let faktor = breite as f32 / vanilla_breite as f32;
    (vanilla_breite, ((hoehe as f32 / faktor).round() as u32).max(1))
}

// Ein 4x-Pack mit allen Container-Sheets passt nicht in den Atlas (30 Sheets zu 1024x1024
// sind fast doppelt so viel Flaeche wie 4096x4096), und der Packer liesse dann Bilder weg.
// Stattdessen die am feinsten gerasterten Bilder halbieren, bis alles Platz hat -- sie bleiben
// mindestens so fein wie Vanilla.
fn shrink_to_fit(pending: &mut [Pending]) {
    let budget = (MAX_ATLAS_SIZE as u64 * MAX_ATLAS_SIZE as u64) * 7 / 10;
    loop {
        let flaeche: u64 = pending.iter().map(|p| ((p.width + PADDING) as u64) * ((p.height + PADDING) as u64)).sum();
        if flaeche <= budget {
            return;
        }
        let dichte = |p: &Pending| p.width / p.logical.0.max(1);
        let Some(max) = pending.iter().map(dichte).max().filter(|&d| d >= 2) else { return };
        for p in pending.iter_mut().filter(|p| dichte(p) == max) {
            halve(p);
        }
    }
}

// Auf halbe Kantenlaenge, je 2x2 Pixel gemittelt (Farbe nach Deckkraft gewichtet, damit
// durchsichtige Pixel keinen dunklen Rand hinterlassen).
fn halve(p: &mut Pending) {
    let (w, h) = ((p.width / 2).max(1), (p.height / 2).max(1));
    let mut out = vec![0u8; (w * h * 4) as usize];
    for y in 0..h {
        for x in 0..w {
            let mut summe = [0u32; 4];
            for (dx, dy) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
                let (sx, sy) = ((x * 2 + dx).min(p.width - 1), (y * 2 + dy).min(p.height - 1));
                let i = ((sy * p.width + sx) * 4) as usize;
                let a = p.rgba[i + 3] as u32;
                for c in 0..3 {
                    summe[c] += p.rgba[i + c] as u32 * a;
                }
                summe[3] += a;
            }
            let o = ((y * w + x) * 4) as usize;
            if summe[3] > 0 {
                for c in 0..3 {
                    out[o + c] = (summe[c] / summe[3]) as u8;
                }
            }
            out[o + 3] = (summe[3] / 4) as u8;
        }
    }
    p.width = w;
    p.height = h;
    p.rgba = out;
}

// Baut den Atlas und loest gleichzeitig fuer jeden Itemnamen sein Icon auf.
pub fn build(
    mc: &mut MinecraftInstallation,
    item_names: &[String],
    block_atlas: &BlockAtlas,
) -> (UiAtlas, HashMap<String, ItemIcon>, HashMap<String, crate::ItemDefinition>) {
    let mut pending: Vec<Pending> = Vec::new();
    let mut by_name: HashMap<String, usize> = HashMap::new();


    for (name, path) in GUI_SHEETS {
        intern_gui(mc, name, path, &mut pending, &mut by_name);
    }
    for name in MOB_EFFECT_SPRITES {
        intern_gui(mc, &format!("mob_effect/{name}"), &format!("minecraft/textures/mob_effect/{name}.png"), &mut pending, &mut by_name);
    }
    // Einfarbig weisse Flaeche fuer getoente Schleier ueber Sprites (Abklingzeit im Slot).
    // Rechtecke taugen dafuer nicht: sie werden VOR den Sprites gezeichnet.
    pending.push(Pending { name: UI_WHITE.to_string(), width: 4, height: 4, rgba: vec![255; 16 * 4], logical: (4, 4) });

    // Gras-Colormap fuer Toenungen vom Typ minecraft:grass (s. resolve_tint). Fehlt sie,
    // bleiben die betroffenen Icons ungetoent statt gar nicht zu erscheinen.
    let grass_colormap = crate::Colormap::load(mc, "grass").ok();

    let mut s = Sammlung::default();
    // Item-Definitionen aus Packs mit eigenem Namensraum (Server-Resourcepacks: "uhc:storm_wall")
    // -- ihre Icons braucht die item_model-Component.
    let mut ids: Vec<String> = item_names.to_vec();
    ids.extend(mc.item_definition_ids().into_iter().filter(|id| !id.starts_with("minecraft:")));
    for item in &ids {
        if item == "air" {
            continue;
        }
        // Das Icon unter dem Itemnamen: der Ruhezustand (s. first_model_ref).
        let (model, tints, potion_tints) = item_model_ref(mc, item, grass_colormap.as_ref(), Some("gui"));
        resolve_icon(mc, item, item.clone(), &model, &tints, &potion_tints, block_atlas, &mut pending, &mut by_name, &mut s);
    }
    // Dazu jeder Zweig jeder Definition unter "<item>@<modell>" -- welcher gilt, entscheidet erst
    // die Auswertung je Stapel (s. item_model).
    let mut definitions = HashMap::new();
    for item in &ids {
        let Ok(bytes) = mc.load_bytes(&crate::blocks::resource_path("items", item, ".json")) else { continue };
        let Some(mut def) = crate::ItemDefinition::parse(&bytes) else { continue };
        for leaf in &def.leaves {
            let key = leaf_icon_key(item, &leaf.model);
            if !s.has(&key) {
                let tints: Vec<Option<[f32; 3]>> = leaf.tints.iter().map(|e| resolve_tint(e, grass_colormap.as_ref())).collect();
                let potion: Vec<bool> = leaf.tints.iter().map(|e| e.get("type").and_then(|t| t.as_str()).map(strip_ns) == Some("potion")).collect();
                resolve_icon(mc, item, key.clone(), &leaf.model, &tints, &potion, block_atlas, &mut pending, &mut by_name, &mut s);
            }
            def.icon_keys.push(key);
        }
        definitions.insert(item.clone(), def);
    }
    let Sammlung { sprite_layers, block_items, model_items } = s;

    shrink_to_fit(&mut pending);
    let (rgba, size, rects) = pack(&pending);
    let mut icons: HashMap<String, ItemIcon> = HashMap::new();
    for (item, (layers, tints, potion_tints, gui)) in sprite_layers {
        // Lage i traegt tintindex i (so erzeugt Vanilla die generierten Item-Modelle):
        // beim Trank faerbt tints[0] die Fuellung, die Glasflasche darueber bleibt weiss.
        let resolved: Vec<IconLayer> = layers
            .iter()
            .enumerate()
            .filter_map(|(i, key)| {
                rects.get(key).map(|r| IconLayer {
                    uv: r.uv,
                    tint: tints.get(i).copied().flatten().unwrap_or([1.0, 1.0, 1.0]),
                    potion_tint: potion_tints.get(i).copied().unwrap_or(false),
                })
            })
            .collect();
        // Ohne Lagen nur, wenn das Modell gar keine Textur hatte (s. resolve_icon, Schluss).
        if !resolved.is_empty() || layers.is_empty() {
            icons.insert(item, ItemIcon::Sprite { layers: resolved, gui });
        }
    }
    for (item, (quads, gui)) in model_items {
        // Flaechen-UV (Texturpixel 0..16) in den Atlas-Ausschnitt der Textur umrechnen.
        let quads: Vec<IconQuad> = quads
            .iter()
            .filter_map(|q| {
                let r = rects.get(&format!("tex/{}", q.texture))?.uv;
                let an = |t: f32, a: f32, b: f32| a + (b - a) * t / 16.0;
                let uv = [an(q.uv[0], r[0], r[2]), an(q.uv[1], r[1], r[3]), an(q.uv[2], r[0], r[2]), an(q.uv[3], r[1], r[3])];
                Some(IconQuad { corners: q.corners, normal: q.normal, uv, face: q.face })
            })
            .collect();
        if !quads.is_empty() {
            icons.insert(item, ItemIcon::Model { quads, gui });
        }
    }
    for (item, (faces, face_tints, overlay)) in block_items {
        let layers: [IconLayer; 6] = std::array::from_fn(|i| IconLayer { uv: faces[i], tint: face_tints[i].unwrap_or([1.0, 1.0, 1.0]), potion_tint: false });
        icons.insert(item, ItemIcon::Block { faces: Box::new(layers), overlay: Box::new(overlay) });
    }

    let animations = item_animations(mc, &rects, size);
    (UiAtlas { rgba, size, rects, animations }, icons, definitions)
}

// Animationen der Item-Texturen ("tex/<pfad>") an ihrem Platz im Atlas. Nur quadratische
// Frames, wie im Atlas abgelegt (s. load_image).
fn item_animations(mc: &mut MinecraftInstallation, rects: &HashMap<String, AtlasRect>, size: u32) -> Vec<crate::blocks::TileAnimation> {
    let mut out = Vec::new();
    for (name, rect) in rects {
        let Some(path) = name.strip_prefix("tex/") else { continue };
        let x = (rect.uv[0] * size as f32).round() as u32;
        let y = (rect.uv[1] * size as f32).round() as u32;
        let breite = ((rect.uv[2] - rect.uv[0]) * size as f32).round() as u32;
        let hoehe = ((rect.uv[3] - rect.uv[1]) * size as f32).round() as u32;
        if breite == 0 || breite != hoehe {
            continue;
        }
        if let Some(anim) = crate::blocks::TileAnimation::load(mc, path, (x, y), breite, 1) {
            out.push(anim);
        }
    }
    out
}

// Unter welchem Schluessel das Icon eines Zweigs einer Item-Definition steht.
pub fn leaf_icon_key(item: &str, model: &str) -> String {
    format!("{item}@{model}")
}

// Ein Bild in den Packer geben, einmal je Name. false, wenn es sich nicht laden laesst.
fn intern(mc: &mut MinecraftInstallation, name: String, path: String, pending: &mut Vec<Pending>, by_name: &mut HashMap<String, usize>) -> bool {
    if by_name.contains_key(&name) {
        return true;
    }
    match load_image(mc, &path) {
        Some(mut image) => {
            image.name = name.clone();
            by_name.insert(name, pending.len());
            pending.push(image);
            true
        }
        None => false,
    }
}

// Die Icons, bevor ihre Bilder gepackt sind: je Schluessel Sprite-Lagen, Blockflaechen oder
// Modellflaechen.
#[derive(Default)]
struct Sammlung {
    sprite_layers: HashMap<String, (Vec<String>, Vec<Option<[f32; 3]>>, Vec<bool>, ItemDisplay)>,
    block_items: HashMap<String, ([UvRect; 6], [Option<[f32; 3]>; 6], [Option<IconLayer>; 6])>,
    // Items mit eigenem 3D-Modell: Flaechen + gui-Eintrag; die Texturen kommen in den UI-Atlas.
    model_items: HashMap<String, (Vec<crate::ModelQuad>, ItemDisplay)>,
}

impl Sammlung {
    fn has(&self, key: &str) -> bool {
        self.sprite_layers.contains_key(key) || self.block_items.contains_key(key) || self.model_items.contains_key(key)
    }
}

// Das Icon fuer `model` unter `key` einsammeln. `item` ist der Itemname, unter dem ein
// Blockitem seine Flaechen im Block-Atlas findet.
#[allow(clippy::too_many_arguments)]
fn resolve_icon(
    mc: &mut MinecraftInstallation,
    item: &str,
    key: String,
    model: &str,
    tints: &[Option<[f32; 3]>],
    potion_tints: &[bool],
    block_atlas: &BlockAtlas,
    pending: &mut Vec<Pending>,
    by_name: &mut HashMap<String, usize>,
    s: &mut Sammlung,
) {
    let gui_von = |mc: &mut MinecraftInstallation| crate::blocks::load_model_display(mc, model, "gui").unwrap_or(ItemDisplay::IDENTITY);
    // Ein Modell unter block/ ist ein Blockitem -- die Texturen dafuer stecken schon im
    // Block-Atlas (unter dem Blocknamen, der bei Blockitems dem Itemnamen entspricht).
    if model.starts_with("block/") {
        // Kein voller Wuerfel (Schleifstein, Treppen, Zaeune, ...): Vanilla zeichnet das
        // echte Blockmodell. Als Wuerfel mit den Flaechentexturen sah der Schleifstein
        // falsch aus.
        if let Some(quads) = non_cube_block_quads(mc, model) {
            let alle_da = quads.iter().all(|q| intern(mc, format!("tex/{}", q.texture), crate::blocks::resource_path("textures", &q.texture, ".png"), pending, by_name));
            if alle_da {
                let gui = gui_von(mc);
                s.model_items.insert(key, (quads, gui));
                return;
            }
        }
        if let Some(faces) = block_atlas.uvs.get(item) {
            s.block_items.insert(key, block_entry(mc, item, model, tints, *faces, block_atlas));
            return;
        }
    }
    // Eigenes 3D-Modell (Resourcepack): Vanilla zeichnet es auch im Inventar als Modell,
    // nicht als flaches Bild.
    if let Some(quads) = crate::item_model_quads(mc, model) {
        let alle_da = quads.iter().all(|q| intern(mc, format!("tex/{}", q.texture), crate::blocks::resource_path("textures", &q.texture, ".png"), pending, by_name));
        if alle_da {
            let gui = gui_von(mc);
            s.model_items.insert(key, (quads, gui));
            return;
        }
    }
    let Ok((textures, _elements)) = load_model_chain(mc, model) else {
        return;
    };
    let mut layers = Vec::new();
    for index in 0..8 {
        let Some(path) = textures.get(&format!("layer{index}")).cloned() else {
            break;
        };
        let Some(resolved) = resolve_var(&textures, path) else {
            break;
        };
        let name = format!("tex/{resolved}");
        if intern(mc, name.clone(), crate::blocks::resource_path("textures", &resolved, ".png"), pending, by_name) {
            layers.push(name);
        }
    }
    if !layers.is_empty() {
        let gui = gui_von(mc);
        s.sprite_layers.insert(key, (layers, tints.to_vec(), potion_tints.to_vec(), gui));
        return;
    }
    // Kein layer0 und kein Blockmodell: als letzten Versuch die Flaechen aus dem
    // Block-Atlas (z.B. Bloecke, deren Item-Modell ueber "special" laeuft -- Kisten,
    // Betten, Banner) -- sonst bleibt das Item ohne Icon.
    if let Some(faces) = block_atlas.uvs.get(item) {
        s.block_items.insert(key, block_entry(mc, item, model, tints, *faces, block_atlas));
        return;
    }
    // Uebrig bleiben die echten 3D-Sondermodelle ohne Blockentsprechung (Schild,
    // Fernrohr): kein layer0, kein Block. Statt sie leer zu lassen, die erste Textur
    // des Modells flach zeichnen -- nicht Vanillas Optik, aber ein erkennbares Icon.
    let mut fallback: Vec<&String> = textures.values().collect();
    fallback.sort(); // deterministisch, sonst waehlt die HashMap-Reihenfolge
    if let Some(first) = fallback.first().and_then(|v| resolve_var(&textures, (*v).clone())) {
        let name = format!("tex/{first}");
        if intern(mc, name.clone(), crate::blocks::resource_path("textures", &first, ".png"), pending, by_name) {
            let gui = gui_von(mc);
            s.sprite_layers.insert(key, (vec![name], tints.to_vec(), potion_tints.to_vec(), gui));
            return;
        }
    }
    // Ganz ohne Textur (ein Pack-Schild nur mit "parent": "builtin/entity" und display): der
    // Eintrag mit der gui-Haltung bleibt trotzdem -- das Schild zeichnet ItemIcons ueber sein
    // Entity-Modell, braucht dafuer aber genau diese Haltung. Vorher fehlte er, und das Schild
    // war im Inventar unsichtbar. Nur unter dem Itemnamen selbst: ein leerer Eintrag fuer einen
    // Definitionszweig oder eine item_model-ID stuende sonst ueber dem Rueckfall aufs Item-Icon.
    if key == item {
        let gui = gui_von(mc);
        s.sprite_layers.insert(key, (Vec::new(), tints.to_vec(), potion_tints.to_vec(), gui));
    }
}

// Die Flaechen eines Blockmodells, das KEIN voller Wuerfel ist -- None fuer Wuerfel (auch aus
// mehreren deckungsgleichen Elementen wie beim Grasblock mit Overlay), fuer getoente Modelle
// (die Model-Icons kennen keine Toenung) und fuer Modelle ohne Elemente.
fn non_cube_block_quads(mc: &mut MinecraftInstallation, model: &str) -> Option<Vec<crate::ModelQuad>> {
    let (_textures, elements) = load_model_chain(mc, model).ok()?;
    let elements = elements.filter(|e| !e.is_empty())?;
    let wuerfel = elements.iter().all(|e| e.from == [0.0; 3] && e.to == [16.0; 3]);
    let getoent = elements.iter().any(|e| e.faces.values().any(|f| f.tintindex.is_some()));
    if wuerfel || getoent {
        return None;
    }
    crate::model_quads::block_model_quads(mc, model, 0, 0)
}

// Die drei Bestandteile eines Blockitem-Icons: Basis-UVs, Toenung je Basisflaeche und die
// getoente Overlay-Lage.
//
// Das Modell wird NUR nachgeladen, wenn das Item ueberhaupt eine Toenung deklariert -- das
// sind in Vanilla 1.21.11 genau sieben Blockitems (Grasblock + sechs Laubarten) von rund
// tausend. Ohne diese Abkuerzung waere hier fuer jedes Blockitem eine zweite komplette
// Modellkette zu lesen, nur um dann fast immer "keine Toenung" herauszubekommen.
fn block_entry(
    mc: &mut MinecraftInstallation,
    item: &str,
    model: &str,
    tints: &[Option<[f32; 3]>],
    faces: [UvRect; 6],
    block_atlas: &BlockAtlas,
) -> ([UvRect; 6], [Option<[f32; 3]>; 6], [Option<IconLayer>; 6]) {
    let mut face_tints: [Option<[f32; 3]>; 6] = Default::default();
    let mut overlay: [Option<IconLayer>; 6] = Default::default();
    if tints.iter().all(|t| t.is_none()) {
        return (faces, face_tints, overlay);
    }

    // Welche Flaeche einen tintindex traegt, steht im Blockmodell. Dieselbe Regel wie
    // blocks::resolve_block_faces: fuer jede Richtung gilt die ERSTE Deklaration -- beim
    // Grasblock also die ungetoente Erd-Seite aus Element 0, waehrend das getoente
    // Seiten-Overlay aus Element 1 ueber overlay_uvs kommt.
    if let Ok((_textures, Some(elements))) = load_model_chain(mc, model) {
        // "Erste Deklaration gewinnt" heisst: sobald eine Richtung ueberhaupt vorkam, zaehlt
        // nur deren tintindex. Sonst wuerde beim Grasblock das zweite (Overlay-)Element die
        // Seiten nachtraeglich einfaerben -- die Basisflaeche dort ist aber Erde und bleibt
        // ungetoent, gruen wird sie erst durch die Overlay-Lage darueber.
        let mut declared = [false; 6];
        for element in &elements {
            for (i, name) in crate::blocks::FACE_NAMES.iter().enumerate() {
                if declared[i] {
                    continue;
                }
                let Some(face) = element.faces.get(*name) else { continue };
                declared[i] = true;
                if let Some(index) = face.tintindex {
                    face_tints[i] = tints.get(index.max(0) as usize).copied().flatten();
                }
            }
        }
    }

    // Overlay-Flaechen sind per Definition die getoenten (sie existieren nur, weil das
    // Modell dieselbe Richtung ein zweites Mal MIT tintindex deklariert).
    if let Some(uvs) = block_atlas.overlay_uvs.get(item) {
        let tint = tints.first().copied().flatten().unwrap_or([1.0, 1.0, 1.0]);
        for i in 0..6 {
            overlay[i] = uvs[i].map(|uv| IconLayer { uv, tint, potion_tint: false });
        }
    }
    (faces, face_tints, overlay)
}

// Regal-Packer: nach Hoehe absteigend sortiert in Zeilen legen. Reicht voellig -- der Atlas
// wird einmal beim Start gebaut und ein paar Prozent Verschnitt sind egal.
fn pack(pending: &[Pending]) -> (Vec<u8>, u32, HashMap<String, AtlasRect>) {
    let mut order: Vec<usize> = (0..pending.len()).collect();
    order.sort_by_key(|&i| std::cmp::Reverse((pending[i].height, pending[i].width)));

    let total_area: u64 = pending.iter().map(|p| ((p.width + PADDING) as u64) * ((p.height + PADDING) as u64)).sum();
    let mut size = (total_area as f64).sqrt().ceil() as u32;
    size = size.next_power_of_two().max(64);

    loop {
        if let Some(result) = try_pack(pending, &order, size) {
            return result;
        }
        if size >= MAX_ATLAS_SIZE {
            // Passt selbst in 4096 nicht: mit dem, was hineinpasst, weitermachen -- fehlende
            // Eintraege liefern spaeter schlicht kein Icon.
            return try_pack_lossy(pending, &order, MAX_ATLAS_SIZE);
        }
        size *= 2;
    }
}

fn try_pack(pending: &[Pending], order: &[usize], size: u32) -> Option<(Vec<u8>, u32, HashMap<String, AtlasRect>)> {
    let mut rgba = vec![0u8; (size * size * 4) as usize];
    let mut rects = HashMap::new();
    let (mut x, mut y, mut row_height) = (0u32, 0u32, 0u32);

    for &index in order {
        let image = &pending[index];
        if image.width > size || image.height > size {
            return None;
        }
        if x + image.width > size {
            x = 0;
            y += row_height + PADDING;
            row_height = 0;
        }
        if y + image.height > size {
            return None;
        }
        blit(&mut rgba, size, image, x, y);
        rects.insert(image.name.clone(), atlas_rect(image, x, y, size));
        x += image.width + PADDING;
        row_height = row_height.max(image.height);
    }
    Some((rgba, size, rects))
}

fn try_pack_lossy(pending: &[Pending], order: &[usize], size: u32) -> (Vec<u8>, u32, HashMap<String, AtlasRect>) {
    let mut rgba = vec![0u8; (size * size * 4) as usize];
    let mut rects = HashMap::new();
    let (mut x, mut y, mut row_height) = (0u32, 0u32, 0u32);

    for &index in order {
        let image = &pending[index];
        if image.width > size || image.height > size {
            continue;
        }
        if x + image.width > size {
            x = 0;
            y += row_height + PADDING;
            row_height = 0;
        }
        if y + image.height > size {
            break;
        }
        blit(&mut rgba, size, image, x, y);
        rects.insert(image.name.clone(), atlas_rect(image, x, y, size));
        x += image.width + PADDING;
        row_height = row_height.max(image.height);
    }
    (rgba, size, rects)
}

fn atlas_rect(image: &Pending, x: u32, y: u32, size: u32) -> AtlasRect {
    AtlasRect {
        uv: [
            x as f32 / size as f32,
            y as f32 / size as f32,
            (x + image.width) as f32 / size as f32,
            (y + image.height) as f32 / size as f32,
        ],
        width: image.logical.0,
        height: image.logical.1,
    }
}

fn blit(rgba: &mut [u8], size: u32, image: &Pending, x: u32, y: u32) {
    for row in 0..image.height {
        let src = (row * image.width * 4) as usize;
        let dst = (((y + row) * size + x) * 4) as usize;
        let len = (image.width * 4) as usize;
        rgba[dst..dst + len].copy_from_slice(&image.rgba[src..src + len]);
    }
}

// --- Halte-Transformation eines Items (display.thirdperson_*) --------------------------
//
// Wie ein Item in der HAND eines Spielers sitzt, steht nicht im Code, sondern im
// Modell-JSON: `item/generated` (flache Sprites) und `item/handheld` (Werkzeuge/Schwerter)
// setzen unterschiedliche `display.thirdperson_righthand`-Werte, und ein Resourcepack darf
// beide ueberschreiben. Deshalb wird hier GELESEN statt eingebaut -- ein Pack, das seine
// Schwerter anders in der Hand liegen laesst, wirkt damit automatisch auch bei uns.
//
// Zahlen und Reihenfolge exakt nach Transformation.java (1.21.11 nachgesehen):
//   * translation wird beim Einlesen mit 0.0625 (= 1/16) multipliziert und auf +-5 geklemmt
//   * scale wird auf +-4 geklemmt
//   * angewendet wird T(translation) -> R(XYZ-Euler) -> S(scale) -> T(-0.5,-0.5,-0.5);
//     die letzte Verschiebung zentriert das Modell, dessen Koordinaten von 0 bis 1 laufen
//   * fuer die LINKE Hand kehren translation.x, rotation.y und rotation.z ihr Vorzeichen um
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ItemDisplay {
    pub rotation: [f32; 3],
    // Bereits durch 16 geteilt und geklemmt -- also direkt in Blockeinheiten.
    pub translation: [f32; 3],
    pub scale: [f32; 3],
}

impl ItemDisplay {
    pub const IDENTITY: ItemDisplay = ItemDisplay { rotation: [0.0; 3], translation: [0.0; 3], scale: [1.0; 3] };

    // Spiegelung fuer die linke Hand (Transformation.apply, leftHanded = true).
    pub fn mirrored(self) -> ItemDisplay {
        ItemDisplay {
            rotation: [self.rotation[0], -self.rotation[1], -self.rotation[2]],
            translation: [-self.translation[0], self.translation[1], self.translation[2]],
            scale: self.scale,
        }
    }
}

// Die Transformation fuer die rechte Hand in der 3. Person. Vanilla erbt PRO EINTRAG die
// parent-Kette hoch (JsonUnbakedModel.getTransformation): ein Modell, das nur `gui` setzt,
// bekommt `thirdperson_righthand` weiterhin vom Elternteil. Genau das macht die Schleife --
// sie nimmt den ERSTEN Vorfahren, der den Eintrag ueberhaupt definiert.
pub fn item_hand_display(mc: &mut MinecraftInstallation, item: &str) -> ItemDisplay {
    item_display(mc, item, "thirdperson_righthand")
}

// Dasselbe fuer die eigene Hand in der Ego-Ansicht. Vanilla nimmt dort einen EIGENEN
// display-Eintrag (ItemDisplayContext.FIRST_PERSON_RIGHT_HAND) -- bei `item/handheld` liegt
// das Schwert dadurch anders in der Hand als bei einem fremden Spieler, und ein Pack darf
// beides getrennt setzen.
// ItemDisplayContext.FIXED -- Itemrahmen und Vanillas schwebendes Pop-Totem
// (InGameOverlayRenderer.renderFloatingItem).
pub fn item_fixed_display(mc: &mut MinecraftInstallation, item: &str) -> ItemDisplay {
    item_display(mc, item, "fixed")
}

// ItemDisplayContext.GROUND: liegende Items und Wurfgeschosse (ItemEntityRenderer,
// FlyingItemEntityRenderer).
pub fn item_ground_display(mc: &mut MinecraftInstallation, item: &str) -> ItemDisplay {
    item_display(mc, item, "ground")
}

pub fn item_first_person_display(mc: &mut MinecraftInstallation, item: &str) -> ItemDisplay {
    item_display(mc, item, "firstperson_righthand")
}

// Die LINKSHAND-Eintraege. Vanilla nimmt immer den Eintrag des jeweiligen
// ItemDisplayContext und spiegelt ihn danach (ItemDisplay::mirrored) -- beides zusammen,
// nicht eines statt des anderen. Der Unterschied ist gross: item/handheld setzt links
// [0,90,-55] gegen rechts [0,-90,55], gespiegelt liegt ein Werkzeug links also genau wie
// rechts; die gespiegelte RECHTE Haltung stellte es quer. Fehlt der Eintrag, s.
// blocks::load_model_display.
pub fn item_hand_display_left(mc: &mut MinecraftInstallation, item: &str) -> ItemDisplay {
    item_display(mc, item, "thirdperson_lefthand")
}

pub fn item_first_person_display_left(mc: &mut MinecraftInstallation, item: &str) -> ItemDisplay {
    item_display(mc, item, "firstperson_lefthand")
}

// Die Halte-Transformation eines MODELLS statt eines Items -- fuer die Faelle, in denen
// Vanilla je nach Zustand ein anderes Modell waehlt. Beim Schild ist genau das der ganze
// Unterschied zwischen "haengt an der Seite" und "ist hochgerissen": items/shield.json ist
// eine `condition` auf `minecraft:using_item`, deren zwei Zweige auf item/shield bzw.
// item/shield_blocking zeigen -- zwei Modelle mit voellig verschiedenen display-Eintraegen.
// Ein Resourcepack, das models/item/shield_blocking.json ersetzt, greift damit weiterhin.
// Beliebiger display-Eintrag ("head", "gui", ...) des Item-Modells.
pub fn item_named_display(mc: &mut MinecraftInstallation, item: &str, key: &str) -> ItemDisplay {
    item_display(mc, item, key)
}

pub fn model_hand_display(mc: &mut MinecraftInstallation, model: &str, key: &str) -> ItemDisplay {
    crate::blocks::load_model_display(mc, model, key).unwrap_or(ItemDisplay::IDENTITY)
}

fn item_display(mc: &mut MinecraftInstallation, item: &str, key: &str) -> ItemDisplay {
    let (model, _tints, _) = item_model_ref(mc, item, None, Some(key));
    crate::blocks::load_model_display(mc, &model, key).unwrap_or(ItemDisplay::IDENTITY)
}

#[cfg(test)]
mod display_tests {
    use super::*;

    // Ein Pack-Inventar in doppelter Aufloesung wird in Vanilla-Pixeln adressiert: der
    // Hintergrund (176x166 oben links) bleibt derselbe Bildausschnitt.
    #[test]
    fn hochaufgeloestes_gui_bild_behaelt_vanilla_masse() {
        assert_eq!(logical_size(512, 512, 256), (256, 256));
        assert_eq!(logical_size(1024, 512, 512), (512, 256));
        assert_eq!(logical_size(256, 256, 256), (256, 256));
        let r = AtlasRect { uv: [0.0, 0.0, 0.5, 0.5], width: 256, height: 256 };
        let uv = r.sub(0, 0, 176, 166);
        assert!((uv[2] - 0.5 * 176.0 / 256.0).abs() < 1e-6 && (uv[3] - 0.5 * 166.0 / 256.0).abs() < 1e-6);

        let mut p = Pending { name: String::new(), width: 4, height: 2, rgba: vec![200; 32], logical: (2, 1) };
        halve(&mut p);
        assert_eq!((p.width, p.height, p.rgba.len()), (2, 1, 8));
        assert_eq!(p.logical, (2, 1));
    }

    // Die Spiegelung fuer die linke Hand dreht GENAU drei Vorzeichen um (translation.x,
    // rotation.y, rotation.z) -- nicht mehr. Ein zusaetzlich gedrehtes rotation.x wuerde das
    // Item in der Nebenhand auf den Kopf stellen.
    #[test]
    fn linke_hand_spiegelt_nur_drei_werte() {
        let rechts = ItemDisplay { rotation: [1.0, 2.0, 3.0], translation: [4.0, 5.0, 6.0], scale: [0.5, 0.6, 0.7] };
        let links = rechts.mirrored();
        assert_eq!(links.rotation, [1.0, -2.0, -3.0]);
        assert_eq!(links.translation, [-4.0, 5.0, 6.0]);
        assert_eq!(links.scale, rechts.scale, "die Groesse bleibt in beiden Haenden gleich");
    }
}

// --- Texturen fuer Items IN DER WELT ---------------------------------------------------
//
// Fuer das Inventar reichen die fertigen Atlas-Rechtecke (s. ItemIcon). Ein Item in der HAND
// wird dagegen in den Entity-Atlas nachgeladen, sobald es zum ersten Mal auftaucht -- dafuer
// braucht der Aufrufer nicht das Rechteck, sondern den TEXTURPFAD. Genau denselben Weg
// nehmen auch Skins und Umhaenge (s. entity_atlas::SKIN_SLOTS).
//
// Beide Pfadsorten sind relativ zu textures/ und ohne .png, also direkt fuer
// MinecraftInstallation::load_texture brauchbar, wenn man "minecraft/textures/" davorsetzt.
#[derive(Clone, Debug, PartialEq)]
pub enum ItemWorldTextures {
    // Flaches Item: die layer0-Textur (weitere Lagen -- Trankfuellung, Zaubertrank-Schimmer --
    // bleiben in der Hand weg; sie brauchten je eine eigene Geometrie-Ebene).
    Sprite(String),
    // Blockitem: die sechs Flaechentexturen in der Reihenfolge FACE_*.
    Block(Box<[String; 6]>),
    // Eigenes 3D-Modell aus Elementen (Resourcepacks), s. model_quads.
    Model(Vec<crate::ModelQuad>),
}

// Die Geometrie fuer die HAND (dritte Person; die Ego-Ansicht waehlt bei Vanilla-Items
// dasselbe Modell).
pub fn item_world_textures(mc: &mut MinecraftInstallation, item: &str) -> Option<ItemWorldTextures> {
    let (model, _tints, _) = item_model_ref(mc, item, None, Some(HAND_CONTEXT));
    // Ein Modell unter block/ ist ein Blockitem -- dann gelten dieselben Flaechentexturen wie
    // fuer den Block in der Welt (der Itemname ist bei Blockitems der Blockname).
    if model.starts_with("block/") {
        // Wie im Inventar: kein voller Wuerfel -> das echte Modell (s. non_cube_block_quads).
        if let Some(quads) = non_cube_block_quads(mc, &model) {
            return Some(ItemWorldTextures::Model(quads));
        }
        if let Some((faces, _overlay, _content)) = crate::blocks::resolve_block_faces(mc, item) {
            return Some(ItemWorldTextures::Block(Box::new(faces)));
        }
    }
    model_world_textures(mc, &model)
}

// Dasselbe fuer ein MODELL statt eines Items -- fuer die Zustaende, in denen Vanilla ein
// anderes Modell waehlt als die Vorgabe des Items: der gespannte Bogen (item/bow_pulling_0..2),
// die ladende und die geladene Armbrust (item/crossbow_pulling_0..2, item/crossbow_arrow,
// item/crossbow_firework). Die Blockitem-Abzweigung entfaellt -- das sind alles flache Items.
pub fn model_world_textures(mc: &mut MinecraftInstallation, model: &str) -> Option<ItemWorldTextures> {
    if let Some(quads) = crate::item_model_quads(mc, model) {
        return Some(ItemWorldTextures::Model(quads));
    }
    // Ein Blockmodell, das kein Item ist (der Gegenstandsrahmen): seine Elemente.
    if strip_ns(model).starts_with("block/") {
        if let Some(quads) = crate::model_quads::block_model_quads(mc, model, 0, 0) {
            return Some(ItemWorldTextures::Model(quads));
        }
    }
    let (textures, _elements) = crate::blocks::load_model_chain(mc, model).ok()?;
    // "layer0" ist der SCHLUESSEL in der textures-Map, keine "#"-Referenz -- erst den Wert
    // holen, und den dann aufloesen (er darf seinerseits auf eine andere Var zeigen).
    let layer0 = crate::blocks::resolve_var(&textures, textures.get("layer0")?.clone())?;
    Some(ItemWorldTextures::Sprite(layer0))
}

// Alle Lagen eines flachen Items (layer0, layer1, ...) mit der Angabe, ob die Lage die
// Trankfarbe traegt (s. IconLayer::potion_tint). Fuer die Hand, die sonst nur layer0 zeichnet
// -- beim Trank ist das die Fluessigkeit allein, ohne Flasche. Leer, wenn das Item kein
// flaches Mehrlagen-Bild ist.
pub fn item_sprite_layers(mc: &mut MinecraftInstallation, item: &str) -> Vec<(String, bool)> {
    let (model, _tints, potion) = item_model_ref(mc, item, None, Some(HAND_CONTEXT));
    if model.starts_with("block/") || crate::item_model_quads(mc, &model).is_some() {
        return Vec::new();
    }
    let Ok((textures, _elements)) = load_model_chain(mc, &model) else {
        return Vec::new();
    };
    let mut layers = Vec::new();
    for index in 0..8 {
        let Some(path) = textures.get(&format!("layer{index}")).cloned() else { break };
        let Some(resolved) = resolve_var(&textures, path) else { break };
        layers.push((resolved, potion.get(index).copied().unwrap_or(false)));
    }
    layers
}

// Die Textur der Item-Splitter (BreakingItemParticle): das "particle"-Feld des Modells, das
// Vanilla am Boden zeigt (Kontext "ground"), bei flachen Items ohne Feld deren layer0. Ein
// Resourcepack schaltet die Splitter ab, indem es "particle" auf ein leeres Bild zeigen laesst.
pub fn item_particle_texture(mc: &mut MinecraftInstallation, item: &str) -> Option<String> {
    let (model, _tints, _) = item_model_ref(mc, item, None, Some("ground"));
    let (textures, _elements) = load_model_chain(mc, &model).ok()?;
    let var = textures.get("particle").or_else(|| textures.get("layer0"))?.clone();
    resolve_var(&textures, var)
}

#[cfg(test)]
mod display_context_tests {
    use super::*;

    #[test]
    fn speer_waehlt_nach_anzeigeort() {
        let def: serde_json::Value = serde_json::from_str(
            r#"{"type":"minecraft:select","property":"minecraft:display_context",
                "cases":[{"model":{"type":"minecraft:model","model":"minecraft:item/wooden_spear"},"when":["gui","ground","fixed","on_shelf"]}],
                "fallback":{"type":"minecraft:model","model":"minecraft:item/wooden_spear_in_hand"}}"#,
        )
        .unwrap();
        assert_eq!(first_model_ref(&def, 0, Some("gui")).as_deref(), Some("item/wooden_spear"));
        assert_eq!(first_model_ref(&def, 0, Some("thirdperson_righthand")).as_deref(), Some("item/wooden_spear_in_hand"));
        assert_eq!(first_model_ref(&def, 0, None).as_deref(), Some("item/wooden_spear_in_hand"));
    }
}
