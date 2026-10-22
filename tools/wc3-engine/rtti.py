#!/usr/bin/env python3
"""Recover MSVC RTTI class vtables from a Warcraft III Game.dll.

Usage:
  rtti.py GAME_DLL vtable CLASS          print the vtable slots of CLASS
  rtti.py GAME_DLL slot OFFSET [PREFIX]  list classes (name starting PREFIX) whose slot OFFSET
                                         differs from the most common implementation
  rtti.py GAME_DLL classes [PREFIX]      list recovered class names
  rtti.py GAME_DLL const OFFSET [PREFIX] for slots that are `mov eax, imm; ret`, print the constants

Addresses are virtual addresses using the DLL's preferred image base (0x6f000000 for the
August 2017 build analysed in docs/verification/engine-evidence-gaps.md).
"""
import collections
import os
import pickle
import re
import struct
import sys

import numpy as np
import pefile


def load(path):
    pe = pefile.PE(path, fast_load=True)
    base = pe.OPTIONAL_HEADER.ImageBase
    data = bytearray()
    spans = []
    for s in pe.sections:
        va = base + s.VirtualAddress
        raw = s.get_data()
        spans.append((va, len(raw), len(data)))
        data += raw
    return base, bytes(data), spans


class Image:
    def __init__(self, path):
        self.base, self.data, self.spans = load(path)
        words = np.frombuffer(self.data[: len(self.data) // 4 * 4], dtype="<u4")
        self._order = np.argsort(words, kind="stable")
        self._sorted = words[self._order]

    def off(self, va):
        for start, size, off in self.spans:
            if start <= va < start + size:
                return off + va - start
        return None

    def va(self, off):
        for start, size, o in self.spans:
            if o <= off < o + size:
                return start + off - o
        return None

    def u32(self, va):
        o = self.off(va)
        return None if o is None or o + 4 > len(self.data) else struct.unpack_from("<I", self.data, o)[0]

    def find_u32(self, value):
        lo = int(np.searchsorted(self._sorted, value, side="left"))
        hi = int(np.searchsorted(self._sorted, value, side="right"))
        return sorted(self.va(int(i) * 4) for i in self._order[lo:hi])


def classes(img):
    result = collections.defaultdict(list)
    for m in re.finditer(rb"\.\?AV([A-Za-z0-9_@$?]+?)@@\x00", img.data):
        name = m.group(1).decode()
        td = img.va(m.start()) - 8
        for ref in img.find_u32(td):
            col = ref - 12
            if img.u32(col) != 0:
                continue
            for p in img.find_u32(col):
                result[name].append((img.u32(col + 4), p + 4))
    return result


def slots(img, vt, count=0x200 // 4):
    out = []
    for i in range(count):
        v = img.u32(vt + 4 * i)
        if v is None or not (img.base <= v < img.base + 0x1000000) or img.off(v) is None:
            break
        out.append(v)
    return out


def cached_classes(img, path):
    cache = os.environ.get("WC3_RTTI_CACHE")
    key = (path, os.path.getmtime(path))
    if cache and os.path.exists(cache):
        with open(cache, "rb") as f:
            stored = pickle.load(f)
        if stored[0] == key:
            return stored[1]
    table = classes(img)
    if cache:
        with open(cache, "wb") as f:
            pickle.dump((key, dict(table)), f)
    return table


def main():
    img = Image(sys.argv[1])
    cmd = sys.argv[2]
    table = cached_classes(img, sys.argv[1])
    if cmd == "classes":
        prefix = sys.argv[3] if len(sys.argv) > 3 else ""
        for n in sorted(table):
            if n.startswith(prefix):
                print(n, " ".join(f"{o}:{v:#x}" for o, v in table[n]))
    elif cmd == "vtable":
        for off, vt in table[sys.argv[3]]:
            print(f"vtable {vt:#x} (this-offset {off})")
            for i, v in enumerate(slots(img, vt)):
                print(f"  +{i * 4:#x} {v:#x}")
    elif cmd == "const":
        index = int(sys.argv[3], 0) // 4
        prefix = sys.argv[4] if len(sys.argv) > 4 else ""
        for n, lst in sorted(table.items()):
            if not n.startswith(prefix):
                continue
            for off, vt in lst:
                if off != 0:
                    continue
                s = slots(img, vt)
                if index >= len(s):
                    continue
                o = img.off(s[index])
                code = img.data[o : o + 6]
                if code[0] == 0xB8 and code[5] == 0xC3:
                    value = struct.unpack_from("<I", code, 1)[0]
                    print(n, value, hex(s[index]))
                elif code[0] in (0x31, 0x33) and code[1] == 0xC0 and code[2] == 0xC3:
                    print(n, 0, hex(s[index]))
    elif cmd == "slot":
        index = int(sys.argv[3], 0) // 4
        prefix = sys.argv[4] if len(sys.argv) > 4 else ""
        pairs = {}
        for n, lst in table.items():
            if not n.startswith(prefix):
                continue
            for off, vt in lst:
                if off == 0:
                    s = slots(img, vt)
                    if index < len(s):
                        pairs[n] = s[index]
        majority = collections.Counter(pairs.values()).most_common(1)[0][0]
        print(f"default {majority:#x}")
        for n, v in sorted(pairs.items()):
            if v != majority:
                print(f"{n} {v:#x}")


if __name__ == "__main__":
    main()
