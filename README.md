# Coral Client

**English** · [Deutsch](README.de.md)

A Minecraft Java client of its own, written from scratch in Rust. It is built for PvP: as little
delay as possible between mouse, picture and sound.

It speaks the normal Minecraft protocol and connects to ordinary servers. The server needs no mod
and no plugin for it. It ships no game assets such as textures, sounds or models. It reads them
from your own Minecraft installation.

**Target version: Minecraft Java 1.21.11.**

## Installation

### What you need

- A purchased copy of Minecraft: Java Edition. You sign in with your Microsoft account.
- Minecraft **1.21.11**, installed once. That works through the official launcher, Prism
  Launcher or a folder of your own.
- Linux or Windows (64-bit) and a graphics card with Vulkan. DirectX 12 also works on Windows.

### With the launcher

1. Download the launcher for your system from [Releases](../../releases)
   (`coral-client-launcher-linux-x86_64` or `coral-client-launcher-windows-x86_64.exe`).
2. Start it (on Linux, make it executable first) and click **Client herunterladen** (download client).
3. Pick your Minecraft installation in the launcher and click **Spielen** (play).

The launcher looks for updates by itself. It only shows beta versions once
"Beta-Versionen anzeigen" (show beta versions) is turned on. The launcher itself is in German
for now; the client's interface can be switched to English.

## Features

### Latency

- Raw mouse input, an adjustable present mode, and control over where in the picture a tear
  may fall.
- The client predicts movement, eating, crystals and placed blocks itself instead of waiting for
  the server.
- Sound goes to the output with small buffers, because in PvP sound counts as much as the
  picture.

### Graphics

- **PvP render path:** fast and plain, built for high frame rates.
- **Resource packs** with custom item models, core shaders, HD inventories and skies in
  OptiFine format.
- **Far view like Voxy:** the Chunk Saver stores every loaded chunk and draws landscape from it
  far beyond the render distance.
- **Particles** can be switched on and off per type and tuned in duration, opacity and speed.

### Modules

Every module can be switched on and off on its own in the module menu. The groups match the ones
in the game.

**Crystal PvP**
- **Totem indicator:** shows whether a totem sits in the hotbar and the offhand.
- **Totem warning:** shows an X at the crosshair when a totem is not in its place.
- **Totem preview:** shows where the totems are before you open the inventory.
- **Pop sounds:** custom sounds for pops, explosions and anchors.
- **Pop visuals:** turns off the flying totem and the particle cloud on a pop.
- **Crystal optimizer:** crystals you hit vanish at once, without waiting for the server.

**Render**
- **Hitbox display:** hitboxes of other entities, smoothed and/or raw.
- **Shields:** cooldown of your own and other players' shields, and other players' raised shields.
- **Timer:** a circle at the crosshair. It fills while you eat until you are done, and empties
  while you draw a bow until the arrow is critical.
- **Arrows:** ammo display for bow and crossbow, and white trails behind arrows.
- **Sky:** a solid color, a gradient with sun and moon, or the sky from the pack, plus a fixed
  time of day.
- **Weather:** rain, snow or clear, fixed, falling within an adjustable distance around you.
  This only affects the picture.
- **Color:** saturation of the whole picture.
- **Block outline:** color, opacity and width of the frame around the targeted block.
- **Dynamic fire:** with fire resistance, fire blocks fade out.
- **Chunk Saver:** the far view described above.

**Utility**
- **Consumable optimizer:** eating and drinking finish on the right tick, without waiting for
  the server.
- **No Tool Break:** blocks mining before a diamond or netherite tool with Mending breaks.
- **Zoom:** narrows the field of view while the zoom key is held.
- **Voice chat:** compatible with Simple Voice Chat. You hear nearby players and talk with the
  push-to-talk key.
- **Discord status:** shows version, server and play time in Discord.
- **Input log:** writes keys, mouse buttons and the scroll wheel with timestamps to a file.
- **Sustainability:** saves power. The client draws no frames in the background, and the world
  stands still behind the pause menu.

**Debug**
- **Ghost block indicator:** blocks the server has not confirmed yet (placed, broken, anchors)
  turn semi-transparent after an adjustable tolerance.
- **Botcheck pass:** on joining, loads the world first and then starts moving, like Vanilla. That
  is slower but passes bot checks.
- **Show barriers:** makes barriers, light blocks and structure voids visible.
- **Sound debug:** two bars showing the volume of the left and right channel.

### Controls

- The interface is available in German, English and Latin.
- Several settings configs, selectable in the launcher.
- A game mode wheel and freely bindable keys, including double bindings.

Version 1.0 is the first stable release. A lot works like Vanilla; some things are still
missing or look different. The client is built and tested against 1.21.11. Other versions are
not supported.

### Building it yourself

You need a current Rust toolchain (stable). On Linux you also need the ALSA development files
(`libasound2-dev` or similar).

```sh
cargo build --release -p app -p launcher
```

Afterwards `target/release/coral-client` and `target/release/coral-client-launcher` are ready. The
launcher starts the client that sits in the same folder.

| Crate | Purpose |
|---|---|
| `protocol` | Reading and writing Minecraft protocol packets |
| `net` | Server connection, login, encryption |
| `auth` | Microsoft sign-in |
| `world` | World state from server packets: chunks, entities, items |
| `assets` | Loading from the Minecraft installation and from resource packs |
| `render` | Drawing with wgpu: terrain, far view, sky, shader path |
| `chunk_saver` | Saved chunks and their far view |
| `audio` | Low-latency sound output |
| `voice` | Voice chat |
| `app` | Window, input, UI, game logic; ties everything together |
| `launcher` | Picking the installation, updates, configs, launching |

## License and contributions

**All rights reserved.** The code is public so you can read it, learn from it and help improve
it. That does not make it free to republish.

- **Allowed:** forking this repository on GitHub, changing things in your fork and opening pull
  requests. Downloading and using the releases is of course fine too.
- **Not allowed:** publishing the code anywhere other than a GitHub fork of this repository, for
  example as a new repository, as a download or as part of another client. This also applies if
  you give credit.

**Exception, the launcher:** everything in `crates/launcher/` is in the public domain under
[CC0 1.0](crates/launcher/LICENSE). The finished launcher (`coral-client-launcher-*` from the
releases) may be shared, uploaded and passed on freely, including the parts of the `assets` crate
it is built with.

**Pull requests:** by opening a pull request you allow the author to use your contribution in
this project under any license, including a later open-source license.

### Third-party files

These files are not covered by the above and keep their own licenses:

| Files | Source | License |
|---|---|---|
| `assets/fonts/` | DM Sans, Lexend, Lexend Exa, Noto Sans, Outfit | SIL Open Font License 1.1 (license files next to the fonts) |
| `assets/sounds/kenney-*` | [Kenney](https://kenney.nl): Interface Sounds, UI Audio | CC0 1.0 |
| `assets/sounds/custom/` | made from Kenney sounds and synthesis | CC0 1.0 |
| `assets/sounds/totemingpromax/click.wav` | [Kenney](https://kenney.nl): Interface Sounds | CC0 1.0 |
| `assets/sounds/totemingpromax/glass_break_01.wav` | [rubberduck: 75 CC0 breaking / falling / hit sfx](https://opengameart.org/content/75-cc0-breaking-falling-hit-sfx) | CC0 1.0 |

## Legal

- This is **not an official Minecraft product**. It is not approved by or associated with
  Mojang or Microsoft. "Minecraft" is a trademark of Mojang AB.
- Coral Client is made and published by DerAntonia, who alone is responsible for it. Mojang
  and Microsoft are not.
- Contact: hubertjohnson1231236@gmail.com, or on Discord: `derantonia`.
- The project contains no textures, sounds, models or code from Mojang. All game assets come
  from your own installation at runtime. It does contain tables of game data read out of the
  game (block states, collision shapes, mining values), as projects like PrismarineJS do too.
- The client does not bypass anticheats and gives no unfair advantage. It does what Vanilla
  does, only with less delay.
