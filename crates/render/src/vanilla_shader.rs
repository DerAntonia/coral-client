// Vanillas Core-Shader (assets/<ns>/shaders/core/*.vsh/.fsh) aus Resourcepacks ausfuehren.
//
// Resourcepacks ersetzen in Vanilla nicht nur Texturen, sondern auch die GLSL-Programme, mit
// denen gezeichnet wird -- und Server bauen darauf ganze Effekte (eine Sturmwand, die aus einem
// einzigen Item-Quad einen Zylinder rechnet; eingefaerbte Weltgrenzen; Text, der sich bewegt).
// Dieser Client zeichnet mit wgpu, nicht mit OpenGL. Damit dieselben Programme laufen, werden
// sie hier so umgeschrieben, dass naga sie als Vulkan-GLSL liest, und mit genau den Eingaben
// versorgt, die Vanilla ihnen gibt (Uniform-Bloecke, Sampler, Vertex-Attribute).
//
// Was die Umschreibung macht:
//   * `#moj_import <ns:pfad>` / `"pfad"` einsetzen (jede Datei einmal, wie Vanillas
//     GlslPreprocessor), `#version` auf 450 heben.
//   * Feste Bindungen: Vanillas Uniform-Bloecke nach Namen (s. UNIFORM_BLOCKS), jeder
//     `uniform sampler2D X` als Textur + Sampler (naga kennt keine kombinierten Sampler als
//     Variablen) mit `#define X sampler2D(X_t, X_s)`.
//   * Orte: Vertex-Eingaben nach Vanillas Attributnamen (Position, Color, UV0 ...), die
//     Varyings zwischen Vertex- und Fragmentstufe nach Namen, so dass beide zusammenpassen.
//   * OpenGL-Konventionen: Clip-z von [-w, w] auf [0, w], gl_FragCoord.y von unten statt von
//     oben, gl_VertexID heisst gl_VertexIndex. `inverse()` kommt als eigene Funktion mit (nicht
//     jedes wgpu-Backend kann es).
//   * Ausgabe: Vanilla zeichnet in einen Gamma-Framebuffer. Ist das Ziel hier sRGB, wird die
//     Farbe vor dem Schreiben linearisiert (s. McInternal).
//
// Was nicht geht, meldet compile() als Fehler mit Grund; der Aufrufer zeichnet dann wie bisher
// und schreibt den Grund ins Log.

use std::collections::{HashMap, HashSet};

// Uniform-Bloecke, die Vanilla 1.21.11 an Core-Shader bindet, mit fester Bindung. McInternal
// gehoert diesem Client (Bildgroesse fuer gl_FragCoord, sRGB-Ziel).
pub const UNIFORM_BLOCKS: [(&str, u32); 6] =
    [("DynamicTransforms", 0), ("Projection", 1), ("Globals", 2), ("Fog", 3), ("Lighting", 4), ("McInternal", 5)];

// Sampler0..2: Textur auf 6/8/10, Sampler auf 7/9/11.
pub const SAMPLERS: [&str; 3] = ["Sampler0", "Sampler1", "Sampler2"];

pub fn sampler_bindings(index: usize) -> (u32, u32) {
    (6 + 2 * index as u32, 7 + 2 * index as u32)
}

// Vanillas Vertex-Attribute (VertexFormatElement) in der Reihenfolge, in der sie hier im
// Vertex liegen -- ihr Index ist zugleich der Shader-Ort.
pub const ATTRIBUTES: [&str; 6] = ["Position", "Color", "UV0", "UV1", "UV2", "Normal"];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Stage {
    Vertex,
    Fragment,
}

// Eine Variable, die die Umschreibung gefunden hat: Name und GLSL-Typ.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Var {
    pub name: String,
    pub ty: String,
}

#[derive(Debug)]
pub struct Processed {
    pub glsl: String,
    pub inputs: Vec<Var>,
    pub outputs: Vec<Var>,
}

// Ein uebersetztes Programm: beide Stufen als naga-Modul, plus welche Attribute die
// Vertexstufe liest (fuer das Vertex-Layout).
pub struct Program {
    pub name: String,
    pub vertex: naga::Module,
    pub fragment: naga::Module,
    pub attributes: Vec<Var>,
}

const PREAMBLE: &str = r#"#version 450
layout(std140, set = 0, binding = 5) uniform McInternal {
    vec4 McViewport;
};

mat2 mc_inverse(mat2 m) {
    float d = m[0][0] * m[1][1] - m[0][1] * m[1][0];
    return mat2(m[1][1], -m[0][1], -m[1][0], m[0][0]) / d;
}

mat3 mc_inverse(mat3 m) {
    vec3 a = m[0];
    vec3 b = m[1];
    vec3 c = m[2];
    vec3 r0 = cross(b, c);
    vec3 r1 = cross(c, a);
    vec3 r2 = cross(a, b);
    float d = dot(r2, c);
    return transpose(mat3(r0, r1, r2)) / d;
}

mat4 mc_inverse(mat4 m) {
    float a00 = m[0][0], a01 = m[0][1], a02 = m[0][2], a03 = m[0][3];
    float a10 = m[1][0], a11 = m[1][1], a12 = m[1][2], a13 = m[1][3];
    float a20 = m[2][0], a21 = m[2][1], a22 = m[2][2], a23 = m[2][3];
    float a30 = m[3][0], a31 = m[3][1], a32 = m[3][2], a33 = m[3][3];
    float b00 = a00 * a11 - a01 * a10;
    float b01 = a00 * a12 - a02 * a10;
    float b02 = a00 * a13 - a03 * a10;
    float b03 = a01 * a12 - a02 * a11;
    float b04 = a01 * a13 - a03 * a11;
    float b05 = a02 * a13 - a03 * a12;
    float b06 = a20 * a31 - a21 * a30;
    float b07 = a20 * a32 - a22 * a30;
    float b08 = a20 * a33 - a23 * a30;
    float b09 = a21 * a32 - a22 * a31;
    float b10 = a21 * a33 - a23 * a31;
    float b11 = a22 * a33 - a23 * a32;
    float det = b00 * b11 - b01 * b10 + b02 * b09 + b03 * b08 - b04 * b07 + b05 * b06;
    return mat4(
        a11 * b11 - a12 * b10 + a13 * b09,
        a02 * b10 - a01 * b11 - a03 * b09,
        a31 * b05 - a32 * b04 + a33 * b03,
        a22 * b04 - a21 * b05 - a23 * b03,
        a12 * b08 - a10 * b11 - a13 * b07,
        a00 * b11 - a02 * b08 + a03 * b07,
        a32 * b02 - a30 * b05 - a33 * b01,
        a20 * b05 - a22 * b02 + a23 * b01,
        a10 * b10 - a11 * b08 + a13 * b06,
        a01 * b08 - a00 * b10 - a03 * b06,
        a30 * b04 - a31 * b02 + a33 * b00,
        a21 * b02 - a20 * b04 - a23 * b00,
        a11 * b07 - a10 * b09 - a12 * b06,
        a00 * b09 - a01 * b07 + a02 * b06,
        a31 * b01 - a30 * b03 - a32 * b00,
        a20 * b03 - a21 * b01 + a22 * b00) / det;
}

vec3 mc_srgb_to_linear(vec3 c) {
    vec3 lo = c / 12.92;
    vec3 hi = pow(max((c + 0.055) / 1.055, vec3(0.0)), vec3(2.4));
    return mix(hi, lo, lessThanEqual(c, vec3(0.04045)));
}

#define inverse mc_inverse
"#;

// Pfad einer Include-Datei: `<ns:pfad>` liegt unter <ns>/shaders/include/, `"pfad"` neben der
// einschliessenden Datei (Vanillas GlslPreprocessor / ShaderLoader).
fn include_path(spec: &str, current_dir: &str) -> Option<String> {
    let spec = spec.trim();
    if let Some(inner) = spec.strip_prefix('<').and_then(|s| s.strip_suffix('>')) {
        let (ns, pfad) = inner.split_once(':').unwrap_or(("minecraft", inner));
        return Some(format!("{ns}/shaders/include/{pfad}"));
    }
    if let Some(inner) = spec.strip_prefix('"').and_then(|s| s.strip_suffix('"')) {
        return Some(format!("{current_dir}{inner}"));
    }
    None
}

fn dir_of(path: &str) -> &str {
    match path.rfind('/') {
        Some(i) => &path[..=i],
        None => "",
    }
}

fn expand(
    source: &str,
    path: &str,
    load: &mut dyn FnMut(&str) -> Option<String>,
    seen: &mut HashSet<String>,
    out: &mut String,
    depth: u32,
) -> Result<(), String> {
    if depth > 32 {
        return Err(format!("#moj_import zu tief verschachtelt ({path})"));
    }
    for line in source.lines() {
        let t = line.trim_start();
        if t.starts_with("#version") {
            continue;
        }
        if let Some(rest) = t.strip_prefix("#moj_import") {
            let ziel = include_path(rest, dir_of(path)).ok_or_else(|| format!("unlesbares #moj_import: {rest}"))?;
            if seen.insert(ziel.clone()) {
                let text = load(&ziel).ok_or_else(|| format!("Include fehlt: {ziel}"))?;
                expand(&text, &ziel, load, seen, out, depth + 1)?;
            }
            continue;
        }
        out.push_str(line);
        out.push('\n');
    }
    Ok(())
}

// Eine Deklaration `[flat] in|out typ name;` auf oberster Ebene.
fn parse_inout(line: &str) -> Option<(String, &'static str, String, String)> {
    let mut rest = line.trim();
    let mut quali = String::new();
    loop {
        let wort = rest.split_whitespace().next()?;
        if matches!(wort, "flat" | "smooth" | "noperspective" | "centroid" | "highp" | "mediump" | "lowp") {
            quali.push_str(wort);
            quali.push(' ');
            rest = rest[wort.len()..].trim_start();
        } else {
            break;
        }
    }
    let (dir, rest) = if let Some(r) = rest.strip_prefix("in ") {
        ("in", r)
    } else if let Some(r) = rest.strip_prefix("out ") {
        ("out", r)
    } else {
        return None;
    };
    let rest = rest.trim();
    let rest = rest.strip_suffix(';')?.trim();
    let mut teile = rest.split_whitespace();
    let ty = teile.next()?.to_string();
    let name = teile.next()?.to_string();
    if teile.next().is_some() || !name.chars().all(|c| c.is_alphanumeric() || c == '_' || c == '[' || c == ']') {
        return None;
    }
    Some((quali, dir, ty, name))
}

// Die Umschreibung einer Stufe. `varyings` ordnet Namen von Vertex-Ausgaben Orte zu; die
// Fragmentstufe muss mit derselben Tabelle laufen, damit beide Seiten zusammenpassen.
pub fn preprocess(
    stage: Stage,
    source: &str,
    path: &str,
    load: &mut dyn FnMut(&str) -> Option<String>,
    varyings: &mut HashMap<String, u32>,
) -> Result<Processed, String> {
    let mut body = String::new();
    let mut seen = HashSet::new();
    expand(source, path, load, &mut seen, &mut body, 0)?;

    let mut out = String::from(PREAMBLE);
    if stage == Stage::Vertex {
        out.push_str("#define gl_VertexID int(gl_VertexIndex)\n#define gl_InstanceID int(gl_InstanceIndex)\n");
    } else {
        out.push_str("vec4 mc_FragCoord;\n#define gl_FragCoord mc_FragCoord\n");
    }

    let mut inputs = Vec::new();
    let mut outputs = Vec::new();
    let mut tiefe = 0i32;
    let mut main_gefunden = false;
    for line in body.lines() {
        let t = line.trim();
        if tiefe == 0 {
            if let Some((quali, dir, ty, name)) = parse_inout(t) {
                let ort = match (stage, dir) {
                    (Stage::Vertex, "in") => {
                        ATTRIBUTES.iter().position(|a| *a == name).ok_or_else(|| format!("unbekanntes Vertex-Attribut {name}"))? as u32
                    }
                    (Stage::Vertex, _) | (Stage::Fragment, "in") => {
                        let n = varyings.len() as u32;
                        *varyings.entry(name.clone()).or_insert(n)
                    }
                    (Stage::Fragment, _) => outputs.len() as u32,
                };
                out.push_str(&format!("layout(location = {ort}) {quali}{dir} {ty} {name};\n"));
                let var = Var { name, ty };
                if dir == "in" {
                    inputs.push(var);
                } else {
                    outputs.push(var);
                }
                continue;
            }
            if let Some(rest) = t.strip_prefix("layout(std140) uniform").or_else(|| t.strip_prefix("layout (std140) uniform")) {
                let name: String = rest.trim().chars().take_while(|c| c.is_alphanumeric() || *c == '_').collect();
                let (_, b) = UNIFORM_BLOCKS.iter().find(|(n, _)| *n == name).ok_or_else(|| format!("unbekannter Uniform-Block {name}"))?;
                let nach = &rest.trim()[name.len()..];
                out.push_str(&format!("layout(std140, set = 0, binding = {b}) uniform {name}{nach}\n"));
                tiefe += nach.matches('{').count() as i32 - nach.matches('}').count() as i32;
                continue;
            }
            if let Some(rest) = t.strip_prefix("uniform ") {
                let rest = rest.trim().trim_end_matches(';').trim();
                let mut teile = rest.split_whitespace();
                let ty = teile.next().unwrap_or("");
                let name = teile.next().unwrap_or("").to_string();
                if ty == "sampler2D" {
                    let i = SAMPLERS.iter().position(|s| *s == name).ok_or_else(|| format!("unbekannter Sampler {name}"))?;
                    let (tb, sb) = sampler_bindings(i);
                    out.push_str(&format!(
                        "layout(set = 0, binding = {tb}) uniform texture2D {name}_t;\nlayout(set = 0, binding = {sb}) uniform sampler {name}_s;\n#define {name} sampler2D({name}_t, {name}_s)\n"
                    ));
                    continue;
                }
                return Err(format!("loses Uniform `{t}` (gibt es seit 1.21.5 nicht mehr)"));
            }
            if t.starts_with("void main(") || t.starts_with("void main (") {
                main_gefunden = true;
                out.push_str(&line.replacen("main", "mc_vanilla_main", 1));
                out.push('\n');
                tiefe += t.matches('{').count() as i32 - t.matches('}').count() as i32;
                continue;
            }
        }
        // Klammertiefe nur grob ueber geschweifte Klammern -- reicht, um Deklarationen auf
        // oberster Ebene von solchen in Funktionen zu trennen.
        let ohne_kommentar = t.split("//").next().unwrap_or("");
        tiefe += ohne_kommentar.matches('{').count() as i32 - ohne_kommentar.matches('}').count() as i32;
        out.push_str(line);
        out.push('\n');
    }
    if !main_gefunden {
        return Err("keine main()".into());
    }

    match stage {
        Stage::Vertex => {
            out.push_str("\nvoid main() {\n    mc_vanilla_main();\n    gl_Position.z = (gl_Position.z + gl_Position.w) * 0.5;\n}\n");
        }
        Stage::Fragment => {
            out.push_str("\n#undef gl_FragCoord\nvoid main() {\n    mc_FragCoord = vec4(gl_FragCoord.x, McViewport.y - gl_FragCoord.y, gl_FragCoord.z, gl_FragCoord.w);\n    mc_vanilla_main();\n");
            for o in &outputs {
                if o.ty == "vec4" {
                    out.push_str(&format!("    if (McViewport.z > 0.5) {{ {0}.rgb = mc_srgb_to_linear({0}.rgb); }}\n", o.name));
                }
            }
            out.push_str("}\n");
        }
    }
    Ok(Processed { glsl: out, inputs, outputs })
}

fn parse(stage: Stage, glsl: &str) -> Result<naga::Module, String> {
    let naga_stage = match stage {
        Stage::Vertex => naga::ShaderStage::Vertex,
        Stage::Fragment => naga::ShaderStage::Fragment,
    };
    let mut fe = naga::front::glsl::Frontend::default();
    let module = fe.parse(&naga::front::glsl::Options::from(naga_stage), glsl).map_err(|e| {
        e.errors
            .iter()
            .map(|err| {
                let (zeile, _) = line_of(glsl, err.meta.to_range().map_or(0, |r| r.start));
                format!("Zeile {zeile}: {:?}", err.kind)
            })
            .collect::<Vec<_>>()
            .join("; ")
    })?;
    naga::valid::Validator::new(naga::valid::ValidationFlags::all(), naga::valid::Capabilities::all())
        .validate(&module)
        .map_err(|e| format!("{:?}", e.into_inner()))?;
    Ok(module)
}

fn line_of(text: &str, offset: usize) -> (usize, usize) {
    let vor = &text[..offset.min(text.len())];
    (vor.matches('\n').count() + 1, vor.rfind('\n').map_or(offset, |i| offset - i - 1))
}

// Ein ganzes Programm: `name` ist der Pfad ohne Endung, z.B.
// "minecraft/shaders/core/rendertype_item_entity_translucent_cull".
pub fn compile(name: &str, load: &mut dyn FnMut(&str) -> Option<String>) -> Result<Program, String> {
    let vsh_pfad = format!("{name}.vsh");
    let fsh_pfad = format!("{name}.fsh");
    let vsh = load(&vsh_pfad).ok_or_else(|| format!("{vsh_pfad} fehlt"))?;
    let fsh = load(&fsh_pfad).ok_or_else(|| format!("{fsh_pfad} fehlt"))?;
    let mut varyings = HashMap::new();
    let v = preprocess(Stage::Vertex, &vsh, &vsh_pfad, load, &mut varyings).map_err(|e| format!("{vsh_pfad}: {e}"))?;
    let f = preprocess(Stage::Fragment, &fsh, &fsh_pfad, load, &mut varyings).map_err(|e| format!("{fsh_pfad}: {e}"))?;
    let vertex = parse(Stage::Vertex, &v.glsl).map_err(|e| format!("{vsh_pfad}: {e}"))?;
    let fragment = parse(Stage::Fragment, &f.glsl).map_err(|e| format!("{fsh_pfad}: {e}"))?;
    Ok(Program { name: name.to_string(), vertex, fragment, attributes: v.inputs })
}

#[cfg(test)]
mod tests {
    use super::*;

    // Die echten Shader aus dem Jar der Installation; ohne Installation wird nichts geprueft.
    fn installation(packs: Vec<std::path::PathBuf>) -> Option<assets::MinecraftInstallation> {
        assets::MinecraftInstallation::locate("1.21.11", packs).ok()
    }

    fn lader(mc: &mut assets::MinecraftInstallation) -> impl FnMut(&str) -> Option<String> + '_ {
        move |pfad| mc.load_bytes(pfad).ok().and_then(|b| String::from_utf8(b).ok())
    }

    #[test]
    fn vanillas_eigene_shader_lassen_sich_uebersetzen() {
        let Some(mut mc) = installation(Vec::new()) else { return };
        let mut load = lader(&mut mc);
        for name in ["rendertype_item_entity_translucent_cull", "rendertype_world_border", "entity", "rendertype_text", "particle"] {
            let p = compile(&format!("minecraft/shaders/core/{name}"), &mut load);
            assert!(p.is_ok(), "{name}: {}", p.err().unwrap_or_default());
        }
    }

    #[test]
    fn die_uhc_sturmwand_laesst_sich_uebersetzen() {
        let pack = std::path::PathBuf::from(std::env::var("HOME").unwrap_or_default()).join("Dokumente/uhc-battle-royale/docs/resourcepack");
        if !pack.exists() {
            return;
        }
        let Some(mut mc) = installation(vec![pack]) else { return };
        let mut load = lader(&mut mc);
        let p = compile("minecraft/shaders/core/rendertype_item_entity_translucent_cull", &mut load);
        assert!(p.is_ok(), "{}", p.err().unwrap_or_default());
        let p = p.unwrap();
        let namen: Vec<&str> = p.attributes.iter().map(|a| a.name.as_str()).collect();
        assert_eq!(namen, ["Position", "Color", "UV0", "UV1", "UV2", "Normal"]);
    }

    #[test]
    fn includes_nach_namespace_und_relativ() {
        assert_eq!(include_path("<minecraft:fog.glsl>", "minecraft/shaders/core/"), Some("minecraft/shaders/include/fog.glsl".into()));
        assert_eq!(include_path("<uhc:wand/zylinder.glsl>", ""), Some("uhc/shaders/include/wand/zylinder.glsl".into()));
        assert_eq!(include_path("\"teil.glsl\"", "minecraft/shaders/core/"), Some("minecraft/shaders/core/teil.glsl".into()));
    }

    #[test]
    fn varyings_bekommen_auf_beiden_seiten_denselben_ort() {
        let vsh = "#version 330\nin vec3 Position;\nout vec2 a;\nflat out int b;\nvoid main() { a = vec2(0.0); b = 1; gl_Position = vec4(Position, 1.0); }\n";
        let fsh = "#version 330\nflat in int b;\nin vec2 a;\nout vec4 fragColor;\nvoid main() { fragColor = vec4(a, float(b), 1.0); }\n";
        let mut load = |p: &str| match p {
            "x.vsh" => Some(vsh.to_string()),
            "x.fsh" => Some(fsh.to_string()),
            _ => None,
        };
        let mut varyings = HashMap::new();
        let v = preprocess(Stage::Vertex, vsh, "x.vsh", &mut load, &mut varyings).unwrap();
        let f = preprocess(Stage::Fragment, fsh, "x.fsh", &mut load, &mut varyings).unwrap();
        assert!(v.glsl.contains("layout(location = 1) flat out int b;"));
        assert!(f.glsl.contains("layout(location = 1) flat in int b;"));
        assert!(f.glsl.contains("layout(location = 0) in vec2 a;"));
        assert!(compile("x", &mut load).is_ok());
    }
}
