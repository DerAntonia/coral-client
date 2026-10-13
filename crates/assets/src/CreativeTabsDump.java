// Liest die Inhalte der Kreativ-Tabs aus dem Client-Jar der Installation des Nutzers. Welche
// Items in welchem Tab stehen, steht in Vanilla nur als Java-Code (CreativeModeTabs); dieses
// Programm laesst genau diesen Code einmal laufen und schreibt das Ergebnis als Text. Gestartet
// von assets::creative_tabs mit dem Java der Installation:
//
//   java -cp <client.jar>:<bibliotheken> CreativeTabsDump.java <ausgabedatei>
//
// Ausgabe, je Zeile ein Eintrag:
//   T <tab> <typ> <reihe> <spalte> <rechts> <titel> <scrollt> <hintergrund> <symbol> <name> <sichtbar>
//   I <item> <anzahl>                       ein Stapel des zuletzt genannten Tabs
//   C <component> <id>\t<hex>\t<json>       eine gesetzte Component dieses Stapels
//   R <component> <id>                      eine entfernte Component dieses Stapels
// <id> ist die Netz-ID des Component-Typs (Position in seiner Registry).
// <hex> ist die Netzform (ohne Typ-ID und Laenge) mit den eingebauten Registries, oder "-",
// wenn die Component eine Registry braucht, die erst der Server festlegt (Verzauberungen,
// Gemaelde, ...). Dann zaehlt das JSON, in dem solche Verweise als Namen stehen.
//
// Alles per Reflection: zuerst die Mojang-Namen (ab 26.1 ist das Jar nicht mehr obfuskiert),
// sonst die obfuskierten Namen der 1.21.11 (aus Mojangs Mappings). Eigenes Paket, weil das
// Client-Jar signiert ist.
package coralclient;

import java.lang.reflect.Field;
import java.lang.reflect.Method;
import java.nio.file.Files;
import java.nio.file.Path;
import java.io.InputStream;
import java.io.InputStreamReader;
import java.nio.charset.StandardCharsets;
import java.util.ArrayList;
import java.util.Collection;
import java.util.HashSet;
import java.util.HexFormat;
import java.util.List;
import java.util.Map;
import java.util.Optional;
import java.util.Set;

public class CreativeTabsDump {
    // Klasse: Mojang-Name -> Name in 1.21.11.
    static final Map<String, String> CLASSES = Map.ofEntries(
        Map.entry("net.minecraft.SharedConstants", "w"),
        Map.entry("net.minecraft.server.Bootstrap", "amv"),
        Map.entry("net.minecraft.data.registries.VanillaRegistries", "pc"),
        Map.entry("net.minecraft.world.flag.FeatureFlags", "dhb"),
        Map.entry("net.minecraft.world.item.CreativeModeTabs", "dkl"),
        Map.entry("net.minecraft.core.registries.BuiltInRegistries", "mi"),
        Map.entry("net.minecraft.core.RegistryAccess", "jr"),
        Map.entry("net.minecraft.network.RegistryFriendlyByteBuf", "xq"),
        Map.entry("net.minecraft.network.chat.contents.TranslatableContents", "zq"),
        Map.entry("net.minecraft.core.registries.Registries", "mj"),
        Map.entry("net.minecraft.tags.PaintingVariantTags", "bdz"),
        Map.entry("net.minecraft.tags.InstrumentTags", "bdx"),
        Map.entry("net.minecraft.tags.TagKey", "bef"),
        Map.entry("net.minecraft.core.HolderSet$Named", "jh$c"),
        Map.entry("net.minecraft.core.HolderLookup$Provider", "jf$a"),
        Map.entry("net.minecraft.core.HolderLookup$RegistryLookup", "jf$b"));

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

    // Alle IDs eines Tags aus den Daten im Jar, verschachtelte Tags aufgeloest.
    static void tagValues(String registry, String tag, Set<String> out) throws Exception {
        String[] teile = tag.split(":", 2);
        String pfad = "data/" + teile[0] + "/tags/" + registry + "/" + teile[1] + ".json";
        try (InputStream in = CreativeTabsDump.class.getClassLoader().getResourceAsStream(pfad)) {
            if (in == null) return;
            com.google.gson.JsonObject json = com.google.gson.JsonParser.parseReader(new InputStreamReader(in, StandardCharsets.UTF_8)).getAsJsonObject();
            for (com.google.gson.JsonElement e : json.getAsJsonArray("values")) {
                String v = e.isJsonObject() ? e.getAsJsonObject().get("id").getAsString() : e.getAsString();
                if (v.startsWith("#")) tagValues(registry, v.substring(1), out);
                else out.add(v.contains(":") ? v : "minecraft:" + v);
            }
        }
    }

    // Die eingebauten Registries kennen ihre Tags nicht (die kommen sonst aus den Datapacks).
    // Die Tabs fragen genau zwei ab -- Gemaelde zum Aufhaengen und Ziegenhoerner --, die
    // werden hier aus den Tag-Dateien des Jars gebunden.
    static Object bindTag(Object provider, String registryField, String registryObf, String registry, String tagClass, String tagField, String tagObf, String tagId) throws Exception {
        Object registryKey = field(cls("net.minecraft.core.registries.Registries"), registryField, registryObf);
        Object tagKey = field(cls(tagClass), tagField, tagObf);
        Set<String> ids = new HashSet<>();
        tagValues(registry, tagId, ids);
        Object lookup = call(provider, provider.getClass(), new String[] {"lookupOrThrow", "e"}, registryKey);
        List<Object> refs = new ArrayList<>();
        ((java.util.stream.Stream<?>) call(lookup, "listElements", "c")).forEach(refs::add);
        List<Object> drin = new ArrayList<>();
        for (Object ref : refs) {
            Object key = call(ref, "key", "h");
            String id = String.valueOf(call(key, "identifier", "a"));
            call(ref, "bindTags", "a", ids.contains(id) ? List.of(tagKey) : List.of());
            if (ids.contains(id)) drin.add(ref);
        }
        // Dazu die Liste des Tags selbst, fuer Abfragen nach dem ganzen Tag (s. withTag).
        java.lang.reflect.Constructor<?> ctor = cls("net.minecraft.core.HolderSet$Named").getDeclaredConstructors()[0];
        ctor.setAccessible(true);
        Object named = ctor.newInstance(lookup, tagKey);
        call(named, "bind", "b", drin);
        return named;
    }

    // Der Provider, nur dass die Registry `registryKey` auf die Frage nach dem Tag `tagKey`
    // die gebundene Liste `named` liefert (die eingebaute Registry kann das waehrend ihres
    // Aufbaus nicht -- so fragen die Tabs die Ziegenhoerner ab).
    static Object withTag(Object provider, Object registryKey, Object tagKey, Object named) throws Exception {
        Class<?> lookupType = cls("net.minecraft.core.HolderLookup$RegistryLookup");
        Class<?> tagKeyType = cls("net.minecraft.tags.TagKey");
        java.util.function.Function<Object, Object> wrap = real -> java.lang.reflect.Proxy.newProxyInstance(lookupType.getClassLoader(), new Class<?>[] {lookupType}, (proxy, m, a) -> {
            if (a != null && a.length == 1 && tagKeyType.isInstance(a[0]) && m.getReturnType() == Optional.class && a[0].equals(tagKey)) {
                return Optional.of(named);
            }
            m.setAccessible(true);
            try {
                return m.invoke(real, a);
            } catch (java.lang.reflect.InvocationTargetException e) {
                throw e.getCause();
            }
        });
        Class<?> providerType = cls("net.minecraft.core.HolderLookup$Provider");
        return java.lang.reflect.Proxy.newProxyInstance(providerType.getClassLoader(), new Class<?>[] {providerType}, (proxy, m, a) -> {
            Object r;
            m.setAccessible(true);
            try {
                r = m.invoke(provider, a);
            } catch (java.lang.reflect.InvocationTargetException e) {
                throw e.getCause();
            }
            if (a == null || a.length != 1 || !registryKey.equals(a[0])) return r;
            if (r instanceof Optional<?> o) return o.map(wrap);
            if (lookupType.isInstance(r)) return wrap.apply(r);
            return r;
        });
    }

    public static void main(String[] args) throws Exception {
        callStatic(cls("net.minecraft.SharedConstants"), "tryDetectVersion", "a");
        callStatic(cls("net.minecraft.server.Bootstrap"), "bootStrap", "a");
        Object provider = callStatic(cls("net.minecraft.data.registries.VanillaRegistries"), "createLookup", "a");
        bindTag(provider, "PAINTING_VARIANT", "bn", "painting_variant", "net.minecraft.tags.PaintingVariantTags", "PLACEABLE", "a", "minecraft:placeable");
        Object hoerner = bindTag(provider, "INSTRUMENT", "bi", "instrument", "net.minecraft.tags.InstrumentTags", "GOAT_HORNS", "c", "minecraft:goat_horns");
        provider = withTag(provider, field(cls("net.minecraft.core.registries.Registries"), "INSTRUMENT", "bi"), field(cls("net.minecraft.tags.InstrumentTags"), "GOAT_HORNS", "c"), hoerner);
        Object flags = field(cls("net.minecraft.world.flag.FeatureFlags"), "DEFAULT_FLAGS", "h");
        Class<?> tabs = cls("net.minecraft.world.item.CreativeModeTabs");
        // Mit Operator-Rechten, damit auch der Reiter mit Befehlsbloecken gefuellt ist. Ob er
        // erscheint (und seine Items in der Suche), entscheidet der Client: Vanilla zeigt ihn nur
        // mit Operator-Stufe ab 2 und eingeschalteter Option.
        callStatic(tabs, "tryRebuildTabContents", "a", flags, true, provider);
        Class<?> registries = cls("net.minecraft.core.registries.BuiltInRegistries");
        Object tabRegistry = field(registries, "CREATIVE_MODE_TAB", "aj");
        Object itemRegistry = field(registries, "ITEM", "h");
        Object componentRegistry = field(registries, "DATA_COMPONENT_TYPE", "am");
        // Eine Registry zaehlt beim Durchlaufen in ID-Reihenfolge.
        Map<Object, Integer> componentIds = new java.util.IdentityHashMap<>();
        for (Object t : (Iterable<?>) componentRegistry) componentIds.put(t, componentIds.size());
        Object registryAccess = callStatic(cls("net.minecraft.core.RegistryAccess"), "fromRegistryOfRegistries", "a", field(registries, "REGISTRY", "aR"));
        Object jsonOps = Class.forName("com.mojang.serialization.JsonOps").getField("INSTANCE").get(null);
        Object ops = call(provider, "createSerializationContext", "a", jsonOps);
        Class<?> translatable = cls("net.minecraft.network.chat.contents.TranslatableContents");
        Class<?> friendlyBuf = cls("net.minecraft.network.RegistryFriendlyByteBuf");
        Class<?> unpooled = Class.forName("io.netty.buffer.Unpooled");
        Class<?> byteBuf = Class.forName("io.netty.buffer.ByteBuf");

        StringBuilder out = new StringBuilder();
        for (Object tab : (List<?>) callStatic(tabs, "allTabs", "d")) {
            Object name = call(tab, "getDisplayName", "a");
            Object contents = call(name, "getContents", "b");
            String key = translatable.isInstance(contents) ? (String) call(contents, "getKey", "b") : "-";
            Object icon = call(tab, "getIconItem", "b");
            out.append("T ").append(call(tabRegistry, "getKey", "b", tab))
                .append(' ').append(((Enum<?>) call(tab, "getType", "k")).name())
                .append(' ').append(((Enum<?>) call(tab, "row", "g")).name())
                .append(' ').append(call(tab, "column", "f"))
                .append(' ').append((Boolean) call(tab, "isAlignedRight", "j") ? 1 : 0)
                .append(' ').append((Boolean) call(tab, "showTitle", "d") ? 1 : 0)
                .append(' ').append((Boolean) call(tab, "canScroll", "e") ? 1 : 0)
                .append(' ').append(call(tab, "getBackgroundTexture", "c"))
                .append(' ').append(call(itemRegistry, "getKey", "b", call(icon, "getItem", "h")))
                .append(' ').append(key)
                .append(' ').append((Boolean) call(tab, "shouldDisplay", "i") ? 1 : 0)
                .append('\n');
            for (Object stack : (Collection<?>) call(tab, "getDisplayItems", "l")) {
                out.append("I ").append(call(itemRegistry, "getKey", "b", call(stack, "getItem", "h")))
                    .append(' ').append(call(stack, "getCount", "N")).append('\n');
                Object patch = call(stack, "getComponentsPatch", "d");
                for (Object o : (Set<?>) call(patch, "entrySet", "b")) {
                    Map.Entry<?, ?> e = (Map.Entry<?, ?>) o;
                    Object type = e.getKey();
                    String typeName = call(componentRegistry, "getKey", "b", type) + " " + componentIds.get(type);
                    Optional<?> value = (Optional<?>) e.getValue();
                    if (value.isEmpty()) {
                        out.append("R ").append(typeName).append('\n');
                        continue;
                    }
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
                        encode.invoke(codec, buf, value.get());
                        int n = (Integer) byteBuf.getMethod("readableBytes").invoke(buf);
                        byte[] bytes = new byte[n];
                        byteBuf.getMethod("readBytes", byte[].class).invoke(buf, (Object) bytes);
                        hex = HexFormat.of().formatHex(bytes);
                    } catch (Throwable t) {
                        hex = "-";
                    }
                    String json = "-";
                    Object codec = call(type, "codec", "b");
                    if (codec != null) {
                        Method enc = Class.forName("com.mojang.serialization.Encoder").getMethod("encodeStart", Class.forName("com.mojang.serialization.DynamicOps"), Object.class);
                        Object result = enc.invoke(codec, ops, value.get());
                        Optional<?> r = (Optional<?>) result.getClass().getMethod("result").invoke(result);
                        if (r.isPresent()) json = r.get().toString();
                    }
                    out.append("C ").append(typeName).append('\t').append(hex).append('\t').append(json.replace('\n', ' ')).append('\n');
                }
            }
        }
        Files.writeString(Path.of(args[0]), out.toString());
    }
}
