// Echte Entity-Modelle statt einfarbiger Boxen: derselbe Quader-Aufbau, den Vanilla
// benutzt, nach dem yarn-gemappten 1.21.11-Jar nachgebaut (net.minecraft.client.model.
// ModelPart$Cuboid/$Quad, ModelPart.applyTransform, net.minecraft.client.render.entity.
// model.* fuer die einzelnen Modelle, LivingEntityRenderer.render fuer die Einbettung in
// die Welt). Keine geratenen Zahlen -- jede Kantenlaenge, jeder Pivot und jeder
// Textur-Offset stimmt mit Vanilla ueberein.
//
// Aufbau in drei Schichten:
//   1. Cuboid  -- ein Kasten in MODELLPIXELN (1/16 Block), mit Textur-Offset und Dilation
//   2. Part    -- ein Gelenk: Pivot, Grunddrehung, Kaesten und Kind-Teile
//   3. Model   -- Wurzel-Teil + Texturname + Texturgroesse
// Die Animation (Kopfdrehung, Schrittschwingen) kommt nicht ins Modell, sondern als Pose
// obendrauf -- die Modelldaten bleiben damit reine, unveraenderliche Vanilla-Daten.
//
// Koordinaten: Vanillas Modellraum hat Y NACH UNTEN und den Ursprung am Kopf-Pivot; erst
// LivingEntityRenderer dreht das gerade (s. entity_transform).

use crate::mesher::BlockVertex;
mod geometry;
mod player_models;
mod block_models;
mod animals;
mod model_table;
mod biped_pose;
mod mob_poses;
#[cfg(test)]
mod anim_tests;
mod signs;
mod skinned;
pub use geometry::*;
pub use player_models::*;
pub use block_models::*;
pub use animals::*;
pub use model_table::*;
pub use biped_pose::*;
pub use mob_poses::*;
pub use signs::*;
pub use skinned::*;

#[cfg(test)]
mod tests;

// ---------------------------------------------------------------------------------------
// Modelldaten. Ab hier reine Uebersetzung der Vanilla-Modellklassen -- Zeilen in Vanilla
// stehen jeweils im Kommentar.
// ---------------------------------------------------------------------------------------
