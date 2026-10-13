// Vanilla-Colormaps (textures/colormap/grass.png, foliage.png): 256x256-Bilder, indiziert
// ueber die Klima-Werte eines Bioms (temperature, downfall). Wasser wird NICHT ueber eine
// Colormap gefaerbt, sondern direkt mit der Biom-Farbe effects.water_color (s.
// protocol::packets::BiomeInfo) -- das ist Vanillas eigene Unterscheidung, nicht unsere.
//
// Sampling-Formel per Yarn-Mappings/Vineflower gegen GrassColors/FoliageColors im
// 1.21.11-Client verifiziert: beide Klimawerte zuerst auf
// 0..1 geklemmt, downfall zusaetzlich mit temperature multipliziert (die Farbverlaeufe
// sind ein Dreieck, kein Quadrat -- kalt+nass kommt in der Karte nicht vor), dann
// x = (1-temperature)*255, y = (1-downfall*temperature)*255.

use crate::installation::{AssetsError, MinecraftInstallation};

pub struct Colormap {
    rgba: Vec<u8>,
    size: u32,
}

impl Colormap {
    pub fn load(mc: &mut MinecraftInstallation, name: &str) -> Result<Self, AssetsError> {
        let tex = mc.load_texture(&format!("minecraft/textures/colormap/{name}.png"))?;
        Ok(Self { rgba: tex.rgba, size: tex.width.min(tex.height) })
    }

    pub fn sample(&self, temperature: f32, downfall: f32) -> [f32; 3] {
        let temperature = temperature.clamp(0.0, 1.0);
        let downfall = downfall.clamp(0.0, 1.0) * temperature;
        let x = ((1.0 - temperature) * (self.size - 1) as f32).round() as u32;
        let y = ((1.0 - downfall) * (self.size - 1) as f32).round() as u32;
        let idx = ((y * self.size + x) * 4) as usize;
        let Some(px) = self.rgba.get(idx..idx + 3) else {
            return [1.0, 1.0, 1.0];
        };
        [px[0] as f32 / 255.0, px[1] as f32 / 255.0, px[2] as f32 / 255.0]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_map() -> Colormap {
        // 2x2-Testbild: klar unterscheidbare Ecken, um die Koordinatenrichtung zu pruefen.
        // (0,0)=rot [temperature=1], (1,0)=gruen [temperature=0], Zeilen von oben.
        let rgba = vec![
            255, 0, 0, 255, /**/ 0, 255, 0, 255, //
            0, 0, 255, 255, /**/ 255, 255, 0, 255,
        ];
        Colormap { rgba, size: 2 }
    }

    #[test]
    fn hohe_temperatur_liegt_bei_x0() {
        let m = test_map();
        // temperature=1 -> x=(1-1)*1=0; downfall=0 -> y=(1-0)*1=1 (unten).
        let c = m.sample(1.0, 0.0);
        assert_eq!(c, [0.0, 0.0, 1.0], "sollte die untere-linke Ecke (blau) treffen");
    }

    #[test]
    fn niedrige_temperatur_liegt_bei_x_max() {
        let m = test_map();
        // temperature=0 -> x=(1-0)*1=1; downfall egal (mult. mit temperature=0 -> y=1).
        let c = m.sample(0.0, 1.0);
        assert_eq!(c, [255.0 / 255.0, 255.0 / 255.0, 0.0], "sollte die untere-rechte Ecke treffen");
    }
}
