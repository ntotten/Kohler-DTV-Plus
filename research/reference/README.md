# Kohler reference material

Kohler's own documentation, used to reproduce the K-99693 interface.

|                                    |                                                                                                                                         |
| ---------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------- |
| `DTV-Plus-UserGuide-1241234-5.pdf` | _User Guide — Digital Interface and System Controller for DTV+_, revision 1241234-5-D. 92 pages. The interface screens are pages 42-88. |
| `K-99693-P_spec.pdf`               | K-99693-P specification sheet.                                                                                                          |
| `guide-text.txt`                   | Text extracted from the user guide, for searching.                                                                                      |
| `interface-screens/`               | The seven pages that informed the UI, as WebP.                                                                                          |

## Regenerating the page renders

Full-page renders are **not committed** — 92 PNGs at 11 MB, all reproducible
from the PDF above. To recreate them:

```bash
python - <<'EOF'
import fitz  # pymupdf
d = fitz.open('research/reference/DTV-Plus-UserGuide-1241234-5.pdf')
import os; os.makedirs('research/reference/guide-pages', exist_ok=True)
for i in range(d.page_count):
    d[i].get_pixmap(dpi=110).save(f'research/reference/guide-pages/p{i+1:02d}.png')
EOF
```

`research/reference/guide-pages/` is gitignored, so regenerating leaves the tree
clean.

## Copyright

These documents are Kohler Co.'s, retained here for repair reference under the
terms described in [../../DISCLAIMER.md](../../DISCLAIMER.md). This project is
not affiliated with or endorsed by Kohler.
