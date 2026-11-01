// Die Tests der Items aus main.rs und seinen Wurzelmodulen, je Thema eine Datei.

#[allow(unused_imports)]
use crate::*;

mod model_face_tests;
mod latin_macron_tests;
mod anim_settings_tests;
mod theme_settings_tests;
mod sidebar_link_tests;
mod skin_stack_tests;
mod module_menu_tests;
mod damage_tint_tests;
mod experience_orb_tests;
mod sky_color_tests;
mod text_shadow_tests;
mod hud_split_tests;
mod crit_color_tests;
mod hsv_tests;
mod item_bar_tests;
mod hitbox_edge_tests;
mod tests;

// Layout der Weltbeschriftungen (s. build_world_labels). Getestet wird das, was ohne GPU
// geht: die Zerlegung in Zeilen und der Umbruch -- die Projektion selbst braucht eine
// Kamera und ein Fenster.
mod world_label_tests;

// Die Spielerfigur im Inventar (Vanillas "Puppe", s. doll_camera/App::build_doll).
mod puppen_tests;
mod no_tool_break_tests;
mod latched_width_tests;
mod text_selection_tests;
mod display_text_tests;
mod chat_scroll_layout_tests;
mod effect_display_tests;
mod scroll_fade_tests;
