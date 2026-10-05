# Continuous launch-film production

The user supplied an Apple keynote inspired, entirely 2D continuous-take script. Adapt its subject to the actual product: BranchBox worktrees, setup choices, dev containers, commands, and reviewed teardown replace photographs, print shopping, delivery, and wall footage. This is original product motion treatment, not a recording of app interaction.

Use a warm off-white canvas with black UI; Archivo at width 125 / weight 800 for the wordmark, Geist for UI. Fonts are bundled with their SIL Open Font licenses. See `assets/manifest.json` for official source URLs. Avoid stock footage when unnecessary.

## Motion language

- Every scene grows from the previous shape. Text rises through its own mask; icons draw or spring from zero; cards unfold; windows expand from a pill; a black flood covers the corners before it contracts into the next scene.
- Use a visible cursor only for interactions being explained. Scale it with the camera. Real screenshots can be framed or enlarged, but animation must not fabricate a successful click or result.
- No crossfades, blur-ins, developing brightness, 3D flips, particles, glows, or static holds longer than one second. Something purposeful happens on each half-second beat.
- Opening letters squeeze towards the period, then the period becomes the action pill. Return via that same geometry; last frame equals the first.
- Six iris blades follow a hexagonal aperture with the short connecting arc. Ensure any flood overscales past all four corners over about 0.3 seconds; never switch half a frame in one step.
- Text swaps happen behind a dedicated closed mask. Do not keep a child explicitly visible under a hidden parent.

## Deterministic rendering

One HTML entrypoint, square 1440×1440, `async seek(t)` computes all visuals from time. No CSS transitions, timers, random state, or frame-to-frame accumulation. Use closed-form damped spring step responses; for a value with several targets sum one spring for each change. Clip or zoom real captures without blurring their text.

For glass treatment clone the scene behind each element. Use a rounded-rectangle distance field as an SVG `feImage` displacement map; offset RGB edges with three `feDisplacementMap` scales and a thin rim. Chromium `backdrop-filter: url()` does not reliably map the displacement. When glyph glass or goo is appropriate, use a per-glyph distance field / sharp source atop the blurred alpha-threshold mask; do not add those effects solely as decoration.

Footage, if used, is re-encoded all-intra (`ffmpeg -g 1`), loaded as a blob URL, and awaited on `seeked`. Do not depend on Python's HTTP server for range seeking.

Render four subframes per frame, blend the four with FFmpeg `tmix`, then emit 60 fps. Review beat samples and detect difference spikes exceeding three times both adjacent differences. Investigate flagged frames visually; a deliberate fast wipe is not automatically a failure. Compare repeat seeks in reversed order to verify time purity.

## Audio

Use the supplied royalty-free approximately 120 BPM song and downloaded SFX only after verifying exact license terms at primary source pages. Preserve license/source receipts and local hashes. Measure each SFX peak and align it to the event, align the zoom to the drop and final return to the beat return. Loudnorm the final mix to -14 LUFS. Do not synthesize SFX. If licensed audio is unavailable, deliver a clearly identified silent edition and keep the audio layer optional.

## Quality record

Keep the feature/claim evidence, source screenshot hashes, revision, beat map, proof stills, final duration/dimensions/frame counts, audio status, full-decode result, beat contact sheet, and difference-spike disposition. Flag stale UI or a screenshot that only proves a plan. Review the produced pixels before public integration, especially captured windows: a desktop rectangle may contain an unrelated foreground app.

## Run the maintained source

Requires Node.js, Playwright, Chromium, and FFmpeg; `qa.py` also uses Pillow and NumPy. Prefer the existing bundled workspace runtime (`load_workspace_dependencies`) or project tools. Do not install or download another browser when an available installed/cached binary works. If default Playwright discovery fails, set `PLAYWRIGHT_MODULE` to its module path and `CHROMIUM_PATH` to the local browser executable.

From the skill directory:

```sh
node scripts/render.mjs launch proof /tmp/branchbox-videos
node scripts/render.mjs launch movie /tmp/branchbox-videos
node scripts/render.mjs setup movie /tmp/branchbox-videos
node scripts/render.mjs teardown movie /tmp/branchbox-videos
python3 scripts/qa.py /tmp/branchbox-videos/branchbox-launch.mp4 --expected-duration 27 --output /tmp/branchbox-videos/qa-launch
```

The renderer serves only local skill assets on loopback, verifies required capture/font/source hashes and provenance, and writes only the chosen output directory. Focused modes load only their relevant screenshot. After intentional edits, review them then run `python3 scripts/refresh-manifest.py` to record the new hashes. A matching hash establishes asset identity, not the correctness of a product claim; recheck claims against current source and live receipts. Do not refresh hashes blindly to silence a stale-capture failure.
