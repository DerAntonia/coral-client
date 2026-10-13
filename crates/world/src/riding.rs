// Reiten: wo ein Mitfahrer auf seinem Fahrzeug sitzt.
//
// Vanilla setzt jeden Passagier je Tick an `Fahrzeugposition + PASSENGER-Anker des Fahrzeugs
// - VEHICLE-Anker des Passagiers` (Entity.updatePassengerPosition). Beide Anker stehen beim
// Entity-Typ (EntityType.Builder.passengerAttachments / vehicleAttachment); ohne Angabe ist der
// PASSENGER-Anker "oben auf der Box" (0, Hoehe, 0) und der VEHICLE-Anker null. Einige
// Fahrzeuge rechnen ihren Sitz selbst aus (Boote, Kamele, Pferde, Schreiter, Loren) -- die
// stehen unten von Hand.
//
// Die Tabellen sind maschinell aus den Registrierungen in EntityType (Yarn 1.21.11)
// uebernommen, nur Zahlen. Jeder Punkt dreht sich mit dem Fahrzeug: Vanilla nimmt dafuer
// rotateY(-yaw), s. rotate_yaw.

// Welcher Sitz: Stelle des Passagiers in der Passagierliste des Fahrzeugs und Laenge dieser
// Liste (die Liste aus EntityPassengersSetS2CPacket, in Server-Reihenfolge -- der erste lenkt).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Seat {
    pub index: usize,
    pub count: usize,
}

// Wo der Passagier `passenger` (Typname) auf `vehicle` sitzt: die Fussposition, die Vanilla ihm
// je Tick setzt. `vehicle_yaw_deg` ist die Drehung des Fahrzeugs, nicht die seines Kopfes;
// `vehicle_height` die Hoehe seiner Box in der aktuellen Pose (ein schleichender Spieler ist
// niedriger, und der Standard-Sitz ist "oben auf der Box").
pub fn riding_pos(vehicle: &str, vehicle_height: f64, vehicle_pos: [f64; 3], vehicle_yaw_deg: f32, seat: Seat, passenger: &str) -> [f64; 3] {
    let p = passenger_offset(vehicle, vehicle_height, vehicle_yaw_deg, seat, passenger);
    let v = vehicle_attachment_y(passenger);
    [vehicle_pos[0] + p[0], vehicle_pos[1] + p[1] - v, vehicle_pos[2] + p[2]]
}

// Der PASSENGER-Anker des Fahrzeugs fuer diesen Sitz, schon mit dem Fahrzeug gedreht.
pub fn passenger_offset(vehicle: &str, hoehe: f64, vehicle_yaw_deg: f32, seat: Seat, passenger: &str) -> [f64; 3] {
    if let Some(boot) = boat_kind(vehicle) {
        // AbstractBoatEntity.getPassengerAttachmentPos: allein mittig (Kistenboote 0.15 nach
        // vorn, weil hinten die Kiste steht), zu zweit 0.2 vorn und 0.6 hinten; ein Tier
        // sitzt jeweils 0.2 weiter vorn.
        let mut vorn = if boot.chest { 0.15 } else { 0.0 };
        if seat.count > 1 {
            vorn = if seat.index == 0 { 0.2 } else { -0.6 };
            if is_animal(passenger) {
                vorn += 0.2;
            }
        }
        // Boote sitzen auf einem Drittel ihrer Hoehe, Floesse fast oben (8/9).
        let y = if boot.raft { hoehe * (0.888_888_9_f32 as f64) } else { hoehe / 3.0 };
        return rotate_yaw([0.0, y, vorn], vehicle_yaw_deg);
    }
    if matches!(vehicle, "camel" | "camel_husk") {
        // CamelEntity.getPassengerAttachmentPos, stehend (ein gerittenes Kamel steht): Hoehe
        // minus 0.375; vorn 0.5, zu zweit der Hintere 0.7 zurueck, Tiere je 0.2 weiter vorn.
        let erster = seat.index == 0;
        let mut vorn = 0.5;
        if seat.count > 1 {
            if !erster {
                vorn = -0.7;
            }
            if is_animal(passenger) {
                vorn += 0.2;
            }
        }
        return rotate_yaw([0.0, hoehe - 0.375, vorn], vehicle_yaw_deg);
    }
    if is_minecart(vehicle) && matches!(passenger, "villager" | "wandering_trader") {
        // AbstractMinecartEntity: Dorfbewohner sitzen IN der Lore, auf ihrem Boden.
        return [0.0, 0.0, 0.0];
    }
    let p = match passenger_points(vehicle) {
        // getPointOrDefault: die Liste ist nach Sitz geordnet, ueberzaehlige nehmen den letzten.
        Some(punkte) => punkte[seat.index.min(punkte.len() - 1)],
        None => [0.0, hoehe, 0.0],
    };
    rotate_yaw(p, vehicle_yaw_deg)
}

// Vec3d.rotateY(-yaw * pi/180), in f32 wie Vanilla (rotateY nimmt einen float-Winkel).
pub fn rotate_yaw(p: [f64; 3], yaw_deg: f32) -> [f64; 3] {
    let winkel = -yaw_deg * (std::f32::consts::PI / 180.0);
    let (c, s) = (winkel.cos() as f64, winkel.sin() as f64);
    [p[0] * c + p[2] * s, p[1], p[2] * c - p[0] * s]
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BoatKind {
    pub raft: bool,
    pub chest: bool,
}

// Boote und Floesse (mit und ohne Kiste) -- alles, was AbstractBoatEntity ist.
pub fn boat_kind(name: &str) -> Option<BoatKind> {
    if name.ends_with("_chest_raft") {
        Some(BoatKind { raft: true, chest: true })
    } else if name.ends_with("_raft") {
        Some(BoatKind { raft: true, chest: false })
    } else if name.ends_with("_chest_boat") {
        Some(BoatKind { raft: false, chest: true })
    } else if name.ends_with("_boat") {
        Some(BoatKind { raft: false, chest: false })
    } else {
        None
    }
}

pub fn is_minecart(name: &str) -> bool {
    name == "minecart" || name.ends_with("_minecart")
}

// Wer lenkt ein Fahrzeug, auf dem der eigene Spieler vorn sitzt? Vanillas
// getControllingPassenger je Klasse. Nur was hier `Some` liefert, bewegt der Client selbst
// (isLogicalSideForUpdatingMovement) und meldet es mit VehicleMove; alles andere (Loren, Lamas,
// ungesattelte Tiere) bewegt der Server, man sitzt nur drauf.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Steering {
    // AbstractBoatEntity: der erste Passagier lenkt, immer.
    Boat,
    // AbstractHorseEntity (Pferd, Esel, Maultier, Skelett- und Zombiepferd, Kamele): gesattelt.
    Horse,
    // PigEntity: gesattelt und Karottenrute in einer Hand.
    Pig,
    // StriderEntity: gesattelt und Wirrpilzrute in einer Hand.
    Strider,
    // HappyGhastEntity: mit Geschirr.
    HappyGhast,
}

// Welche Art Lenkung der Typ ueberhaupt kennt (ob sie gerade greift -- Sattel, Rute -- haengt
// am Zustand und steht in Steering-Pruefungen beim Aufrufer).
pub fn steering_kind(vehicle: &str) -> Option<Steering> {
    if boat_kind(vehicle).is_some() {
        return Some(Steering::Boat);
    }
    match vehicle {
        "horse" | "donkey" | "mule" | "skeleton_horse" | "zombie_horse" | "camel" | "camel_husk" => Some(Steering::Horse),
        "pig" => Some(Steering::Pig),
        "strider" => Some(Steering::Strider),
        "happy_ghast" => Some(Steering::HappyGhast),
        _ => None,
    }
}

// AnimalEntity und alles darunter -- maschinell aus der Klassenhierarchie des Jars (fuer jede
// Registrierung die Oberklassen-Kette bis AnimalEntity). Zaehlt nur fuer die Sitzordnung in
// Booten und auf Kamelen.
pub fn is_animal(name: &str) -> bool {
    matches!(
        name,
        "armadillo" | "axolotl" | "bee" | "camel" | "camel_husk" | "cat" | "chicken" | "cow" | "donkey" | "fox" | "frog" | "goat" | "happy_ghast"
            | "hoglin" | "horse" | "llama" | "mooshroom" | "mule" | "nautilus" | "ocelot" | "panda" | "parrot" | "pig" | "polar_bear" | "rabbit"
            | "sheep" | "skeleton_horse" | "sniffer" | "strider" | "trader_llama" | "turtle" | "wolf" | "zombie_horse" | "zombie_nautilus"
    )
}

// PASSENGER-Anker aus EntityType.Builder.passengerAttachments, je Sitz ein Punkt.
pub fn passenger_points(name: &str) -> Option<&'static [[f64; 3]]> {
    Some(match name {
        "cat" => &[[0.0, 0.5125, 0.0]],
        "chest_minecart" => &[[0.0, 0.1875, 0.0]],
        "chicken" => &[[0.0, 0.7, -0.1]],
        "command_block_minecart" => &[[0.0, 0.1875, 0.0]],
        "cow" => &[[0.0, 1.36875, 0.0]],
        "donkey" => &[[0.0, 1.1125, 0.0]],
        "drowned" => &[[0.0, 2.0125, 0.0]],
        "elder_guardian" => &[[0.0, 2.350625, 0.0]],
        "ender_dragon" => &[[0.0, 3.0, 0.0]],
        "enderman" => &[[0.0, 2.80625, 0.0]],
        "endermite" => &[[0.0, 0.2375, 0.0]],
        "evoker" => &[[0.0, 2.0, 0.0]],
        "fox" => &[[0.0, 0.6375, -0.25]],
        "frog" => &[[0.0, 0.375, -0.25]],
        "furnace_minecart" => &[[0.0, 0.1875, 0.0]],
        "ghast" => &[[0.0, 4.0625, 0.0]],
        "goat" => &[[0.0, 1.1125, 0.0]],
        "guardian" => &[[0.0, 0.975, 0.0]],
        "happy_ghast" => &[[0.0, 4.0, 1.7], [-1.7, 4.0, 0.0], [0.0, 4.0, -1.7], [1.7, 4.0, 0.0]],
        "hoglin" => &[[0.0, 1.49375, 0.0]],
        "hopper_minecart" => &[[0.0, 0.1875, 0.0]],
        "horse" => &[[0.0, 1.44375, 0.0]],
        "husk" => &[[0.0, 2.075, 0.0]],
        "illusioner" => &[[0.0, 2.0, 0.0]],
        "llama" => &[[0.0, 1.37, -0.3]],
        "minecart" => &[[0.0, 0.1875, 0.0]],
        "mooshroom" => &[[0.0, 1.36875, 0.0]],
        "mule" => &[[0.0, 1.2125, 0.0]],
        "nautilus" => &[[0.0, 1.1375, 0.0]],
        "ocelot" => &[[0.0, 0.6375, 0.0]],
        "parrot" => &[[0.0, 0.4625, 0.0]],
        "phantom" => &[[0.0, 0.3375, 0.0]],
        "pig" => &[[0.0, 0.86875, 0.0]],
        "piglin" => &[[0.0, 2.0125, 0.0]],
        "piglin_brute" => &[[0.0, 2.0125, 0.0]],
        "pillager" => &[[0.0, 2.0, 0.0]],
        "ravager" => &[[0.0, 2.2625, -0.0625]],
        "sheep" => &[[0.0, 1.2375, 0.0]],
        "silverfish" => &[[0.0, 0.2375, 0.0]],
        "skeleton_horse" => &[[0.0, 1.31875, 0.0]],
        "sniffer" => &[[0.0, 2.09375, 0.0]],
        "spawner_minecart" => &[[0.0, 0.1875, 0.0]],
        "spider" => &[[0.0, 0.765, 0.0]],
        "tnt_minecart" => &[[0.0, 0.1875, 0.0]],
        "trader_llama" => &[[0.0, 1.37, -0.3]],
        "turtle" => &[[0.0, 0.55625, -0.25]],
        "vex" => &[[0.0, 0.7375, 0.0]],
        "vindicator" => &[[0.0, 2.0, 0.0]],
        "warden" => &[[0.0, 3.15, 0.0]],
        "witch" => &[[0.0, 2.2625, 0.0]],
        "wolf" => &[[0.0, 0.81875, -0.0625]],
        "zoglin" => &[[0.0, 1.49375, 0.0]],
        "zombie" => &[[0.0, 2.0125, 0.0]],
        "zombie_horse" => &[[0.0, 1.31875, 0.0]],
        "zombie_nautilus" => &[[0.0, 1.1375, 0.0]],
        "zombie_villager" => &[[0.0, 2.125, 0.0]],
        "zombified_piglin" => &[[0.0, 2.0, 0.0]],
        _ => return None,
    })
}

// VEHICLE-Anker aus EntityType.Builder.vehicleAttachment -- immer senkrecht, also nur Y.
// Spieler und Puppen: PlayerLikeEntity.VEHICLE_ATTACHMENT.
pub fn vehicle_attachment_y(name: &str) -> f64 {
    match name {
        "allay" => 0.04,
        "bogged" => -0.7,
        "drowned" => -0.7,
        "evoker" => -0.6,
        "ghast" => 0.5,
        "giant" => -3.75,
        "happy_ghast" => 0.5,
        "husk" => -0.7,
        "illusioner" => -0.6,
        "mannequin" => 0.6,
        "parched" => -0.7,
        "phantom" => -0.125,
        "piglin" => -0.7,
        "piglin_brute" => -0.7,
        "pillager" => -0.6,
        "player" => 0.6,
        "skeleton" => -0.7,
        "stray" => -0.7,
        "vex" => 0.04,
        "vindicator" => -0.6,
        "wither_skeleton" => -0.875,
        "zombie" => -0.7,
        "zombie_villager" => -0.7,
        "zombified_piglin" => -0.7,
        _ => 0.0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ALLEIN: Seat = Seat { index: 0, count: 1 };

    #[test]
    fn spieler_sitzt_im_boot_auf_einem_drittel_minus_seinem_anker() {
        // Boot 0.5625 hoch -> Sitz 0.1875; Spieler-Anker 0.6 -> Fuesse 0.4125 unter der Bootsunterkante.
        let p = riding_pos("oak_boat", 0.5625, [10.0, 64.0, 5.0], 0.0, ALLEIN, "player");
        assert!((p[1] - (64.0 + 0.1875 - 0.6)).abs() < 1e-6, "{p:?}");
        assert!((p[0] - 10.0).abs() < 1e-9 && (p[2] - 5.0).abs() < 1e-9);
    }

    #[test]
    fn zu_zweit_im_boot_vorn_und_hinten_mit_dem_boot_gedreht() {
        // Yaw 0 = Blick nach +Z: vorn ist +Z.
        let vorn = passenger_offset("oak_boat", 0.5625, 0.0, Seat { index: 0, count: 2 }, "player");
        let hinten = passenger_offset("oak_boat", 0.5625, 0.0, Seat { index: 1, count: 2 }, "player");
        assert!((vorn[2] - 0.2).abs() < 1e-6 && (hinten[2] + 0.6).abs() < 1e-6);
        // Yaw 90 = Blick nach -X.
        let vorn = passenger_offset("oak_boat", 0.5625, 90.0, Seat { index: 0, count: 2 }, "player");
        assert!((vorn[0] + 0.2).abs() < 1e-6 && vorn[2].abs() < 1e-6, "{vorn:?}");
    }

    #[test]
    fn ohne_tabelleneintrag_oben_auf_der_box() {
        // Schreiter: kein eigener Anker -> seine Hoehe 1.7.
        assert!((passenger_offset("strider", 1.7, 0.0, ALLEIN, "player")[1] - 1.7).abs() < 1e-6);
        assert!((passenger_offset("horse", 1.6, 0.0, ALLEIN, "player")[1] - 1.44375).abs() < 1e-6);
        assert!((passenger_offset("minecart", 0.7, 0.0, ALLEIN, "player")[1] - 0.1875).abs() < 1e-6);
    }

    #[test]
    fn gluecklicher_ghast_hat_vier_plaetze() {
        let dritter = passenger_offset("happy_ghast", 4.0, 0.0, Seat { index: 2, count: 4 }, "player");
        assert!((dritter[2] + 1.7).abs() < 1e-6 && (dritter[1] - 4.0).abs() < 1e-6);
        // Mehr als vier: der letzte Platz.
        let fuenfter = passenger_offset("happy_ghast", 4.0, 0.0, Seat { index: 7, count: 8 }, "player");
        assert!((fuenfter[0] - 1.7).abs() < 1e-6);
    }

    #[test]
    fn lenkung_je_typ() {
        assert_eq!(steering_kind("bamboo_chest_raft"), Some(Steering::Boat));
        assert_eq!(steering_kind("mule"), Some(Steering::Horse));
        assert_eq!(steering_kind("minecart"), None);
        assert_eq!(steering_kind("llama"), None);
    }
}
