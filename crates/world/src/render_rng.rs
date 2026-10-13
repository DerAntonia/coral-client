// Vanillas pseudozufaellige Block-Textur-Rotation/-Spiegelung (z.B. Erde/Sand/Stein/
// Grasblock: mehrere gleichwertige Modell-Varianten je Blockstate-Selektor, s.
// assets::shapes::resolve_random_variants) -- rein optisch, deterministisch aus der
// Weltposition, kein echter Zufall/RNG-Zustand ueber Ticks hinweg (jeder Client kommt bei
// derselben Position auf dieselbe Variante).
//
// Aus dem Yarn-1.21.11-Jar nachgesehen (BlockRenderManager.renderBlock() ->
// state.getRenderingSeed(pos), Default-Implementierung AbstractBlock.getRenderingSeed():
// MathHelper.hashCode(pos); WeightedBlockStateModel.addParts(): random.nextInt(totalWeight)
// waehlt den Index (bei den betroffenen Vanilla-Bloecken -- Erde/Sand/Podzol/Grasblock/
// Stein -- immer Gewicht 1 pro Eintrag, "totalWeight" ist also schlicht die Anzahl
// Varianten); random selbst ist CheckedRandom, der klassische java.util.Random-LCG.
//
// MathHelper.hashCode(x,y,z) = x*3129871 ^ z*116129781L ^ y (SO, nicht x/y/z in der
// naheliegenden Reihenfolge -- der Parameter namens "z" traegt den 116129781-Faktor, "y"
// geht unveraendert ein). world::block_offset::mc_seed(a,b,c) implementiert exakt dieselbe
// Formel mit a->x-Rolle (3129871), b->z-Rolle (116129781), c->y-Rolle (unveraendert) --
// mc_seed(x, z, y) liefert also MathHelper.hashCode(x, y, z).

const MULTIPLIER: u64 = 0x5DEECE66D;
const INCREMENT: u64 = 0xB;
const MASK: u64 = (1u64 << 48) - 1;

// 1:1-Nachbau von java.util.Random (CheckedRandom in Vanilla) -- nur die beiden hier
// gebrauchten Operationen (setSeed ueber den Konstruktor, next(bits), nextInt(bound) nach
// BaseRandom.nextInt()).
struct JavaRandom(u64);

impl JavaRandom {
    fn new(seed: i64) -> Self {
        Self((seed as u64 ^ MULTIPLIER) & MASK)
    }

    fn next(&mut self, bits: u32) -> i32 {
        self.0 = self.0.wrapping_mul(MULTIPLIER).wrapping_add(INCREMENT) & MASK;
        (self.0 >> (48 - bits)) as i32
    }

    // BaseRandom.nextInt(bound): Schnellpfad fuer Zweierpotenzen (alle bisher bekannten
    // Vanilla-Faelle -- 2 oder 4 Varianten), sonst der allgemeine Ablehnungs-Loop 1:1 wie
    // im Yarn-Quellcode.
    fn next_int(&mut self, bound: i32) -> i32 {
        if bound & (bound - 1) == 0 {
            return ((bound as i64).wrapping_mul(self.next(31) as i64) >> 31) as i32;
        }
        loop {
            let i = self.next(31);
            let j = i % bound;
            if i.wrapping_sub(j).wrapping_add(bound - 1) >= 0 {
                return j;
            }
        }
    }
}

// Waehlt EINEN von `count` gleich gewichteten Kandidaten (Vanillas betroffene Bloecke
// haben nie ein `weight` ungleich 1 im Modell-JSON) -- deterministisch aus der Position.
pub fn pick_variant(x: i32, y: i32, z: i32, count: usize) -> usize {
    debug_assert!(count > 0);
    let seed = crate::block_offset::mc_seed(x, z, y);
    let mut rng = JavaRandom::new(seed);
    rng.next_int(count as i32) as usize
}

#[cfg(test)]
mod tests {
    use super::*;

    // Gegen minecraft.wiki/w/Java_Edition_random-block-Textur-Rotation dokumentierte
    // Beispielwerte gegengeprueft waeren ideal, aber ohne verlaessliche unabhaengige
    // Quelle bleibt hier nur ein Determinismus-/Streuungstest: dieselbe Position liefert
    // immer denselben Index, und ueber viele Positionen kommen alle Indizes vor (kein
    // Bias/Bug wie "immer 0").
    #[test]
    fn deterministisch_und_ueber_positionen_gestreut() {
        let a = pick_variant(100, 64, -200, 4);
        let b = pick_variant(100, 64, -200, 4);
        assert_eq!(a, b, "dieselbe Position muss immer denselben Index liefern");

        let mut seen = [false; 4];
        for x in 0..200 {
            seen[pick_variant(x, 64, 0, 4)] = true;
        }
        assert!(seen.iter().all(|&s| s), "alle 4 Varianten sollten ueber genug Positionen vorkommen: {seen:?}");
    }

    #[test]
    fn bound_1_liefert_immer_index_0() {
        for x in 0..10 {
            assert_eq!(pick_variant(x, 0, 0, 1), 0);
        }
    }
}
