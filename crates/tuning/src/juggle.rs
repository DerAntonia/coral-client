// Jonglieren statt Ansehen (s. item_inspect): Items ohne Haltbarkeit fliegen zwischen beiden
// Haenden. Zwei Tricks:
// - Drei: die Kaskade, jede Hand wirft im Bogen zwischen den beiden anderen hindurch zur
//   anderen Hand, abwechselnd rechts und links.
// - Zwei: R, L, L, R, R, L, L, ... -- jede Hand wirft zweimal hintereinander zur anderen, der
//   zweite Wurf hoeher ueber das andere Item hinweg.
// Alles im Sichtraum der Ego-Ansicht (-Z nach vorn, wie item_inspect), Zeit in Ticks. Die
// dritte Person bekommt Armwinkel und, wo die Items sind (an welcher Hand, im Flug).

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Trick {
    Two,
    Three,
}

// So lange gleiten die Haende aus der Ruhe in den Trick.
pub const RAMP: f32 = 8.0;

const RIGHT: usize = 0;
const LEFT: usize = 1;

// Das Item sitzt so weit ueber der Faust.
const BALL_ABOVE_FIST: [f32; 3] = [0.0, 0.075, -0.01];

// Schwerkraft im Sichtraum, Einheiten pro Tick^2: fuer alle Wuerfe dieselbe. Ein hoher Wurf
// fliegt darum laenger (Hoehe = g * Flugzeit^2 / 8), nicht nur hoeher.
pub const GRAVITY: f32 = 0.0437;

// Mit zwei Items: eine Runde (vier Wuerfe R, L, L, R) dauert so lange; die Wuerfe kommen bei
// 0, 1/4, 1/2, 3/4 davon. Der flache fliegt 10 Ticks (etwa bis zur Bildmitte), der hohe 14
// (rund 1.07 hoch, oben aus dem Bild) -- so wirft jede Hand ihr Item, bevor das andere ankommt.
const TWO_ROUND: f32 = 40.0;
const TWO_HIGH: f32 = 14.0;
const TWO_LOW: f32 = 10.0;

// Faust der rechten Hand in Ruhe (s. item_inspect::FIST) und die linke unter dem Bildrand.
const REST: [[f32; 3]; 2] = [[0.714, -0.499, -1.010], [-0.75, -1.35, -1.010]];

impl Trick {
    #[must_use] 
    pub const fn balls(self) -> usize {
        match self {
            Self::Two => 2,
            Self::Three => 3,
        }
    }

    // Abstand zweier Wuerfe in Ticks.
    fn beat(self) -> f32 {
        match self {
            Self::Two => TWO_ROUND / 4.0,
            Self::Three => 5.5,
        }
    }

    // Wo die Hand faengt (rechte Hand, links gespiegelt): weit auseinander unten in den
    // Bildecken, so tief im Bild wie die leere Hand (item_inspect::FIST) -- naeher dran wirkten
    // Haende und Items herangezoomt.
    const fn catch_point(self) -> [f32; 3] {
        match self {
            Self::Three => [0.88, -0.50, -1.01],
            _ => [0.87, -0.50, -1.01],
        }
    }
}

// Wo geworfen wird (rechte Hand, links gespiegelt). Bei dreien weiter zur Mitte hin als
// gefangen; bei zweien der erste Wurf einer Hand (der flache) von weiter mittig, der zweite
// (hohe) von aussen.
const THREE_THROW: [f32; 3] = [0.52, -0.52, -1.01];
const TWO_FIRST_THROW: [f32; 3] = [0.42, -0.52, -1.01];
const TWO_SECOND_THROW: [f32; 3] = [0.80, -0.52, -1.01];

// Ein Wurf: wann, von welcher Hand und wo (rechts gemessen, s. mirror), welches Item, wie
// lange es fliegt und wie hoch (aus der Flugzeit, s. GRAVITY).
#[derive(Clone, Copy, Debug)]
struct Throw {
    k: i64,
    time: f32,
    hand: usize,
    from: [f32; 3],
    ball: usize,
    flight: f32,
    apex: f32,
}

fn throw_at(trick: Trick, k: i64) -> Throw {
    let b = trick.beat();
    let time = k as f32 * b;
    let (hand, from, ball, flight) = match trick {
        Trick::Three => ((k % 2) as usize, THREE_THROW, (k % 3) as usize, 2.2 * b),
        // R, L, L, R: der zweite Wurf einer Hand (gerades k) geht hoch ueber den ersten. Das
        // Item landet erst NACH dem naechsten Wurf der fangenden Hand: erst wirft sie ihres,
        // dann faengt sie.
        _ => {
            let hand = if matches!(k % 4, 1 | 2) { LEFT } else { RIGHT };
            if k % 2 == 0 {
                (hand, TWO_SECOND_THROW, 0, TWO_HIGH)
            } else {
                (hand, TWO_FIRST_THROW, 1, TWO_LOW)
            }
        }
    };
    Throw { k, time, hand, from, ball, flight, apex: GRAVITY * flight * flight / 8.0 }
}

fn mirror(p: [f32; 3], hand: usize) -> [f32; 3] {
    if hand == RIGHT {
        p
    } else {
        [-p[0], p[1], p[2]]
    }
}

fn lerp(a: [f32; 3], b: [f32; 3], f: f32) -> [f32; 3] {
    std::array::from_fn(|i| a[i] + (b[i] - a[i]) * f)
}

fn add(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    std::array::from_fn(|i| a[i] + b[i])
}

fn smooth(u: f32) -> f32 {
    let u = u.clamp(0.0, 1.0);
    u * u * (3.0 - 2.0 * u)
}

// Die Wuerfe um den Zeitpunkt c (Uhr des Tricks, ab 0).
fn throws_near(trick: Trick, c: f32) -> Vec<Throw> {
    let k = (c / trick.beat()).floor() as i64;
    ((k - 6).max(0)..=k + 6).map(|k| throw_at(trick, k)).collect()
}

// So weit sind die Arme beim Jonglieren nach innen gedreht: in F5 die Drehung des Armteils
// (Radiant, s. third_person_arms), in der Ego-Ansicht die Neigung auf dem Bild (Grad, 0 =
// senkrecht, s. world::item_inspect::arm_on_screen).
const ARM_INWARD_RAD: f32 = 0.2;
pub const ARM_SCREEN_LEAN_DEG: f32 = 20.0;

// So tief holt die Hand mit dem Item vor dem Wurf aus.
const AUSHOLEN: f32 = 0.32;

// Bei dreien bleibt die Hand nach dem Wurf so lange (Anteil des Wegs zum Fang) innen und geht
// erst kurz vor dem Fang wieder nach aussen.
const THREE_STAY_INSIDE: f32 = 0.4;

// Ereignis einer Hand: Wurf (an seinem Wurfpunkt) oder Fang (am Fangpunkt).
#[derive(Clone, Copy)]
enum Event {
    Throw([f32; 3]),
    Catch,
}

fn hand_in_pattern(trick: Trick, hand: usize, c: f32, wuerfe: &[Throw]) -> [f32; 3] {
    let fang = mirror(trick.catch_point(), hand);
    let mut ereignisse: Vec<(f32, Event)> = Vec::new();
    for w in wuerfe {
        if w.hand == hand {
            ereignisse.push((w.time, Event::Throw(mirror(w.from, hand))));
        } else {
            ereignisse.push((w.time + w.flight, Event::Catch));
        }
    }
    ereignisse.sort_by(|a, b| a.0.total_cmp(&b.0));
    let naechstes = ereignisse.iter().position(|e| e.0 > c);
    let Some(n) = naechstes else { return fang };
    let (tn, en) = ereignisse[n];
    let (tp, ep) = if n > 0 { ereignisse[n - 1] } else { (tn - trick.beat(), Event::Catch) };
    let punkt = |e: Event| if let Event::Throw(p) = e { p } else { fang };
    let u = ((c - tp) / (tn - tp).max(1e-3)).clamp(0.0, 1.0);
    let bogen = (u * std::f32::consts::PI).sin();
    match (ep, en) {
        // Leer vom Wurf zum Fang: leicht nach oben aussen; bei dreien erst spaet.
        (Event::Throw(wurf), Event::Catch) => {
            let f = if trick == Trick::Three { smooth((u - THREE_STAY_INSIDE) / (1.0 - THREE_STAY_INSIDE)) } else { smooth(u) };
            add(lerp(wurf, fang, f), [0.0, 0.03 * bogen, 0.0])
        }
        // Mit Item vom Fang zum Wurf: tief unten durch ausholen.
        (Event::Catch, Event::Throw(wurf)) => add(lerp(fang, wurf, smooth(u)), [0.0, -AUSHOLEN * bogen, 0.0]),
        // Noch ein Item in der Hand: ueber den Fangpunkt zurueck zum Wurf.
        (Event::Throw(a), Event::Throw(b)) => {
            let p = if u < 0.5 { lerp(a, fang, smooth(u * 2.0)) } else { lerp(fang, b, smooth(u * 2.0 - 1.0)) };
            add(p, [0.0, -AUSHOLEN * bogen, 0.0])
        }
        (Event::Catch, Event::Catch) => lerp(punkt(ep), punkt(en), smooth(u)),
    }
}

// Wer ein Item gerade hat: eine Hand oder der Flug von einer zur anderen (u = 0..1, `apex`
// Hoehe des Bogens ueber der Verbindung).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Holder {
    Hand(usize),
    Flight { from: usize, to: usize, u: f32, apex: f32 },
}

// Der Trick zum Zeitpunkt t (Ticks seit dem Start): Haende (rechts, links) und je Item, wer es
// hat. Die Haende im Sichtraum der Ego-Ansicht (Faust, s. item_inspect::arm_from).
pub struct State {
    pub hands: [[f32; 3]; 2],
    pub balls: Vec<Holder>,
    // Je Item: die Lage im Sichtraum.
    pub ball_pos: Vec<[f32; 3]>,
    // Je Item: Drehung im Flug um seine Mitte, (Achse, Winkel in Radiant); in der Hand 0.
    pub ball_spin: Vec<([f32; 3], f32)>,
}

// Im Flug dreht sich jedes Item gleichmaessig um eine je Wurf zufaellige Achse, so viele ganze
// Umdrehungen (zufaellig zwischen den Grenzen), dass es beim Fangen wieder zur Kamera zeigt.
const SPIN_TURNS: [i64; 2] = [1, 2];

// Zufall je Wurf, aber fest: derselbe Wurf dreht sich in jedem Bild gleich (splitmix64).
const fn throw_random(k: i64, salt: u64) -> u64 {
    let mut z = (k as u64).wrapping_add(salt.wrapping_mul(0x9E37_79B9_7F4A_7C15));
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

fn unit_random(k: i64, salt: u64) -> f32 {
    (throw_random(k, salt) >> 40) as f32 / (1u64 << 24) as f32
}

// Achse und Winkel des Wurfs k beim Anteil u (0..1) seines Flugs.
fn spin(k: i64, u: f32) -> ([f32; 3], f32) {
    // Gleichverteilt auf der Kugel.
    let z = unit_random(k, 1) * 2.0 - 1.0;
    let phi = unit_random(k, 2) * std::f32::consts::TAU;
    let r = (1.0 - z * z).sqrt();
    let achse = [r * phi.cos(), r * phi.sin(), z];
    let spanne = (SPIN_TURNS[1] - SPIN_TURNS[0] + 1) as u64;
    let umdrehungen = SPIN_TURNS[0] + (throw_random(k, 3) % spanne) as i64;
    (achse, umdrehungen as f32 * std::f32::consts::TAU * u)
}

#[must_use] 
pub fn state(trick: Trick, t: f32) -> State {
    let c = t - RAMP;
    let (hands, balls) = if c < 0.0 {
        // Aus der Ruhe hinein: die Haende gleiten zur Lage beim Start, Items noch in der Hand.
        let (start, balls) = pattern(trick, 0.0);
        let f = smooth(t / RAMP);
        let balls = balls.into_iter().map(|h| if let Holder::Flight { from, .. } = h { Holder::Hand(from) } else { h }).collect();
        ([lerp(REST[RIGHT], start[RIGHT], f), lerp(REST[LEFT], start[LEFT], f)], balls)
    } else {
        pattern(trick, c)
    };
    let mut ball_pos = Vec::new();
    let mut ball_spin = Vec::new();
    let fang = trick.catch_point();
    for (b, h) in balls.iter().enumerate() {
        let p = match *h {
            Holder::Hand(i) => add(hands[i], BALL_ABOVE_FIST),
            // Geworfen: eine reine Wurfparabel vom Wurf- zum Fangpunkt, waagerecht gleichmaessig,
            // senkrecht mit GRAVITY -- die Haende sind dabei, wo sie wollen.
            Holder::Flight { from, to, u, apex } => {
                let w = last_throw(trick, b, c);
                ball_spin.push(w.map_or(([0.0, 1.0, 0.0], 0.0), |w| spin(w.k, u)));
                let wurf = w.map_or(fang, |w| w.from);
                let p = lerp(add(mirror(wurf, from), BALL_ABOVE_FIST), add(mirror(fang, to), BALL_ABOVE_FIST), u);
                add(p, [0.0, apex * 4.0 * u * (1.0 - u), 0.0])
            }
        };
        ball_pos.push(p);
        if ball_spin.len() < ball_pos.len() {
            ball_spin.push(([0.0, 1.0, 0.0], 0.0));
        }
    }
    State { hands, balls, ball_pos, ball_spin }
}

// Der letzte Wurf des Items `ball` bis zum Zeitpunkt c.
fn last_throw(trick: Trick, ball: usize, c: f32) -> Option<Throw> {
    throws_near(trick, c).into_iter().rfind(|w| w.ball == ball && w.time <= c)
}

fn pattern(trick: Trick, c: f32) -> ([[f32; 3]; 2], Vec<Holder>) {
    let wuerfe = throws_near(trick, c);
    let hands = [hand_in_pattern(trick, RIGHT, c, &wuerfe), hand_in_pattern(trick, LEFT, c, &wuerfe)];
    let balls = (0..trick.balls())
        .map(|b| {
            match last_throw(trick, b, c) {
                Some(w) if c < w.time + w.flight => {
                    // Im Flug zwischen Wurf- und Fangpunkt; die Haende stehen dann genau dort.
                    Holder::Flight { from: w.hand, to: 1 - w.hand, u: (c - w.time) / w.flight, apex: w.apex }
                }
                Some(w) => Holder::Hand(1 - w.hand),
                // Noch nicht geworfen: in der Hand seines ersten Wurfs.
                None => Holder::Hand(throw_at(trick, b as i64).hand),
            }
        })
        .collect();
    (hands, balls)
}

// Dritte Person: Armwinkel (Radiant, wie Pose::rotate) fuer (rechts, links) aus der Lage der
// Ego-Haende -- weit vorn und oben heisst Arm hoch, nach innen heisst Arm eingedreht.
#[must_use] 
pub fn third_person_arms(trick: Trick, t: f32) -> [[f32; 3]; 2] {
    let s = state(trick, t);
    std::array::from_fn(|i| {
        let sign = if i == RIGHT { 1.0 } else { -1.0 };
        let p = s.hands[i];
        let x = -0.1 - (p[1] + 1.4) * 1.1;
        let y = -sign * (ARM_INWARD_RAD + (0.8 - p[0].abs()).max(0.0) * 1.5);
        [x, y, 0.0]
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    // Jedes Item ist immer genau an einer Stelle: im Flug landet es bei der Hand, die es
    // danach haelt, und keine Hand wirft ein Item, das sie nicht hat.
    #[test]
    fn wuerfe_passen_zusammen() {
        for trick in [Trick::Two, Trick::Three] {
            for k in 0..40 {
                let w = throw_at(trick, k);
                // Das naechste Mal fliegt dieses Item von der Hand, die es gefangen hat.
                let naechster = (k + 1..k + 10).map(|j| throw_at(trick, j)).find(|j| j.ball == w.ball).expect("weiter");
                assert_eq!(naechster.hand, 1 - w.hand, "{trick:?} k={k}");
                assert!(naechster.time >= w.time + w.flight, "{trick:?} k={k}: geworfen bevor gefangen");
            }
        }
    }

    // Keine Hand faengt im selben Moment, in dem sie wirft: zwischen Wurf und Fang derselben
    // Hand liegt immer etwas Zeit.
    #[test]
    fn erst_werfen_dann_fangen() {
        for trick in [Trick::Two, Trick::Three] {
            let wuerfe: Vec<Throw> = (0..40).map(|k| throw_at(trick, k)).collect();
            for w in &wuerfe {
                let fang = w.time + w.flight;
                for x in wuerfe.iter().filter(|x| x.hand == 1 - w.hand) {
                    assert!((x.time - fang).abs() > 0.2 * trick.beat(), "{trick:?}: Wurf bei {} und Fang bei {fang}", x.time);
                }
            }
        }
    }

    // Schwerkraft: jedes fliegende Item faellt mit derselben Beschleunigung GRAVITY, waagerecht
    // gleichmaessig -- auch die hohen Wuerfe, die dafuer laenger fliegen.
    #[test]
    fn schwerkraft() {
        let d = 0.5;
        for trick in [Trick::Two, Trick::Three] {
            let mut t = RAMP + 1.0;
            let mut geprueft = 0;
            while t < RAMP + 200.0 {
                let (a, b, c) = (state(trick, t - d), state(trick, t), state(trick, t + d));
                for i in 0..trick.balls() {
                    let fliegt = |s: &State| match s.balls[i] {
                        Holder::Flight { from, u, .. } => Some((from, u)),
                        Holder::Hand(_) => None,
                    };
                    let (Some((fa, ua)), Some((fb, _)), Some((fc, uc))) = (fliegt(&a), fliegt(&b), fliegt(&c)) else { continue };
                    if fa != fb || fb != fc || uc <= ua {
                        continue;
                    }
                    let beschl: [f32; 3] = std::array::from_fn(|k| (a.ball_pos[i][k] - 2.0 * b.ball_pos[i][k] + c.ball_pos[i][k]) / (d * d));
                    assert!((beschl[1] + GRAVITY).abs() < 1e-3, "{trick:?} t={t} Item {i}: {beschl:?}");
                    assert!(beschl[0].abs() < 1e-3 && beschl[2].abs() < 1e-3, "{trick:?} t={t} Item {i}: {beschl:?}");
                    geprueft += 1;
                }
                t += 0.37;
            }
            assert!(geprueft > 100, "{trick:?}: nur {geprueft} Proben");
        }
    }

    // Mit zwei Items: R, L, L, R, und der zweite Wurf einer Hand fliegt hoeher.
    #[test]
    fn zwei_items_rllr() {
        let haende: Vec<usize> = (0..8).map(|k| throw_at(Trick::Two, k).hand).collect();
        assert_eq!(haende, [RIGHT, LEFT, LEFT, RIGHT, RIGHT, LEFT, LEFT, RIGHT]);
        assert!(throw_at(Trick::Two, 2).apex > throw_at(Trick::Two, 1).apex);
    }

    // Die Hand steht beim Wurf dort, wo das Item losfliegt: bei zweien der flache Wurf mittiger
    // als der hohe; bei dreien bleibt sie nach dem Wurf erst innen.
    #[test]
    fn wurfpunkte() {
        for trick in [Trick::Two, Trick::Three] {
            for k in 2..12 {
                let w = throw_at(trick, k);
                let s = state(trick, RAMP + w.time);
                assert!((s.hands[w.hand][0] - mirror(w.from, w.hand)[0]).abs() < 1e-3, "{trick:?} k={k}");
            }
        }
        assert!(throw_at(Trick::Two, 1).from[0] < throw_at(Trick::Two, 2).from[0]);
        let w = throw_at(Trick::Three, 4);
        let kurz_danach = state(Trick::Three, RAMP + w.time + 2.0).hands[w.hand][0];
        assert!((kurz_danach - THREE_THROW[0]).abs() < 0.02, "{kurz_danach}");
    }

    // Im Flug dreht sich jedes Item, gefangen zeigt es wieder genau zur Kamera; die Wuerfe
    // drehen sich um verschiedene Achsen.
    #[test]
    fn drehen_im_flug() {
        for k in 0..20 {
            let (a, w) = spin(k, 1.0);
            let ganz = w / std::f32::consts::TAU;
            assert!((ganz - ganz.round()).abs() < 1e-4 && ganz >= 1.0, "k={k}: {ganz}");
            assert!((a.iter().map(|x| x * x).sum::<f32>() - 1.0).abs() < 1e-4);
            assert_eq!(spin(k, 0.0).1, 0.0);
        }
        assert!(spin(0, 1.0).0 != spin(1, 1.0).0);
        let s = state(Trick::Three, RAMP + 30.0);
        for (h, (_, w)) in s.balls.iter().zip(&s.ball_spin) {
            assert_eq!(matches!(h, Holder::Hand(_)), *w == 0.0);
        }
    }

    // Keine Spruenge: die Haende und Items bewegen sich von Tick zu Tick nur ein Stueck.
    #[test]
    fn ohne_spruenge() {
        for trick in [Trick::Two, Trick::Three] {
            let mut vorher = state(trick, 0.0);
            let mut t = 0.0;
            while t < 120.0 {
                t += 0.25;
                let jetzt = state(trick, t);
                let weit = |a: [f32; 3], b: [f32; 3]| (0..3).map(|i| (a[i] - b[i]).abs()).fold(0.0, f32::max);
                for i in 0..2 {
                    assert!(weit(vorher.hands[i], jetzt.hands[i]) < 0.12, "{trick:?} Hand {i} bei t={t}");
                }
                for b in 0..trick.balls() {
                    assert!(weit(vorher.ball_pos[b], jetzt.ball_pos[b]) < 0.2, "{trick:?} Item {b} bei t={t}");
                }
                vorher = jetzt;
            }
        }
    }
}
