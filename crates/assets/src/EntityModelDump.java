// Liest die Entity-Modelle aus dem Client-Jar der Installation des Nutzers und schreibt ihre
// Geometrie (Teilebaum, Kaesten, Drehpunkte, Texturgroesse) im Format von
// world::model_data. Gestartet von assets::entity_models mit dem Java der Installation:
//
//   java -cp <client.jar>:<bibliotheken> EntityModelDump.java <ausgabedatei>
//
// Alles per Reflection ueber die obfuskierten Namen der 1.21.11 (aus den Intermediary-
// Mappings): SharedConstants "w", Bootstrap "amv", EntityModels "hdd", ModelPartData "hdq",
// ModelCuboidData "hdj", ModelTransform "hdi". Bei einem Versionswechsel neu nachschlagen.
// Eigenes Paket, weil das Client-Jar signiert ist: eine Klasse im (unbenannten) Paket der
// Spielklassen wuerde vom Classloader abgelehnt.
package coralclient;

import java.lang.reflect.Field;
import java.lang.reflect.Method;
import java.lang.reflect.Modifier;
import java.nio.file.Files;
import java.nio.file.Path;
import java.util.List;
import java.util.Locale;
import java.util.Map;
import java.util.Set;

public class EntityModelDump {
    static final String[] FACES = {"DOWN", "UP", "NORTH", "SOUTH", "WEST", "EAST"};

    static Object get(Object o, String f) throws Exception {
        Field fl = o.getClass().getDeclaredField(f);
        fl.setAccessible(true);
        return fl.get(o);
    }

    static String num(float x) {
        String s = String.format(Locale.ROOT, "%.6f", x);
        if (s.contains(".")) {
            s = s.replaceAll("0+$", "").replaceAll("\\.$", "");
        }
        return s.equals("-0") || s.isEmpty() ? "0" : s;
    }

    static float[] vec3(Object v) throws Exception {
        Class<?> c = v.getClass();
        return new float[] {(float) c.getMethod("x").invoke(v), (float) c.getMethod("y").invoke(v), (float) c.getMethod("z").invoke(v)};
    }

    // Zwei Floats ohne bekannte Feldnamen (UV und UV-Skalierung): alle Instanzfelder der Reihe nach.
    static float[] vec2(Object v) throws Exception {
        float[] out = new float[2];
        int i = 0;
        for (Field f : v.getClass().getDeclaredFields()) {
            if (Modifier.isStatic(f.getModifiers()) || i >= 2) continue;
            f.setAccessible(true);
            out[i++] = ((Number) f.get(v)).floatValue();
        }
        return out;
    }

    static void part(String name, Object p, int depth, StringBuilder sb) throws Exception {
        Object t = get(p, "b");
        sb.append("P ").append(depth).append(' ').append(name);
        for (String f : new String[] {"b", "c", "d", "e", "f", "g", "h", "i", "j"}) {
            sb.append(' ').append(num((float) get(t, f)));
        }
        sb.append('\n');
        for (Object c : (List<?>) get(p, "a")) {
            Object dil = get(c, "d");
            float[] uv = vec2(get(c, "f"));
            float[] off = vec3(get(c, "b"));
            float[] size = vec3(get(c, "c"));
            float[] uvs = vec2(get(c, "g"));
            int mask = 0;
            for (Object f : (Set<?>) get(c, "h")) {
                String n = ((Enum<?>) f).name();
                for (int i = 0; i < 6; i++) if (FACES[i].equals(n)) mask |= 1 << i;
            }
            sb.append('C');
            float[] werte = {uv[0], uv[1], off[0], off[1], off[2], size[0], size[1], size[2],
                (float) get(dil, "b"), (float) get(dil, "c"), (float) get(dil, "d"), uvs[0], uvs[1]};
            for (float w : werte) sb.append(' ').append(num(w));
            sb.append(' ').append(((Boolean) get(c, "e")) ? 1 : 0).append(' ').append(mask).append('\n');
        }
        for (Map.Entry<?, ?> e : ((Map<?, ?>) get(p, "c")).entrySet()) {
            part((String) e.getKey(), e.getValue(), depth + 1, sb);
        }
    }

    public static void main(String[] args) throws Exception {
        for (String c : new String[] {"w", "amv"}) {
            Method b = Class.forName(c).getDeclaredMethod("a");
            b.setAccessible(true);
            b.invoke(null);
        }
        Method m = Class.forName("hdd").getDeclaredMethod("a");
        m.setAccessible(true);
        Map<?, ?> map = (Map<?, ?>) m.invoke(null);
        java.util.TreeMap<String, String> sortiert = new java.util.TreeMap<>();
        for (Map.Entry<?, ?> e : map.entrySet()) {
            String key = (get(e.getKey(), "a") + "#" + get(e.getKey(), "b")).replace("minecraft:", "");
            // Ruestungsschichten zeichnet der Client mit eigenen Modellen.
            if (key.matches(".*#(helmet|chestplate|leggings|boots|inner_armor|outer_armor)")) continue;
            Object tmd = e.getValue();
            Object dims = get(tmd, "b");
            StringBuilder sb = new StringBuilder();
            sb.append("L ").append(key).append(' ').append(get(dims, "a")).append(' ').append(get(dims, "b")).append('\n');
            try {
                part("root", get(get(tmd, "a"), "a"), 0, sb);
            } catch (Exception ex) {
                System.err.println(key + ": " + ex);
                continue;
            }
            sortiert.put(key, sb.toString());
        }
        StringBuilder out = new StringBuilder();
        for (String s : sortiert.values()) out.append(s);
        Files.writeString(Path.of(args[0]), out.toString());
    }
}
