#!/usr/bin/env bash
# Render the hardware drawings and check every diagram in the repository.
#
#   carrier.kicad_sch, carrier.kicad_sym  <- tools/carrier_schematic.py
#   carrier.svg                           <- KiCad, after its netlist is checked
#   cabling.svg                           <- cabling.wireviz.yml (WireViz)
#   every ```mermaid block in the repository's Markdown must parse
#
# Needs: python3, kicad-cli (KiCad 7 or later; KICAD_CLI overrides), wireviz
# 0.4 with Graphviz (pip install wireviz==0.4.1), and npx for mermaid-cli.
# Pass --check to fail when a committed drawing differs from its source.
set -euo pipefail

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
command -v wireviz >/dev/null || { echo "wireviz not found: pip install wireviz==0.4.1" >&2; exit 1; }

echo "1/4 carrier schematic from tools/carrier_schematic.py"
python3 "$here/tools/carrier_schematic.py"

echo "2/4 KiCad reads it back: netlist check, then carrier.svg"
"$kicad_cli" sch export netlist -o "$tmp/carrier.net" "$here/carrier.kicad_sch" >/dev/null
python3 "$here/tools/check_netlist.py" "$tmp/carrier.net"
"$kicad_cli" sch export svg -o "$tmp" "$here/carrier.kicad_sch" >/dev/null
# Drop the export timestamp so an unchanged schematic renders byte-identical.
sed -e 's/^<title>SVG Image created as carrier.svg date .*<\/title>$/<title>carrier.svg<\/title>/' \
  "$tmp/carrier.svg" >"$here/carrier.svg"

echo "3/4 cabling.svg from cabling.wireviz.yml"
wireviz -f s -o "$tmp" -O cabling "$here/cabling.wireviz.yml" >/dev/null
cp "$tmp/cabling.svg" "$here/cabling.svg"

echo "4/4 Mermaid blocks parse"
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

if [ "${1:-}" = "--check" ]; then
  git -C "$repo" diff --exit-code -- controller/hardware ||
    { echo "Committed drawings differ from their sources; run tools/render.sh and commit." >&2; exit 1; }
fi
echo "Done."
