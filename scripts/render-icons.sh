#!/usr/bin/env bash
# Regera os PNGs do ícone (hicolor) a partir do SVG, a fonte única.
# Usa `rsvg-convert` (pacote librsvg2-bin / librsvg); sem ele, cai para os bindings
# Python do librsvg (python3-gi + gir1.2-rsvg-2.0), que é o mesmo renderizador.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
ID="io.github.rafaelprofmgz.Doknyu"
SVG="$ROOT/assets/icons/$ID.svg"
SIZES=(16 24 32 48 64 128 256 512)

render_python() {
    python3 -I -W ignore::DeprecationWarning - "$SVG" "$1" "$2" <<'PY'
import sys
import gi
gi.require_version("Rsvg", "2.0")
from gi.repository import Rsvg

svg, out, size = sys.argv[1], sys.argv[2], int(sys.argv[3])
# Sem python3-gi-cairo não há como passar um cairo.Context; então o SVG é redimensionado
# (width/height da raiz, mantendo o viewBox) e renderizado direto para um GdkPixbuf.
data = open(svg, encoding="utf-8").read()
data = data.replace('width="128" height="128"', f'width="{size}" height="{size}"', 1)
handle = Rsvg.Handle.new_from_data(data.encode("utf-8"))
handle.get_pixbuf().savev(out, "png", [], [])
PY
}

for n in "${SIZES[@]}"; do
    dir="$ROOT/assets/icons/hicolor/${n}x${n}/apps"
    out="$dir/$ID.png"
    mkdir -p "$dir"
    if command -v rsvg-convert >/dev/null 2>&1; then
        rsvg-convert --width "$n" --height "$n" --format png --output "$out" "$SVG"
    else
        render_python "$out" "$n"
    fi
    echo "gerado $out"
done
