#!/usr/bin/env python3
"""Generate Planetfall's original low-poly glTF (GLB) models.

Every model is built from simple convex solids (boxes, prisms, cones and
bipyramids) with flat normals.

* Props and vehicles keep per-part named nodes whose translation is the
  part's pivot, so the game can turn doors, rotors and holograms. Their
  materials use only glTF base colour, metallic/roughness and emissive
  factors, which Quasar's classic material conversion supports.
* Characters are skinned: one joint per part (rigid weights), a lit body
  primitive and an unlit glow primitive that sample a shared colour-palette
  texture, and authored animation clips (`idle`, `walk`, `attack`, `aim`,
  `pulse`) sampled into quaternion keyframes. Walk clips are authored in
  walk-distance units, so the game plays them by distance travelled.

Output is deterministic: running the script twice produces identical bytes.

Usage:
    python3 DevUtils/assets/generate_models.py [--out assets/models]
"""

from __future__ import annotations

import argparse
import json
import math
import struct
import zlib
from pathlib import Path

REPO = Path(__file__).resolve().parents[2]

# name: (rgba, metallic, roughness, emissive)
MATERIALS: dict[str, tuple[tuple[float, float, float, float], float, float, tuple[float, float, float]]] = {
    "suit": ((0.86, 0.87, 0.84, 1), 0.0, 0.55, (0, 0, 0)),
    "suit_dark": ((0.22, 0.24, 0.27, 1), 0.2, 0.6, (0, 0, 0)),
    "suit_orange": ((0.95, 0.47, 0.12, 1), 0.0, 0.5, (0, 0, 0)),
    "visor": ((0.05, 0.12, 0.2, 1), 0.6, 0.15, (0.05, 0.35, 0.55)),
    "glow_cyan": ((0.3, 0.9, 1.0, 1), 0.0, 0.4, (0.25, 0.85, 1.0)),
    "glow_orange": ((1.0, 0.55, 0.15, 1), 0.0, 0.4, (1.0, 0.45, 0.1)),
    "glow_red": ((1.0, 0.2, 0.1, 1), 0.0, 0.4, (1.0, 0.15, 0.05)),
    "glow_yellow": ((1.0, 0.9, 0.3, 1), 0.0, 0.4, (0.95, 0.8, 0.2)),
    "glow_green": ((0.4, 1.0, 0.5, 1), 0.0, 0.4, (0.3, 1.0, 0.45)),
    "glow_purple": ((0.75, 0.35, 1.0, 1), 0.0, 0.3, (0.6, 0.2, 1.0)),
    "glow_blue": ((0.35, 0.55, 1.0, 1), 0.0, 0.3, (0.25, 0.45, 1.0)),
    "husk_skin": ((0.36, 0.52, 0.30, 1), 0.0, 0.85, (0, 0, 0)),
    "husk_suit": ((0.46, 0.27, 0.16, 1), 0.0, 0.9, (0, 0, 0)),
    "husk_pants": ((0.20, 0.20, 0.25, 1), 0.0, 0.9, (0, 0, 0)),
    "chitin": ((0.24, 0.13, 0.30, 1), 0.3, 0.35, (0, 0, 0)),
    "chitin_light": ((0.45, 0.28, 0.52, 1), 0.3, 0.35, (0, 0, 0)),
    "bloat": ((0.62, 0.66, 0.28, 1), 0.0, 0.6, (0, 0, 0)),
    "bloat_dark": ((0.38, 0.40, 0.16, 1), 0.0, 0.7, (0, 0, 0)),
    "brute_skin": ((0.25, 0.36, 0.22, 1), 0.0, 0.85, (0, 0, 0)),
    "armor": ((0.42, 0.44, 0.46, 1), 0.8, 0.35, (0, 0, 0)),
    "hull": ((0.78, 0.80, 0.82, 1), 0.5, 0.35, (0, 0, 0)),
    "hull_dark": ((0.28, 0.30, 0.33, 1), 0.6, 0.4, (0, 0, 0)),
    "accent": ((0.95, 0.47, 0.12, 1), 0.3, 0.4, (0, 0, 0)),
    "rubber": ((0.08, 0.08, 0.09, 1), 0.0, 0.9, (0, 0, 0)),
    "ferrite": ((0.72, 0.42, 0.22, 1), 0.85, 0.3, (0.06, 0.02, 0.0)),
    "lumite": ((0.35, 0.95, 1.0, 1), 0.0, 0.2, (0.2, 0.75, 0.9)),
    "voidstone": ((0.55, 0.25, 0.85, 1), 0.2, 0.2, (0.45, 0.12, 0.75)),
    "white": ((1.0, 1.0, 1.0, 1), 0.0, 0.7, (0, 0, 0)),
    "screen": ((0.1, 0.3, 0.45, 1), 0.0, 0.2, (0.15, 0.55, 0.85)),
    "ice": ((0.72, 0.86, 0.96, 1), 0.1, 0.12, (0.04, 0.08, 0.12)),
    "obsidian": ((0.07, 0.06, 0.07, 1), 0.3, 0.15, (0, 0, 0)),
    "stalk": ((0.62, 0.58, 0.46, 1), 0.0, 0.8, (0, 0, 0)),
}


def rot_matrix(rx: float = 0.0, ry: float = 0.0, rz: float = 0.0):
    cx, sx = math.cos(rx), math.sin(rx)
    cy, sy = math.cos(ry), math.sin(ry)
    cz, sz = math.cos(rz), math.sin(rz)
    # R = Ry * Rx * Rz
    rxm = ((1, 0, 0), (0, cx, -sx), (0, sx, cx))
    rym = ((cy, 0, sy), (0, 1, 0), (-sy, 0, cy))
    rzm = ((cz, -sz, 0), (sz, cz, 0), (0, 0, 1))

    def mul(a, b):
        return tuple(tuple(sum(a[i][k] * b[k][j] for k in range(3)) for j in range(3)) for i in range(3))

    return mul(rym, mul(rxm, rzm))


def apply(m, v):
    return tuple(sum(m[i][k] * v[k] for k in range(3)) for i in range(3))


def sub(a, b):
    return (a[0] - b[0], a[1] - b[1], a[2] - b[2])


def cross(a, b):
    return (a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0])


def dot(a, b):
    return a[0] * b[0] + a[1] * b[1] + a[2] * b[2]


def normalize(v):
    length = math.sqrt(dot(v, v)) or 1.0
    return (v[0] / length, v[1] / length, v[2] / length)


class Part:
    """One named node. Geometry is given in model space; the pivot becomes the
    node translation and vertices are stored relative to it."""

    def __init__(self, name: str, pivot=(0.0, 0.0, 0.0)):
        self.name = name
        self.pivot = tuple(float(p) for p in pivot)
        self.tris: dict[str, list[tuple]] = {}

    def _solid(self, material: str, triangles: list[tuple], center):
        out = self.tris.setdefault(material, [])
        for a, b, c in triangles:
            n = cross(sub(b, a), sub(c, a))
            centroid = tuple((a[i] + b[i] + c[i]) / 3 for i in range(3))
            if dot(n, sub(centroid, center)) < 0:
                b, c = c, b
            out.append((a, b, c))

    def box(self, material, center, size, rot=(0.0, 0.0, 0.0)):
        hx, hy, hz = (s / 2 for s in size)
        m = rot_matrix(*rot)
        corners = []
        for x in (-hx, hx):
            for y in (-hy, hy):
                for z in (-hz, hz):
                    p = apply(m, (x, y, z))
                    corners.append((p[0] + center[0], p[1] + center[1], p[2] + center[2]))

        def c(ix, iy, iz):
            return corners[ix * 4 + iy * 2 + iz]

        quads = [
            (c(1, 0, 0), c(1, 1, 0), c(1, 1, 1), c(1, 0, 1)),
            (c(0, 0, 0), c(0, 0, 1), c(0, 1, 1), c(0, 1, 0)),
            (c(0, 1, 0), c(0, 1, 1), c(1, 1, 1), c(1, 1, 0)),
            (c(0, 0, 0), c(1, 0, 0), c(1, 0, 1), c(0, 0, 1)),
            (c(0, 0, 1), c(1, 0, 1), c(1, 1, 1), c(0, 1, 1)),
            (c(0, 0, 0), c(0, 1, 0), c(1, 1, 0), c(1, 0, 0)),
        ]
        tris = []
        for q in quads:
            tris += [(q[0], q[1], q[2]), (q[0], q[2], q[3])]
        self._solid(material, tris, center)

    def prism(self, material, center, radius, height, sides=8, rot=(0.0, 0.0, 0.0), top_radius=None):
        """Vertical n-gon prism / frustum; top_radius=0 gives a cone."""
        top = radius if top_radius is None else top_radius
        m = rot_matrix(*rot)

        def pt(r, angle, y):
            p = apply(m, (r * math.cos(angle), y, r * math.sin(angle)))
            return (p[0] + center[0], p[1] + center[1], p[2] + center[2])

        h = height / 2
        tris = []
        top_c = pt(0, 0, h)
        bot_c = pt(0, 0, -h)
        for i in range(sides):
            a0 = 2 * math.pi * i / sides + math.pi / sides
            a1 = 2 * math.pi * (i + 1) / sides + math.pi / sides
            b0, b1 = pt(radius, a0, -h), pt(radius, a1, -h)
            t0, t1 = pt(top, a0, h), pt(top, a1, h)
            tris.append((bot_c, b1, b0))
            if top > 0:
                tris.append((top_c, t0, t1))
                tris += [(b0, b1, t1), (b0, t1, t0)]
            else:
                tris.append((b0, b1, top_c))
        self._solid(material, tris, center)

    def crystal(self, material, center, radius, height, sides=5, rot=(0.0, 0.0, 0.0)):
        """Elongated bipyramid crystal centred on `center`."""
        m = rot_matrix(*rot)

        def pt(v):
            p = apply(m, v)
            return (p[0] + center[0], p[1] + center[1], p[2] + center[2])

        top = pt((0, height * 0.62, 0))
        bot = pt((0, -height * 0.38, 0))
        ring = [pt((radius * math.cos(2 * math.pi * i / sides), 0, radius * math.sin(2 * math.pi * i / sides))) for i in range(sides)]
        tris = []
        for i in range(sides):
            a, b = ring[i], ring[(i + 1) % sides]
            tris += [(a, b, top), (b, a, bot)]
        self._solid(material, tris, center)

    def sphere(self, material, center, radius, subdivisions=2):
        t = (1 + math.sqrt(5)) / 2
        verts = [normalize(v) for v in [(-1, t, 0), (1, t, 0), (-1, -t, 0), (1, -t, 0), (0, -1, t), (0, 1, t), (0, -1, -t), (0, 1, -t), (t, 0, -1), (t, 0, 1), (-t, 0, -1), (-t, 0, 1)]]
        faces = [(0, 11, 5), (0, 5, 1), (0, 1, 7), (0, 7, 10), (0, 10, 11), (1, 5, 9), (5, 11, 4), (11, 10, 2), (10, 7, 6), (7, 1, 8), (3, 9, 4), (3, 4, 2), (3, 2, 6), (3, 6, 8), (3, 8, 9), (4, 9, 5), (2, 4, 11), (6, 2, 10), (8, 6, 7), (9, 8, 1)]
        tris = [(verts[a], verts[b], verts[c]) for a, b, c in faces]
        for _ in range(subdivisions):
            nxt = []
            for a, b, c in tris:
                ab = normalize(tuple((a[i] + b[i]) / 2 for i in range(3)))
                bc = normalize(tuple((b[i] + c[i]) / 2 for i in range(3)))
                ca = normalize(tuple((c[i] + a[i]) / 2 for i in range(3)))
                nxt += [(a, ab, ca), (b, bc, ab), (c, ca, bc), (ab, bc, ca)]
            tris = nxt
        scaled = [tuple(tuple(center[i] + v[i] * radius for i in range(3)) for v in tri) for tri in tris]
        self._solid(material, scaled, center)


def write_glb(path: Path, parts: list[Part]) -> None:
    used = sorted({m for p in parts for m in p.tris})
    mat_index = {m: i for i, m in enumerate(used)}
    binary = bytearray()
    buffer_views, accessors, meshes, nodes = [], [], [], []

    def add_view(data: bytes, target: int) -> int:
        while len(binary) % 4:
            binary.append(0)
        buffer_views.append({"buffer": 0, "byteOffset": len(binary), "byteLength": len(data), "target": target})
        binary.extend(data)
        return len(buffer_views) - 1

    for part in parts:
        primitives = []
        for material in sorted(part.tris):
            positions, normals, uvs, indices = [], [], [], []
            for tri in part.tris[material]:
                rel = [sub(v, part.pivot) for v in tri]
                n = normalize(cross(sub(rel[1], rel[0]), sub(rel[2], rel[0])))
                for v in rel:
                    indices.append(len(positions))
                    positions.append(v)
                    normals.append(n)
                    uvs.append((0.0, 0.0))
            pos_bytes = b"".join(struct.pack("<3f", *p) for p in positions)
            nrm_bytes = b"".join(struct.pack("<3f", *n) for n in normals)
            uv_bytes = b"".join(struct.pack("<2f", *uv) for uv in uvs)
            idx_bytes = b"".join(struct.pack("<I", i) for i in indices)
            lo = [round(min(p[i] for p in positions), 6) for i in range(3)]
            hi = [round(max(p[i] for p in positions), 6) for i in range(3)]
            accessors.append({"bufferView": add_view(pos_bytes, 34962), "componentType": 5126, "count": len(positions), "type": "VEC3", "min": lo, "max": hi})
            pos_acc = len(accessors) - 1
            accessors.append({"bufferView": add_view(nrm_bytes, 34962), "componentType": 5126, "count": len(normals), "type": "VEC3"})
            nrm_acc = len(accessors) - 1
            accessors.append({"bufferView": add_view(uv_bytes, 34962), "componentType": 5126, "count": len(uvs), "type": "VEC2"})
            uv_acc = len(accessors) - 1
            accessors.append({"bufferView": add_view(idx_bytes, 34963), "componentType": 5125, "count": len(indices), "type": "SCALAR"})
            idx_acc = len(accessors) - 1
            primitives.append({"attributes": {"POSITION": pos_acc, "NORMAL": nrm_acc, "TEXCOORD_0": uv_acc}, "indices": idx_acc, "material": mat_index[material], "mode": 4})
        meshes.append({"name": part.name, "primitives": primitives})
        node = {"name": part.name, "mesh": len(meshes) - 1}
        if any(part.pivot):
            node["translation"] = [round(v, 6) for v in part.pivot]
        nodes.append(node)

    materials = []
    for name in used:
        rgba, metallic, roughness, emissive = MATERIALS[name]
        material = {"name": name, "pbrMetallicRoughness": {"baseColorFactor": list(rgba), "metallicFactor": metallic, "roughnessFactor": roughness}}
        if any(emissive):
            material["emissiveFactor"] = list(emissive)
        materials.append(material)

    while len(binary) % 4:
        binary.append(0)
    document = {
        "asset": {"version": "2.0", "generator": "Planetfall DevUtils/assets/generate_models.py"},
        "scene": 0,
        "scenes": [{"name": path.stem, "nodes": list(range(len(nodes)))}],
        "nodes": nodes,
        "meshes": meshes,
        "materials": materials,
        "accessors": accessors,
        "bufferViews": buffer_views,
        "buffers": [{"byteLength": len(binary)}],
    }
    json_bytes = json.dumps(document, separators=(",", ":"), sort_keys=True).encode()
    while len(json_bytes) % 4:
        json_bytes += b" "
    total = 12 + 8 + len(json_bytes) + 8 + len(binary)
    out = struct.pack("<III", 0x46546C67, 2, total)
    out += struct.pack("<II", len(json_bytes), 0x4E4F534A) + json_bytes
    out += struct.pack("<II", len(binary), 0x004E4942) + bytes(binary)
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_bytes(out)


# ---------------------------------------------------------------------------
# Skinned characters: palette texture, joints, clips
# ---------------------------------------------------------------------------

PALETTE_SIZE = 8
PALETTE = sorted(MATERIALS)  # lit colour at index i, glow colour at 32 + i
assert len(PALETTE) <= 32


def srgb_byte(v: float) -> int:
    v = max(0.0, min(1.0, v))
    s = v * 12.92 if v <= 0.0031308 else 1.055 * v ** (1 / 2.4) - 0.055
    return int(round(s * 255))


def palette_uv(slot: int) -> tuple[float, float]:
    return ((slot % PALETTE_SIZE + 0.5) / PALETTE_SIZE, (slot // PALETTE_SIZE + 0.5) / PALETTE_SIZE)


def palette_png() -> bytes:
    """RGBA palette: base colours, then emissive colours for glow parts."""
    texels = [(0, 0, 0, 255)] * (PALETTE_SIZE * PALETTE_SIZE)
    for i, name in enumerate(PALETTE):
        rgba, _, _, emissive = MATERIALS[name]
        texels[i] = tuple(srgb_byte(c) for c in rgba[:3]) + (255,)
        glow = emissive if any(emissive) else rgba[:3]
        peak = max(glow) or 1.0
        texels[32 + i] = tuple(srgb_byte(c / peak) for c in glow) + (255,)
    rows = b"".join(
        b"\x00" + bytes(c for x in range(PALETTE_SIZE) for c in texels[y * PALETTE_SIZE + x])
        for y in range(PALETTE_SIZE)
    )

    def chunk(kind: bytes, data: bytes) -> bytes:
        return struct.pack(">I", len(data)) + kind + data + struct.pack(">I", zlib.crc32(kind + data) & 0xFFFFFFFF)

    header = struct.pack(">IIBBBBB", PALETTE_SIZE, PALETTE_SIZE, 8, 6, 0, 0, 0)
    return b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", header) + chunk(b"IDAT", zlib.compress(rows, 9)) + chunk(b"IEND", b"")


def quat_axis(axis: int, angle: float) -> tuple[float, float, float, float]:
    s, c = math.sin(angle / 2), math.cos(angle / 2)
    q = [0.0, 0.0, 0.0, c]
    q[axis] = s
    return tuple(q)


def qmul(a, b):
    ax, ay, az, aw = a
    bx, by, bz, bw = b
    return (
        aw * bx + ax * bw + ay * bz - az * by,
        aw * by - ax * bz + ay * bw + az * bx,
        aw * bz + ax * by - ay * bx + az * bw,
        aw * bw - ax * bx - ay * by - az * bz,
    )


def qx(a):
    return quat_axis(0, a)


def qy(a):
    return quat_axis(1, a)


def qz(a):
    return quat_axis(2, a)


class Clip:
    """A clip sampled from `pose(u)` for u in [0, 1). `pose` returns
    {joint: {"rotation": quat, "translation": offset from rest, "scale": vec3}}.
    Looping clips repeat their first key at the end; pose clips hold one key."""

    def __init__(self, duration: float, pose, keys: int = 24, loop: bool = True):
        self.duration = duration
        self.pose = pose
        self.keys = keys if loop else 1
        self.loop = loop

    def samples(self):
        if not self.loop:
            frame = self.pose(0.0)
            return [0.0, self.duration], [frame, frame]
        times = [self.duration * k / self.keys for k in range(self.keys + 1)]
        return times, [self.pose((k % self.keys) / self.keys) for k in range(self.keys + 1)]


def write_skinned_glb(path: Path, parts: list[Part], parents: dict[str, str | None], clips: dict[str, Clip]) -> None:
    joints = ["root"] + [p.name for p in parts]
    pivots = {"root": (0.0, 0.0, 0.0)} | {p.name: p.pivot for p in parts}
    parent_of = {"root": None} | {p.name: parents.get(p.name, "root") for p in parts}
    joint_index = {name: i for i, name in enumerate(joints)}
    binary = bytearray()
    buffer_views, accessors = [], []

    def add_view(data: bytes, target: int | None = None) -> int:
        while len(binary) % 4:
            binary.append(0)
        view = {"buffer": 0, "byteOffset": len(binary), "byteLength": len(data)}
        if target is not None:
            view["target"] = target
        buffer_views.append(view)
        binary.extend(data)
        return len(buffer_views) - 1

    def add_accessor(data: bytes, component: int, count: int, kind: str, target=None, **extra) -> int:
        accessors.append({"bufferView": add_view(data, target), "componentType": component, "count": count, "type": kind, **extra})
        return len(accessors) - 1

    # Two primitives: lit body and unlit glow, both sampling the palette.
    groups = {"body": [], "glow": []}
    for part in parts:
        for material in sorted(part.tris):
            glow = any(MATERIALS[material][3]) and material.startswith(("glow", "visor", "screen"))
            slot = PALETTE.index(material) + (32 if glow else 0)
            for tri in part.tris[material]:
                groups["glow" if glow else "body"].append((tri, joint_index[part.name], slot))
    primitives = []
    for group_index, group in enumerate(("body", "glow")):
        if not groups[group]:
            continue
        positions, normals, uvs, joint_ids, indices = [], [], [], [], []
        for tri, joint, slot in groups[group]:
            n = normalize(cross(sub(tri[1], tri[0]), sub(tri[2], tri[0])))
            for v in tri:
                indices.append(len(positions))
                positions.append(v)
                normals.append(n)
                uvs.append(palette_uv(slot))
                joint_ids.append(joint)
        lo = [round(min(p[i] for p in positions), 6) for i in range(3)]
        hi = [round(max(p[i] for p in positions), 6) for i in range(3)]
        attributes = {
            "POSITION": add_accessor(b"".join(struct.pack("<3f", *v) for v in positions), 5126, len(positions), "VEC3", 34962, min=lo, max=hi),
            "NORMAL": add_accessor(b"".join(struct.pack("<3f", *v) for v in normals), 5126, len(normals), "VEC3", 34962),
            "TEXCOORD_0": add_accessor(b"".join(struct.pack("<2f", *v) for v in uvs), 5126, len(uvs), "VEC2", 34962),
            "JOINTS_0": add_accessor(b"".join(struct.pack("<4B", j, 0, 0, 0) for j in joint_ids), 5121, len(joint_ids), "VEC4", 34962),
            "WEIGHTS_0": add_accessor(b"".join(struct.pack("<4f", 1.0, 0.0, 0.0, 0.0) for _ in joint_ids), 5126, len(joint_ids), "VEC4", 34962),
        }
        index_acc = add_accessor(b"".join(struct.pack("<I", i) for i in indices), 5125, len(indices), "SCALAR", 34963)
        primitives.append({"attributes": attributes, "indices": index_acc, "material": group_index, "mode": 4})

    nodes = []
    for name in joints:
        parent = parent_of[name]
        rel = sub(pivots[name], pivots[parent]) if parent else pivots[name]
        node = {"name": name}
        if any(rel):
            node["translation"] = [round(v, 6) for v in rel]
        children = [joint_index[c] for c in joints if parent_of[c] == name]
        if children:
            node["children"] = children
        nodes.append(node)
    mesh_node = len(nodes)
    nodes.append({"name": "mesh", "mesh": 0, "skin": 0})

    inverse_bind = b""
    for name in joints:
        x, y, z = pivots[name]
        inverse_bind += struct.pack("<16f", 1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1, 0, -x, -y, -z, 1)
    ibm = add_accessor(inverse_bind, 5126, len(joints), "MAT4")

    animations = []
    for clip_name in sorted(clips):
        clip = clips[clip_name]
        times, frames = clip.samples()
        time_acc = add_accessor(b"".join(struct.pack("<f", t) for t in times), 5126, len(times), "SCALAR", min=[times[0]], max=[times[-1]])
        channels, samplers = [], []
        animated = sorted({(j, prop) for frame in frames for j, props in frame.items() for prop in props})
        for joint, prop in animated:
            rest = pivots[joint] if parent_of[joint] is None else sub(pivots[joint], pivots[parent_of[joint]])
            values = []
            for frame in frames:
                v = frame.get(joint, {}).get(prop)
                if prop == "rotation":
                    values.append(v or (0.0, 0.0, 0.0, 1.0))
                elif prop == "translation":
                    off = v or (0.0, 0.0, 0.0)
                    values.append(tuple(rest[i] + off[i] for i in range(3)))
                else:
                    values.append(v or (1.0, 1.0, 1.0))
            kind, fmt = ("VEC4", "<4f") if prop == "rotation" else ("VEC3", "<3f")
            out_acc = add_accessor(b"".join(struct.pack(fmt, *[round(c, 6) for c in v]) for v in values), 5126, len(values), kind)
            samplers.append({"input": time_acc, "output": out_acc, "interpolation": "LINEAR"})
            channels.append({"sampler": len(samplers) - 1, "target": {"node": joint_index[joint], "path": prop}})
        animations.append({"name": clip_name, "channels": channels, "samplers": samplers})

    image_view = add_view(palette_png())
    while len(binary) % 4:
        binary.append(0)
    document = {
        "asset": {"version": "2.0", "generator": "Planetfall DevUtils/assets/generate_models.py"},
        "extensionsUsed": ["KHR_materials_unlit"],
        "scene": 0,
        "scenes": [{"name": path.stem, "nodes": [0, mesh_node]}],
        "nodes": nodes,
        "meshes": [{"name": path.stem, "primitives": primitives}],
        "skins": [{"joints": list(range(len(joints))), "inverseBindMatrices": ibm, "skeleton": 0}],
        "animations": animations,
        "materials": [
            {"name": "body", "pbrMetallicRoughness": {"baseColorTexture": {"index": 0}, "metallicFactor": 0.1, "roughnessFactor": 0.6}},
            {"name": "glow", "pbrMetallicRoughness": {"baseColorTexture": {"index": 0}}, "extensions": {"KHR_materials_unlit": {}}},
        ],
        "textures": [{"source": 0, "sampler": 0}],
        "samplers": [{"magFilter": 9728, "minFilter": 9728, "wrapS": 33071, "wrapT": 33071}],
        "images": [{"bufferView": image_view, "mimeType": "image/png"}],
        "accessors": accessors,
        "bufferViews": buffer_views,
        "buffers": [{"byteLength": len(binary)}],
    }
    json_bytes = json.dumps(document, separators=(",", ":"), sort_keys=True).encode()
    while len(json_bytes) % 4:
        json_bytes += b" "
    total = 12 + 8 + len(json_bytes) + 8 + len(binary)
    out = struct.pack("<III", 0x46546C67, 2, total)
    out += struct.pack("<II", len(json_bytes), 0x4E4F534A) + json_bytes
    out += struct.pack("<II", len(binary), 0x004E4942) + bytes(binary)
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_bytes(out)


# ---------------------------------------------------------------------------
# Characters (all face +Z, feet at y = 0)
# ---------------------------------------------------------------------------


def astronaut() -> list[Part]:
    parts = []
    for side, name in ((-1, "leg_l"), (1, "leg_r")):
        leg = Part(name, (side * 0.14, 0.75, 0))
        leg.box("suit", (side * 0.14, 0.42, 0), (0.22, 0.66, 0.26))
        leg.box("suit_dark", (side * 0.14, 0.07, 0.04), (0.25, 0.14, 0.34))
        leg.box("suit_orange", (side * 0.14, 0.45, 0.135), (0.16, 0.12, 0.02))
        parts.append(leg)
    torso = Part("torso", (0, 0.75, 0))
    torso.box("suit", (0, 1.09, 0), (0.56, 0.62, 0.34))
    torso.box("suit_dark", (0, 0.8, 0), (0.6, 0.12, 0.37))
    torso.box("suit_orange", (0, 1.16, 0.18), (0.4, 0.28, 0.04))
    torso.box("glow_cyan", (0.12, 1.2, 0.205), (0.08, 0.06, 0.02))
    torso.box("suit_dark", (0, 1.1, -0.27), (0.46, 0.56, 0.2))
    torso.prism("hull_dark", (-0.13, 1.12, -0.38), 0.07, 0.48, sides=8)
    torso.prism("hull_dark", (0.13, 1.12, -0.38), 0.07, 0.48, sides=8)
    torso.box("glow_cyan", (0, 1.32, -0.375), (0.12, 0.05, 0.02))
    parts.append(torso)
    head = Part("head", (0, 1.4, 0))
    head.box("suit", (0, 1.62, 0), (0.44, 0.42, 0.44))
    head.box("visor", (0, 1.63, 0.21), (0.36, 0.22, 0.04))
    head.box("suit_dark", (0.24, 1.62, 0), (0.06, 0.2, 0.2))
    head.box("suit_dark", (-0.24, 1.62, 0), (0.06, 0.2, 0.2))
    head.prism("suit_dark", (-0.16, 1.92, -0.1), 0.02, 0.2, sides=6)
    head.box("glow_red", (-0.16, 2.03, -0.1), (0.05, 0.05, 0.05))
    parts.append(head)
    for side, name in ((-1, "arm_l"), (1, "arm_r")):
        arm = Part(name, (side * 0.37, 1.34, 0))
        arm.box("suit", (side * 0.37, 1.07, 0), (0.18, 0.58, 0.2))
        arm.box("suit_orange", (side * 0.37, 0.74, 0), (0.2, 0.12, 0.22))
        arm.box("suit_dark", (side * 0.37, 1.3, 0), (0.2, 0.14, 0.22))
        parts.append(arm)
    return parts


def husk() -> list[Part]:
    parts = []
    for side, name in ((-1, "leg_l"), (1, "leg_r")):
        leg = Part(name, (side * 0.13, 0.78, 0))
        leg.box("husk_pants", (side * 0.13, 0.39, 0), (0.25, 0.78, 0.25))
        leg.box("husk_skin", (side * 0.13, 0.04, 0.02), (0.26, 0.08, 0.28))
        parts.append(leg)
    torso = Part("torso", (0, 0.78, 0))
    torso.box("husk_suit", (0, 1.15, 0), (0.52, 0.74, 0.26))
    torso.box("husk_skin", (0.1, 1.0, 0.135), (0.18, 0.2, 0.02))
    torso.box("husk_pants", (0, 0.83, 0), (0.54, 0.08, 0.28))
    parts.append(torso)
    head = Part("head", (0, 1.52, 0))
    head.box("husk_skin", (0, 1.77, 0), (0.5, 0.5, 0.5))
    head.box("glow_yellow", (-0.11, 1.8, 0.255), (0.1, 0.07, 0.02))
    head.box("glow_yellow", (0.11, 1.8, 0.255), (0.1, 0.07, 0.02))
    head.box("rubber", (0, 1.63, 0.255), (0.22, 0.06, 0.02))
    head.box("husk_pants", (0.12, 2.03, -0.05), (0.2, 0.04, 0.3))
    parts.append(head)
    for side, name in ((-1, "arm_l"), (1, "arm_r")):
        arm = Part(name, (side * 0.38, 1.46, 0))
        arm.box("husk_suit", (side * 0.38, 1.3, 0), (0.24, 0.32, 0.25))
        arm.box("husk_skin", (side * 0.38, 0.98, 0), (0.22, 0.4, 0.23))
        parts.append(arm)
    return parts


def brute() -> list[Part]:
    parts = []
    for side, name in ((-1, "leg_l"), (1, "leg_r")):
        leg = Part(name, (side * 0.28, 1.05, 0))
        leg.box("husk_pants", (side * 0.28, 0.55, 0), (0.42, 1.0, 0.42))
        leg.box("armor", (side * 0.28, 0.6, 0.22), (0.34, 0.36, 0.06))
        leg.box("brute_skin", (side * 0.28, 0.06, 0.04), (0.44, 0.12, 0.5))
        parts.append(leg)
    torso = Part("torso", (0, 1.05, 0))
    torso.box("brute_skin", (0, 1.65, 0), (1.15, 1.1, 0.62))
    torso.box("armor", (0, 1.75, 0.33), (0.9, 0.6, 0.06))
    torso.box("armor", (-0.62, 2.12, 0), (0.42, 0.18, 0.62))
    torso.box("armor", (0.62, 2.12, 0), (0.42, 0.18, 0.62))
    torso.box("glow_orange", (0, 1.62, 0.37), (0.16, 0.16, 0.02))
    parts.append(torso)
    head = Part("head", (0, 2.2, 0.1))
    head.box("brute_skin", (0, 2.42, 0.12), (0.56, 0.48, 0.52))
    head.box("glow_orange", (-0.13, 2.47, 0.385), (0.12, 0.07, 0.02))
    head.box("glow_orange", (0.13, 2.47, 0.385), (0.12, 0.07, 0.02))
    head.prism("armor", (-0.3, 2.72, 0.05), 0.07, 0.34, sides=5, rot=(0, 0, 0.5), top_radius=0)
    head.prism("armor", (0.3, 2.72, 0.05), 0.07, 0.34, sides=5, rot=(0, 0, -0.5), top_radius=0)
    parts.append(head)
    for side, name in ((-1, "arm_l"), (1, "arm_r")):
        arm = Part(name, (side * 0.78, 2.0, 0))
        arm.box("brute_skin", (side * 0.8, 1.55, 0), (0.38, 0.85, 0.4))
        arm.box("armor", (side * 0.8, 1.05, 0), (0.5, 0.36, 0.5))
        parts.append(arm)
    return parts


def skitter() -> list[Part]:
    parts = []
    body = Part("body", (0, 0.45, 0))
    body.box("chitin", (0, 0.45, -0.1), (0.62, 0.32, 0.8))
    body.box("chitin_light", (0, 0.63, -0.15), (0.44, 0.08, 0.6))
    body.box("chitin", (0, 0.5, -0.62), (0.5, 0.36, 0.4))
    body.box("glow_red", (0, 0.55, -0.83), (0.14, 0.14, 0.02))
    parts.append(body)
    head = Part("head", (0, 0.45, 0.3))
    head.box("chitin_light", (0, 0.45, 0.45), (0.4, 0.26, 0.32))
    head.box("glow_red", (-0.1, 0.52, 0.615), (0.07, 0.06, 0.02))
    head.box("glow_red", (0.1, 0.52, 0.615), (0.07, 0.06, 0.02))
    head.box("rubber", (-0.12, 0.36, 0.67), (0.05, 0.05, 0.16), rot=(0.4, 0.2, 0))
    head.box("rubber", (0.12, 0.36, 0.67), (0.05, 0.05, 0.16), rot=(0.4, -0.2, 0))
    parts.append(head)
    for i, z in enumerate((0.15, -0.1, -0.35)):
        for side in (-1, 1):
            leg = Part(f"leg_{'l' if side < 0 else 'r'}{i}", (side * 0.3, 0.45, z))
            leg.box("chitin", (side * 0.55, 0.55, z), (0.5, 0.07, 0.07), rot=(0, 0, side * -0.45))
            leg.box("chitin_light", (side * 0.82, 0.25, z), (0.07, 0.5, 0.07), rot=(0, 0, side * 0.2))
            parts.append(leg)
    return parts


def bloater() -> list[Part]:
    body = Part("body", (0, 0.2, 0))
    body.sphere("bloat", (0, 0.95, 0), 0.72, subdivisions=1)
    body.sphere("bloat_dark", (0, 0.55, -0.1), 0.5, subdivisions=1)
    for x, y, z, r in ((0.45, 1.25, 0.38, 0.13), (-0.38, 1.05, 0.5, 0.11), (0.0, 1.5, 0.32, 0.12), (-0.5, 1.35, -0.2, 0.1), (0.55, 0.85, -0.3, 0.1)):
        body.sphere("glow_green", (x, y, z), r, subdivisions=0)
    body.box("glow_yellow", (-0.15, 1.12, 0.68), (0.1, 0.07, 0.04))
    body.box("glow_yellow", (0.15, 1.12, 0.68), (0.1, 0.07, 0.04))
    body.box("bloat_dark", (0, 0.92, 0.7), (0.3, 0.08, 0.04))
    parts = [body]
    for side, name in ((-1, "leg_l"), (1, "leg_r")):
        leg = Part(name, (side * 0.3, 0.4, 0))
        leg.box("bloat_dark", (side * 0.3, 0.2, 0), (0.24, 0.4, 0.26))
        parts.append(leg)
    return parts


# ---------------------------------------------------------------------------
# Character rigs and clips. Walk clips span one gait loop in walk units (the
# distance accumulator the simulation advances), idle clips are in seconds.
# ---------------------------------------------------------------------------

HUMANOID = {"torso": "root", "head": "torso", "arm_l": "torso", "arm_r": "torso", "leg_l": "root", "leg_r": "root"}
TAU = 2 * math.pi


def astronaut_clips() -> dict[str, Clip]:
    def walk(u):
        swing = math.sin(TAU * u) * 0.7
        return {
            "root": {"translation": (0.0, abs(math.sin(2 * TAU * u)) * 0.06, 0.0)},
            "leg_l": {"rotation": qx(swing)},
            "leg_r": {"rotation": qx(-swing)},
            "arm_l": {"rotation": qx(-swing * 0.8)},
            "arm_r": {"rotation": qx(swing * 0.8)},
        }

    def idle(u):
        return {
            "root": {"translation": (0.0, math.sin(2 * TAU * u) * 0.01, 0.0)},
            "head": {"rotation": qy(math.sin(TAU * u) * 0.15)},
        }

    return {
        "walk": Clip(TAU / 2.2, walk),
        "idle": Clip(4.0, idle),
        # Weapon held out: overrides only the right arm when blended.
        "aim": Clip(1.0, lambda u: {"arm_r": {"rotation": qx(-1.2)}}, loop=False),
    }


def shambler_clips() -> dict[str, Clip]:
    """Husks and brutes: arms out front, a lurching sway every other step."""

    def walk(u):
        swing = math.sin(2 * TAU * u) * 0.6
        sway = math.sin(TAU * u) * 0.08
        return {
            "root": {"rotation": qz(sway)},
            "leg_l": {"rotation": qx(swing)},
            "leg_r": {"rotation": qx(-swing)},
            "arm_l": {"rotation": qx(-1.35 + swing * 0.15)},
            "arm_r": {"rotation": qx(-1.35 - swing * 0.15)},
            "head": {"rotation": qz(sway * 2.0)},
            "torso": {"rotation": qx(0.12)},
        }

    def idle(u):
        drift = math.sin(TAU * u) * 0.05
        return {
            "root": {"rotation": qz(drift * 0.5)},
            "arm_l": {"rotation": qx(-1.3 + drift)},
            "arm_r": {"rotation": qx(-1.3 - drift)},
            "head": {"rotation": qz(drift * 2.0)},
            "torso": {"rotation": qx(0.12)},
        }

    def attack(u):
        return {
            "arm_l": {"rotation": qx(-1.95)},
            "arm_r": {"rotation": qx(-1.95)},
            "torso": {"rotation": qx(0.37)},
        }

    return {
        "walk": Clip(TAU, walk),
        "idle": Clip(3.0, idle),
        "attack": Clip(1.0, attack, loop=False),
    }


SKITTER = {"head": "body"} | {f"leg_{s}{i}": "body" for s in "lr" for i in range(3)} | {"body": "root"}


def skitter_clips() -> dict[str, Clip]:
    def legs(gait, amount):
        pose = {}
        for i in range(3):
            for s, side in (("l", 1.0), ("r", -1.0)):
                phase = gait + i * 2.1 + (0.0 if side > 0 else math.pi)
                pose[f"leg_{s}{i}"] = {"rotation": qmul(qy(math.sin(phase) * 0.35 * amount), qz(side * max(math.cos(phase), 0.0) * 0.25 * amount))}
        return pose

    def walk(u):
        pose = legs(TAU * u, 1.0)
        pose["body"] = {"translation": (0.0, abs(math.sin(2 * TAU * u)) * 0.05, 0.0)}
        return pose

    return {
        "walk": Clip(TAU / 3.5, walk),
        "idle": Clip(2.0, lambda u: legs(TAU * u, 0.15)),
        "attack": Clip(1.0, lambda u: {"head": {"rotation": qx(-0.5)}}, loop=False),
    }


BLOATER = {"body": "root", "leg_l": "root", "leg_r": "root"}


def bloater_clips() -> dict[str, Clip]:
    def walk(u):
        swing = math.sin(TAU * u) * 0.6
        return {"leg_l": {"rotation": qx(swing)}, "leg_r": {"rotation": qx(-swing)}}

    def pulse(u):
        k = 1.0 + math.sin(TAU * u) * 0.05
        return {"body": {"scale": (k, k, k)}}

    return {
        "walk": Clip(TAU / 2.0, walk),
        # Body-only track: layers over walk or rest.
        "pulse": Clip(TAU / 6.0, pulse),
    }


SKINNED = {
    "astronaut": (astronaut, HUMANOID, astronaut_clips),
    "husk": (husk, HUMANOID, shambler_clips),
    "brute": (brute, HUMANOID, shambler_clips),
    "skitter": (skitter, SKITTER, skitter_clips),
    "bloater": (bloater, BLOATER, bloater_clips),
}


# ---------------------------------------------------------------------------
# Vehicles, gear and props
# ---------------------------------------------------------------------------


def drop_pod() -> list[Part]:
    body = Part("body")
    body.prism("hull", (0, 1.25, 0), 0.95, 1.7, sides=8)
    body.prism("hull", (0, 2.45, 0), 0.95, 0.7, sides=8, top_radius=0.42)
    body.prism("accent", (0, 2.95, 0), 0.42, 0.3, sides=8, top_radius=0.2)
    body.prism("glow_red", (0, 3.16, 0), 0.06, 0.14, sides=6)
    body.prism("hull_dark", (0, 0.32, 0), 0.82, 0.24, sides=8, top_radius=0.95)
    body.box("hull_dark", (0, 1.25, -0.9), (0.9, 1.2, 0.1))
    body.box("glow_cyan", (0, 1.95, 0.86), (0.36, 0.16, 0.06))
    for i in range(4):
        a = math.pi / 4 + i * math.pi / 2
        x, z = math.sin(a) * 0.98, math.cos(a) * 0.98
        body.box("accent", (x, 0.75, z), (0.1, 1.1, 0.5), rot=(0, a, 0))
        body.box("rubber", (math.sin(a) * 1.2, 0.1, math.cos(a) * 1.2), (0.16, 0.2, 0.4), rot=(0, a, 0))
    thruster = Part("thruster", (0, 0.2, 0))
    thruster.prism("hull_dark", (0, 0.12, 0), 0.55, 0.24, sides=8, top_radius=0.75)
    thruster.prism("glow_orange", (0, -0.0, 0), 0.45, 0.06, sides=8)
    door = Part("door", (0.55, 0.42, 0.86))
    door.box("hull_dark", (0, 1.2, 0.88), (1.0, 1.55, 0.08))
    door.box("accent", (0, 0.5, 0.93), (0.9, 0.08, 0.04))
    door.box("glow_cyan", (0, 1.5, 0.93), (0.5, 0.06, 0.02))
    return [body, thruster, door]


def arc_caster() -> list[Part]:
    gun = Part("gun")
    gun.box("hull_dark", (0, 0, 0), (0.16, 0.18, 0.5))
    gun.box("hull_dark", (0, -0.15, -0.1), (0.1, 0.22, 0.12), rot=(0.25, 0, 0))
    for z in (0.12, 0.24, 0.36):
        gun.prism("glow_blue", (0, 0.02, z), 0.1, 0.04, sides=8, rot=(math.pi / 2, 0, 0))
    gun.prism("hull", (0, 0.02, 0.48), 0.05, 0.2, sides=6, rot=(math.pi / 2, 0, 0))
    gun.crystal("glow_blue", (0, 0.02, 0.62), 0.05, 0.14, rot=(math.pi / 2, 0, 0))
    return [gun]


def plasma_mortar() -> list[Part]:
    gun = Part("gun")
    gun.box("hull_dark", (0, 0, -0.1), (0.24, 0.22, 0.34))
    gun.prism("hull", (0, 0.1, 0.12), 0.11, 0.42, sides=8, rot=(1.0, 0, 0))
    gun.prism("glow_green", (0, 0.24, 0.3), 0.08, 0.04, sides=8, rot=(1.0, 0, 0))
    gun.box("accent", (0.13, 0.0, -0.1), (0.03, 0.12, 0.26))
    return [gun]


def drone() -> list[Part]:
    core = Part("core")
    core.sphere("hull", (0, 0, 0), 0.18, subdivisions=1)
    core.box("glow_cyan", (0, 0, 0.17), (0.12, 0.05, 0.04))
    rotor = Part("rotor")
    for i in range(3):
        a = i * 2 * math.pi / 3
        rotor.box("glow_orange", (math.sin(a) * 0.32, 0, math.cos(a) * 0.32), (0.08, 0.03, 0.34), rot=(0, a, 0))
    return [core, rotor]


def ore(material: str, seed: int, crystal: bool) -> list[Part]:
    """Decorations for a 2 x 2 x 2 wall cube spanning [-1, 1] on every axis."""
    part = Part("ore")
    rng = seed
    def rand():
        nonlocal rng
        rng = (rng * 1103515245 + 12345) & 0x7FFFFFFF
        return rng / 0x7FFFFFFF
    faces = [((1, 0, 0), (0, 0, 1)), ((-1, 0, 0), (0, 0, 1)), ((0, 0, 1), (1, 0, 0)), ((0, 0, -1), (1, 0, 0)), ((0, 1, 0), (1, 0, 0))]
    for normal, tangent in faces:
        bitangent = cross(normal, tangent)
        for _ in range(4 if crystal else 6):
            u, v = rand() * 1.4 - 0.7, rand() * 1.4 - 0.7
            base = tuple(normal[i] * 0.98 + tangent[i] * u + bitangent[i] * v for i in range(3))
            if crystal:
                tilt = (rand() - 0.5) * 0.7
                if normal[1] == 1:
                    rot = (tilt, rand() * 3, 0)
                elif normal[0] != 0:
                    rot = (0, 0, -normal[0] * (math.pi / 2) + tilt)
                else:
                    rot = (normal[2] * (math.pi / 2) + tilt, 0, 0)
                height = 0.45 + rand() * 0.55
                center = tuple(base[i] + normal[i] * height * 0.15 for i in range(3))
                part.crystal(material, center, 0.09 + rand() * 0.08, height, rot=rot)
            else:
                size = 0.16 + rand() * 0.22
                center = tuple(base[i] + normal[i] * size * 0.25 for i in range(3))
                part.box(material, center, (size, size * (0.6 + rand() * 0.6), size), rot=(rand(), rand() * 3, rand()))
    return [part]


def pickup(material: str, kind: str) -> list[Part]:
    part = Part("pickup")
    if kind == "crystal":
        part.crystal(material, (0, 0, 0), 0.14, 0.42)
        part.crystal(material, (0.12, -0.06, 0.04), 0.07, 0.22, rot=(0, 0, -0.5))
        part.crystal(material, (-0.1, -0.06, -0.05), 0.07, 0.2, rot=(0.3, 0, 0.6))
    elif kind == "shard":
        part.crystal(material, (0, 0, 0), 0.1, 0.32, sides=4)
    else:
        part.box(material, (0, 0, 0), (0.26, 0.22, 0.24), rot=(0.3, 0.5, 0.2))
        part.box(material, (0.14, -0.05, 0.08), (0.16, 0.14, 0.15), rot=(0.7, 0.2, 0.4))
        part.box(material, (-0.12, -0.04, -0.06), (0.14, 0.13, 0.15), rot=(0.1, 0.9, 0.6))
    return [part]


def beacon() -> list[Part]:
    base = Part("base")
    for i in range(3):
        a = i * 2 * math.pi / 3
        base.box("hull_dark", (math.sin(a) * 0.4, 0.35, math.cos(a) * 0.4), (0.08, 0.8, 0.08), rot=(0, a, 0.45 * 0))
    base.prism("hull", (0, 0.9, 0), 0.12, 1.4, sides=6)
    base.prism("accent", (0, 0.3, 0), 0.5, 0.12, sides=6)
    light = Part("light", (0, 1.75, 0))
    light.prism("glow_orange", (0, 1.75, 0), 0.18, 0.3, sides=8)
    light.prism("hull_dark", (0, 1.95, 0), 0.22, 0.08, sides=8)
    return [base, light]


def nav_console() -> list[Part]:
    part = Part("console")
    part.box("hull_dark", (0, 0.45, 0), (1.6, 0.9, 0.8))
    part.box("hull", (0, 0.92, 0.05), (1.7, 0.06, 0.9))
    part.box("screen", (0, 1.25, -0.2), (1.4, 0.7, 0.06), rot=(-0.35, 0, 0))
    for x in (-0.5, -0.2, 0.1, 0.4):
        part.box("glow_orange" if x < 0 else "glow_cyan", (x, 0.97, 0.3), (0.16, 0.04, 0.1))
    holo = Part("holo", (0, 1.9, 0.0))
    holo.sphere("glow_cyan", (0, 1.9, 0.0), 0.28, subdivisions=1)
    return [part, holo]


def pod_cradle() -> list[Part]:
    part = Part("cradle")
    part.prism("hull_dark", (0, 0.06, 0), 1.6, 0.12, sides=12)
    part.prism("accent", (0, 0.13, 0), 1.45, 0.04, sides=12)
    for i in range(4):
        a = i * math.pi / 2 + math.pi / 4
        part.box("hull", (math.sin(a) * 1.45, 0.9, math.cos(a) * 1.45), (0.2, 1.8, 0.2), rot=(0, a, 0))
        part.box("glow_orange", (math.sin(a) * 1.35, 1.6, math.cos(a) * 1.35), (0.06, 0.2, 0.06), rot=(0, a, 0))
    return [part]


def reactor_core() -> list[Part]:
    part = Part("frame")
    part.prism("hull_dark", (0, 0.2, 0), 1.0, 0.4, sides=8)
    part.prism("hull_dark", (0, 2.6, 0), 0.9, 0.3, sides=8)
    for i in range(4):
        a = i * math.pi / 2
        part.box("hull", (math.sin(a) * 0.8, 1.4, math.cos(a) * 0.8), (0.14, 2.4, 0.14))
    core = Part("core", (0, 1.4, 0))
    core.prism("glow_blue", (0, 1.4, 0), 0.42, 2.0, sides=8)
    core.prism("glow_cyan", (0, 1.4, 0), 0.5, 0.12, sides=8)
    return [part, core]


def med_bay() -> list[Part]:
    part = Part("bed")
    part.box("hull_dark", (0, 0.3, 0), (1.0, 0.6, 2.1))
    part.box("white", (0, 0.66, 0.05), (0.85, 0.12, 1.9))
    part.box("hull", (0, 1.25, -1.0), (1.0, 1.3, 0.1))
    part.box("glow_green", (0, 1.4, -0.94), (0.5, 0.25, 0.02))
    part.prism("glow_green", (0, 2.0, 0), 0.6, 0.04, sides=12)
    return [part]


def weapon_rack() -> list[Part]:
    part = Part("rack")
    part.box("hull_dark", (0, 1.0, 0), (2.0, 2.0, 0.3))
    for x in (-0.6, 0.0, 0.6):
        part.box("hull", (x, 1.3, 0.2), (0.12, 0.7, 0.08))
        part.box("glow_orange", (x, 0.55, 0.16), (0.3, 0.06, 0.02))
    part.box("accent", (0, 1.95, 0.16), (1.9, 0.08, 0.04))
    return [part]


def drill_rig() -> list[Part]:
    base = Part("base")
    base.box("hull_dark", (0, 0.35, 0), (1.4, 0.7, 1.0))
    base.box("accent", (0, 0.72, 0), (1.45, 0.06, 1.05))
    base.box("hull", (0, 1.3, -0.3), (0.4, 1.2, 0.4))
    bit = Part("bit", (0, 1.4, 0.3))
    bit.prism("ferrite", (0, 1.4, 0.65), 0.22, 0.7, sides=6, rot=(math.pi / 2, 0, 0), top_radius=0.02)
    bit.prism("hull_dark", (0, 1.4, 0.25), 0.26, 0.2, sides=8, rot=(math.pi / 2, 0, 0))
    return [base, bit]


def ship_exterior() -> list[Part]:
    hull = Part("hull")
    hull.box("hull", (0, 0, 0), (2.2, 1.2, 5.0))
    hull.box("hull", (0, 0.2, 2.9), (1.6, 0.9, 1.2))
    hull.box("visor", (0, 0.55, 3.2), (1.2, 0.35, 0.8), rot=(-0.3, 0, 0))
    hull.box("hull_dark", (0, -0.7, -0.5), (1.6, 0.3, 3.0))
    for side in (-1, 1):
        hull.box("hull", (side * 2.0, -0.1, -0.8), (2.0, 0.18, 2.2), rot=(0, 0, side * -0.08))
        hull.box("accent", (side * 2.9, -0.04, -0.8), (0.2, 0.3, 2.0))
        hull.prism("hull_dark", (side * 1.0, 0.0, -2.8), 0.45, 0.9, sides=8, rot=(math.pi / 2, 0, 0))
    hull.box("accent", (0, 0.62, 0.6), (0.3, 0.06, 3.0))
    engines = Part("engines")
    for side in (-1, 1):
        engines.prism("glow_cyan", (side * 1.0, 0.0, -3.27), 0.35, 0.06, sides=8, rot=(math.pi / 2, 0, 0))
    return [hull, engines]


def glowcap() -> list[Part]:
    """A cluster of luminous cave mushrooms (secondary objective pickups)."""
    part = Part("caps")
    for x, z, h, r in ((0.0, 0.0, 0.5, 0.28), (0.25, 0.15, 0.32, 0.18), (-0.2, 0.2, 0.26, 0.15)):
        part.prism("bloat_dark", (x, h * 0.5, z), 0.05, h, sides=6)
        part.prism("glow_green", (x, h, z), r, 0.1, sides=8, top_radius=r * 0.4)
    return [part]


def fungus() -> list[Part]:
    """Cave fungus (Verdant decoration): pale stalks with wide shelf caps."""
    part = Part("fungus")
    for x, z, h, r in ((0.0, 0.0, 0.9, 0.42), (0.38, 0.22, 0.55, 0.28), (-0.3, 0.3, 0.42, 0.22), (0.1, -0.35, 0.3, 0.16)):
        part.prism("stalk", (x, h * 0.5, z), 0.07, h, sides=6, top_radius=0.05)
        part.prism("chitin_light", (x, h, z), r, 0.12, sides=9, top_radius=r * 0.35)
        part.prism("glow_purple", (x, h - 0.07, z), r * 0.8, 0.02, sides=9)
    return [part]


def crystal_growth() -> list[Part]:
    """Obsidian shards with ember cores (Cinder decoration)."""
    part = Part("crystals")
    for x, z, h, rx, rz in ((0.0, 0.0, 1.7, 0.12, 0.05), (0.38, 0.2, 1.1, -0.3, 0.2), (-0.32, 0.26, 0.95, 0.25, -0.35), (0.12, -0.38, 0.8, -0.35, -0.1)):
        part.crystal("obsidian", (x, h * 0.3, z), 0.24, h, sides=5, rot=(rx, 0.0, rz))
        # A glowing ember seam breaking out of each shard's side.
        part.crystal("glow_orange", (x + 0.1, h * 0.2, z + 0.1), 0.09, h * 0.55, sides=4, rot=(rx + 0.3, 0.0, rz - 0.3))
    return [part]


def ice_spire() -> list[Part]:
    """Leaning ice spires (Hollow Frost decoration)."""
    part = Part("ice")
    for x, z, h, r, rx, rz in ((0.0, 0.0, 1.8, 0.28, 0.05, 0.08), (0.36, 0.2, 1.1, 0.2, -0.2, 0.25), (-0.32, 0.28, 0.9, 0.18, 0.2, -0.25), (0.06, -0.36, 0.7, 0.15, -0.3, 0.0)):
        part.prism("ice", (x, h * 0.5, z), r, h, sides=6, rot=(rx, 0.0, rz), top_radius=0.0)
    return [part]


def trade_station() -> list[Part]:
    """The trade station seen from the ship: a ring on spokes around a hub
    with docking arms and lit windows. About 2 units across."""
    ring = Part("ring")
    segments = 24
    for i in range(segments):
        a = 2 * math.pi * i / segments
        x, z = math.cos(a) * 0.9, math.sin(a) * 0.9
        ring.box("hull", (x, 0, z), (0.26, 0.16, 0.24), rot=(0.0, -a, 0.0))
        ring.box("glow_yellow" if i % 3 else "glow_cyan", (x * 1.07, 0, z * 1.07), (0.03, 0.05, 0.12), rot=(0.0, -a, 0.0))
    hub = Part("hub")
    hub.prism("hull_dark", (0, 0, 0), 0.22, 0.6, sides=8)
    hub.prism("hull", (0, 0.36, 0), 0.16, 0.14, sides=8, top_radius=0.06)
    hub.prism("glow_cyan", (0, -0.34, 0), 0.12, 0.06, sides=8)
    for i in range(4):
        a = math.pi / 2 * i + math.pi / 4
        hub.box("hull_dark", (math.cos(a) * 0.46, 0, math.sin(a) * 0.46), (0.5, 0.05, 0.05), rot=(0.0, -a, 0.0))
    for i in range(2):
        a = math.pi * i
        hub.box("accent", (math.cos(a) * 0.32, 0.22, math.sin(a) * 0.32), (0.18, 0.06, 0.06), rot=(0.0, -a, 0.0))
    return [ring, hub]


def locker() -> list[Part]:
    """Four crew lockers in a row, one per class colour, facing +Z."""
    part = Part("lockers")
    part.box("hull_dark", (0, 0.06, 0), (2.3, 0.12, 0.7))
    part.box("hull_dark", (0, 2.3, 0), (2.3, 0.1, 0.7))
    for i, glow in enumerate(("glow_orange", "glow_yellow", "glow_blue", "glow_green")):
        x = -0.84 + i * 0.56
        part.box("hull", (x, 1.18, 0), (0.5, 2.1, 0.6))
        part.box("hull_dark", (x, 1.18, 0.31), (0.42, 1.95, 0.02))
        part.box(glow, (x, 1.75, 0.33), (0.3, 0.06, 0.02))
        part.box("rubber", (x + 0.14, 1.15, 0.34), (0.04, 0.25, 0.03))
        for y in (2.0, 2.06, 2.12):
            part.box("rubber", (x, y, 0.325), (0.28, 0.02, 0.02))
    part.box("accent", (0, 2.38, 0.3), (2.3, 0.06, 0.04))
    return [part]


def planet() -> list[Part]:
    body = Part("planet")
    body.sphere("white", (0, 0, 0), 1.0, subdivisions=3)
    return [body]


MODELS = {
    "drop-pod": drop_pod,
    "arc-caster": arc_caster,
    "plasma-mortar": plasma_mortar,
    "drone": drone,
    "ferrite-ore": lambda: ore("ferrite", 11, False),
    "lumite-ore": lambda: ore("lumite", 23, True),
    "voidstone-ore": lambda: ore("voidstone", 37, True),
    "xp-shard": lambda: pickup("glow_green", "shard"),
    "ferrite-chunk": lambda: pickup("ferrite", "chunk"),
    "lumite-chunk": lambda: pickup("lumite", "crystal"),
    "voidstone-chunk": lambda: pickup("voidstone", "crystal"),
    "beacon": beacon,
    "nav-console": nav_console,
    "pod-cradle": pod_cradle,
    "locker": locker,
    "glowcap": glowcap,
    "fungus": fungus,
    "crystal-growth": crystal_growth,
    "ice-spire": ice_spire,
    "trade-station": trade_station,
    "reactor-core": reactor_core,
    "med-bay": med_bay,
    "weapon-rack": weapon_rack,
    "drill-rig": drill_rig,
    "ship-exterior": ship_exterior,
    "planet": planet,
}


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--out", type=Path, default=REPO / "assets" / "models")
    args = parser.parse_args()
    for name, (build, parents, clips) in SKINNED.items():
        path = args.out / f"{name}.glb"
        write_skinned_glb(path, build(), parents, clips())
        print(f"wrote {path.relative_to(REPO) if path.is_relative_to(REPO) else path} (skinned)")
    for name, build in MODELS.items():
        path = args.out / f"{name}.glb"
        write_glb(path, build())
        print(f"wrote {path.relative_to(REPO) if path.is_relative_to(REPO) else path}")


if __name__ == "__main__":
    main()
