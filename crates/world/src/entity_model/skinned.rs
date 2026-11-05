// Feste Netze fuers GPU-Skinning (PLAN-skinning.md, Phase 1).
//
// Heute baut emit() jedes Modell jeden Frame neu: jede Ecke mit der Matrix ihres Teils in die
// Welt, Licht je Seite aus der gedrehten Normale, UV ins Atlas-Rechteck. Davon haengt nur die
// Matrix von der Pose ab. Das feste Netz haelt den Rest einmal je Modell fest -- je Ecke ihre
// Lage im Raum ihres Teils, den Teil, die Lichtrichtung ihrer Seite und die Modell-UV --, und
// pro Frame braucht es nur noch eine Matrix je Teil (part_matrices).
//
// skinned_vertices rechnet das auf der CPU genauso aus, wie es der Vertex-Shader spaeter tut.
// Die Tests vergleichen das mit emit(): gleiche Rechenschritte, also Bit fuer Bit gleich.

use super::*;
use super::geometry::{cuboid_layout, part_world_transform, AxisShades, QUAD_BACK, QUAD_FRONT};

// Eine Ecke des festen Netzes. 32 Byte, fuer den GPU-Puffer ohne Luecken.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, bytemuck::Pod, bytemuck::Zeroable)]
pub struct SkinVertex {
    // Kastenecke in Bloecken (Modellpixel / 16), im Raum ihres Teils.
    pub position: [f32; 3],
    // Modell-UV geteilt durch die (mit tex_scale gestreckte) Texturgroesse: 0..1 ueber die
    // ganze Modelltextur. Das Atlas-Rechteck kommt erst beim Zeichnen dazu.
    pub uv: [f32; 2],
    // Index des Teils in Tiefensuche-Reihenfolge (wie emit_part die Teile besucht).
    pub part: u32,
    // Lichtrichtung der Seite als AxisShades-Index (0..5: +X, -X, +Y, -Y, +Z, -Z im Teilraum),
    // bei der Rueckseite zweiseitiger Netze schon umgedreht.
    pub shade: u32,
    pub _pad: u32,
}

pub struct SkinMesh {
    pub vertices: Vec<SkinVertex>,
    // So viele Teile hat das Modell (= Laenge von part_matrices).
    pub parts: usize,
    pub two_sided: bool,
}

// Das feste Netz eines Modells: alle Seiten aller Kaesten aller Teile, in derselben
// Reihenfolge wie emit() / emit_two_sided() sie schreiben.
#[must_use]
pub fn skin_mesh(model: &Model, two_sided: bool) -> SkinMesh {
    fn teil(part: &Part, tex_size: [f32; 2], two_sided: bool, index: &mut u32, out: &mut Vec<SkinVertex>) {
        let eigen = *index;
        *index += 1;
        for cuboid in &part.cuboids {
            let ts = [tex_size[0] * cuboid.tex_scale[0], tex_size[1] * cuboid.tex_scale[1]];
            let layout = cuboid_layout(cuboid);
            for (face_index, quad) in layout.faces.iter().enumerate() {
                if cuboid.faces & (1 << face_index) == 0 {
                    continue;
                }
                let ecke = |&(c, u, v): &(usize, f32, f32), shade: usize| {
                    let p = layout.corners[c];
                    SkinVertex { position: [p[0] / 16.0, p[1] / 16.0, p[2] / 16.0], uv: [u / ts[0], v / ts[1]], part: eigen, shade: shade as u32, _pad: 0 }
                };
                let vorn = layout.normals[face_index];
                out.extend(QUAD_FRONT.map(|i| ecke(&quad[i], vorn)));
                if two_sided {
                    out.extend(QUAD_BACK.map(|i| ecke(&quad[i], vorn ^ 1)));
                }
            }
        }
        for child in &part.children {
            teil(child, tex_size, two_sided, index, out);
        }
    }
    let mut vertices = Vec::new();
    let mut index = 0;
    for part in &model.parts {
        teil(part, model.tex_size, two_sided, &mut index, &mut vertices);
    }
    SkinMesh { vertices, parts: index as usize, two_sided }
}

// Die Matrix jedes Teils fuer diese Pose, in derselben Reihenfolge wie SkinVertex::part.
// None = Teil versteckt (PartPose::hidden), samt allen Kindern -- wie emit_part, das dort
// gar nicht erst hinabsteigt.
#[must_use]
pub fn part_matrices(model: &Model, pose: &Pose, base: Affine) -> Vec<Option<Affine>> {
    let mut out = Vec::new();
    part_matrices_into(model, pose, base, &mut out);
    out
}

// Wie part_matrices, in einen wiederverwendeten Puffer (wird vorher geleert): je Instanz und
// Frame sonst eine eigene Allokation.
pub fn part_matrices_into(model: &Model, pose: &Pose, base: Affine, out: &mut Vec<Option<Affine>>) {
    fn teil(part: &Part, pose: &Pose, parent: Option<Affine>, out: &mut Vec<Option<Affine>>) {
        let welt = parent.filter(|_| !pose.get(part.name).hidden).map(|p| part_world_transform(part, pose, p));
        out.push(welt);
        for child in &part.children {
            teil(child, pose, welt, out);
        }
    }
    out.clear();
    for part in &model.parts {
        teil(part, pose, Some(base), out);
    }
}

// Das Netz in dieser Pose als fertige Vertices, wie es der Vertex-Shader rechnet: Ecke mal
// Matrix ihres Teils, Licht aus der gedrehten Achse, UV ins Atlas-Rechteck. `color` ist schon
// mit der Helligkeit verrechnet (wie bei emit_into). Versteckte Teile fallen weg.
#[must_use]
pub fn skinned_vertices(mesh: &SkinMesh, matrices: &[Option<Affine>], tex: TexRect, color: [f32; 3]) -> Vec<BlockVertex> {
    let licht: Vec<Option<AxisShades>> = matrices.iter().map(|m| m.map(AxisShades::of)).collect();
    let [u0, v0, u1, v1] = tex;
    mesh.vertices
        .iter()
        .filter_map(|v| {
            let teil = v.part as usize;
            let m = matrices.get(teil).copied().flatten()?;
            let shade = licht[teil]?.0[v.shade as usize];
            Some(BlockVertex {
                position: m.apply(v.position),
                color: [color[0] * shade, color[1] * shade, color[2] * shade],
                uv: [u0 + v.uv[0] * (u1 - u0), v0 + v.uv[1] * (v1 - v0)],
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn namen(model: &Model) -> Vec<&'static str> {
        fn teil(p: &Part, out: &mut Vec<&'static str>) {
            out.push(p.name);
            for c in &p.children {
                teil(c, out);
            }
        }
        let mut out = Vec::new();
        for p in &model.parts {
            teil(p, &mut out);
        }
        out
    }

    // Jedes Teil verdreht und verschoben, jedes vierte versteckt (wenn `verstecken`).
    fn wilde_pose(model: &Model, verstecken: bool) -> Pose {
        let mut pose = Pose::new();
        for (i, name) in namen(model).into_iter().enumerate() {
            let f = i as f32;
            pose.set(
                name,
                PartPose {
                    rotation: [0.3 * f.sin(), -0.7 + 0.11 * f, 0.25 * (f * 1.7).cos()],
                    pivot_offset: [0.5 * f.cos(), -1.5 + 0.2 * f, 0.75],
                    rotation_matrix: None,
                    hidden: verstecken && i % 4 == 2,
                },
            );
        }
        pose
    }

    fn vergleiche(model: &Model, pose: &Pose, wo: &str) {
        let tex = [0.125, 0.25, 0.375, 0.75];
        let base = Affine::translation([0.4, -1.3, 2.2]).then(Affine::rotation_xyz_deg(12.0, 47.0, -8.0)).then(Affine::scale([0.9375, -0.9375, -0.9375]));
        let farbe = [0.8, 0.55, 0.3];
        for two_sided in [false, true] {
            let mut erwartet = vec![bytemuck::Zeroable::zeroed(); vertex_count(model, pose, two_sided)];
            emit_into(model, pose, base, tex, farbe, two_sided, &mut erwartet);
            let netz = skin_mesh(model, two_sided);
            let matrizen = part_matrices(model, pose, base);
            assert_eq!(matrizen.len(), netz.parts, "{wo}: Teilezahl");
            let bekommen = skinned_vertices(&netz, &matrizen, tex, farbe);
            assert_eq!(bekommen.len(), erwartet.len(), "{wo}, zweiseitig={two_sided}: Anzahl");
            for (i, (a, b)) in bekommen.iter().zip(&erwartet).enumerate() {
                assert!(bytemuck::bytes_of(a) == bytemuck::bytes_of(b), "{wo}, zweiseitig={two_sided}, Ecke {i}: {a:?} != {b:?}");
            }
        }
    }

    fn alle_posen(model: &Model, wo: &str) {
        vergleiche(model, &Pose::new(), &format!("{wo} ruhend"));
        vergleiche(model, &wilde_pose(model, false), &format!("{wo} verdreht"));
        vergleiche(model, &wilde_pose(model, true), &format!("{wo} verdreht+versteckt"));
    }

    #[test]
    fn spieler_wie_emit() {
        for slim in [false, true] {
            let model = player_model(slim);
            alle_posen(&model, &format!("Spieler slim={slim}"));
            let mut schleichen = biped_pose(0.4, -0.3, 1.2, 0.8, true, 7.0);
            vergleiche(&model, &schleichen, "Spieler schleicht");
            schleichen.set("head", PartPose { hidden: true, ..schleichen.get("head") });
            vergleiche(&model, &schleichen, "Spieler ohne Kopf");
        }
    }

    #[test]
    fn mobs_wie_emit() {
        for (name, model, _) in model_table() {
            alle_posen(&model, name);
        }
    }

    #[test]
    fn ruestung_umhang_elytra_wie_emit() {
        for slot in [crate::armor::ArmorSlot::Head, crate::armor::ArmorSlot::Chest, crate::armor::ArmorSlot::Legs, crate::armor::ArmorSlot::Feet] {
            alle_posen(&armor_model(slot), &format!("Ruestung {slot:?}"));
        }
        for small in [false, true] {
            for (i, m) in armor_stand_armor_models(small).iter().enumerate() {
                alle_posen(m, &format!("Staender-Ruestung small={small} {i}"));
            }
        }
        let umhang = cape_model();
        let koerper = biped_pose(0.0, 0.0, 0.6, 0.5, false, 3.0);
        vergleiche(&umhang, &cape_pose(&koerper, 0.4, 0.2, -0.1), "Umhang");
        alle_posen(&elytra_model(), "Elytra");
    }

    // Was die Tabellen-Modelle nicht alle haben: gestreckte Textur, ungleiche Dilation,
    // Spiegelung, fehlende Seiten, Skalierung und Kinder unter einem versteckten Teil.
    #[test]
    fn sonderfaelle_wie_emit() {
        let kasten = |uv: [f32; 2], o: [f32; 3], g: [f32; 3]| Cuboid::new(uv, o, g);
        let mut ungleich = kasten([4.0, 8.0], [-2.0, 0.0, -1.0], [4.0, 6.0, 2.0]);
        ungleich.dilation = [0.25, 0.5, 0.1];
        let mut teilseiten = kasten([0.0, 16.0], [-1.0, -1.0, -1.0], [2.0, 3.0, 2.0]);
        teilseiten.faces = 0b10_1001;
        let model = Model {
            texture: "",
            tex_size: [64.0, 32.0],
            parts: vec![
                Part::new("rumpf", [0.0, 12.0, 0.0], vec![ungleich, kasten([16.0, 0.0], [-3.0, -2.0, -1.5], [6.0, 4.0, 3.0]).with_tex_scale(0.5, 2.0)])
                    .with(Part::new("arm", [5.0, -2.0, 0.0], vec![kasten([40.0, 16.0], [-1.0, -2.0, -1.0], [3.0, 12.0, 4.0]).mirrored().dilated(0.25)]).scaled(0.75).with(
                        Part::new("hand", [0.0, 10.0, 0.0], vec![teilseiten]),
                    ))
                    .with(Part::new("kopf", [0.0, -4.0, 0.0], vec![kasten([0.0, 0.0], [-4.0, -8.0, -4.0], [8.0, 8.0, 8.0])]).rotated([0.2, -0.4, 0.1])),
            ],
        };
        alle_posen(&model, "Sondermodell");
        let mut pose = wilde_pose(&model, false);
        pose.set("arm", PartPose { hidden: true, ..pose.get("arm") });
        vergleiche(&model, &pose, "Sondermodell, Arm samt Hand versteckt");
    }
}
