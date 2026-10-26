#!/usr/bin/env python3
"""Export the koan brand kit from its SVG masters; no app dependencies."""

import argparse
import copy
import hashlib
import json
from pathlib import Path
import re
import shutil
import struct
import subprocess
import tempfile
import xml.etree.ElementTree as ET


KIT = Path(__file__).resolve().parent
NS = 'http://www.w3.org/2000/svg'
ET.register_namespace('', NS)
SIZES = (16, 24, 32, 48, 64, 128, 256, 512, 1024)
COLORS = json.loads((KIT / 'palette.json').read_text())['colors']
WORDMARK_WIDTH = 491
WORDMARK_HEIGHT = 166
SIGNATURE_WIDTH = 286 + WORDMARK_WIDTH
SIGNATURE_HEIGHT = 256
WORDMARK_Y = (SIGNATURE_HEIGHT - WORDMARK_HEIGHT) / 2


def element(tag, **attributes):
    return ET.Element(f'{{{NS}}}{tag}', {key.replace('_', '-'): str(value) for key, value in attributes.items()})


def canvas(width, height, title):
    root = element('svg', width=width, height=height, viewBox=f'0 0 {width} {height}', role='img', aria_labelledby='asset-title')
    ET.SubElement(root, f'{{{NS}}}title', id='asset-title').text = title
    return root


def save_svg(root, path):
    path.parent.mkdir(parents=True, exist_ok=True)
    ET.indent(root, space='  ')
    path.write_text(ET.tostring(root, encoding='unicode') + '\n')


def artwork(name, prefix, left=None, right=None, ink=None, transform=None):
    source = ET.parse(KIT / 'sources' / f'{name}.svg').getroot()
    group = element('g', **({'transform': transform} if transform else {}))
    for child in source:
        if child.tag.rsplit('}', 1)[-1] in ('title', 'desc'):
            continue
        node = copy.deepcopy(child)
        for item in node.iter():
            identity = item.get('id')
            if identity:
                item.set('id', prefix + '-' + identity)
            if ink and item.tag.endswith('path'):
                item.set('fill', ink)
            elif identity == 'left-leaf' and left:
                item.set('fill', left)
            elif identity == 'right-leaf' and right:
                item.set('fill', right)
        group.append(node)
    return group


def mark(prefix, reverse=False, mono=False, compact=False, transform=None):
    foreground = COLORS['limestone'] if reverse else COLORS['plum']
    return artwork('open-book-compact' if compact else 'open-book', prefix,
                   left=foreground, right=foreground if mono else COLORS['mauve'], transform=transform)


def wordmark(prefix, reverse=False, transform=None):
    return artwork('wordmark', prefix, ink=COLORS['limestone'] if reverse else COLORS['plum'], transform=transform)


def lockup(prefix, reverse=False, mono=False, stacked=False):
    if stacked:
        root = canvas(800, 500, 'koan — stacked signature')
        root.append(mark(prefix + '-mark', reverse, mono, transform='translate(272 16)'))
        root.append(wordmark(prefix + '-name', reverse, transform=f'translate({(800 - WORDMARK_WIDTH) / 2} 296)'))
    else:
        root = canvas(SIGNATURE_WIDTH, SIGNATURE_HEIGHT, 'koan — horizontal signature')
        root.append(mark(prefix + '-mark', reverse, mono))
        root.append(wordmark(prefix + '-name', reverse, transform=f'translate(286 {WORDMARK_Y})'))
    return root


def app_icon(plated=False, compact=False):
    root = canvas(1024, 1024, 'koan app icon')
    root.append(element('rect', width=1024, height=1024, rx=224 if plated else 0, fill=COLORS['plum']))
    root.append(mark('app', reverse=True, compact=compact, transform='translate(160 144) scale(2.875)'))
    return root


def render(svg, destination, width):
    source_width = float(ET.parse(svg).getroot().get('width'))
    destination.parent.mkdir(parents=True, exist_ok=True)
    result = subprocess.run([
        'magick', '-font', str(KIT.parents[1] / 'assets/fonts/Newsreader-Regular.ttf'),
        '-background', 'none', '-density', str(384 * width / source_width),
        'MSVG:' + str(svg), '-resize', str(width),
        '-channel', 'A', '-level', '1%,100%', '+channel', '-strip',
        '-define', 'png:exclude-chunks=date,time', 'PNG32:' + str(destination),
    ], capture_output=True, text=True)
    if result.returncode:
        raise SystemExit(f'Could not render {svg}:\n{result.stderr.strip()}')


def write_ico(pngs, destination):
    payloads = [(size, path.read_bytes()) for size, path in pngs]
    offset = 6 + 16 * len(payloads)
    entries = []
    for size, data in payloads:
        entries.append(struct.pack('<BBBBHHII', size if size < 256 else 0,
                                   size if size < 256 else 0, 0, 0, 1, 32, len(data), offset))
        offset += len(data)
    destination.write_bytes(struct.pack('<HHH', 0, 1, len(payloads)) + b''.join(entries) + b''.join(data for _, data in payloads))


def write_icns(iconset, destination):
    # PNG-backed standard and Retina resources, with an explicit table of contents.
    types = ((16, 1, b'icp4'), (16, 2, b'ic11'), (32, 1, b'icp5'),
             (32, 2, b'ic12'), (128, 1, b'ic07'), (128, 2, b'ic13'),
             (256, 1, b'ic08'), (256, 2, b'ic14'), (512, 1, b'ic09'),
             (512, 2, b'ic10'))
    resources = []
    for logical, scale, kind in types:
        name = f'icon_{logical}x{logical}' + ('@2x' if scale == 2 else '') + '.png'
        payload = (iconset / name).read_bytes()
        resources.append((kind, payload))
    table = b''.join(kind + struct.pack('>I', len(payload) + 8) for kind, payload in resources)
    body = b'TOC ' + struct.pack('>I', len(table) + 8) + table
    body += b''.join(kind + struct.pack('>I', len(payload) + 8) + payload for kind, payload in resources)
    destination.write_bytes(b'icns' + struct.pack('>I', len(body) + 8) + body)


def preview(output):
    root = canvas(1536, 1024, 'koan brand kit — light, reverse, monochrome, and app icons')
    root.append(element('rect', width=1536, height=1024, fill=COLORS['limestone']))
    root.append(element('rect', x=0, y=640, width=600, height=384, fill=COLORS['plum']))
    root.append(element('path', d='M 600 640 H 1536 M 1150 640 V 1024', fill='none', stroke=COLORS['mauve'], stroke_opacity='.18'))
    for label, x, y, fill, size, spacing in [('KOAN · OPEN BOOK', 60, 68, COLORS['mauve'], 22, 4),
                                           ('REVERSE', 52, 699, COLORS['limestone'], 18, 3),
                                           ('ONE COLOR', 644, 699, COLORS['plum'], 18, 3),
                                           ('APP ICON', 1208, 699, COLORS['plum'], 18, 3)]:
        text = element('text', x=x, y=y, fill=fill, font_family='sans-serif', font_size=size, letter_spacing=spacing)
        text.text = label
        root.append(text)
    for prefix, transform, reverse, mono in [('primary', 'translate(144 194) scale(1.6)', False, False),
                                             ('reverse', 'translate(46 776) scale(.65)', True, False),
                                             ('mono', 'translate(639 780) scale(.61)', False, True)]:
        group = element('g', transform=transform)
        for child in list(lockup(prefix, reverse, mono))[1:]:
            group.append(child)
        root.append(group)
    icon = element('g', transform='translate(1246 742) scale(.19)')
    for child in list(app_icon(plated=True))[1:]:
        icon.append(child)
    root.append(icon)
    for x, size in ((1254, 16), (1332, 24), (1418, 32)):
        root.append(mark('small-' + str(size), compact=True, transform=f'translate({x} 963) scale({size / 256})'))
    save_svg(root, output / 'preview.svg')
    render(output / 'preview.svg', output / 'preview.png', 1536)


def native_paths(output):
    """Bake the masters into Iced paths; no runtime SVG parser or font needed."""
    functions = []
    for name in ('open-book', 'open-book-compact', 'wordmark'):
        root = ET.parse(KIT / 'sources' / f'{name}.svg').getroot()
        paths = list(root.iter(f'{{{NS}}}path'))
        groups = [('wordmark', paths)] if name == 'wordmark' else [
            (name.replace('-', '_') + '_' + path.get('id').split('-')[0], [path]) for path in paths]
        for function, members in groups:
            lines = [f'fn {function}() -> Path {{', '    Path::new(|builder| {']
            for path in members:
                transform = path.get('transform', '')
                translate = re.search(r'translate\(([-.\d]+) ([-.\d]+)\)', transform)
                scale = re.search(r'scale\(([-.\d]+) ([-.\d]+)\)', transform)
                dx, dy = map(float, translate.groups()) if translate else (0, 0)
                sx, sy = map(float, scale.groups()) if scale else (1, 1)
                if name == 'wordmark':
                    dx += 294  # Lockup translation (286, WORDMARK_Y) + inset (8, 8).
                    dy += WORDMARK_Y + 8
                def point(x, y):
                    return f'Point::new({x * sx + dx:.3f}, {y * sy + dy:.3f})'
                tokens = re.findall(r'[A-Za-z]|-?\d*\.?\d+', path.get('d'))
                x = y = 0
                while tokens:
                    command = tokens.pop(0)
                    count = {'M': 2, 'L': 2, 'H': 1, 'V': 1, 'C': 6, 'Q': 4, 'Z': 0}[command]
                    args = [float(tokens.pop(0)) for _ in range(count)]
                    if command == 'Z':
                        lines.append('        builder.close();')
                        continue
                    if command == 'H': args = [args[0], y]
                    if command == 'V': args = [x, args[0]]
                    points = [point(*args[i:i+2]) for i in range(0, len(args), 2)]
                    method = {'M': 'move_to', 'L': 'line_to', 'H': 'line_to', 'V': 'line_to',
                              'C': 'bezier_curve_to', 'Q': 'quadratic_curve_to'}[command]
                    lines.append(f'        builder.{method}({", ".join(points)});')
                    x, y = args[-2:]
            lines += ['    })', '}']
            functions.append('\n'.join(lines))
    output.mkdir(parents=True, exist_ok=True)
    (output / 'native-paths.rs').write_text(
        '// Generated by branding/koan/export.py from sources/*.svg. Do not edit.\n'
        + f'const SIGNATURE_WIDTH: f32 = {SIGNATURE_WIDTH}.0;\n'
        + f'const SIGNATURE_HEIGHT: f32 = {SIGNATURE_HEIGHT}.0;\n\n'
        + '\n\n'.join(functions) + '\n')


def generate(output):
    native_paths(output)
    for reverse, mono, suffix in [(False, False, ''), (True, False, '-reverse'),
                                  (False, True, '-ink'), (True, True, '-paper')]:
        root = canvas(256, 256, 'koan open book')
        root.append(mark('symbol', reverse, mono))
        save_svg(root, output / 'logos' / f'open-book{suffix}.svg')
        save_svg(lockup('horizontal', reverse, mono), output / 'logos' / f'lockup-horizontal{suffix}.svg')
    for reverse, suffix in [(False, ''), (True, '-reverse')]:
        root = canvas(WORDMARK_WIDTH, WORDMARK_HEIGHT, 'koan')
        root.append(wordmark('name', reverse))
        save_svg(root, output / 'logos' / f'wordmark{suffix}.svg')
        save_svg(lockup('stacked', reverse, stacked=True), output / 'logos' / f'lockup-stacked{suffix}.svg')
    icons = output / 'icons'
    save_svg(app_icon(), icons / 'koan-square.svg')
    save_svg(app_icon(plated=True), icons / 'koan-plated.svg')
    save_svg(app_icon(compact=True), icons / 'koan-square-compact.svg')
    save_svg(app_icon(plated=True, compact=True), icons / 'koan-plated-compact.svg')
    for size in SIZES:
        suffix = '-compact' if size <= 32 else ''
        render(icons / f'koan-plated{suffix}.svg', icons / 'png' / f'koan-{size}.png', size)
    render(icons / 'koan-square.svg', icons / 'koan-1024.png', 1024)
    write_ico([(size, icons / 'png' / f'koan-{size}.png') for size in SIZES if size <= 256], icons / 'koan.ico')
    iconset = icons / 'koan.iconset'
    for logical in (16, 32, 128, 256, 512):
        for scale in (1, 2):
            size = logical * scale
            suffix = '-compact' if size <= 32 else ''
            name = f'icon_{logical}x{logical}' + ('@2x' if scale == 2 else '') + '.png'
            render(icons / f'koan-square{suffix}.svg', iconset / name, size)
    write_icns(iconset, icons / 'koan.icns')
    preview(output)


def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def derived_files(root):
    return {path.relative_to(root) for path in root.rglob('*') if path.is_file()
            and (path.relative_to(root).parts[0] in ('logos', 'icons') or path.name in ('preview.svg', 'preview.png', 'native-paths.rs'))}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--output', type=Path, default=KIT, help='Destination for derived assets')
    parser.add_argument('--check', action='store_true', help='Regenerate privately and compare every derived asset')
    args = parser.parse_args()
    for command in ('magick',):
        if not shutil.which(command):
            parser.error(f'{command} is required; see README.md for export prerequisites')
    if args.check:
        with tempfile.TemporaryDirectory(prefix='koan-brand-') as temporary:
            fresh = Path(temporary)
            generate(fresh)
            expected, actual = derived_files(fresh), derived_files(args.output)
            changed = [str(path) for path in sorted(expected & actual) if digest(fresh / path) != digest(args.output / path)]
            missing = [str(path) for path in sorted(expected - actual)]
            extra = [str(path) for path in sorted(actual - expected)]
            if changed or missing or extra:
                raise SystemExit(json.dumps({'changed': changed, 'missing': missing, 'extra': extra}, indent=2))
            print(f'Verified {len(expected)} reproducible derived assets.')
    else:
        generate(args.output)
        print(f'Exported {len(derived_files(args.output))} assets to {args.output}.')


if __name__ == '__main__':
    main()
