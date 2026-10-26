#!/usr/bin/env python3
"""Author the wordmark with FontTools 4.61.1 and uharfbuzz 0.57.0; no runtime dependency."""

import hashlib
from io import BytesIO
import json
from pathlib import Path
import xml.etree.ElementTree as ET

from fontTools.pens.basePen import BasePen
from fontTools.pens.boundsPen import BoundsPen
from fontTools.ttLib import TTFont
from fontTools.varLib.instancer import instantiateVariableFont
import uharfbuzz as hb


KIT = Path(__file__).resolve().parent
NS = 'http://www.w3.org/2000/svg'
ET.register_namespace('', NS)


class OutlinePen(BasePen):
    def __init__(self, glyphs, offset, left, top, scale):
        super().__init__(glyphs)
        self.offset, self.left, self.top, self.scale = offset, left, top, scale
        self.commands = []

    def point(self, p):
        return f'{(p[0] + self.offset - self.left) * self.scale + 8:.3f} {(self.top - p[1]) * self.scale + 8:.3f}'

    def _moveTo(self, p):
        self.commands.append('M ' + self.point(p))

    def _lineTo(self, p):
        self.commands.append('L ' + self.point(p))

    def _curveToOne(self, p1, p2, p3):
        self.commands.append('C ' + ' '.join(map(self.point, (p1, p2, p3))))

    def _qCurveToOne(self, p1, p2):
        self.commands.append('Q ' + ' '.join(map(self.point, (p1, p2))))

    def _closePath(self):
        self.commands.append('Z')


def main():
    metadata = json.loads((KIT / 'sources/wordmark-font.json').read_text())
    path = KIT / 'sources' / metadata['file']
    assert hashlib.sha256(path.read_bytes()).hexdigest() == metadata['sha256']
    font = instantiateVariableFont(TTFont(path), metadata['axes'])
    glyphs = font.getGlyphSet()
    # Shape the fixed instance so its kerning and outline coordinates agree.
    buffer = BytesIO()
    font.save(buffer)
    shaping_font = hb.Font(hb.Face(buffer.getvalue()))
    units = font['head'].unitsPerEm
    shaping_font.scale = (units, units)
    text = hb.Buffer()
    text.add_str('dosty')
    text.guess_segment_properties()
    hb.shape(shaping_font, text, {'kern': True})
    letters = []
    offset = 0
    for info, position in zip(text.glyph_infos, text.glyph_positions):
        assert position.y_offset == position.y_advance == 0
        glyph = glyphs[font.getGlyphName(info.codepoint)]
        pen = BoundsPen(glyphs)
        glyph.draw(pen)
        letters.append((info.cluster, glyph, offset + position.x_offset, pen.bounds))
        offset += position.x_advance
    left = min(x + b[0] for _, _, x, b in letters)
    right = max(x + b[2] for _, _, x, b in letters)
    bottom = min(b[1] for _, _, _, b in letters)
    top = max(b[3] for _, _, _, b in letters)
    scale = 150 / (top - bottom)
    width = round((right - left) * scale + 16)
    root = ET.Element(f'{{{NS}}}svg', width=str(width), height='166', viewBox=f'0 0 {width} 166', role='img')
    ET.SubElement(root, f'{{{NS}}}title').text = 'dosty'
    ET.SubElement(root, f'{{{NS}}}desc').text = 'DM Sans Semibold, optical size 24, weight 600. OpenType kerning shaped with HarfBuzz; nonzero font contours. No runtime font dependency.'
    for letter, glyph, offset, _ in letters:
        pen = OutlinePen(glyphs, offset, left, top, scale)
        glyph.draw(pen)
        ET.SubElement(root, f'{{{NS}}}path', id='glyph-' + str(letter), fill='#20231F',
                      **{'fill-rule': 'nonzero', 'd': ' '.join(pen.commands)})
    ET.indent(root, space='  ')
    (KIT / 'sources/wordmark.svg').write_text(ET.tostring(root, encoding='unicode') + '\n')
    print(f'Outlined Dosty wordmark: {width} × 166')


if __name__ == '__main__':
    main()
