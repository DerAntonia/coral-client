#!/bin/sh
# Baut Client + Launcher fuer Linux und Windows und legt damit ein GitHub-Release an, das der
# Launcher als Update findet (s. crates/launcher/src/update.rs).
#
#   scripts/release.sh v0.2.0 ["Notizen"]
#   scripts/release.sh v0.3.0-beta.1 ["Notizen"]     -> Beta (Pre-release)
#
# Ein Tag mit Bindestrich (v0.3.0-beta.1, v0.3.0-rc.1) wird als Pre-release veroeffentlicht:
# der Launcher zeigt es nur mit eingeschaltetem "Beta-Versionen anzeigen" an. Ohne
# Notizen-Argument werden, falls vorhanden, die Notizen aus release-notes/<tag>.md genommen.
#
# Die Asset-Namen muessen zu update::ASSET_NAME passen. Windows wird mit cargo-zigbuild gebaut
# (ring braucht sonst eine MinGW-Toolchain).
set -e
tag=${1:?Tag fehlt, z.B. v0.2.0}
notes=${2:-}
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
# Aenderungen, und den Branch vorher pushen (sonst legt GitHub den Tag auf den Default-Branch).
if [ -n "$(git status --porcelain)" ]; then
    echo "Uncommittete Aenderungen -- erst committen." >&2
    exit 1
fi
git push -u origin HEAD
# Das oeffentliche Repo DerAntonia/coral-client hat eine EIGENE, frische Historie (ohne die alte
# mit Vanilla-Zitaten). Derselbe Stand kommt dort als ein neuer Commit auf main, und darauf
# zeigt der Tag -- den lokalen Commit gibt es dort nicht.
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
cp target/release/coral-client "$out/coral-client-linux-x86_64"
cp target/release/coral-client-launcher "$out/coral-client-launcher-linux-x86_64"
cp target/x86_64-pc-windows-gnu/release/coral-client.exe "$out/coral-client-windows-x86_64.exe"
cp target/x86_64-pc-windows-gnu/release/coral-client-launcher.exe "$out/coral-client-launcher-windows-x86_64.exe"

gh release create "$tag" --target "$commit" --repo DerAntonia/coral-client --title "$tag" --notes "$notes" $vorab "$out"/*
rm -r "$out"
