// Liest die Standard-Components jedes Items aus dem Client-Jar der Installation. Ein Server
// schickt von einem Stapel nur die ABWEICHUNGEN vom Standard des Items; was ein Item von sich
// aus ist (Elytra gleitet, Totem rettet, Schild blockt, Schwert hat Reichweite ...), steht nur
// im Jar. Gestartet von assets::storage mit dem Java der Installation:
//
//   java -cp <client.jar>:<bibliotheken> ItemComponentsDump.java <ausgabedatei>
//
// Ausgabe, je Zeile ein Eintrag:
//   I <item>                                ein Item (in Registry-Reihenfolge)
//   C <component> <id>\t<hex>\t<json>       eine Standard-Component dieses Items
// <id> ist die Netz-ID des Component-Typs (Position in seiner Registry), <hex> die Netzform
// (ohne Typ-ID und Laenge) -- also genau das, was ein Server fuer einen gesetzten Wert schickt,
// lesbar mit demselben Parser (protocol::item). "-", wenn die Netzform eine Registry braucht,
// die erst der Server festlegt; dann zaehlt das JSON.
//
// Alles per Reflection: zuerst die Mojang-Namen (ab 26.1 ist das Jar nicht mehr obfuskiert),
// sonst die obfuskierten Namen der 1.21.11 (aus Mojangs Mappings). Eigenes Paket, weil das
// Client-Jar signiert ist.
package coralclient;

import java.lang.reflect.Field;
import java.lang.reflect.Method;
import java.nio.file.Files;
import java.nio.file.Path;
import java.util.HexFormat;
import java.util.Map;
import java.util.Optional;

public class ItemComponentsDump {
    // Klasse: Mojang-Name -> Name in 1.21.11.
    static final Map<String, String> CLASSES = Map.ofEntries(
        Map.entry("net.minecraft.SharedConstants", "w"),
        Map.entry("net.minecraft.server.Bootstrap", "amv"),
        Map.entry("net.minecraft.data.registries.VanillaRegistries", "pc"),
        Map.entry("net.minecraft.core.registries.BuiltInRegistries", "mi"),
        Map.entry("net.minecraft.core.RegistryAccess", "jr"),
        Map.entry("net.minecraft.network.RegistryFriendlyByteBuf", "xq"));

    static Class<?> cls(String name) throws ClassNotFoundException {
        try {
            return Class.forName(name);
        } catch (ClassNotFoundException e) {
            return Class.forName(CLASSES.get(name));
        }
    }

    // Erste Methode mit einem der Namen und passender Argumentzahl, auch geerbte.
    static Object call(Object target, Class<?> c, String[] names, Object... args) throws Exception {
        for (Class<?> k = c; k != null; k = k.getSuperclass()) {
            for (String name : names) {
                for (Method m : k.getDeclaredMethods()) {
                    if (!m.getName().equals(name) || m.getParameterCount() != args.length) continue;
                    // Obfuskiert heissen viele Methoden gleich ("a"): nur eine mit passenden
                    // Argumenttypen und, ohne Ziel, eine statische.
                    if ((target == null) != java.lang.reflect.Modifier.isStatic(m.getModifiers())) continue;
                    Class<?>[] typen = m.getParameterTypes();
                    boolean passt = true;
                    for (int i = 0; i < args.length; i++) {
                        Class<?> t = typen[i].isPrimitive() ? boxed(typen[i]) : typen[i];
                        if (args[i] != null && !t.isInstance(args[i])) passt = false;
                    }
                    if (!passt) continue;
                    m.setAccessible(true);
                    return m.invoke(target, args);
                }
            }
        }
        for (Class<?> i : c.getInterfaces()) {
            try {
                return call(target, i, names, args);
            } catch (NoSuchMethodException e) {
                // weiter
            }
        }
        throw new NoSuchMethodException(c.getName() + "." + String.join("/", names));
    }

    static Class<?> boxed(Class<?> p) {
        if (p == boolean.class) return Boolean.class;
        if (p == int.class) return Integer.class;
        return p;
    }

    static Object call(Object target, String mojang, String obf, Object... args) throws Exception {
        return call(target, target.getClass(), new String[] {mojang, obf}, args);
    }

    static Object callStatic(Class<?> c, String mojang, String obf, Object... args) throws Exception {
        return call(null, c, new String[] {mojang, obf}, args);
    }

    static Object field(Class<?> c, String mojang, String obf) throws Exception {
        Field f;
        try {
            f = c.getDeclaredField(mojang);
        } catch (NoSuchFieldException e) {
            f = c.getDeclaredField(obf);
        }
        f.setAccessible(true);
        return f.get(null);
    }

    public static void main(String[] args) throws Exception {
        callStatic(cls("net.minecraft.SharedConstants"), "tryDetectVersion", "a");
        callStatic(cls("net.minecraft.server.Bootstrap"), "bootStrap", "a");
        Object provider = callStatic(cls("net.minecraft.data.registries.VanillaRegistries"), "createLookup", "a");
        Class<?> registries = cls("net.minecraft.core.registries.BuiltInRegistries");
        Object itemRegistry = field(registries, "ITEM", "h");
        Object componentRegistry = field(registries, "DATA_COMPONENT_TYPE", "am");
        Map<Object, Integer> componentIds = new java.util.IdentityHashMap<>();
        for (Object t : (Iterable<?>) componentRegistry) componentIds.put(t, componentIds.size());
        Object registryAccess = callStatic(cls("net.minecraft.core.RegistryAccess"), "fromRegistryOfRegistries", "a", field(registries, "REGISTRY", "aR"));
        Object jsonOps = Class.forName("com.mojang.serialization.JsonOps").getField("INSTANCE").get(null);
        Object ops = call(provider, "createSerializationContext", "a", jsonOps);
        Class<?> friendlyBuf = cls("net.minecraft.network.RegistryFriendlyByteBuf");
        Class<?> unpooled = Class.forName("io.netty.buffer.Unpooled");
        Class<?> byteBuf = Class.forName("io.netty.buffer.ByteBuf");
        Method encodeStart = Class.forName("com.mojang.serialization.Encoder").getMethod("encodeStart", Class.forName("com.mojang.serialization.DynamicOps"), Object.class);

        StringBuilder out = new StringBuilder();
        for (Object item : (Iterable<?>) itemRegistry) {
            out.append("I ").append(call(itemRegistry, "getKey", "b", item)).append('\n');
            for (Object typed : (Iterable<?>) call(item, "components", "f")) {
                Object type = call(typed, "type", "a");
                Object value = call(typed, "value", "b");
                String typeName = call(componentRegistry, "getKey", "b", type) + " " + componentIds.get(type);
                String hex = "-";
                try {
                    Object raw = callStatic(unpooled, "buffer", "buffer");
                    Object buf = friendlyBuf.getConstructor(byteBuf, cls("net.minecraft.core.RegistryAccess")).newInstance(raw, registryAccess);
                    Object codec = call(type, "streamCodec", "f");
                    Method encode = null;
                    for (Method m : codec.getClass().getMethods()) {
                        if (m.getName().equals("encode") && m.getParameterCount() == 2) encode = m;
                    }
                    encode.setAccessible(true);
                    encode.invoke(codec, buf, value);
                    int n = (Integer) byteBuf.getMethod("readableBytes").invoke(buf);
                    byte[] bytes = new byte[n];
                    byteBuf.getMethod("readBytes", byte[].class).invoke(buf, (Object) bytes);
                    hex = HexFormat.of().formatHex(bytes);
                } catch (Throwable t) {
                    hex = "-";
                }
                String json = "-";
                try {
                    Object codec = call(type, "codec", "b");
                    if (codec != null) {
                        Object result = encodeStart.invoke(codec, ops, value);
                        Optional<?> r = (Optional<?>) result.getClass().getMethod("result").invoke(result);
                        if (r.isPresent()) json = r.get().toString();
                    }
                } catch (Throwable t) {
                    json = "-";
                }
                out.append("C ").append(typeName).append('\t').append(hex).append('\t').append(json.replace('\n', ' ')).append('\n');
            }
        }
        Files.writeString(Path.of(args[0]), out.toString());
    }
}
