#!/bin/sh
# Baut Client + Launcher fuer Linux und Windows und legt damit ein GitHub-Release an, das der
# Launcher als Update findet (s. crates/launcher/src/update.rs).
#
#   scripts/release.sh v26.10.1 ["Notizen"]
#   scripts/release.sh v26.10.2-beta.1 ["Notizen"]     -> Beta (Pre-release)
#
# Versionen seit 1.0.0: Jahr (zweistellig).Monat.Nummer im Monat, ab 1 und ohne fuehrende Nullen
# (v26.10.1, v26.10.2, v26.11.1, v27.1.1) -- so sortieren sie auch ueber den Jahreswechsel.
#
# Ein Tag mit Bindestrich (v26.10.2-beta.1, v26.10.2-rc.1) wird als Pre-release veroeffentlicht:
# der Launcher zeigt es nur mit eingeschaltetem "Beta-Versionen anzeigen" an. Ohne
# Notizen-Argument werden, falls vorhanden, die Notizen aus release-notes/<tag>.md genommen.
#
# Die Dateien tragen die Version im Namen (coral-client-v26.10.1-linux-x86_64), s.
# update::Release::asset_for_platform. Dazu dieselben Dateien unter den festen Namen von
# update::ASSET_NAME/LAUNCHER_ASSET_NAME: nur die finden Launcher bis 1.0.0. Raus, sobald keiner
# mehr laeuft (die alten aktualisieren sich darueber selbst auf einen neuen Launcher).
#
# Windows wird mit cargo-zigbuild gebaut (ring braucht sonst eine MinGW-Toolchain).
set -e
tag=${1:?Tag fehlt, z.B. v26.10.1}
notes=${2:-}
if ! printf '%s\n' "$tag" | grep -Eq '^v[0-9]{2}\.([1-9]|1[0-2])\.[1-9][0-9]*(-[0-9A-Za-z.]+)?$'; then
    echo "Tag $tag passt nicht zum Format vJJ.M.N (z.B. v26.10.1, Beta v26.10.2-beta.1)." >&2
    exit 1
fi
repo=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
cd "$repo"
if [ -z "$notes" ] && [ -f "release-notes/$tag.md" ]; then
    notes=$(cat "release-notes/$tag.md")
fi
case "$tag" in
    *-*) vorab=--prerelease ;;
    *) vorab= ;;
esac

# Versionsnummer im Workspace muss zum Tag passen, sonst meldet ein frisch gebauter Launcher
# ohne launcher.conf die falsche installierte Version.
version=$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -n1)
if [ "v$version" != "$tag" ]; then
    echo "Cargo.toml hat version = \"$version\", Tag ist $tag -- erst angleichen." >&2
    exit 1
fi

# Der Tag soll genau auf den Stand zeigen, aus dem gebaut wird: also nur ohne uncommittete
# Aenderungen.
if [ -n "$(git status --porcelain)" ]; then
    echo "Uncommittete Aenderungen -- erst committen." >&2
    exit 1
fi
# Das oeffentliche Repo DerAntonia/coral-client hat eine EIGENE, frische Historie (ohne die alte
# mit Vanilla-Zitaten). Derselbe Stand kommt dort als ein neuer Commit auf main, und darauf
# zeigt der Tag -- den lokalen Commit gibt es dort nicht. Das alte private Repo
# (DerAntonia/mc-client, Remote origin) wird nicht mehr bedient.
git remote get-url coral >/dev/null 2>&1 || git remote add coral https://github.com/DerAntonia/coral-client.git
git fetch -q coral
if [ "$(git rev-parse 'HEAD^{tree}')" = "$(git rev-parse 'coral/main^{tree}')" ]; then
    commit=$(git rev-parse coral/main)
else
    commit=$(git commit-tree 'HEAD^{tree}' -p coral/main -m "$tag")
    git push coral "$commit:refs/heads/main"
fi

export PATH="$HOME/.local/zig/zig-linux-x86_64-0.13.0:$PATH"
# zig bringt keine Importbibliothek fuer d3dcompiler mit (wgpu linkt dagegen); ein Stub aus
# d3dcompiler_47.def liegt in ~/.local/winlibs.
export CARGO_TARGET_X86_64_PC_WINDOWS_GNU_RUSTFLAGS="-L native=$HOME/.local/winlibs"
# Einzeln bauen: in EINEM Aufruf vereinigt Cargo die windows-sys-Features beider Pakete, und
# der Launcher will dann gegen d3dcompiler linken (wgpu), das zig nicht mitbringt.
for paket in app launcher; do
    cargo build --release -p "$paket"
    cargo zigbuild --release -p "$paket" --target x86_64-pc-windows-gnu
done

out=$(mktemp -d)
for name in coral-client coral-client-launcher; do
    cp "target/release/$name" "$out/$name-$tag-linux-x86_64"
    cp "target/x86_64-pc-windows-gnu/release/$name.exe" "$out/$name-$tag-windows-x86_64.exe"
    # Uebergang: feste Namen fuer Launcher bis 1.0.0.
    cp "target/release/$name" "$out/$name-linux-x86_64"
    cp "target/x86_64-pc-windows-gnu/release/$name.exe" "$out/$name-windows-x86_64.exe"
done

gh release create "$tag" --target "$commit" --repo DerAntonia/coral-client --title "$tag" --notes "$notes" $vorab "$out"/*
rm -r "$out"
