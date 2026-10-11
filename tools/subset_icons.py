#!/usr/bin/env python3
"""Copy named glyphs from the Material Symbols variable font (its default instance:
weight 400, unfilled, optical size 24) into a small static TTF mapped from U+E900, and
write the matching Rust constants. Needs the Material Symbols font installed (Arch:
ttf-material-symbols-variable); no Python packages."""

import struct


class Font:
    def __init__(self, path):
        self.data = open(path, 'rb').read()
        d = self.data
        n = struct.unpack('>H', d[4:6])[0]
        self.tables = {}
        for i in range(n):
            tag, cs, off, ln = struct.unpack('>4sIII', d[12+16*i:28+16*i])
            self.tables[tag.decode()] = d[off:off+ln]
        head = self.tables['head']
        self.upem = struct.unpack('>H', head[18:20])[0]
        self.loc_format = struct.unpack('>h', head[50:52])[0]
        self.num_glyphs = struct.unpack('>H', self.tables['maxp'][4:6])[0]
        loca = self.tables['loca']
        if self.loc_format == 0:
            self.loca = [struct.unpack('>H', loca[2*i:2*i+2])[0]*2 for i in range(self.num_glyphs+1)]
        else:
            self.loca = [struct.unpack('>I', loca[4*i:4*i+4])[0] for i in range(self.num_glyphs+1)]
        hhea = self.tables['hhea']
        self.num_hmetrics = struct.unpack('>H', hhea[34:36])[0]
        hmtx = self.tables['hmtx']
        self.metrics = []
        for i in range(self.num_glyphs):
            if i < self.num_hmetrics:
                self.metrics.append(struct.unpack('>Hh', hmtx[4*i:4*i+4]))
            else:
                lsb = struct.unpack('>h', hmtx[4*self.num_hmetrics+2*(i-self.num_hmetrics):][:2])[0]
                self.metrics.append((self.metrics[self.num_hmetrics-1][0], lsb))
        self.names = self._post_names()

    def glyph(self, gid):
        return self.tables['glyf'][self.loca[gid]:self.loca[gid+1]]

    def _post_names(self):
        post = self.tables['post']
        fmt = struct.unpack('>I', post[0:4])[0]
        if fmt != 0x20000:
            return None
        n = struct.unpack('>H', post[32:34])[0]
        idx = struct.unpack('>%dH' % n, post[34:34+2*n])
        pos = 34 + 2*n
        strings = []
        while pos < len(post):
            l = post[pos]; strings.append(post[pos+1:pos+1+l].decode('latin1')); pos += 1+l
        names = []
        for i in idx:
            names.append('std%d' % i if i < 258 else strings[i-258])
        return names


# Names as in the Material Symbols glyph table. Append new icons at the end so existing
# code points stay stable.
NAMES = """
play_arrow pause stop skip_next open_with autorenew open_in_full
visibility visibility_off lock lock_open videocam lightbulb deployed_code view_in_ar code account_tree
folder folder_open description image texture volume_up grain text_fields person public category data_object
sports_esports landscape search close add more_horiz more_vert expand_more chevron_right settings tune bug_report pets
delete content_copy save upload download grid_on construction timeline help check warning error info circle memory
inventory_2 layers hub schema movie terminal speed palette animation directions_run route select_all
weight link sensors grid_view dashboard hearing map smart_toy group lan home widgets
radio_button_checked undo redo filter_list arrow_drop_down wb_sunny flare water_drop cloud forest token extension
dock_to_right dock_to_left dock_to_bottom fullscreen center_focus_strong my_location straighten crop_free ads_click
pan_tool restart_alt refresh flag rocket_launch psychology package_2 shapes polyline keyboard schedule edit
""".split()

SRC = "/usr/share/fonts/ttf-material-symbols-variable/MaterialSymbolsRounded[FILL,GRAD,opsz,wght].ttf"
ROOT = __file__.rsplit("/tools/", 1)[0]
OUT_TTF = ROOT + "/apps/editor/assets/fonts/BozzardIcons.ttf"
OUT_RS = ROOT + "/apps/editor/src/icons.rs"
BASE = 0xE900
src = Font(SRC)
index = {n: i for i, n in enumerate(src.names)}
gids = [0] + [index[n] for n in NAMES]  # .notdef first

def pad4(b):
    return b + b'\0' * (-len(b) % 4)

glyf, loca = b'', []
for gid in gids:
    loca.append(len(glyf))
    glyf += pad4(src.glyph(gid))
loca.append(len(glyf))
num = len(gids)

head = bytearray(src.tables['head'])
struct.pack_into('>I', head, 8, 0)          # checkSumAdjustment, fixed below
struct.pack_into('>h', head, 50, 1)         # long loca
hhea = bytearray(src.tables['hhea'])
struct.pack_into('>H', hhea, 34, num)
maxp = bytearray(src.tables['maxp'])
struct.pack_into('>H', maxp, 4, num)
hmtx = b''.join(struct.pack('>Hh', *src.metrics[g]) for g in gids)
os2 = bytearray(src.tables['OS/2'])
struct.pack_into('>HH', os2, 64, BASE, BASE + len(NAMES) - 1)
post = bytearray(src.tables['post'][:32])
struct.pack_into('>I', post, 0, 0x30000)

# cmap format 4: one segment per glyph run plus the 0xFFFF terminator.
codes = [BASE + i for i in range(len(NAMES))]
starts, ends, deltas = [BASE, 0xFFFF], [BASE + len(NAMES) - 1, 0xFFFF], [(1 - BASE) & 0xFFFF, 1]
seg = len(starts)
search = 2 ** (seg.bit_length() - 1)
sub = struct.pack('>HHHHHHH', 4, 0, 0, seg * 2, search * 2, search.bit_length() - 1, seg * 2 - search * 2)
sub += struct.pack('>%dH' % seg, *ends) + b'\0\0' + struct.pack('>%dH' % seg, *starts)
sub += struct.pack('>%dH' % seg, *deltas) + struct.pack('>%dH' % seg, *([0] * seg))
sub = sub[:2] + struct.pack('>H', len(sub)) + sub[4:]
cmap = struct.pack('>HHHHI', 0, 1, 3, 1, 12) + sub

def name_record(name_id, text):
    return name_id, text.encode('utf-16-be')
records = [name_record(1, 'Bozzard Icons'), name_record(2, 'Regular'),
           name_record(4, 'Bozzard Icons'), name_record(6, 'BozzardIcons-Regular'),
           name_record(0, 'Glyphs from Material Symbols Rounded, Copyright Google LLC, Apache License 2.0')]
records.sort()
strings, recs = b'', b''
for nid, data in records:
    recs += struct.pack('>HHHHHH', 3, 1, 0x409, nid, len(data), len(strings))
    strings += data
name = struct.pack('>HHH', 0, len(records), 6 + 12 * len(records)) + recs + strings

tables = {
    'OS/2': bytes(os2), 'cmap': cmap, 'glyf': glyf, 'head': bytes(head), 'hhea': bytes(hhea),
    'hmtx': hmtx, 'loca': struct.pack('>%dI' % len(loca), *loca), 'maxp': bytes(maxp),
    'name': name, 'post': bytes(post),
}

def checksum(b):
    b = pad4(b)
    return sum(struct.unpack('>%dI' % (len(b) // 4), b)) & 0xFFFFFFFF

tags = sorted(tables)
n = len(tags)
search = 2 ** (n.bit_length() - 1)
out = struct.pack('>IHHHH', 0x00010000, n, search * 16, search.bit_length() - 1, n * 16 - search * 16)
offset = 12 + 16 * n
directory, body = b'', b''
for tag in tags:
    data = tables[tag]
    directory += struct.pack('>4sIII', tag.encode(), checksum(data), offset + len(body), len(data))
    body += pad4(data)
font = bytearray(out + directory + body)
adjust = (0xB1B0AFBA - checksum(bytes(font))) & 0xFFFFFFFF
head_offset = offset + sum(len(pad4(tables[t])) for t in tags[:tags.index('head')])
struct.pack_into('>I', font, head_offset + 8, adjust)
open(OUT_TTF, 'wb').write(font)

with open(OUT_RS, 'w') as rs:
    rs.write('//! Editor icons: glyphs from Material Symbols Rounded (Apache License 2.0), subset into\n')
    rs.write('//! `assets/fonts/BozzardIcons.ttf` and mapped from U+E900, which Inter leaves unused.\n')
    rs.write('//! Regenerate both files with `tools/subset_icons.py` after changing the list.\n')
    rs.write('// The palette is wider than what the editor draws today.\n#![allow(dead_code)]\n')
    for i, n in enumerate(NAMES):
        const = n.upper()
        if const[0].isdigit():
            const = 'I' + const
        rs.write(f'pub const {const}: &str = "\\u{{{BASE + i:X}}}";\n')
print(f'{len(NAMES)} icons, {len(font)} bytes')
