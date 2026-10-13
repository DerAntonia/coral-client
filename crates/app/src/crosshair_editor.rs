// Eigenes Fadenkreuz, Pixel fuer Pixel gesetzt (HUD-Editor -> Fadenkreuz -> Pixel bearbeiten).
//
// Ein Rasterpixel ist genau ein Bildschirmpixel -- dasselbe Kreuz ist in 4K also halb so gross
// wie in Full HD (so gewollt: nichts wird skaliert oder geglaettet, jeder Pixel sitzt, wo man
// ihn gesetzt hat). Das Raster hat eine GERADE Kantenlaenge: seine Mitte ist die Ecke zwischen
// den vier mittleren Pixeln und faellt damit auf die Bildmitte, wo der Blickstrahl hingeht.
//
// Je Pixel drei Zustaende: leer, Fuellung (Fadenkreuzfarbe, auch komplementaer) oder Kontur
// (schwarz). Gespeichert als eine Zeile aus '.', '#' und 'o', zeilenweise.

pub const GRID: usize = 48;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Pixel {
    Empty,
    Fill,
    Outline,
}

impl Pixel {
    fn to_char(self) -> char {
        match self {
            Pixel::Empty => '.',
            Pixel::Fill => '#',
            Pixel::Outline => 'o',
        }
    }

    fn from_char(c: char) -> Option<Self> {
        match c {
            '.' => Some(Pixel::Empty),
            '#' => Some(Pixel::Fill),
            'o' => Some(Pixel::Outline),
            _ => None,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Pattern {
    pub pixels: Vec<Pixel>,
}

// Wie gemalt wird: einzeln, links/rechts gespiegelt oder an beiden Achsen.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Symmetry {
    Off,
    Mirror,
    Quad,
}

impl Symmetry {
    pub fn label(self) -> &'static str {
        match self {
            Symmetry::Off => "Aus",
            Symmetry::Mirror => "Spiegeln",
            Symmetry::Quad => "4-fach",
        }
    }

    pub fn next(self) -> Self {
        match self {
            Symmetry::Off => Symmetry::Mirror,
            Symmetry::Mirror => Symmetry::Quad,
            Symmetry::Quad => Symmetry::Off,
        }
    }
}

impl Pattern {
    pub fn empty() -> Self {
        Self { pixels: vec![Pixel::Empty; GRID * GRID] }
    }

    // Das eingebaute Kreuz (Arme 10 lang, 2 dick, wahlweise 1 Pixel Kontur) als Startpunkt.
    pub fn standard(outline: bool) -> Self {
        let mut p = Self::empty();
        let m = (GRID / 2) as i32;
        let (halb_lang, halb_dick) = (5, 1);
        let mut setze = |x0: i32, y0: i32, x1: i32, y1: i32, wert: Pixel| {
            for y in y0..y1 {
                for x in x0..x1 {
                    p.set(x, y, wert);
                }
            }
        };
        if outline {
            setze(m - halb_dick - 1, m - halb_lang - 1, m + halb_dick + 1, m + halb_lang + 1, Pixel::Outline);
            setze(m - halb_lang - 1, m - halb_dick - 1, m + halb_lang + 1, m + halb_dick + 1, Pixel::Outline);
        }
        setze(m - halb_dick, m - halb_lang, m + halb_dick, m + halb_lang, Pixel::Fill);
        setze(m - halb_lang, m - halb_dick, m + halb_lang, m + halb_dick, Pixel::Fill);
        p
    }

    pub fn get(&self, x: i32, y: i32) -> Pixel {
        if (0..GRID as i32).contains(&x) && (0..GRID as i32).contains(&y) {
            self.pixels[y as usize * GRID + x as usize]
        } else {
            Pixel::Empty
        }
    }

    pub fn set(&mut self, x: i32, y: i32, v: Pixel) {
        if (0..GRID as i32).contains(&x) && (0..GRID as i32).contains(&y) {
            self.pixels[y as usize * GRID + x as usize] = v;
        }
    }

    // Einen Pixel setzen, mit seinen Spiegelbildern um die Rastermitte.
    pub fn paint(&mut self, x: i32, y: i32, v: Pixel, sym: Symmetry) {
        let g = GRID as i32 - 1;
        self.set(x, y, v);
        if sym != Symmetry::Off {
            self.set(g - x, y, v);
        }
        if sym == Symmetry::Quad {
            self.set(x, g - y, v);
            self.set(g - x, g - y, v);
        }
    }

    pub fn encode(&self) -> String {
        self.pixels.iter().map(|p| p.to_char()).collect()
    }

    pub fn decode(s: &str) -> Option<Self> {
        let pixels: Vec<Pixel> = s.chars().map(Pixel::from_char).collect::<Option<_>>()?;
        (pixels.len() == GRID * GRID).then_some(Self { pixels })
    }

    // Waagerechte Laeufe gleicher Pixel als Rechtecke (x, y, Breite) relativ zur Rastermitte,
    // je Art -- ein Viereck je Lauf statt je Pixel.
    pub fn runs(&self, art: Pixel) -> Vec<(i32, i32, i32)> {
        let m = (GRID / 2) as i32;
        let mut out = Vec::new();
        for y in 0..GRID as i32 {
            let mut x = 0;
            while x < GRID as i32 {
                if self.get(x, y) != art {
                    x += 1;
                    continue;
                }
                let start = x;
                while x < GRID as i32 && self.get(x, y) == art {
                    x += 1;
                }
                out.push((start - m, y - m, x - start));
            }
        }
        out
    }

    // Ausdehnung der gesetzten Pixel relativ zur Mitte (x0, y0, x1, y1), None wenn leer.
    pub fn bounds(&self) -> Option<(i32, i32, i32, i32)> {
        let m = (GRID / 2) as i32;
        let mut b: Option<(i32, i32, i32, i32)> = None;
        for y in 0..GRID as i32 {
            for x in 0..GRID as i32 {
                if self.get(x, y) != Pixel::Empty {
                    let (x, y) = (x - m, y - m);
                    b = Some(match b {
                        None => (x, y, x + 1, y + 1),
                        Some((a, c, d, e)) => (a.min(x), c.min(y), d.max(x + 1), e.max(y + 1)),
                    });
                }
            }
        }
        b
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn standardkreuz_liegt_symmetrisch_um_die_mitte() {
        let p = Pattern::standard(true);
        // Gefuellt: 10 lang, 2 dick, Mitte zwischen den Pixeln -- von -5 bis +5.
        assert_eq!(p.bounds(), Some((-6, -6, 6, 6)), "mit Kontur ein Pixel mehr");
        let fuellung: i32 = p.runs(Pixel::Fill).iter().map(|r| r.2).sum();
        assert_eq!(fuellung, 2 * 10 * 2 - 4, "zwei Balken, die Mitte doppelt");
        assert_eq!(Pattern::standard(false).bounds(), Some((-5, -5, 5, 5)));
    }

    #[test]
    fn speichern_laden_und_spiegeln() {
        let mut p = Pattern::empty();
        p.paint(3, 5, Pixel::Fill, Symmetry::Quad);
        let g = GRID as i32 - 1;
        for (x, y) in [(3, 5), (g - 3, 5), (3, g - 5), (g - 3, g - 5)] {
            assert_eq!(p.get(x, y), Pixel::Fill);
        }
        p.paint(10, 10, Pixel::Outline, Symmetry::Off);
        assert_eq!(Pattern::decode(&p.encode()), Some(p.clone()));
        assert!(Pattern::decode("###").is_none());
        // Ein Lauf je Zeile und Art.
        let mut q = Pattern::empty();
        for x in 0..4 {
            q.set(x, 0, Pixel::Fill);
        }
        assert_eq!(q.runs(Pixel::Fill), vec![(-(GRID as i32) / 2, -(GRID as i32) / 2, 4)]);
    }
}
