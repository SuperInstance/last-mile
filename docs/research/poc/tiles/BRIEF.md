# E1 — Does "only iterate on what changed" hold for a game view? (THROWAWAY PoC)

Open question: recognizing or generating per tile is cheap only if few tiles change per frame and the tile vocabulary repeats. Measure it.
Write `sim.py` HERE ONLY: Python 3 stdlib, deterministic seeds, no network, no third-party code. Run it and print results plus a 6-line reading.

## Scene
- Top-down world, 512x288 px luma (0..255). Static map: floor texture (seeded value-noise, quantized), walls (rects), 3 beacons.
- 8 agents (6x6 px blobs at different luma) move at about walking speed with seeded random-walk goals.
- 30 fps for 60 s.
- Optional "storm" of events: 2 s bursts where all agents move at 3x speed.

## Encoding (independent re-implementation of the syzygy-lattice idea; don't import any repo)
- Cell = 2x4 px. Per cell:
  - luma band = mean luma * 10 // 256 (0..9);
  - orientation class from an integer 3x3 Sobel at the cell centre (flat/h/v/diag+/diag-; threshold |g| >= 16, else flat);
  - glyph byte = band*5 + class;
  - braille mask = 8 bits, threshold 128.
- Tile = 8x4 cells (16x16 px). Tile key = FNV-1a 64 of its cells' glyph+mask bytes.

## Cameras
- (A) STATIC full-map camera.
- (B) FOLLOW camera: a 256x144 view centred on agent 0, moving with it (sub-pixel positions floor to integers).
- (C) FOLLOW + WORLD-ANCHORED tiles: same view, but tiles are keyed on world-grid tiles (snap the tile grid to world coords), so a pan doesn't shift the tile grid.

## Metrics per camera
- changed_tiles_per_frame: mean, p95 (% of the tiles in view)
- cumulative distinct tile keys over time (print at 1, 10, 30, 60 s)
- LRU cache hit rate for capacities 64, 256, 1024, 4096 (a hit = key seen and still cached)
- the same three metrics during storms only
- determinism: two runs give an identical results hash

## Also
Report the mean bytes per frame if you send only changed tiles (key + 8x4 glyph+mask bytes) vs the full frame.
