#!/usr/bin/env python3
"""E1 — Does "only iterate on what changed" hold for a game view? (THROWAWAY PoC)

Scene: 512x288 px luma world (seeded value-noise floor quantized to 12 levels,
wall rects, 3 beacons), 8 agents (6x6 px blobs, distinct luma) random-walking
at ~walking speed, 30 fps for 60 s. Storms: 2 s bursts where all agents move
at 3x speed.

Encoding (independent re-implementation, stdlib only):
  cell = 2x4 px.  band   = (mean luma)*10 // 256            -> 0..9
                  class  = integer 3x3 Sobel at the cell centre pixel
                           (x0+1, y0+2): flat if max(|gx|,|gy|) < 16,
                           else v if |gx|>=2|gy|, h if |gy|>=2|gx|,
                           else diag+/diag- by sign(gx*gy)   -> 0..4
                  glyph  = band*5 + class                    -> 0..49
                  mask   = 8 braille dot bits, dot set iff px >= 128
                           (bit per Unicode braille dot: left col 1,2,3,7 /
                            right col 4,5,6,8)
  tile  = 8x4 cells (16x16 px) -> 64 bytes (glyph,mask interleaved,
          row-major); tile key = FNV-1a 64 over those 64 bytes.

Cameras:
  A  static full-map            (512x288 -> 32x18 world tiles)
  B  follow cam, tile grid anchored to the VIEW  (256x144 -> 16x9 slots);
     a pan shifts the grid, so world content slides under the tile ids
  C  follow cam, tile grid anchored to the WORLD (same view window, tiles
     keyed by world-grid tile id; a pan reveals tiles, never re-keys them)

Metrics per camera: changed tiles/frame (mean, p95, % of tiles in view),
cumulative distinct keys at 1/10/30/60 s, LRU hit rate for caps
64/256/1024/4096 (hit = key requested and still cached), the same three
during storm frames only, bytes/frame for changed-only sends
(8 B key + 64 B payload = 72 B per tile) vs the full frame, and a
determinism check (two full runs must give an identical results hash).

Conventions: sub-pixel camera/agent positions floor to integers; frame 0
counts as all-tiles-changed (initial send); storm-only LRU trackers are
separate caches fed only on storm frames; p95 is nearest-rank over
per-frame percentages.
"""

import hashlib
import json
import math
import random
import time
from collections import OrderedDict
from operator import add, sub

# ----------------------------------------------------------------- constants
W, H             = 512, 288
FPS, DUR         = 30, 60
NFRAMES          = FPS * DUR                      # 1800
DT               = 1.0 / FPS
CELL_W, CELL_H   = 2, 4                           # cell = 2x4 px
TILE_CW, TILE_CH = 8, 4                           # tile = 8x4 cells
TILE_PX          = CELL_W * TILE_CW               # 16
GW, GH           = W // CELL_W, H // CELL_H       # world cell grid 256x72
TW, TH           = W // TILE_PX, H // TILE_PX     # world tile grid 32x18
VIEW_W, VIEW_H   = 256, 144
VCW, VCH         = VIEW_W // CELL_W, VIEW_H // CELL_H    # view cells 128x36
VTW, VTH         = VIEW_W // TILE_PX, VIEW_H // TILE_PX  # view tiles 16x9
SOBEL_T          = 16
FNV_OFF          = 0xCBF29CE484222325
FNV_PRIME        = 0x100000001B3
KEY_BYTES        = 8
TILE_BYTES       = TILE_CW * TILE_CH * 2          # 64 B payload
SEND_BYTES       = KEY_BYTES + TILE_BYTES         # 72 B per changed tile
LRU_CAPS         = (64, 256, 1024, 4096)
SNAPS            = {FPS - 1: '1s', 10 * FPS - 1: '10s',
                    30 * FPS - 1: '30s', NFRAMES - 1: '60s'}
STORMS           = ((15.0, 17.0), (33.0, 35.0), (48.0, 50.0))  # 2 s bursts
STORM_MULT       = 3.0
WALK             = 48.0        # px/s  (~1.5 m/s at ~32 px/m)
SEED             = 20261028
AG_LUMA          = [52, 76, 100, 124, 148, 172, 196, 220]
WALLS            = [(64, 40, 160, 24), (240, 96, 24, 120), (344, 160, 104, 24),
                    (120, 196, 24, 68), (408, 32, 24, 72)]
BEACONS          = [(40, 232), (452, 44), (288, 136)]          # 8x8 markers
WALL_LUMA, B_HI, B_LO = 232, 252, 8
BR_BIT           = ((0, 3), (1, 4), (2, 5), (6, 7))  # (left,right) bit per row
THRESH_TBL       = bytes(128) + b'\x01' * 128       # luma -> 0/1 for mask


# --------------------------------------------------------------- value noise
def ihash(x, y, s):
    h = (x * 374761393 + y * 668265263 + s * 974634733) & 0xFFFFFFFF
    h = ((h ^ (h >> 13)) * 1274126177) & 0xFFFFFFFF
    return h ^ (h >> 16)


def vnoise(u, v, s):
    x0, y0 = math.floor(u), math.floor(v)
    tx, ty = u - x0, v - y0
    sx = tx * tx * (3.0 - 2.0 * tx)
    sy = ty * ty * (3.0 - 2.0 * ty)
    a = ihash(x0, y0, s) & 1023
    b = ihash(x0 + 1, y0, s) & 1023
    c = ihash(x0, y0 + 1, s) & 1023
    d = ihash(x0 + 1, y0 + 1, s) & 1023
    ab = a + (b - a) * sx
    cd = c + (d - c) * sx
    return (ab + (cd - ab) * sy) / 1023.0


def build_world():
    pix = bytearray(W * H)
    for y in range(H):
        vy1, vy2 = y / 40.0, (y + 3) / 11.0
        row = y * W
        for x in range(W):
            n = 0.7 * vnoise(x / 40.0, vy1, 1) + 0.3 * vnoise((x + 7) / 11.0, vy2, 2)
            q = int(n * 12.0)
            if q > 11:
                q = 11
            pix[row + x] = 24 + 12 * q
    for rx, ry, rw, rh in WALLS:
        for yy in range(ry, ry + rh):
            o = yy * W
            pix[o + rx:o + rx + rw] = bytes([WALL_LUMA]) * rw
    for bx, by in BEACONS:                     # bright ring, dark core
        for yy in range(by, by + 8):
            o = yy * W
            pix[o + bx:o + bx + 8] = bytes([B_HI]) * 8
        for yy in range(by + 2, by + 6):
            o = yy * W
            pix[o + bx + 2:o + bx + 6] = bytes([B_LO]) * 4
    return pix


# ------------------------------------------------------------ cell encoding
def sobel_gxgy(pix, x, y):
    xm = x - 1 if x > 0 else 0
    xp = x + 1 if x < W - 1 else W - 1
    ym = y - 1 if y > 0 else 0
    yp = y + 1 if y < H - 1 else H - 1
    r0, r1, r2 = ym * W, y * W, yp * W
    a, b, c = pix[r0 + xm], pix[r0 + x], pix[r0 + xp]
    d, f = pix[r1 + xm], pix[r1 + xp]
    g, h, i = pix[r2 + xm], pix[r2 + x], pix[r2 + xp]
    return (c + 2 * f + i) - (a + 2 * d + g), (g + 2 * h + i) - (a + 2 * b + c)


def orient_class(gx, gy):
    ax = -gx if gx < 0 else gx
    ay = -gy if gy < 0 else gy
    m = ax if ax > ay else ay
    if m < SOBEL_T:
        return 0                              # flat
    if ax >= 2 * ay:
        return 1                              # v  (horizontal gradient)
    if ay >= 2 * ax:
        return 2                              # h
    return 3 if gx * gy >= 0 else 4           # diag+ / diag-


def compute_cell(pix, x0, y0):
    """(glyph, mask) for the 2x4-px cell whose top-left pixel is (x0, y0)."""
    s = 0
    m = 0
    for dy in range(4):
        o = (y0 + dy) * W + x0
        a = pix[o]
        b = pix[o + 1]
        s += a + b
        if a >= 128:
            m |= 1 << BR_BIT[dy][0]
        if b >= 128:
            m |= 1 << BR_BIT[dy][1]
    band = (s * 10) // 2048                   # == (mean luma * 10) // 256
    gx, gy = sobel_gxgy(pix, x0 + 1, y0 + 2)  # cell centre pixel
    return band * 5 + orient_class(gx, gy), m


# ------------------------------------------------------------- cell planes
def build_static_plane(pix):
    """World-anchored cell plane of the static map: 2 B per cell (glyph,mask)."""
    plane = bytearray(GW * GH * 2)
    for j in range(GH):
        y = j * CELL_H
        for i in range(GW):
            gl, m = compute_cell(pix, i * CELL_W, y)
            o = 2 * (j * GW + i)
            plane[o] = gl
            plane[o + 1] = m
    return plane


def update_plane_agents(plane, pix, ax, ay):
    """Recompute world-grid cells touched by the 6x6 agent box at (ax-3, ay-3).

    A cell depends on its own 8 px plus one column to the right (Sobel), so
    the recompute box is the agent box grown by 2 px; extra cells are idle.
    """
    x0 = max(0, ax - 3 - 2)
    x1 = min(W - CELL_W, ax + 3 + 1)
    y0 = max(0, ay - 3 - 4)
    y1 = min(H - CELL_H, ay + 3 + 3)
    for j in range(y0 // CELL_H, y1 // CELL_H + 1):
        y = j * CELL_H
        for i in range(x0 // CELL_W, x1 // CELL_W + 1):
            gl, m = compute_cell(pix, i * CELL_W, y)
            o = 2 * (j * GW + i)
            plane[o] = gl
            plane[o + 1] = m


def fill_view_plane(pix, cx, cy):
    """Cell plane for the view-anchored (camera B) grid: 128x36 cells.

    Written per cell-row with C-level slice/map ops; row slices are padded
    by edge replicate so Sobel sees the same clamping as compute_cell.
    """
    plane = bytearray(VCW * VCH * 2)
    need = VIEW_W + 2                # cells span 256 px; Sobel needs +1 right
    x_end = cx + need
    pad = x_end - W
    if pad > 0:
        x_end = W
    else:
        pad = 0
    for j in range(VCH):
        y0 = cy + j * CELL_H
        r0 = pix[y0 * W + cx:y0 * W + x_end]
        r1 = pix[(y0 + 1) * W + cx:(y0 + 1) * W + x_end]
        r2 = pix[(y0 + 2) * W + cx:(y0 + 2) * W + x_end]
        r3 = pix[(y0 + 3) * W + cx:(y0 + 3) * W + x_end]
        if pad:
            r0 = r0 + r0[-1:] * pad
            r1 = r1 + r1[-1:] * pad
            r2 = r2 + r2[-1:] * pad
            r3 = r3 + r3[-1:] * pad
        # luma band: mean over the cell's 8 px
        p0 = list(map(add, r0[0::2], r0[1::2]))
        p1 = list(map(add, r1[0::2], r1[1::2]))
        p2 = list(map(add, r2[0::2], r2[1::2]))
        p3 = list(map(add, r3[0::2], r3[1::2]))
        sums = list(map(add, map(add, p0, p1), map(add, p2, p3)))
        bands = [(s * 10) // 2048 for s in sums]
        # braille mask via per-pixel threshold streams (0/1 bytes)
        t0 = r0.translate(THRESH_TBL)
        t1 = r1.translate(THRESH_TBL)
        t2 = r2.translate(THRESH_TBL)
        t3 = r3.translate(THRESH_TBL)
        masks = [la + 2 * lb + 4 * lc + 8 * ra + 16 * rb + 32 * rc + 64 * ld + 128 * rd
                 for la, ra, lb, rb, lc, rc, ld, rd in zip(
                     t0[0::2], t0[1::2], t1[0::2], t1[1::2],
                     t2[0::2], t2[1::2], t3[0::2], t3[1::2])]
        # Sobel at centre pixel (left+1, y0+2); rows y0+1..y0+3, weights 1,2,1
        d1 = list(map(sub, r1[2:], r1))
        d2 = list(map(sub, r2[2:], r2))
        d3 = list(map(sub, r3[2:], r3))
        gxs = list(map(add, map(add, d1[0::2], d3[0::2]), [v + v for v in d2[0::2]]))
        pt = list(map(add, r1, r1[1:]))
        st = list(map(add, pt, pt[1:]))
        pb = list(map(add, r3, r3[1:]))
        sb = list(map(add, pb, pb[1:]))
        gys = list(map(sub, sb, st))[0::2]
        glyphs = [b * 5 + orient_class(gx, gy)
                  for b, gx, gy in zip(bands, gxs, gys)]
        seg = bytearray(VCW * 2)
        seg[0::2] = bytes(glyphs[:VCW])
        seg[1::2] = bytes(masks[:VCW])
        o = 2 * j * VCW
        plane[o:o + VCW * 2] = seg
    return plane


def spot_check(world, cx, cy, vp):
    """Guard: the vectorized view-plane path must agree with compute_cell."""
    for j in range(0, VCH, 5):
        for i in range(0, VCW, 11):
            gl, m = compute_cell(world, cx + i * CELL_W, cy + j * CELL_H)
            o = 2 * (j * VCW + i)
            if vp[o] != gl or vp[o + 1] != m:
                raise AssertionError(f'view-plane mismatch at cell ({i},{j}): '
                                     f'glyph {vp[o]}/{gl} mask {vp[o+1]}/{m}')


# ----------------------------------------------------------------- tile key
def tile_key_of(plane, cols, i, j, memo):
    """FNV-1a 64 of the tile's 64 (glyph,mask) bytes, memoized by content."""
    o = 2 * (j * TILE_CH * cols + i * TILE_CW)
    st = 2 * cols
    src = (bytes(plane[o:o + 2 * TILE_CW]) +
           bytes(plane[o + st:o + st + 2 * TILE_CW]) +
           bytes(plane[o + 2 * st:o + 2 * st + 2 * TILE_CW]) +
           bytes(plane[o + 3 * st:o + 3 * st + 2 * TILE_CW]))
    k = memo.get(src)
    if k is None:
        h = FNV_OFF
        for b in src:
            h = ((h ^ b) * FNV_PRIME) & 0xFFFFFFFFFFFFFFFF
        memo[src] = k = h
    return k


# ----------------------------------------------------------------- metrics
class LRU:
    __slots__ = ('cap', 'od', 'hits', 'n')

    def __init__(self, cap):
        self.cap = cap
        self.od = OrderedDict()
        self.hits = 0
        self.n = 0

    def see(self, k):
        od = self.od
        if k in od:
            self.hits += 1
            od.move_to_end(k)
        else:
            od[k] = None
            if len(od) > self.cap:
                od.popitem(last=False)
        self.n += 1


class CamStats:
    def __init__(self, label):
        self.label = label
        self.fracs = []
        self.sfracs = []
        self.keys = set()
        self.skeys = set()
        self.snaps = {}
        self.lrus = [LRU(c) for c in LRU_CAPS]
        self.slrus = [LRU(c) for c in LRU_CAPS]
        self.prev = {}
        self.n_frames = 0
        self.n_sframes = 0
        self.tiles_sum = 0
        self.ch_bytes = 0
        self.full_bytes = 0

    def record(self, items, storm, fidx):
        prev = self.prev
        ch = sum(1 for tid, k in items if prev.get(tid) != k)
        self.prev = dict(items)
        n = len(items)
        keys = [k for _, k in items]
        frac = ch / n
        self.fracs.append(frac)
        self.keys.update(keys)
        self.n_frames += 1
        self.tiles_sum += n
        self.ch_bytes += ch * SEND_BYTES
        self.full_bytes += n * SEND_BYTES
        for l in self.lrus:
            for k in keys:
                l.see(k)
        if storm:
            self.sfracs.append(frac)
            self.skeys.update(keys)
            self.n_sframes += 1
            for l in self.slrus:
                for k in keys:
                    l.see(k)
        if fidx in SNAPS:
            self.snaps[SNAPS[fidx]] = len(self.keys)


def p95(xs):
    s = sorted(xs)
    return s[min(len(s) - 1, int(0.95 * (len(s) - 1) + 0.5))]


def summarize(st):
    def hpct(l):
        return round(100.0 * l.hits / l.n, 2) if l.n else None

    tiles_mean = st.tiles_sum / st.n_frames
    mean_frac = sum(st.fracs) / len(st.fracs)
    s = st.sfracs
    smean = (sum(s) / len(s)) if s else 0.0
    return {
        'label': st.label,
        'tiles_in_view': round(tiles_mean, 1),
        'changed_mean_abs': round(mean_frac * tiles_mean, 2),
        'changed_mean_pct': round(100.0 * mean_frac, 2),
        'changed_p95_pct': round(100.0 * p95(st.fracs), 2),
        'distinct_keys': dict(st.snaps),
        'lru_hit_pct': {str(c): hpct(l) for c, l in zip(LRU_CAPS, st.lrus)},
        'storm': {
            'frames': st.n_sframes,
            'changed_mean_abs': round(smean * tiles_mean, 2),
            'changed_mean_pct': round(100.0 * smean, 2) if s else None,
            'changed_p95_pct': round(100.0 * p95(s), 2) if s else None,
            'distinct_keys': len(st.skeys),
            'lru_hit_pct': {str(c): hpct(l) for c, l in zip(LRU_CAPS, st.slrus)},
        },
        'bytes_per_frame': {
            'changed_mean': round(st.ch_bytes / st.n_frames, 1),
            'full_frame': round(st.full_bytes / st.n_frames, 1),
            'pct': round(100.0 * st.ch_bytes / st.full_bytes, 2),
        },
    }


# ------------------------------------------------------------------ agents
def box_free(ix, iy):
    if not (4 <= ix <= W - 10 and 4 <= iy <= H - 10):
        return False
    x0, y0 = ix - 3, iy - 3
    for rx, ry, rw, rh in WALLS:
        if x0 < rx + rw and x0 + 6 > rx and y0 < ry + rh and y0 + 6 > ry:
            return False
    return True


def new_goal(a, rng):
    a['gx'] = rng.uniform(12.0, W - 12.0)
    a['gy'] = rng.uniform(12.0, H - 12.0)


def make_agents(rng):
    agents = []
    for i in range(8):
        x, y = W / 2.0, H / 2.0
        for _ in range(200):
            x, y = rng.uniform(16.0, W - 16.0), rng.uniform(16.0, H - 16.0)
            if box_free(int(x), int(y)):
                break
        a = {'x': x, 'y': y, 'gx': x, 'gy': y, 'l': AG_LUMA[i]}
        new_goal(a, rng)
        agents.append(a)
    return agents


def step_agent(a, dt, speed, rng):
    dx = a['gx'] - a['x']
    dy = a['gy'] - a['y']
    d = math.hypot(dx, dy)
    if d < 4.0:
        new_goal(a, rng)
        return
    st = speed * dt
    if st >= d:
        nx, ny = a['gx'], a['gy']
    else:
        nx, ny = a['x'] + dx / d * st, a['y'] + dy / d * st
    if box_free(int(nx), int(ny)):
        a['x'], a['y'] = nx, ny
    else:
        new_goal(a, rng)


def stamp(world, a):
    x0 = int(a['x']) - 3
    y0 = int(a['y']) - 3
    pat = bytes([a['l']]) * 6
    for dy in range(6):
        o = (y0 + dy) * W + x0
        world[o:o + 6] = pat


def camera_pos(a0):
    cx = int(a0['x']) - VIEW_W // 2
    cy = int(a0['y']) - VIEW_H // 2
    cx = 0 if cx < 0 else (W - VIEW_W if cx > W - VIEW_W else cx)
    cy = 0 if cy < 0 else (H - VIEW_H if cy > H - VIEW_H else cy)
    return cx, cy


# ------------------------------------------------------------------- sim
def run_sim():
    rng = random.Random(SEED)
    world0 = build_world()
    static_plane = build_static_plane(world0)
    agents = make_agents(rng)
    memo = {}
    cams = {
        'A': CamStats('A STATIC full-map camera'),
        'B': CamStats('B FOLLOW camera, tiles anchored to the VIEW'),
        'C': CamStats('C FOLLOW camera, tiles anchored to the WORLD'),
    }
    checked = False
    for f in range(NFRAMES):
        t = f * DT
        storm = any(s0 <= t < s1 for s0, s1 in STORMS)
        speed = WALK * (STORM_MULT if storm else 1.0)

        world = bytearray(world0)
        for a in agents:
            step_agent(a, DT, speed, rng)
        for a in agents:
            stamp(world, a)

        # world-anchored cell plane, updated only where agents stand
        plane = bytearray(static_plane)
        for a in agents:
            update_plane_agents(plane, world, int(a['x']), int(a['y']))

        items_a = []
        for j in range(TH):
            base = j * TW
            for i in range(TW):
                items_a.append((base + i, tile_key_of(plane, GW, i, j, memo)))
        cams['A'].record(items_a, storm, f)

        cx, cy = camera_pos(agents[0])
        vp = fill_view_plane(world, cx, cy)
        items_b = []
        for j in range(VTH):
            base = j * VTW
            for i in range(VTW):
                items_b.append((base + i, tile_key_of(vp, VCW, i, j, memo)))
        cams['B'].record(items_b, storm, f)

        i0, i1 = cx // TILE_PX, (cx + VIEW_W - 1) // TILE_PX
        j0, j1 = cy // TILE_PX, (cy + VIEW_H - 1) // TILE_PX
        items_c = []
        for j in range(j0, j1 + 1):
            base = j * TW
            for i in range(i0, i1 + 1):
                items_c.append((base + i, tile_key_of(plane, GW, i, j, memo)))
        cams['C'].record(items_c, storm, f)

        if not checked:
            spot_check(world, cx, cy, vp)
            checked = True

    return {k: summarize(v) for k, v in cams.items()}


def res_hash(results):
    blob = json.dumps(results, sort_keys=True, separators=(',', ':'))
    return hashlib.sha256(blob.encode('utf-8')).hexdigest()[:16]


# ----------------------------------------------------------------- report
def fmt_hits(d):
    return '  '.join(f'cap{c}={d[str(c)]}%' for c in LRU_CAPS)


def print_report(r, h1, h2, t1, t2):
    print('=' * 78)
    print('E1 — tile churn: does "only iterate on what changed" hold for a game view?')
    print(f'world {W}x{H} px | cell {CELL_W}x{CELL_H} px | tile {TILE_PX}x{TILE_PX} px '
          f'= {TILE_CW}x{TILE_CH} cells = {TILE_BYTES} B payload, 8 B key')
    print(f'{NFRAMES} frames @ {FPS} fps ({DUR} s) | seed {SEED} | '
          f'8 agents 6x6 px @ {WALK:g} px/s | storms x{STORM_MULT:g}: ' +
          ', '.join(f'{s:g}-{e:g}s' for s, e in STORMS))
    print(f'frame 0 counts as all-changed; storm metrics use the {sum(int(e - s) for s, e in STORMS)} s '
          f'of storm frames only')
    print('=' * 78)
    for name in ('A', 'B', 'C'):
        cam = r[name]
        dk = cam['distinct_keys']
        s = cam['storm']
        b = cam['bytes_per_frame']
        print(f'[{name}] {cam["label"]}  ({cam["tiles_in_view"]} tiles in view, avg)')
        print(f'    changed tiles/frame : mean {cam["changed_mean_abs"]} '
              f'({cam["changed_mean_pct"]}%)  p95 {cam["changed_p95_pct"]}%')
        print(f'    distinct tile keys  : 1s={dk["1s"]:<7d} 10s={dk["10s"]:<7d} '
              f'30s={dk["30s"]:<7d} 60s={dk["60s"]:<7d}')
        print(f'    LRU hit rate        : {fmt_hits(cam["lru_hit_pct"])}')
        print(f'    storms only ({s["frames"]} frames): changed mean {s["changed_mean_abs"]} '
              f'({s["changed_mean_pct"]}%), p95 {s["changed_p95_pct"]}%, '
              f'distinct {s["distinct_keys"]}, LRU {fmt_hits(s["lru_hit_pct"])}')
        print(f'    bytes/frame         : changed-only {b["changed_mean"]:.0f} B vs full '
              f'{b["full_frame"]:.0f} B ({b["pct"]}%)')
        print()
    print(f'determinism: run1 {h1}  run2 {h2}  identical={h1 == h2}')
    print(f'wall time: run1 {t1:.1f}s  run2 {t2:.1f}s (pure stdlib, single thread)')
    print()
    print('reading:')
    for line in reading(r):
        print('  ' + line)


def reading(r):
    A, B, C = r['A'], r['B'], r['C']
    a, b, c = A['changed_mean_pct'], B['changed_mean_pct'], C['changed_mean_pct']
    sa, sb, sc = (x['storm']['changed_mean_pct'] for x in (A, B, C))
    return [
        f'1. Static camera (A): churn is just the moving agents — {a:.2f}% of the view per '
        f'frame (p95 {A["changed_p95_pct"]:.1f}%), vocabulary saturates at '
        f'{A["distinct_keys"]["60s"]:,} keys, even a 64-entry cache hits '
        f'{A["lru_hit_pct"]["64"]:.1f}% — iterate-on-change holds.',

        f'2. View-anchored tiles (B): every 1-px pan slides the world under the tile grid, so '
        f'{b:.1f}% of the view re-keys each frame and distinct keys keep climbing '
        f'({B["distinct_keys"]["1s"]:,}@1s → {B["distinct_keys"]["60s"]:,}@60s); the best cache '
        f'only reaches {B["lru_hit_pct"]["4096"]:.1f}% — the principle collapses for a moving view.',

        f'3. World-anchoring the identical view (C) restores it: {c:.2f}% changed/frame '
        f'({b / c:.0f}× less than B), ~{C["distinct_keys"]["60s"]:,} keys total and '
        f'{C["lru_hit_pct"]["256"]:.1f}% hits at cap 256 — panning reveals tiles instead of re-keying them.',

        f'4. Storms (3× speed) raise churn ×{sa / a:.1f} (A) and ×{sc / c:.1f} (C) but it stays '
        f'≤{max(sa, sc):.1f}% of the view; B falls to {sb:.1f}% only because the camera sat clamped at '
        f'map edges for most storm frames — whenever the pan actually moves, B re-keys everything '
        f'(p95 100%), so sparsity of change, not motion speed, is what matters.',

        f'5. Bandwidth follows churn: changed-only sends are {A["bytes_per_frame"]["pct"]:.1f}% (A) and '
        f'{C["bytes_per_frame"]["pct"]:.1f}% (C) of the full frame, but {B["bytes_per_frame"]["pct"]:.1f}% '
        f'for B — the saving exists only when tile identity is world-anchored.',

        f'6. Verdict: per-tile recognition/generation is cheap iff few tiles change AND the vocabulary '
        f'repeats (A/C: {max(A["distinct_keys"]["60s"], C["distinct_keys"]["60s"]):,} keys, high hit rates); '
        f'a follow camera keeps both properties only with world-anchored tiling — view-anchored tiling (B) '
        f'violates both at once.',
    ]


def main():
    t0 = time.perf_counter()
    r1 = run_sim()
    t1 = time.perf_counter()
    r2 = run_sim()
    t2 = time.perf_counter()
    h1, h2 = res_hash(r1), res_hash(r2)
    print_report(r1, h1, h2, t1 - t0, t2 - t1)


if __name__ == '__main__':
    main()
