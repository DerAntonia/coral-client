// Welche States "Kreuz"-Bloecke sind (Blumen, Gras, Zuckerrohr, ...): zwei diagonale
// Ebenen statt eines Quaders, s. assets::build_cross. Reine Daten wie block_uvs.rs /
// block_shapes.rs: die Aufloesung aus den echten Vanilla-Modell-JSONs liegt im assets-Crate,
// gebaut wird die Tabelle in der App.

pub struct CrossBlocks {
    per_state: Vec<bool>,
}

impl CrossBlocks {
    pub fn new(per_state: Vec<bool>) -> Self {
        Self { per_state }
    }

    // Kein Kreuz-Block bekannt -- fuer Tests und Aufrufer ohne echte Modell-Daten.
    pub fn none() -> Self {
        Self { per_state: Vec::new() }
    }

    pub fn is_cross(&self, state: u32) -> bool {
        self.per_state.get(state as usize).copied().unwrap_or(false)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unbekannter_state_ist_kein_kreuz() {
        let cross = CrossBlocks::new(vec![false, true]);
        assert!(!cross.is_cross(0));
        assert!(cross.is_cross(1));
        assert!(!cross.is_cross(99));
        assert!(!CrossBlocks::none().is_cross(1));
    }
}
