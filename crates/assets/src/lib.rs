// Laden/Parsen von .minecraft-Assets (jar, Blockmodelle, Texturen).

mod atlas_sources;
mod blocks;
mod colormap;
mod configs;
mod entity_atlas;
mod equipment;
mod font;
mod installation;
mod item_model;
mod javazip;
mod locate;
mod model_quads;
mod pack_unzip;
mod particle_atlas;
mod shapes;
mod sounds;
mod storage;
mod source;
mod ui_atlas;

pub use blocks::{is_fire_texture, resource_path, AnimationState, BlockAtlas, ANIM_VARIANTS, TileAnimation, UvRect, DESTROY_STAGE_COUNT, FACE_DOWN, FACE_EAST, FACE_NORTH, FACE_SOUTH, FACE_UP, FACE_WEST};
pub use colormap::Colormap;
pub use item_model::{ItemDefinition, ItemModelInput, Leaf as ItemModelLeaf, SELECTED_ITEM};
pub use equipment::{parse_equipment_model, EquipmentLayer, EquipmentModel};
pub use storage::{extract_game_data, game_data_dir, game_data_status, stored_creative_tabs, stored_entity_models, stored_item_components, GameDataStatus};
pub use configs::{
    config_path, configs_dir, create_config, private_config_path, delete_config, last_used, list_configs, rename_config, set_last_used, startup_config, validate_name, DEFAULT_CONFIG,
};
pub use entity_atlas::{
    build as build_entity_atlas, build_trim_texture, EntityAtlas, CAPE_HEIGHT, CAPE_SLOTS, CAPE_WIDTH, HEAD_SLOTS, HEAD_SLOT_HEIGHT, HEAD_SLOT_WIDTH, ITEM_SLOTS, MAP_SLOTS, MAP_SLOT_SIZE, TRIM_HEIGHT, TRIM_SLOTS, TRIM_WIDTH, ITEM_SLOT_SIZE, SKIN_SIZE, SKIN_SLOTS,
};
pub use font::{full_font_id, AltFontGlyphs, AltGlyph, BitmapFont, FontPage, PackFonts, PackGlyph, UniGlyph, Unihex};
pub use installation::{decode_cape_png, decode_skin_png, path_excluded, AssetsError, DecodedTexture, MinecraftInstallation, PackDir, RESOURCE_PACK_FORMAT};
pub use locate::locate_minecraft_dir;

// Die Dateien in einem Zip, ohne Ordner-Eintraege (z.B. fuer den Dateibaum eines Server-Packs).
pub fn zip_file_names(path: &std::path::Path) -> Vec<String> {
    javazip::JavaZip::open(path).map(|z| z.names().iter().filter(|n| !n.ends_with('/')).cloned().collect()).unwrap_or_default()
}
pub use model_quads::{item_model_quads, ModelQuad};
pub use pack_unzip::extract_resource_pack;
pub use particle_atlas::{build as build_particle_atlas, ParticleAtlas};
pub use shapes::{
    build as build_shapes, build_ao_disabled, build_model_quads, state_variant_models, build_cross, build_cross_extras, build_random_rotations, build_rotations, default_face_uv, permute_faces_x,
    permute_faces_y, ElementBox,
};
pub use sounds::{SoundIndex, SoundVariant};
pub use source::{active_source, config_dir, data_dir, discover, migrate_legacy_dirs, user_dir, load_selection, save_selection, Candidate, GameFiles, GameSource};
pub use ui_atlas::{item_tints, FIRE_OVERLAY, UI_WHITE, 
    build as build_ui_atlas, leaf_icon_key, item_first_person_display, item_fixed_display, item_ground_display, item_named_display, item_first_person_display_left, item_hand_display, item_hand_display_left,
    item_particle_texture, item_sprite_layers, item_world_textures, model_hand_display, model_world_textures, AtlasRect, IconLayer, IconQuad, ItemDisplay, ItemIcon, ItemWorldTextures, UiAtlas, GUI_SHEETS, MOB_EFFECT_SPRITES,
};
