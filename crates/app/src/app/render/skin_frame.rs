// GPU-Skinning auf der App-Seite (PLAN-skinning.md, Phase 3): statt die Ecken eines festen
// Modells in die CPU-Liste zu schreiben, legt build_entity_vertices eine Instanz an -- Netz,
// Atlas-Rechteck, Farbe, Teil-Matrizen dieser Pose. upload_skinning gibt das danach an den
// Renderer (render::skinning); neue Netze entstehen dabei einmal je Modell.
//
// Welche Modelle schon umgestellt sind und welche bewusst auf dem CPU-Weg bleiben, steht an
// skinning_active und an den Aufrufstellen in build_entity_vertices.

use super::*;
use render::skinning::{MeshId, SkinInstance, SkinMatrix};

// Schluessel eines Netzes: Adresse des Modells und Seitigkeit, dazu ein Fingerabdruck seiner
// Geometrie -- wird ein Modell neu gebaut und landet ein anderes an derselben Adresse, passt der
// Fingerabdruck nicht mehr und das Netz entsteht neu.
type MeshKey = (usize, bool);

#[derive(Clone, Copy)]
enum MeshRef {
    Known(MeshId),
    New(usize),
}

#[derive(Default)]
pub(crate) struct SkinFrame {
    // Je Instanz: Netz, Daten, Motion-Blur-Schluessel (s. entity_motion::key) und Teilezahl.
    instances: Vec<(MeshRef, SkinInstance, u64, u32)>,
    // Parallel dazu, wie jede Instanz gezeichnet wird (s. Shown).
    shown: Vec<Shown>,
    matrices: Vec<SkinMatrix>,
    new_meshes: Vec<(MeshKey, u64, Vec<world::entity_model::SkinVertex>)>,
    // Nur Puffer, die ueber die Frames ihre Kapazitaet behalten (keine Allokation je Instanz):
    // die Teil-Matrizen einer Instanz (push_skinned) und die drei Zeichenlisten (upload_skinning).
    teile: Vec<Option<world::Affine>>,
    instanzen: Vec<(MeshId, SkinInstance)>,
    umrisse: Vec<(MeshId, SkinInstance)>,
    glanz: Vec<(MeshId, SkinInstance)>,
}

impl SkinFrame {
    // Leeren fuer das naechste Frame, ohne die Kapazitaet abzugeben.
    fn clear(&mut self) {
        self.instances.clear();
        self.shown.clear();
        self.matrices.clear();
        self.new_meshes.clear();
        self.teile.clear();
        self.instanzen.clear();
        self.umrisse.clear();
        self.glanz.clear();
    }
}

#[derive(Clone, Copy)]
struct Shown {
    // Den Koerper selbst zeichnen (nein bei unsichtbar und leuchtend: nur die Silhouette).
    main: bool,
    // Leucht-Umrissfarbe, falls die Instanz auch ins Umriss-Bild soll.
    outline: Option<[f32; 4]>,
    // Haut (Koerper)? "Nur Skin" faerbt nur die, nicht Ruestung und Umhang.
    skin: bool,
    // Glint: (Massstab, Texturwahl) wie render::glint, None = nicht verzaubert.
    glint: Option<[f32; 2]>,
}

#[derive(Default)]
pub(crate) struct SkinCache {
    meshes: rustc_hash::FxHashMap<MeshKey, (u64, MeshId)>,
    // Motion Blur: die Matrizen jeder Instanz im vorigen Bild (relativ zum damaligen Auge), je
    // (Schluessel, wievielte Instanz dieses Schluessels) -- wie EntityMotion bei den Ecken. Der
    // Wert ist (Anfang, Anzahl) in prev_data, der Kopie der Matrizen des vorigen Bildes.
    prev: rustc_hash::FxHashMap<(u64, u32), (u32, u32)>,
    prev_data: Vec<SkinMatrix>,
    prev_eye: Option<DVec3>,
    // Puffer von prev_matrices, die ueber die Frames ihre Kapazitaet behalten.
    next: rustc_hash::FxHashMap<(u64, u32), (u32, u32)>,
    counter: rustc_hash::FxHashMap<u64, u32>,
    vorher: Vec<SkinMatrix>,
}

// Weiter als so viele Bloecke in einem Bild: ein Teleport (wie entity_motion::MAX_JUMP_BLOCKS).
const MAX_JUMP_BLOCKS: f32 = 4.0;

// Die Vormatrizen dieses Bildes: zu jeder aktuellen Matrix die des vorigen Bildes, relativ zum
// vorigen Auge. Ohne Partner (neu, andere Teilezahl, Sprung) die aktuelle, um die Augenbewegung
// verschoben -- dann verwischt die Instanz nur mit der Kamera. Rueckgabe samt "hat sich etwas
// selbst bewegt".
fn prev_matrices<'a>(cache: &'a mut SkinCache, instances: &[(MeshRef, SkinInstance, u64, u32)], matrices: &[SkinMatrix], eye: DVec3) -> (&'a [SkinMatrix], bool) {
    let SkinCache { prev, prev_data, prev_eye, next, counter, vorher, .. } = cache;
    let [sx, sy, sz] = prev_eye.map_or([0.0; 3], |alt| (eye - alt).as_vec3().to_array());
    let ruhend = |m: &SkinMatrix| {
        let mut r = *m;
        if r != SkinMatrix::HIDDEN {
            r.0[0][3] += sx;
            r.0[1][3] += sy;
            r.0[2][3] += sz;
        }
        r
    };
    vorher.clear();
    next.clear();
    counter.clear();
    let mut bewegt = false;
    let sprung2 = MAX_JUMP_BLOCKS * MAX_JUMP_BLOCKS;
    for (_, inst, key, teile) in instances {
        let n = counter.entry(*key).or_insert(0);
        let schluessel = (*key, *n);
        *n += 1;
        let von = inst.matrix as usize;
        let Some(jetzt) = matrices.get(von..von + *teile as usize) else { continue };
        // Die aktuellen Matrizen, um die Augenbewegung verschoben -- ohne eigenen Puffer, die
        // Verschiebung ist billig genug, um sie bei Bedarf neu zu rechnen.
        let ruhe = || jetzt.iter().map(ruhend);
        let alt = prev
            .get(&schluessel)
            .and_then(|&(a, l)| prev_data.get(a as usize..a as usize + l as usize))
            .filter(|a| a.len() == jetzt.len() && prev_eye.is_some())
            .filter(|a| match (a.first(), ruhe().next()) {
                (Some(t0), Some(t1)) => {
                    let (t0, t1) = (t0.0, t1.0);
                    let (dx, dy, dz) = (t0[0][3] - t1[0][3], t0[1][3] - t1[1][3], t0[2][3] - t1[2][3]);
                    dx * dx + dy * dy + dz * dz <= sprung2
                }
                _ => true,
            });
        match alt {
            Some(alt) => {
                bewegt |= alt.iter().zip(ruhe()).any(|(a, b)| a.0.iter().flatten().zip(b.0.iter().flatten()).any(|(x, y)| (x - y).abs() > 1e-5));
                vorher.extend_from_slice(alt);
            }
            None => vorher.extend(ruhe()),
        }
        // Verweist auf `matrices`, das unten als prev_data fuer das naechste Bild kopiert wird.
        next.insert(schluessel, (inst.matrix, *teile));
    }
    std::mem::swap(prev, next);
    prev_data.clear();
    prev_data.extend_from_slice(matrices);
    *prev_eye = Some(eye);
    (vorher, bewegt)
}

fn fingerprint(model: &world::Model) -> u64 {
    fn teil(p: &world::entity_model::Part, h: &mut u64) {
        *h = h.wrapping_mul(0x100_0000_01b3) ^ (p.cuboids.len() as u64) ^ (u64::from(p.pivot[0].to_bits()) << 20) ^ u64::from(p.pivot[1].to_bits());
        for c in &p.children {
            teil(c, h);
        }
    }
    let mut h = 0xcbf2_9ce4_8422_2325u64 ^ u64::from(model.tex_size[0].to_bits()) ^ (u64::from(model.tex_size[1].to_bits()) << 32);
    for p in &model.parts {
        teil(p, &mut h);
    }
    h
}

impl App {
    // Skinning in diesem Frame? Immer im PvP-Pfad -- der Shader-Pfad hat Licht je Ecke, das die
    // Instanzen nicht kennen.
    pub(crate) fn skinning_active(&self) -> bool {
        !crate::shader_path_on() && self.gpu.is_some()
    }

    // Ein festes Modell als Instanz statt als Ecken. Liefert den Index der Instanz (fuer die
    // Faerbung am Ende des Entities, s. finish_skin_entity).
    // `motion`: Schluessel fuer den Motion Blur (s. entity_motion::key), je Entity derselbe --
    // mehrere Instanzen eines Entities unterscheidet ihre Reihenfolge.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn push_skinned(&self, model: &world::Model, pose: &world::Pose, base: world::Affine, tex: [f32; 4], color: [f32; 3], two_sided: bool, motion: u64) -> usize {
        let key = (std::ptr::from_ref(model) as usize, two_sided);
        self.push_skin_instance(
            key,
            fingerprint(model),
            || world::entity_model::skin_mesh(model, two_sided).vertices,
            |teile| world::entity_model::part_matrices_into(model, pose, base, teile),
            tex,
            color,
            motion,
        )
    }

    // Ein starres Netz, das kein Modell ist (Item in der Hand, s. HeldItems::rigid_pre): ein
    // Teil, eine Matrix. `key` unterscheidet die Netze -- ein Hash, hier mit gesetztem oberstem
    // Bit, damit er keiner Modelladresse gleicht --, `abdruck` laesst es bei geaenderter
    // Geometrie neu entstehen.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn push_skinned_rigid(
        &self,
        key: u64,
        abdruck: u64,
        build: impl FnOnce() -> Vec<world::entity_model::SkinVertex>,
        base: world::Affine,
        tex: [f32; 4],
        color: [f32; 3],
        motion: u64,
    ) -> usize {
        let key = ((key as usize) | (1usize << (usize::BITS - 1)), false);
        self.push_skin_instance(
            key,
            abdruck,
            build,
            |teile| {
                teile.clear();
                teile.push(Some(base));
            },
            tex,
            color,
            motion,
        )
    }

    // Netz suchen oder (einmal) bauen, die Matrizen dieser Instanz anhaengen, die Instanz anlegen.
    #[allow(clippy::too_many_arguments)]
    fn push_skin_instance(
        &self,
        key: MeshKey,
        abdruck: u64,
        build: impl FnOnce() -> Vec<world::entity_model::SkinVertex>,
        matrizen: impl FnOnce(&mut Vec<Option<world::Affine>>),
        tex: [f32; 4],
        color: [f32; 3],
        motion: u64,
    ) -> usize {
        let mut frame = self.skin_frame.borrow_mut();
        let frame = &mut *frame;
        let mesh = match self.skin_cache.borrow().meshes.get(&key) {
            Some(&(f, id)) if f == abdruck => MeshRef::Known(id),
            _ => match frame.new_meshes.iter().position(|(k, f, _)| *k == key && *f == abdruck) {
                Some(i) => MeshRef::New(i),
                None => {
                    frame.new_meshes.push((key, abdruck, build()));
                    MeshRef::New(frame.new_meshes.len() - 1)
                }
            },
        };
        let matrix = frame.matrices.len() as u32;
        matrizen(&mut frame.teile);
        SkinMatrix::extend_from_parts(&mut frame.matrices, &frame.teile);
        frame.instances.push((mesh, SkinInstance { rect: tex, color, matrix, tint: [0.0; 4] }, motion, frame.teile.len() as u32));
        frame.shown.push(Shown { main: true, outline: None, skin: false, glint: None });
        frame.instances.len() - 1
    }

    // Wie push_skinned, fuer den Koerper selbst (Haut, s. finish_skin_entity).
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn push_skinned_body(&self, model: &world::Model, pose: &world::Pose, base: world::Affine, tex: [f32; 4], two_sided: bool, motion: u64) {
        let i = self.push_skinned(model, pose, base, tex, [ENTITY_BRIGHTNESS; 3], two_sided, motion);
        if let Some(s) = self.skin_frame.borrow_mut().shown.get_mut(i) {
            s.skin = true;
        }
    }

    // Unsichtbar, aber leuchtend: NUR die Silhouette, kein Koerper (wie in Vanilla).
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn push_skinned_outline_only(&self, model: &world::Model, pose: &world::Pose, base: world::Affine, tex: [f32; 4], two_sided: bool, motion: u64, color: [f32; 4]) {
        let i = self.push_skinned(model, pose, base, tex, [1.0; 3], two_sided, motion);
        if let Some(s) = self.skin_frame.borrow_mut().shown.get_mut(i) {
            *s = Shown { main: false, outline: Some(color), skin: true, glint: None };
        }
    }

    // Die Instanz `index` glaenzt (verzauberte Ruestung): Massstab und Textur wie
    // render::glint::glint_vertices.
    pub(crate) fn skin_glint(&self, index: usize, scale: f32, texture: f32) {
        if let Some(s) = self.skin_frame.borrow_mut().shown.get_mut(index) {
            s.glint = Some([scale, texture]);
        }
    }

    // So viele Instanzen gibt es bisher in diesem Frame (Anfang der Instanzen eines Entities).
    pub(crate) fn skin_instance_count(&self) -> usize {
        self.skin_frame.borrow().instances.len()
    }

    // Am Ende eines Entities fuer seine Instanzen ab `from`, wie bei den Ecken: der
    // Schadensblitz (mit "Nur Skin" nur auf der Haut), darunter der Schildfilter, wo kein Blitz
    // liegt (er gilt ausdruecklich fuer den ganzen Spieler), und bei Leuchtenden der Umriss.
    // Bei voll deckendem Schildfilter faellt der Glanz weg, wie bei den Ecken.
    pub(crate) fn finish_skin_entity(&self, from: usize, flash: [f32; 4], shield: Option<[f32; 4]>, glow: Option<[f32; 4]>) {
        const KEINE: [f32; 4] = [0.0; 4];
        let nur_haut = self.settings.damage_tint_skin_only;
        let mut frame = self.skin_frame.borrow_mut();
        let SkinFrame { instances, shown, .. } = &mut *frame;
        for ((_, i, _, _), s) in instances.iter_mut().zip(shown.iter_mut()).skip(from) {
            let mut t = if s.skin || !nur_haut { flash } else { KEINE };
            if t == KEINE {
                t = shield.unwrap_or(KEINE);
            }
            i.tint = t;
            if s.main && s.outline.is_none() {
                s.outline = glow;
            }
            if shield.is_some_and(|f| f[3] >= 1.0) {
                s.glint = None;
            }
        }
    }

    // Nach build_entity_vertices: neue Netze hochladen und die Instanzen an den Renderer.
    // Das Frame wird danach geleert zurueckgelegt, damit seine Listen ihre Kapazitaet behalten.
    pub(crate) fn upload_skinning(&mut self) {
        let mut frame = std::mem::take(&mut *self.skin_frame.borrow_mut());
        if let Some(gpu) = self.gpu.as_mut() {
            let mut neu = Vec::with_capacity(frame.new_meshes.len());
            {
                let mut cache = self.skin_cache.borrow_mut();
                for (key, abdruck, ecken) in &frame.new_meshes {
                    let id = gpu.block_renderer.skinning.add_mesh(&gpu.device, &gpu.queue, ecken);
                    cache.meshes.insert(*key, (*abdruck, id));
                    neu.push(id);
                }
            }
            let id = |m: MeshRef| match m {
                MeshRef::Known(id) => id,
                MeshRef::New(n) => neu[n],
            };
            let SkinFrame { instances, shown, matrices, instanzen, umrisse, glanz, .. } = &mut frame;
            let paare = || instances.iter().zip(shown.iter());
            instanzen.extend(paare().filter(|(_, s)| s.main).map(|((m, i, _, _), _)| (id(*m), *i)));
            umrisse.extend(paare().filter_map(|((m, i, _, _), s)| s.outline.map(|farbe| (id(*m), SkinInstance { tint: farbe, ..*i }))));
            glanz.extend(
                paare()
                    .filter(|(_, s)| s.main)
                    .filter_map(|((m, i, _, _), s)| s.glint.map(|[scale, tex]| (id(*m), SkinInstance { tint: [scale, tex, 0.0, 0.0], ..*i }))),
            );
            gpu.block_renderer.skinning.set_instances(&gpu.device, &gpu.queue, instanzen, matrices);
            gpu.block_renderer.skinning.set_outline_instances(&gpu.device, &gpu.queue, umrisse);
            gpu.block_renderer.skinning.set_glint_instances(&gpu.device, &gpu.queue, glanz);
            // Motion Blur bewegter Entities: wie die Ecken (s. frame.rs, entity_motion).
            let mut cache = self.skin_cache.borrow_mut();
            if self.settings.motion_blur_entities_percent > 0 {
                let (vorher, bewegt) = prev_matrices(&mut cache, instances, matrices, self.camera.position);
                gpu.block_renderer.skinning.set_prev_matrices(&gpu.queue, vorher, bewegt);
            } else {
                cache.prev.clear();
                cache.prev_data.clear();
                cache.prev_eye = None;
                gpu.block_renderer.skinning.set_prev_matrices(&gpu.queue, &[], false);
            }
        }
        frame.clear();
        *self.skin_frame.borrow_mut() = frame;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bei(x: f32) -> SkinMatrix {
        SkinMatrix([[1.0, 0.0, 0.0, x], [0.0, 1.0, 0.0, 0.0], [0.0, 0.0, 1.0, 0.0]])
    }

    fn instanz(key: u64, matrix: u32) -> (MeshRef, SkinInstance, u64, u32) {
        (MeshRef::New(0), SkinInstance { rect: [0.0; 4], color: [1.0; 3], matrix, tint: [0.0; 4] }, key, 1)
    }

    #[test]
    fn bewegte_instanz_bekommt_ihre_alten_matrizen() {
        let mut c = SkinCache::default();
        let eye = DVec3::new(5.0, 70.0, 5.0);
        prev_matrices(&mut c, &[instanz(7, 0)], &[bei(1.0)], eye);
        let (vorher, bewegt) = prev_matrices(&mut c, &[instanz(7, 0)], &[bei(1.5)], eye);
        assert!(bewegt);
        assert_eq!(vorher, vec![bei(1.0)]);
    }

    // Neu, oder nur das Auge bewegt: die aktuelle Matrix, um die Augenbewegung verschoben.
    #[test]
    fn ohne_partner_ruhend() {
        let mut c = SkinCache::default();
        prev_matrices(&mut c, &[instanz(7, 0)], &[bei(1.0)], DVec3::ZERO);
        let (vorher, bewegt) = prev_matrices(&mut c, &[instanz(7, 0), instanz(8, 1)], &[bei(0.5), bei(3.0)], DVec3::new(0.5, 0.0, 0.0));
        assert!(!bewegt, "Entity 7 steht in der Welt still, nur das Auge ging 0.5 weiter");
        assert_eq!(vorher, vec![bei(1.0), bei(3.5)]);
    }

    #[test]
    fn teleport_verwischt_nicht() {
        let mut c = SkinCache::default();
        prev_matrices(&mut c, &[instanz(1, 0)], &[bei(0.0)], DVec3::ZERO);
        let (vorher, bewegt) = prev_matrices(&mut c, &[instanz(1, 0)], &[bei(20.0)], DVec3::ZERO);
        assert!(!bewegt);
        assert_eq!(vorher, vec![bei(20.0)]);
    }

    // Zwei Instanzen desselben Entities (Koerper, spaeter Ruestung) behalten je ihre eigenen.
    #[test]
    fn mehrere_instanzen_eines_entities() {
        let mut c = SkinCache::default();
        prev_matrices(&mut c, &[instanz(3, 0), instanz(3, 1)], &[bei(1.0), bei(2.0)], DVec3::ZERO);
        let (vorher, _) = prev_matrices(&mut c, &[instanz(3, 0), instanz(3, 1)], &[bei(1.1), bei(2.1)], DVec3::ZERO);
        assert_eq!(vorher, vec![bei(1.0), bei(2.0)]);
    }

    // Die Puffer werden je Bild getauscht und wiederverwendet: im dritten Bild gelten die
    // Matrizen des zweiten, auch wenn die Instanzen in anderer Reihenfolge kommen.
    #[test]
    fn drittes_bild_nimmt_das_zweite() {
        let mut c = SkinCache::default();
        prev_matrices(&mut c, &[instanz(1, 0), instanz(2, 1)], &[bei(0.0), bei(1.0)], DVec3::ZERO);
        prev_matrices(&mut c, &[instanz(1, 0), instanz(2, 1)], &[bei(0.2), bei(1.2)], DVec3::ZERO);
        let (vorher, bewegt) = prev_matrices(&mut c, &[instanz(2, 0), instanz(1, 1)], &[bei(1.4), bei(0.4)], DVec3::ZERO);
        assert!(bewegt);
        assert_eq!(vorher, vec![bei(1.2), bei(0.2)]);
    }
}
