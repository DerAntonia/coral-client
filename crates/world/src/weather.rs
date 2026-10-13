// Regen an einer Position -- gebraucht fuer den Dreizack mit Sog, der nur "im Wasser oder im
// Regen" funktioniert (TridentItem.use/onStoppedUsing -> Entity.isTouchingWaterOrRain).
//
// Vanilla (World.hasRain):
//   1. es regnet ueberhaupt: Regenstaerke > 0.2 (World.isRaining; die Staerke schickt der
//      Server per Game Event, s. protocol::packets::GAME_EVENT_RAIN_*)
//   2. der Himmel ist frei: kein bewegungsblockierender Block und keine Fluessigkeit darueber
//      (Heightmap MOTION_BLOCKING)
//   3. das Biom hat Niederschlag, und der ist Regen statt Schnee (Temperatur >= 0.15)
//
// BEWUSSTE NAEHERUNGEN:
//   - Vanilla prueft zusaetzlich das Himmelslicht (isSkyVisible). Das kennt dieser Client
//     nicht; die Heightmap-Pruefung deckt praktisch dieselben Faelle ab.
//   - Oberhalb von Meeresspiegel+17 sinkt die Temperatur mit der Hoehe, plus ein kleiner
//     Rauschanteil (Biome.TEMPERATURE_NOISE, +-8 Bloecke). Der Rauschanteil fehlt hier; die
//     Schneegrenze liegt dadurch um wenige Bloecke anders als in Vanilla.
//   - Der Meeresspiegel ist fest 63 (Oberwelt). Nur fuer die Hoehenkorrektur relevant.

use crate::block_shapes::BlockShapes;
use crate::chunk::World;

// World.isRaining: Regenstaerke ueber 0.2.
pub const RAIN_THRESHOLD: f32 = 0.2;
const SEA_LEVEL: i32 = 63;
// Biome.isCold / doesNotSnow: darunter faellt Schnee statt Regen.
const SNOW_TEMPERATURE: f32 = 0.15;

// Was das Biom fuer den Niederschlag beitraegt (aus der Biom-Registry).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BiomeClimate {
    pub has_precipitation: bool,
    pub temperature: f32,
}

// Biome.computeTemperature ohne den Rauschanteil (s. Modulkommentar).
fn temperature_at(climate: BiomeClimate, y: i32) -> f32 {
    let grenze = SEA_LEVEL + 17;
    if y > grenze {
        climate.temperature - (y - grenze) as f32 * 0.05 / 40.0
    } else {
        climate.temperature
    }
}

// Was an einer Stelle vom Himmel faellt, wenn es regnet (Biome.getPrecipitationAt): nichts
// in Biomen ohne Niederschlag (Wueste, Savanne, Nether, End), sonst Schnee, wo es kalt genug
// ist, und Regen ueberall sonst.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Precipitation {
    None,
    Rain,
    Snow,
}

pub fn precipitation(climate: BiomeClimate, y: i32) -> Precipitation {
    if !climate.has_precipitation {
        Precipitation::None
    } else if temperature_at(climate, y) < SNOW_TEMPERATURE {
        Precipitation::Snow
    } else {
        Precipitation::Rain
    }
}

// World.hasRain(pos).
pub fn has_rain(
    world: &World,
    shapes: &BlockShapes,
    rain_gradient: f32,
    climate: impl Fn(u32) -> Option<BiomeClimate>,
    x: i32,
    y: i32,
    z: i32,
) -> bool {
    if rain_gradient <= RAIN_THRESHOLD {
        return false;
    }
    if world.motion_blocking_top(x, z, shapes).is_some_and(|top| top >= y) {
        return false;
    }
    let Some(klima) = climate(world.biome_at(x, y, z)) else { return false };
    precipitation(klima, y) == Precipitation::Rain
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn schnee_statt_regen_hoch_oben() {
        let ebene = BiomeClimate { has_precipitation: true, temperature: 0.8 };
        assert_eq!(temperature_at(ebene, 70), 0.8);
        // Ab 80 sinkt es: 0.8 - (y-80)*0.05/40 < 0.15 ab y = 600 -- in der Ebene praktisch nie.
        assert!(temperature_at(ebene, 300) >= SNOW_TEMPERATURE);
        let kalt = BiomeClimate { has_precipitation: true, temperature: 0.2 };
        // 0.2 - (y-80)/800 < 0.15 ab y > 120: dort schneit es.
        assert!(temperature_at(kalt, 110) >= SNOW_TEMPERATURE);
        assert!(temperature_at(kalt, 130) < SNOW_TEMPERATURE);
        assert_eq!(precipitation(kalt, 110), Precipitation::Rain);
        assert_eq!(precipitation(kalt, 130), Precipitation::Snow);
        let wueste = BiomeClimate { has_precipitation: false, temperature: 2.0 };
        assert_eq!(precipitation(wueste, 70), Precipitation::None);
    }
}
