// Entity, Schnappschuss, gehaltene Items und ihre Eigenschaften.

use super::*;

pub struct Entity {
    pub uuid: [u8; 16],
    // Index in der ENTITY_TYPE-Registry (1.21.11). Wird vorerst nur durchgereicht;
    // fuer das Box-Rendering ist der konkrete Typ egal.
    pub entity_type: i32,
    // AKTUELLE, tick-quantisierte Position (Fuesse) -- Vanillas entity.getEntityPos(). Das
    // ist die Groesse, gegen die gezielt und getroffen wird, NICHT die zuletzt vom Server
    // geschickte (die steht in target_pos).
    pub pos: (f64, f64, f64),
    // Stand zu Beginn des laufenden Ticks (Vanillas lastX/lastY/lastZ, gesetzt in
    // baseTick -> tickLastPos) -- der Startpunkt der Frame-Interpolation.
    pub(crate) last_pos: (f64, f64, f64),
    // Autoritative Serverposition (Vanillas TrackedPosition). Relative Bewegungspakete
    // rechnen ihre Deltas hierauf, NICHT auf die interpolierte Position -- sonst summierte
    // sich der Interpolationsrueckstand von Paket zu Paket auf.
    pub(crate) tracked_pos: (f64, f64, f64),
    // Ziel der laufenden Angleichung samt offener Schritte (PositionInterpolator.Data).
    pub(crate) target_pos: (f64, f64, f64),
    pub(crate) target_yaw_deg: f32,
    pub(crate) target_pitch_deg: f32,
    pub(crate) interp_steps: u32,
    // Nur fuer Projektile (s. projectile_kind): ihre eigene Geschwindigkeit. Der Server
    // schickt sie beim Spawn und danach selten -- geflogen wird clientseitig.
    pub(crate) vel: (f64, f64, f64),
    // Steckt das Projektil in einem Block? Dann bewegt es sich nicht mehr
    // (AbstractArrow.inGround); ein Wurfgeschoss ist an der Stelle ohnehin zu Ende.
    pub(crate) in_ground: bool,
    // AbstractArrow.ID_FLAGS Bit 0x01: voll gespannt geschossen -- zieht im Flug eine Spur aus
    // Krit-Partikeln (s. crit_trail).
    pub(crate) crit_arrow: bool,
    // Groesse eines interaction-Entities (Breite, Hoehe) aus den Metadaten; Vanilla-Vorgabe 1 x 1.
    pub(crate) interaction_size: (f64, f64),
    // Einmal beim Spawn aufgeloest, damit nicht jeder Tick den Typnamen nachschlaegt.
    pub(crate) projectile: Option<ProjectileKind>,
    // Ein liegender/fallender Gegenstand (Typ "item"): der Client simuliert ihn selbst wie
    // Vanillas ItemEntity.tick, s. tick_item. Der Server schickt seine Position nur alle 20
    // Ticks -- ohne eigene Simulation sprang ein geworfenes Item von Paket zu Paket.
    pub(crate) item_physics: bool,
    // Ein fallender Block (Typ "falling_block"): Vanillas FallingBlockEntity.tick laeuft auf
    // dem Client mit (Schwerkraft, Bewegung, Reibung), der Server meldet die Position nur
    // selten. Ohne eigene Simulation hinge der Block zwischen den Meldungen in der Luft.
    pub(crate) block_physics: bool,
    // Ein Erfahrungsorb (Typ "experience_orb"): wie in Vanillas ExperienceOrb.tick fliegt er auf
    // dem Client selbst zum naechsten Spieler, s. tick_orb. Der Server meldet ihn nur alle 20
    // Ticks -- ohne eigene Simulation kam ein Orb erst mit dem naechsten Paket ruckartig an.
    pub(crate) orb_physics: bool,
    // Der Spieler, dem der Orb gerade folgt (Entity-ID), s. tick_orb.
    pub(crate) orb_target: Option<i32>,
    // Der Blockzustand eines fallenden Blocks (Data-Feld des Spawn-Pakets), sonst None.
    pub falling_block: Option<u32>,
    // Gegenstandsrahmen: zeigt in diese Richtung (Direction-ID aus dem Spawn-Paket: 0 unten,
    // 1 oben, 2 Norden, 3 Sueden, 4 Westen, 5 Osten) und dreht seinen Inhalt in Achteln.
    pub frame_facing: u8,
    pub frame_rotation: i32,
    // Jungtier (Metadaten Index 16, nur bei Typen aus can_be_baby): halbe Hitbox.
    pub baby: bool,
    // Attribut SCALE (Update Attributes): Box, Augenhoehe und Modell mal diesen Faktor.
    // Plugins machen damit Spieler riesig oder winzig -- mit der festen Box zielte man daneben.
    pub scale: f32,
    // Dorfbewohner-Daten (Biom-Typ, Beruf, Stufe), s. mob_catalog::villager_layers.
    pub villager_data: Option<(i32, i32, i32)>,
    pub(crate) on_ground: bool,
    // REIN OPTISCH: der Versatz vom echten Spawnpunkt zur Hand des Schuetzen, plus die Zahl
    // der Ticks, ueber die er wieder auf Null zurueckgeht (s. launch_render_offset).
    pub(crate) launch_offset: (f32, f32, f32),
    pub(crate) launch_ticks: u32,
    // Tick-quantisierte Rotation (wie pos) plus ihr Stand zu Tickbeginn.
    pub yaw_deg: f32,
    pub pitch_deg: f32,
    pub(crate) last_yaw_deg: f32,
    pub(crate) last_pitch_deg: f32,
    // Ankunftszeit des letzten Positions-Updates.
    pub(crate) updated_at: Instant,
    // Reine Diagnose (F3-Overlay): die Abstaende der letzten Positions-Updates in
    // Millisekunden, juengster zuletzt. Das ist die Groesse, die bestimmt, wie alt die
    // angezeigte Position eines fremden Entities im schlechtesten Fall ist -- der Server
    // sendet nicht mit 20 Hz, sondern im updateInterval seines Entity-Trackers.
    pub(crate) update_intervals: VecDeque<f32>,
    // Dauer der AKTUELLEN Lerp-Strecke (s. Modul-Kommentar: adaptiv statt fest 1/20s).
    // Ob schon ein echtes Bewegungs-Update nach dem Spawn ankam -- nur noch fuer die
    // Diagnose (der Abstand SPAWN->erstes Update ist kein Paket-Takt und wuerde die
    // Messreihe verzerren).
    pub(crate) has_moved: bool,
    // Schrittanimation (Vanillas limbSwingAnimationProgress / limbSwingAmplitude, gefuehrt
    // von LimbAnimator). Der Server schickt das NICHT -- Vanilla rechnet es genauso lokal aus
    // der zurueckgelegten Strecke aus. Anders als Vanilla haben wir aber keine 20 Ticks pro
    // Sekunde an Positions-Updates, sondern die Update-Rate des Trackers (~10 Hz bei
    // Spielern, ~6.7 Hz bei Mobs), deshalb wird die Strecke auf Tick-Aequivalente
    // umgerechnet statt pro Paket einmal gezaehlt zu werden.
    pub(crate) limbs: crate::entity_model::LimbAnimator,
    // Nachhinkende Position fuer den Umhang (s. entity_model::CapeState). Wie die
    // Schrittanimation rein clientseitig gerechnet und, wie sie, auf Tick-Aequivalente
    // umgerechnet -- der Zustand ist eine Tick-Groesse, unsere Updates sind es nicht.
    //
    // Nur fuer Spieler ueberhaupt von Bedeutung; ein paar Byte pro Mob sind billiger als eine
    // zweite Tabelle daneben, die man synchron halten muesste.
    pub(crate) cape: crate::entity_model::CapeState,
    // Alter in Ticks, seit wir dieses Entity zum ersten Mal gesehen haben. Treibt die
    // Ruhe-Bewegung der Arme (s. entity_model::apply_arm_sway).
    //
    // Vanilla nimmt das ECHTE Entity-Alter (Ticks seit dem Spawn in der Welt), das wir nicht
    // kennen. Fuer die Bewegung ist das gleichwertig: sie ist periodisch (~70 Ticks), es zaehlt
    // also nur der Rest modulo Periode, und ein anderer Nullpunkt aendert nur die Phase. Weil
    // Entities zu verschiedenen Zeiten auftauchen, ergibt sich die Phasenstreuung von selbst --
    // sie atmen nicht im Gleichschritt.
    pub(crate) age_ticks: f32,
    // Kopfdrehung. Geglaettet ueber einen EIGENEN Zaehler (s. HEAD_YAW_STEPS) -- der Server
    // schickt Kopf- und Blickdrehung (`yaw_deg`) in getrennten Paketen, nicht im selben Takt.
    pub(crate) head_yaw_deg: f32,
    pub(crate) last_head_yaw_deg: f32,
    pub(crate) server_head_yaw_deg: f32,
    pub(crate) head_steps: u32,
    // KOERPERdrehung eines Lebewesens (Vanillas bodyYaw). Die kommt NICHT vom Server: `yaw_deg`
    // ist getYaw(), also die Blickrichtung -- den Koerper rechnet jeder Client selbst, in
    // LivingEntity.tick (Ziel = Laufrichtung) + turnHead (s. entity_model::turn_body). Frueher
    // stand hier die Annahme, beide kaemen fertig vom Server; die Figur zeigte dadurch immer
    // in Blickrichtung und lief beim Seitwaertslaufen seitlich, waehrend die Beine vor und
    // zurueck schwangen.
    pub(crate) body_yaw_deg: f32,
    pub(crate) last_body_yaw_deg: f32,
    // Wie lange das Wesen schon ununterbrochen ein Item benutzt -- fuer isBlocking, das erst
    // nach der Block-Verzoegerung des Schildes greift (s. is_blocking).
    pub(crate) use_ticks: u32,
    // Was das Entity in den Haenden haelt (Set Equipment, s. packets::CB_PLAY_SET_EQUIPMENT).
    // None = leere Hand. Der Server schickt das nur bei AENDERUNGEN, deshalb wird der Wert
    // hier gehalten und nicht pro Frame neu erfragt.
    pub main_hand: Option<HeldItem>,
    pub off_hand: Option<HeldItem>,
    // Getragene Ruestung, nach ArmorSlot::index() abgelegt (Fuesse, Beine, Brust, Kopf).
    // Kommt aus demselben Set-Equipment-Paket wie die Haende -- frueher wurden diese vier
    // Slots verworfen, und ein Gegner in voller Diamantruestung sah aus wie in Zivil.
    pub armor: [Option<HeldItem>; 4],
    // Pose-Index aus dem Metadaten-Strom (Entity.POSE, s. packets::read_set_entity_data).
    // Bestimmt die Hoehe der Trefferbox; Standard ist STANDING (0), denn genau das ist auch
    // Vanillas Vorgabewert, solange der Server nichts anderes schickt.
    pub pose: u8,
    // Restliche Ticks des Schadens-Aufblitzens (Vanillas LivingEntity.hurtTime). 0 = kein
    // Blitz. Gesetzt aus dem Damage-Paket, heruntergezaehlt im Tick -- genau wie in Vanilla.
    pub(crate) hurt_ticks: u8,
    // Ticks seit dem letzten Treffer (u32::MAX = nie). Fuer die Schadens-Faerbung, deren
    // Dauer einstellbar ist; hurt_ticks bleibt Vanillas Zaehler.
    pub(crate) ticks_since_hurt: u32,
    // Der Handschlag (LivingEntity.handSwinging / handSwingTicks / handSwingProgress). Anders
    // als das Schrittschwingen ist das KEINE lokale Rechnung: der Server schickt jeden Schlag
    // als eigenes Paket (EntityAnimationS2CPacket, s. packets::CB_PLAY_ANIMATE), weil er nicht
    // aus der Bewegung folgt. Der Verlauf danach ist dann wieder rein clientseitig.
    //
    // `hand_swing_ticks` ist bewusst VORZEICHENBEHAFTET: Vanilla setzt beim Anstossen -1, damit
    // der erste Tick danach auf 0 landet und der Schlag mit progress 0 beginnt statt schon
    // einen Schritt weit fortgeschritten zu sein.
    // Stammt der laufende Trefferblitz von einem KRITISCHEN Treffer? Der Server schickt dafuer
    // ein eigenes Paket (EntityAnimationS2CPacket mit CRIT), und zwar an alle in der Naehe
    // EINSCHLIESSLICH des Getroffenen selbst (ServerPlayerEntity.addCritParticles ->
    // sendToNearbyPlayers -> sendToSelfAndListeners) -- anders als beim Armschwung gibt es hier
    // also keine Selbst-Ausnahme, man sieht auch den Crit auf sich selbst.
    pub(crate) hurt_by_crit: bool,
    // Ein Crit-Paket, das VOR dem Schadenspaket eintrifft, wartet hier ein paar Ticks auf
    // seinen Treffer. In Vanilla ist die Reihenfolge umgekehrt (PlayerEntity.attack ruft erst
    // sidedDamage und nur bei Erfolg addAttackParticlesAndSounds), aber sich auf eine
    // Paketreihenfolge zu verlassen, die man nicht erzwingt, ist eine Wette -- und ein
    // vertauschtes Paar faerbte den Blitz still in der falschen Farbe.
    pub(crate) crit_grace: u8,
    // Vom eigenen Client gelenkt (das Boot, Pferd ... unter dem eigenen Spieler, s.
    // Entities::set_driven): Vanillas isLogicalSideForUpdatingMovement. Solange das gilt,
    // verschieben Positionspakete des Servers nur noch tracked_pos, die Stelle selbst setzt der
    // Client (driven_next) -- sonst zoege die Interpolation das Fahrzeug laufend auf den
    // Stand zurueck, den der Server vor einem halben Ping von uns gehoert hat.
    pub(crate) driven: bool,
    // Die Stelle und Drehung, die der naechste Tick uebernimmt (s. Entities::drive).
    pub(crate) driven_next: Option<((f64, f64, f64), f32, f32)>,
    // Sattel (Slot SADDLE) und Koerper-Slot (BODY: Pferderuestung, Geschirr des gluecklichen
    // Ghasts) -- entscheiden, ob ein Reittier sich lenken laesst (s. riding::Steering).
    pub saddled: bool,
    pub body_item: Option<HeldItem>,
    pub(crate) hand_swinging: bool,
    pub(crate) hand_swing_ticks: i32,
    pub(crate) hand_swing_progress: f32,
    pub(crate) last_hand_swing_progress: f32,
    // Mit welcher Hand geschlagen wurde (Vanillas preferredHand).
    pub(crate) swing_off_hand: bool,
    // Entity.CUSTOM_NAME / Entity.NAME_VISIBLE. Zusammen ergeben sie das Namensschild eines
    // Entities -- und in der Praxis fast jedes Server-"Hologramm": ein unsichtbarer
    // Armorstand mit dauerhaft eingeblendetem Namen. Vanillas Entity.shouldRenderName() ist
    // genau `isCustomNameVisible()`.
    pub custom_name: Option<Vec<protocol::nbt::TextSpan>>,
    pub name_visible: bool,
    // Nur beim End-Kristall: Bodenplatte zeichnen (EndCrystalEntity.SHOW_BOTTOM, Vorgabe an).
    pub end_crystal_show_bottom: bool,
    // TamableAnimal-Flags einer Katze (Bit 1 = sitzt, s. packets::TAMABLE_FLAG_SITTING).
    pub tamable_flags: u8,
    // Registry-ID der Katzenvariante; None, solange der Server keine geschickt hat.
    pub cat_variant: Option<i32>,
    // Nur bei Lebewesen: Farben der Trank-Wirbel und ob es nur Umgebungs-Effekte sind (s.
    // packets::EntityDataUpdate::potion_swirls).
    pub potion_swirls: Vec<u32>,
    pub potion_swirls_ambient: bool,
    // Nur bei einer Trankwolke (area_effect_cloud): Radius, ob sie noch wartet, und welcher
    // Partikel sie fuellt (Vorgabe: weisse Effekt-Wirbel).
    pub cloud_radius: f32,
    pub cloud_waiting: bool,
    pub cloud_particle: protocol::particle::ParticleEffect,
    // Nur bei Booten und Floessen: welches Ruder sich bewegt (links, rechts; AbstractBoat
    // DATA_ID_PADDLE_LEFT/-RIGHT) und wie weit es schon gerudert hat (paddlePositions, je Tick
    // PI/8 weiter, ruhend 0). Daran haengen die Ruder des Modells und die Arme des Fahrers.
    pub boat_paddles: [bool; 2],
    pub(crate) paddle_positions: [f32; 2],
    // Nur bei Loren (AbstractMinecart): der eigene Anzeigeblock (None = die Vorgabe der
    // Lorenart, s. mob_catalog::minecart_display), sein Versatz in Pixeln (None = Vorgabe) und
    // bei der Ofenlore, ob sie brennt (MinecartFurnace.DATA_ID_FUEL).
    pub minecart_block: Option<u32>,
    pub minecart_offset: Option<i32>,
    pub minecart_fuel: bool,
    // Nur bei der TNT-Lore: Ticks bis zur Explosion, -1 = nicht gezuendet. Gezuendet meldet der
    // Server mit Entity-Ereignis 10 (MinecartTNT.handleEntityEvent -> primeFuse, 80 Ticks),
    // heruntergezaehlt wird wie dort im Tick des Clients.
    pub minecart_tnt_fuse: i32,
    // Nur bei Spielern: erkennt den Handtausch fuer Mehr Animationen (s. more_anims).
    pub swap: crate::more_anims::SwapWatch,
    // Nur bei Spielern: das zuletzt gehaltene Schwert (Schwert auf dem Ruecken, s. more_anims).
    pub last_sword: Option<HeldItem>,
    // Nur bei einer Feuerwerksrakete: an welchem Schuetzen sie haengt (Elytra-Schub), s.
    // packets::FIREWORK_INDEX_SHOOTER.
    pub firework_shooter: Option<i32>,
    // Entity.FLAGS Bit 7: gleitet mit Elytra.
    pub gliding: bool,
    // Entity.FLAGS Bit 3: sprintet.
    pub sprinting: bool,
    // LivingEntity.glidingTicks: Ticks seit Beginn des Gleitflugs, 0 am Boden. Daran haengt,
    // wie weit sich die Figur schon in die Flughaltung gelegt hat (s. entity_model::Glide).
    pub(crate) gliding_ticks: u32,
    // LivingEntity.swimAmount / swimAmountO: wie weit sich die Figur in die Schwimmhaltung
    // gelegt hat (0..1, je Tick 0.09 in der Pose SWIMMING hoch, sonst runter).
    pub(crate) swim_amount: f32,
    pub(crate) last_swim_amount: f32,
    // Entity.isInWater: im Wasser richtet sich die schwimmende Figur nach dem Blick, an Land
    // (Kriechen) liegt sie waagerecht. Clientseitig je Tick bestimmt wie in Vanillas baseTick.
    pub(crate) in_water: bool,
    // Entity.FLAGS Bit 0: brennt -- dann zeichnet Vanilla das Feuer um das Entity.
    pub on_fire: bool,
    // Der getragene Gegenstand von Item-Entities und Wurfgeschossen (s. item_render_kind),
    // mit Stapelgroesse -- die entscheidet beim liegenden Item, wie viele Kopien man sieht.
    // None = noch nichts empfangen; dann gilt die Vorgabe des Typs.
    pub item: Option<HeldItem>,
    pub item_count: i32,
    // XP-Kugel: ihr Wert (ExperienceOrb.DATA_VALUE), bestimmt das Bild.
    pub orb_value: i32,
    // Gezuendetes TNT (PrimedTnt): verbleibende Ticks bis zur Explosion (zaehlt auch auf dem
    // Client herunter) und der Block, der gezeichnet wird -- None = TNT.
    pub tnt_fuse: i32,
    pub tnt_block: Option<u32>,
    // ItemStackEntityRenderState.getSeed: Item-Id + Schaden. Streut die Kopien eines Stapels
    // immer gleich, damit sie nicht flackern.
    pub item_seed: i32,
    // Entity.FLAGS Bit 5. Ein unsichtbares Entity bekommt KEIN Modell, sein Namensschild
    // aber sehr wohl -- sonst verschwaende der Traeger eines Hologramms samt Text.
    pub invisible: bool,
    // Entity.FLAGS Bit 6: leuchtet -- Umriss in Teamfarbe, auch durch Waende.
    pub glowing: bool,
    // LivingEntity.LIVING_FLAGS: benutzt dieses Wesen gerade ein Item, und mit welcher Hand?
    // Daran haengt das gehobene Schild (s. blocking_hand).
    pub living_flags: u8,
    // PlayerLikeEntity.MAIN_ARM: Haupthand links? Dann haelt der LINKE Arm das Haupthand-Item
    // und die Nebenhand den rechten (s. arm_items) -- frueher war die Haupthand hier immer
    // rechts, und bei Linkshaendern sass z.B. das Essen da, wo Vanilla das Schild zeigt.
    pub left_handed: bool,
    // Nur bei den drei Display-Typen belegt (s. display_kind). Angelegt wird der Eintrag
    // schon beim Spawn, damit ein Display-Entity auch dann nicht als Kasten erscheint, wenn
    // sein Metadaten-Paket noch unterwegs ist.
    pub display: Option<DisplayInfo>,
    // ArmorStandEntity.ARMOR_STAND_FLAGS, nur bei Armorstaenden gefuellt. Entscheidet ueber
    // die Hoehe (s. armor_stand_height) und damit darueber, wo das Namensschild sitzt.
    pub(crate) armor_stand_flags: u8,
    // Gelenkwinkel in Grad, s. ARMOR_STAND_DEFAULT_POSES.
    pub armor_stand_poses: [[f32; 3]; 6],
}

// Grundhaltung eines Armorstands, solange der Server nichts anderes schickt (ArmorStand.
// DEFAULT_*_POSE): Kopf, Rumpf, linker Arm, rechter Arm, linkes Bein, rechtes Bein, je
// (x, y, z) in Grad.
// Ein Entity als Spawn, Metadaten und Ausruestung (s. Entities::snapshot).
pub struct EntitySnapshot {
    pub spawn: protocol::packets::AddEntity,
    pub meta: protocol::packets::EntityDataUpdate,
    pub equipment: Vec<protocol::packets::EquipmentEntry>,
    pub pose: u8,
}

// Der ganze Stapel hinter einem gehaltenen Item. Ist er aus dem Ring verdraengt, bleibt nur das
// Item selbst.
pub(crate) fn held_stack(h: &HeldItem) -> protocol::item::ItemStack {
    crate::item_components::resolve(h.stack).map_or_else(|| protocol::item::ItemStack::new(h.item_id, 1), |s| (*s).clone())
}

impl Entity {
    pub(crate) fn snapshot(&self, id: i32) -> EntitySnapshot {
        use protocol::packets as p;
        let name = entity_type_name(self.entity_type);
        let data = if self.falling_block.is_some() {
            self.falling_block.unwrap_or(0) as i32
        } else if is_item_frame(self.entity_type) {
            i32::from(self.frame_facing)
        } else {
            0
        };
        let spawn = p::AddEntity {
            entity_id: id,
            uuid: self.uuid,
            entity_type: self.entity_type,
            x: self.tracked_pos.0,
            y: self.tracked_pos.1,
            z: self.tracked_pos.2,
            yaw_deg: self.target_yaw_deg,
            pitch_deg: self.target_pitch_deg,
            head_yaw_deg: self.server_head_yaw_deg,
            vel: self.vel,
            data,
        };
        let mut flags = 0;
        for (an, bit) in [
            (self.on_fire, p::ENTITY_FLAG_ON_FIRE),
            (self.sprinting, p::ENTITY_FLAG_SPRINTING),
            (self.invisible, p::ENTITY_FLAG_INVISIBLE),
            (self.glowing, p::ENTITY_FLAG_GLOWING),
            (self.gliding, p::ENTITY_FLAG_GLIDING),
        ] {
            if an {
                flags |= bit;
            }
        }
        let item = self.item.as_ref().map(held_stack);
        let mut m = p::EntityDataUpdate {
            entity_id: id,
            flags: Some(flags),
            main_arm_left: Some(self.left_handed),
            living_flags: Some(self.living_flags),
            villager_data: self.villager_data,
            baby: Some(self.baby),
            custom_name: Some(self.custom_name.clone()),
            name_visible: Some(self.name_visible),
            potion_swirls: Some(self.potion_swirls.clone()),
            potion_swirls_ambient: Some(self.potion_swirls_ambient),
            firework_shooter: (name == Some("firework_rocket")).then_some(self.firework_shooter),
            frame_item: is_item_frame(self.entity_type).then(|| item.clone()),
            frame_rotation: Some(self.frame_rotation),
            hanging_direction: is_item_frame(self.entity_type).then_some(self.frame_facing),
            orb_value: match name {
                Some("experience_orb") => Some(self.orb_value),
                Some("tnt") => Some(self.tnt_fuse),
                _ => None,
            },
            tnt_block_state: self.tnt_block,
            item_stack: item_render_kind(self.entity_type).is_some().then(|| item.clone()),
            tamable_flags: Some(self.tamable_flags),
            cat_variant: self.cat_variant,
            arrow_in_ground: Some(self.in_ground),
            interaction_width: Some(self.interaction_size.0 as f32),
            interaction_height: Some(self.interaction_size.1 as f32),
            boat_paddles: self.boat_paddles.map(Some),
            cloud_radius: Some(self.cloud_radius),
            cloud_waiting: Some(self.cloud_waiting),
            cloud_particle: Some(self.cloud_particle.clone()),
            end_crystal_show_bottom: Some(self.end_crystal_show_bottom),
            armor_stand_flags: (name == Some("armor_stand")).then_some(self.armor_stand_flags),
            armor_stand_poses: self.armor_stand_poses.map(Some),
            pose: Some(self.pose),
            ..Default::default()
        };
        if let Some(d) = &self.display {
            m.display_translation = Some(d.translation);
            m.display_scale = Some(d.scale);
            m.display_left_rotation = Some(d.left_rotation);
            m.display_right_rotation = Some(d.right_rotation);
            m.display_billboard = Some(d.billboard);
            m.display_view_range = Some(d.view_range);
            m.display_interp_duration = Some(d.interp_duration);
            m.display_teleport_duration = Some(d.teleport_duration);
            m.item_display_mode = Some(d.item_mode);
            match &d.content {
                DisplayContent::Text(t) => {
                    m.display_text = Some(t.spans.clone());
                    m.text_background = Some(t.background);
                    m.text_opacity = Some(t.opacity);
                    m.text_flags = Some(t.flags);
                }
                DisplayContent::Item(i) => m.display_item = Some(i.as_ref().map(held_stack)),
                DisplayContent::Block(b) => m.display_block_state = Some(*b),
            }
        }
        let mut equipment = Vec::new();
        let mut slot = |slot: u8, h: &Option<HeldItem>| {
            if let Some(h) = h {
                equipment.push(p::EquipmentEntry { slot, item: Some(held_stack(h)) });
            }
        };
        slot(p::EQUIPMENT_SLOT_MAINHAND, &self.main_hand);
        slot(p::EQUIPMENT_SLOT_OFFHAND, &self.off_hand);
        slot(p::EQUIPMENT_SLOT_BODY, &self.body_item);
        for s in 2..=5 {
            if let Some(a) = crate::armor::ArmorSlot::from_equipment_slot(s) {
                slot(s, &self.armor[a.index()]);
            }
        }
        if self.saddled {
            if let Some(sattel) = crate::item_registry::item_id_by_name("saddle") {
                equipment.push(p::EquipmentEntry { slot: p::EQUIPMENT_SLOT_SADDLE, item: Some(protocol::item::ItemStack::new(sattel, 1)) });
            }
        }
        EntitySnapshot { spawn, meta: m, equipment, pose: self.pose }
    }
}

pub const ARMOR_STAND_DEFAULT_POSES: [[f32; 3]; 6] =
    [[0.0, 0.0, 0.0], [0.0, 0.0, 0.0], [-10.0, 0.0, -10.0], [-15.0, 0.0, 10.0], [-1.0, 0.0, -1.0], [1.0, 0.0, 1.0]];

// Wie Vanilla ein Entity mit getragenem Gegenstand zeichnet.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum ItemRenderKind {
    // ItemEntityRenderer: liegend, wippend, drehend, je nach Stapel mehrere Kopien.
    Dropped,
    // FlyingItemEntityRenderer: zur Kamera gedreht, mit Skalierung. `default_item` ist der
    // Gegenstand, solange der Server keinen Stack geschickt hat (getDefaultItem).
    Flying { scale: f32, default_item: &'static str },
}

// EntityRendererFactories: welche Typen als Gegenstand gezeichnet werden. Die Feuerwerksrakete
// hat einen eigenen Renderer, zeigt aber ebenso nur ihr Item zur Kamera -- hier wie ein
// fliegendes Item behandelt.
#[must_use] 
pub fn item_render_kind(entity_type: i32) -> Option<ItemRenderKind> {
    let flying = |scale, default_item| Some(ItemRenderKind::Flying { scale, default_item });
    match entity_type_name(entity_type)? {
        "item" => Some(ItemRenderKind::Dropped),
        "egg" => flying(1.0, "egg"),
        "snowball" => flying(1.0, "snowball"),
        "ender_pearl" => flying(1.0, "ender_pearl"),
        "experience_bottle" => flying(1.0, "experience_bottle"),
        "splash_potion" => flying(1.0, "splash_potion"),
        "lingering_potion" => flying(1.0, "lingering_potion"),
        "eye_of_ender" => flying(1.0, "ender_eye"),
        "fireball" => flying(3.0, "fire_charge"),
        "small_fireball" => flying(0.75, "fire_charge"),
        "firework_rocket" => flying(1.0, "firework_rocket"),
        _ => None,
    }
}

// Ein Item in der Hand, reduziert auf das, was zum Zeichnen reicht. Bewusst nicht der ganze
// ItemStack: Anzahl, Haltbarkeit und Name spielen am Modell in der Hand keine Rolle, und so
// bleibt die Entity-Tabelle klein.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct HeldItem {
    // Index in der ITEM-Registry (s. item_registry::item_name) -- NICHT die Block-State-ID.
    pub item_id: u32,
    // Welches Item-MODELL gezeichnet wird: die item_model-Komponente, wenn sie auf ein
    // Vanilla-Item zeigt, sonst item_id. Server bauen so eigene Items -- der Golden Head auf
    // mcpvp.club ist eine Honigflasche (trinkbar) mit item_model player_head und Skin.
    // Spiellogik (Bogen, Schild, Essen) bleibt an item_id, nur das Bild folgt model_id.
    pub model_id: u32,
    // Glint-Schimmer (ItemStack.hasGlint, s. enchantment::has_glint) -- nicht nur "hat
    // Verzauberungen": auch Buch, Netherstern & Co. sowie der Override zaehlen.
    pub enchanted: bool,
    // Trankfarbe (potion::stack_color) -- ein Trank in der Hand ist sonst farblos.
    pub potion_color: Option<u32>,
    // Bei Spielerkoepfen: unter welchem Schluessel der Skin aus der profile-Komponente steht
    // (s. head_skin_key). None bei allen anderen Items.
    pub head_skin: Option<[u8; 16]>,
    // dyed_color (gefaerbtes Leder), 0xRRGGBB.
    pub dyed: Option<u32>,
    // trim: (Material-ID, Muster-ID), s. protocol::item::ItemStack::trim.
    pub trim: Option<(i32, i32)>,
    // Banner auf einem Schild (base_color / banner_patterns), s. ShieldBanner.
    pub banner: Option<ShieldBanner>,
    // Was es getragen zeigt (equippable-Component), s. armor::worn_asset.
    pub equip: crate::armor::Equip,
    // Der ganze Stapel fuer die Item-Modelle (s. item_components::StackRef).
    pub stack: crate::item_components::StackRef,
    // Gefuellte Karte: ihre map_id. Eigens hier und nicht nur im Stapel -- dessen Ring ist auf
    // einem vollen Server irgendwann ueberschrieben, und die Karte im Rahmen fiele dann aufs
    // gewoehnliche Item-Modell zurueck.
    pub map_id: Option<i32>,
}

// Das Banner eines Schildes, in Copy-Form fuer die Entity-Tabelle: Grundfarbe und bis zu acht
// Lagen (Index in der banner_pattern-Registry, Farbe) -- mehr zeigt auch Vanillas Banner
// nicht ueber normale Wege, und ein Server mit mehr verliert nur die obersten.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ShieldBanner {
    pub base: u8,
    pub layers: [(u16, u8); 8],
    pub len: u8,
}

impl ShieldBanner {
    // Vanilla (ShieldModelRenderer) zeigt ein Banner, sobald Grundfarbe ODER Muster gesetzt
    // sind; fehlt die Grundfarbe, ist sie Weiss.
    #[must_use] 
    pub fn from_stack(s: &protocol::item::ItemStack) -> Option<Self> {
        if s.base_color.is_none() && s.banner_patterns.is_empty() {
            return None;
        }
        let mut layers = [(0u16, 0u8); 8];
        let mut len = 0u8;
        for (id, farbe) in s.banner_patterns.iter().filter(|(id, _)| *id >= 0).take(8) {
            layers[len as usize] = (*id as u16, *farbe);
            len += 1;
        }
        Some(Self { base: s.base_color.unwrap_or(0), layers, len })
    }

    #[must_use] 
    pub fn layers(&self) -> &[(u16, u8)] {
        &self.layers[..self.len as usize]
    }
}

impl HeldItem {
    pub fn from_stack(s: &protocol::item::ItemStack) -> Self {
        Self {
            item_id: s.item_id,
            model_id: model_item_id(s),
            enchanted: crate::enchantment::has_glint(s),
            potion_color: crate::potion::stack_color(s),
            head_skin: s.profile.as_ref().and_then(head_skin_key),
            dyed: s.dyed_color,
            trim: s.trim,
            banner: ShieldBanner::from_stack(s),
            equip: crate::armor::Equip::from_stack(s),
            stack: crate::item_components::intern(s),
            map_id: s.map_id,
        }
    }

    #[must_use] 
    pub const fn plain(item_id: u32) -> Self {
        Self {
            item_id,
            model_id: item_id,
            enchanted: false,
            potion_color: None,
            head_skin: None,
            dyed: None,
            trim: None,
            banner: None,
            equip: crate::armor::Equip::Default,
            stack: crate::item_components::StackRef::NONE,
            map_id: None,
        }
    }
}

// Blockt dieses Hand-Item (blocks_attacks)? Ueber den ganzen Stapel, sonst nach Namen.
#[must_use] 
pub fn held_blocks_attacks(h: &HeldItem) -> Option<protocol::item::BlocksAttacks> {
    match crate::item_components::resolve(h.stack) {
        Some(s) => crate::item_behavior::blocks_attacks(&s),
        None => (crate::item_registry::item_name(h.item_id) == Some("shield"))
            .then_some(protocol::item::BlocksAttacks { block_delay_seconds: 0.25, disable_cooldown_scale: 1.0 }),
    }
}

// BlocksAttacks.blockDelayTicks: die Verzoegerung auf ganze Ticks gerundet (Schild: 5).
#[must_use] 
pub fn block_delay_ticks(b: &protocol::item::BlocksAttacks) -> u32 {
    (b.block_delay_seconds * 20.0).round().max(0.0) as u32
}

// Gegenstandsrahmen (auch der leuchtende)?
#[must_use] 
pub fn is_item_frame(entity_type: i32) -> bool {
    matches!(entity_type_name(entity_type), Some("item_frame" | "glow_item_frame"))
}

// Wie man dieses Hand-Item benutzt (item_behavior::use_animation). Ohne den Stapel nach Namen.
#[must_use] 
pub fn held_use_animation(h: &HeldItem) -> i32 {
    use crate::item_behavior as ib;
    match crate::item_components::resolve(h.stack) {
        Some(s) => ib::use_animation(&s),
        None => match crate::item_registry::item_name(h.item_id) {
            Some("bow") => ib::USE_ANIM_BOW,
            Some("crossbow") => ib::USE_ANIM_CROSSBOW,
            Some("trident") => ib::USE_ANIM_TRIDENT,
            Some("spyglass") => ib::USE_ANIM_SPYGLASS,
            Some("goat_horn") => ib::USE_ANIM_TOOT_HORN,
            Some("brush") => ib::USE_ANIM_BRUSH,
            _ => ib::USE_ANIM_NONE,
        },
    }
}

// Eine geladene Armbrust (CrossbowItem.isCharged)?
#[must_use] 
pub fn held_charged_crossbow(h: &HeldItem) -> bool {
    crate::item_registry::item_name(h.item_id) == Some("crossbow") && crate::item_components::resolve(h.stack).is_some_and(|s| s.is_charged())
}

// Das zuletzt gehaltene Schwert: die Nebenhand zaehlt nach der Haupthand (wie in der Mod).
#[must_use] 
pub fn remember_sword(bisher: Option<HeldItem>, main: Option<HeldItem>, off: Option<HeldItem>) -> Option<HeldItem> {
    let schwert = |h: Option<HeldItem>| h.filter(|h| crate::item_registry::item_name(h.item_id).is_some_and(crate::more_anims::is_sword));
    schwert(off).or(schwert(main)).or(bisher)
}

// Dreht man beim Benutzen dieses Hand-Items den Koerper zum Blick (Essen, Trinken, Blocken)?
#[must_use] 
pub fn turns_body_to_head(h: &HeldItem) -> bool {
    held_eaten_or_drunk(h) || held_blocks_attacks(h).is_some()
}

// Isst oder trinkt man dieses Hand-Item? Ueber den ganzen Stapel, sonst nach Namen.
pub fn held_eaten_or_drunk(h: &HeldItem) -> bool {
    match crate::item_components::resolve(h.stack) {
        Some(s) => crate::item_behavior::eaten_or_drunk(&s),
        None => crate::item_registry::item_name(h.item_id).and_then(crate::item_registry::eat_duration_ticks).is_some(),
    }
}

// Sticht dieses Hand-Item (piercing_weapon)? Ueber den ganzen Stapel, sonst nach Namen.
pub fn held_piercing(h: &HeldItem) -> bool {
    match crate::item_components::resolve(h.stack) {
        Some(s) => crate::item_behavior::piercing(&s),
        None => crate::item_registry::item_name(h.item_id).is_some_and(crate::item_registry::is_spear),
    }
}

// Item-ID des Modells, das die item_model-Komponente nennt (s. HeldItem::model_id). Nennt sie
// eine Item-Definition, die es in Vanilla nicht gibt (Server-Resourcepacks: "uhc:storm_wall"
// steht in assets/uhc/items/storm_wall.json), bekommt sie eine eigene Nummer ab
// CUSTOM_MODEL_BASE (s. custom_model_id) -- frueher fiel so ein Modell auf das Item selbst
// zurueck, und eine Sturmwand-Markierung erschien als Trankflasche.
#[must_use] 
pub fn model_item_id(s: &protocol::item::ItemStack) -> u32 {
    match s.item_model.as_deref() {
        None => s.item_id,
        Some(m) => crate::item_registry::item_id_by_name(m.strip_prefix("minecraft:").unwrap_or(m)).unwrap_or_else(|| custom_model_id(m)),
    }
}

// Ab hier zaehlen Modell-Nummern als Item-Definitionen aus Resourcepacks, nicht als Items der
// Registry (die hat rund 1500).
pub const CUSTOM_MODEL_BASE: u32 = 1 << 20;

pub(crate) static CUSTOM_MODELS: std::sync::Mutex<Vec<String>> = std::sync::Mutex::new(Vec::new());

// Die Nummer einer Item-Definition aus einem Resourcepack ("ns:pfad"), beim ersten Auftauchen
// vergeben und dann fest.
pub fn custom_model_id(id: &str) -> u32 {
    let id = if id.contains(':') { id.to_string() } else { format!("minecraft:{id}") };
    let mut liste = CUSTOM_MODELS.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    let index = if let Some(i) = liste.iter().position(|m| *m == id) { i } else {
        liste.push(id);
        liste.len() - 1
    };
    CUSTOM_MODEL_BASE + index as u32
}

// Die ID ("ns:pfad") zu einer Nummer aus custom_model_id.
pub fn custom_model_name(model_id: u32) -> Option<String> {
    let index = model_id.checked_sub(CUSTOM_MODEL_BASE)? as usize;
    CUSTOM_MODELS.lock().unwrap_or_else(std::sync::PoisonError::into_inner).get(index).cloned()
}

// Unter welchem Schluessel der Skin eines Spielerkopfes steht: die UUID aus der
// profile-Komponente, ohne sie ein Hash der Textur-Property (viele Server-Koepfe tragen nur
// die Textur). None, wenn der Kopf gar keinen Skin nennt.
#[must_use] 
pub fn head_skin_key(profil: &protocol::item::HeadProfile) -> Option<[u8; 16]> {
    if let Some(uuid) = profil.uuid.filter(|u| *u != [0; 16]) {
        return Some(uuid);
    }
    // Ohne UUID: die Textur, sonst der Name (ein Kopf, den der Client erst ueber die
    // Mojang-Profile aufloesen muss, s. auth::resolve_profile_textures). Eigene Salze, damit
    // ein Name nie denselben Schluessel ergibt wie eine gleichlautende Textur.
    let (salze, inhalt) = match (&profil.textures, &profil.name) {
        (Some(t), _) => ([0x6b6f_7066_u64, 0x0073_6368_6164_656c], t.clone()),
        (None, Some(n)) if !n.is_empty() => ([0x6e61_6d65_u64, 0x006b_6f70_666e_616d], n.to_ascii_lowercase()),
        _ => return None,
    };
    use std::hash::{Hash, Hasher};
    let mut key = [0u8; 16];
    for (i, salz) in salze.into_iter().enumerate() {
        let mut h = std::collections::hash_map::DefaultHasher::new();
        salz.hash(&mut h);
        inhalt.hash(&mut h);
        key[i * 8..i * 8 + 8].copy_from_slice(&h.finish().to_be_bytes());
    }
    Some(key)
}
