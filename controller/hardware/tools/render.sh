#!/usr/bin/env bash
# Regenerate the carrier's schematic, board, fabrication files and cabling
# drawings from their sources, and check every diagram in the repository.
#
#   carrier.kicad_sch, carrier.kicad_sym  <- tools/carrier_schematic.py
#   carrier.svg                           <- KiCad, after its netlist is checked
#   carrier.pretty/                       <- tools/carrier_footprints.py
#   carrier.kicad_pcb, .kicad_pro, .kicad_dru <- tools/carrier_board.py, routing
#                                            replayed from carrier.ses
#   fab/                                  <- tools/carrier_fab.py
#   cabling.svg                           <- cabling.wireviz.yml (WireViz)
#   cabling-wires.csv                     <- tools/wire_list.py, checked against
#                                            the board's nets
#   every ```mermaid block in the repository's Markdown must parse
#
# Needs: python3; KiCad 7 (kicad-cli, and a Python that can import pcbnew:
# KICAD_CLI and KICAD_PYTHON override the search); WireViz 0.4 with Graphviz
# (pip install wireviz==0.4.1); and npx for mermaid-cli. The committed files
# were made with KiCad 7.0.11; another version makes the same board in
# different bytes.
#
#   --check    fail when a committed file differs from what its source makes
#   --reroute  route the board again with Freerouting 2.4.1 (FREEROUTING_JAR,
#              and JAVA for Java 25) and replace carrier.ses
set -euo pipefail

check=""
reroute=""
for arg in "$@"; do
  case "$arg" in
    --check) check=1 ;;
    --reroute) reroute="--reroute" ;;
    *) echo "unknown option $arg" >&2; exit 2 ;;
  esac
done

here="$(cd "$(dirname "$0")/.." && pwd)"
repo="$(git -C "$here" rev-parse --show-toplevel)"
tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT

kicad_cli="${KICAD_CLI:-}"
if [ -z "$kicad_cli" ]; then
  for c in kicad-cli /Applications/KiCad/KiCad.app/Contents/MacOS/kicad-cli; do
    if command -v "$c" >/dev/null 2>&1; then kicad_cli="$c"; break; fi
  done
fi
[ -n "$kicad_cli" ] || { echo "kicad-cli not found; set KICAD_CLI" >&2; exit 1; }
export KICAD_CLI="$kicad_cli"
kicad_python="${KICAD_PYTHON:-}"
if [ -z "$kicad_python" ]; then
  for c in python3 /Applications/KiCad/KiCad.app/Contents/Frameworks/Python.framework/Versions/Current/bin/python3; do
    if "$c" -c 'import pcbnew' >/dev/null 2>&1; then kicad_python="$c"; break; fi
  done
fi
[ -n "$kicad_python" ] || { echo "no Python with KiCad's pcbnew module; set KICAD_PYTHON" >&2; exit 1; }
version="$("$kicad_cli" version)"
case "$version" in
  7.0.*) ;;
  *) echo "warning: KiCad $version; the committed files were made with 7.0.11" >&2 ;;
esac
command -v wireviz >/dev/null || { echo "wireviz not found: pip install wireviz==0.4.1" >&2; exit 1; }
# WireViz brings PyYAML; the wire list runs on the same Python.
yaml_python="$(head -1 "$(command -v wireviz)" | sed -n 's/^#!//p')"
"${yaml_python:-python3}" -c 'import yaml' 2>/dev/null || yaml_python=python3

echo "1/7 carrier schematic from tools/carrier_schematic.py"
python3 "$here/tools/carrier_schematic.py"

echo "2/7 KiCad reads it back: netlist check, then carrier.svg"
"$kicad_cli" sch export netlist -o "$tmp/carrier.net" "$here/carrier.kicad_sch" >/dev/null
python3 "$here/tools/check_netlist.py" "$tmp/carrier.net"
"$kicad_cli" sch export svg -o "$tmp" "$here/carrier.kicad_sch" >/dev/null
# Drop the export timestamp so an unchanged schematic renders byte-identical.
sed -e 's/^<title>SVG Image created as carrier.svg date .*<\/title>$/<title>carrier.svg<\/title>/' \
  "$tmp/carrier.svg" >"$here/carrier.svg"

echo "3/7 carrier PCB: footprints, placement, routing, ground pour, DRC"
python3 "$here/tools/carrier_footprints.py"
"$kicad_python" "$here/tools/carrier_board.py" "$tmp/carrier.net" $reroute

echo "4/7 fabrication files in fab/"
"$kicad_python" "$here/tools/carrier_fab.py" "$tmp/carrier.net"

echo "5/7 cabling.svg from cabling.wireviz.yml"
wireviz -f s -o "$tmp" -O cabling "$here/cabling.wireviz.yml" >/dev/null
cp "$tmp/cabling.svg" "$here/cabling.svg"

echo "6/7 cabling-wires.csv, checked against the board"
"$yaml_python" "$here/tools/wire_list.py" "$tmp/carrier.net"

echo "7/7 Mermaid blocks parse"
count=0
while IFS= read -r md; do
  count=$((count + 1))
  out="$tmp/mermaid/$count"
  mkdir -p "$out"
  if ! npx -y -p @mermaid-js/mermaid-cli@11 mmdc ${PUPPETEER_CONFIG:+-p "$PUPPETEER_CONFIG"} \
    -i "$repo/$md" -o "$out/out.md" >"$out/log" 2>&1; then
    echo "Mermaid failed in $md:" >&2
    cat "$out/log" >&2
    exit 1
  fi
  echo "  $md"
done < <(git -C "$repo" grep -l '^```mermaid' -- '*.md')

if [ -n "$check" ]; then
  changed="$(git -C "$repo" status --porcelain -- controller/hardware)"
  if [ -n "$changed" ]; then
    echo "$changed" >&2
    echo "Committed files differ from their sources; run tools/render.sh and commit." >&2
    exit 1
  fi
fi
echo "Done."
