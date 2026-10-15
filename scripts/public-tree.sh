#!/bin/sh
# Gibt den Baum aus, der veroeffentlicht wird: HEAD ohne die Commits aus .git/info/local-only
# (eine Commit-ID je Zeile, # leitet Kommentare ein). Die Liste liegt in .git und wandert nie
# mit -- so bleibt lokal, was (noch) nicht ins oeffentliche Repo soll. Zurueckgenommen wird per
# Drei-Wege-Merge wie bei git revert, also auch dann, wenn spaetere Commits in der Naehe etwas
# geaendert haben. Bei einem Konflikt bricht es ab; dann den Commit in der Liste anpassen.
#
#   scripts/public-tree.sh            -> Baum-ID auf stdout
set -e
liste="$(git rev-parse --git-dir)/info/local-only"
stand=$(git rev-parse HEAD)
if [ -f "$liste" ]; then
    ids=$(mktemp)
    trap 'rm -f "$ids"' EXIT
    grep -v '^[[:space:]]*#' "$liste" | awk 'NF { print $1 }' | while read -r c; do
        git rev-parse --verify -q "$c^{commit}" || { echo "local-only: $c gibt es nicht" >&2; exit 1; }
    done >"$ids"
    # Neueste zuerst zuruecknehmen, wie man es von Hand mit git revert taete.
    for c in $(git rev-list HEAD | grep -Fx -f "$ids" || true); do
        if ! baum=$(git merge-tree --write-tree --merge-base="$c" "$stand" "$c^"); then
            echo "local-only: $c laesst sich nicht sauber zuruecknehmen:" >&2
            echo "$baum" >&2
            exit 1
        fi
        stand=$(git commit-tree "$(echo "$baum" | head -n1)" -p "$stand" -m "lokal: $c zurueckgenommen")
    done
fi
git rev-parse "$stand^{tree}"
