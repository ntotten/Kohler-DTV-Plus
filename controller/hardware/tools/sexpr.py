"""Minimal S-expression reader/writer for KiCad files."""
import re

class Sym(str):
    """A bare atom (unquoted)."""

_tok = re.compile(r'\s*(?:(\()|(\))|"((?:[^"\\]|\\.)*)"|([^\s()"]+))')

def parse(text):
    stack = [[]]
    pos = 0
    n = len(text)
    while pos < n:
        m = _tok.match(text, pos)
        if not m:
            if text[pos:].strip() == "":
                break
            raise ValueError("bad token at %d: %r" % (pos, text[pos:pos+40]))
        pos = m.end()
        if m.group(1):
            stack.append([])
        elif m.group(2):
            item = stack.pop()
            stack[-1].append(item)
        elif m.group(3) is not None:
            stack[-1].append(m.group(3).replace('\\"', '"').replace('\\\\', '\\'))
        elif m.group(4) is not None:
            stack[-1].append(Sym(m.group(4)))
    return stack[0]

def q(s):
    return '"' + s.replace('\\', '\\\\').replace('"', '\\"') + '"'

def dump(node, indent=0):
    if isinstance(node, list):
        if not node:
            return "()"
        simple = all(not isinstance(x, list) for x in node)
        parts = [dump(x, indent + 1) for x in node]
        if simple or sum(len(p) for p in parts) < 70 and all("\n" not in p for p in parts):
            return "(" + " ".join(parts) + ")"
        pad = "\n" + "  " * (indent + 1)
        return "(" + parts[0] + "".join(pad + p for p in parts[1:]) + ")"
    if isinstance(node, Sym):
        return str(node)
    if isinstance(node, bool):
        return "yes" if node else "no"
    if isinstance(node, (int, float)):
        return fmt_num(node)
    return q(node)

def fmt_num(x):
    if isinstance(x, int):
        return str(x)
    s = ("%.4f" % x).rstrip("0").rstrip(".")
    return "0" if s in ("-0", "") else s

def find(node, key):
    return [x for x in node if isinstance(x, list) and x and x[0] == key]

def find1(node, key):
    r = find(node, key)
    return r[0] if r else None

def lib_symbol(libfile, name):
    tree = parse(open(libfile).read())[0]
    for s in find(tree, "symbol"):
        if s[1] == name:
            return s
    return None
