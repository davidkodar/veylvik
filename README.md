# Veylvik

A Rust composition engine and native node editor, with Vulkan rendering and an
MCP interface. This is an early compositing foundation.

## Build and run

The workspace pins Rust 1.98.1 for reproducible builds (declared minimum 1.95)
and requires a Vulkan driver. Headless commands need no
display server; the native editor supports Wayland and X11.

```sh
cargo build --workspace --release --locked
./target/release/veylvik demo /tmp/example.veylvik
./target/release/veylvik check /tmp/example.veylvik
./target/release/veylvik render /tmp/example.veylvik /tmp/frame.png 24 24
```

Render time is exact `TICKS / RATE` seconds; `24 24` renders at one second.
Commands return JSON on stdout, errors as JSON on stderr, and a nonzero exit
status on failure. Rendering requires Vulkan; no silent CPU fallback occurs.

Import a still image, inspect available nodes, or render a crop:

```sh
./target/release/veylvik import photo.png /tmp/photo.veylvik
./target/release/veylvik nodes
./target/release/veylvik render /tmp/photo.veylvik /tmp/crop.png --crop 0 0 64 64
```

`render --node UUID` previews any existing node without changing the saved output.
`--crop X Y WIDTH HEIGHT` uses composition pixel coordinates and must lie inside
the composition. Crops are extracted after full-frame evaluation, so blur and
transform dependencies match a full render. This does not yet reduce GPU work.

The render compiler aliases constant zero exposure, zero translation, and
zero-radius blur directly to their input. Their authored nodes remain editable,
but they require no GPU dispatch or extra image slot. Animated controls retain
their passes even when the current evaluated value happens to be zero.
Identical pure operations with the same resolved inputs also share execution,
including whole two-pass blurs. Parameters and animation curves must match
structurally; labels, UUIDs and node positions do not affect sharing. Image and
vector sources retain their separate upload and coverage-cache policies.

## HDR frame export

Use an `.exr` output filename with CLI/MCP `render` or Studio's **Export…** picker
for lossless OpenEXR. Files contain 32-bit float R/G/B/A channels, linear Rec.709
primaries with D65 white, premultiplied alpha, and ZIP16 scanline compression.
RGB values retain HDR highlights, negatives, and emission at zero alpha. No
unpremultiplication, display transform, or RGB clamping is applied. Cropped output
is a standalone image with its origin at zero, as for PNG.

```sh
./target/release/veylvik render animation.veylvik /tmp/frame.exr 24 24
```

Encoding streams through a same-directory temporary file; failures before
publication preserve the existing destination. `.png` still exports SDR sRGB
with straight alpha. Other frame extensions are rejected. OpenEXR import is not currently exposed.

## Animation sequences

```sh
./target/release/veylvik render-sequence animation.veylvik /tmp/frames 0 95
```

Start and end are inclusive absolute frame indices at the composition's rational
frame rate. Negative frame indices are supported. Each request allows 1–10000
frames and validates every timestamp before writing. Optional `--node` and
`--crop` work as for a single frame. Use `--format exr` for HDR OpenEXR frames;
PNG remains the default. The output directory must not exist; its
parent must already exist. Outputs never replace earlier renders.

Files are `frame-000000.png` or `frame-000000.exr` onward, with timing, region, source frame range, and
completion recorded in `sequence.json`. Only `status: "complete"` denotes a
finished sequence. Interrupted or failed jobs can leave a `rendering` manifest
and usable completed frames. The library supports cancellation between frames,
recorded as `cancelled`. CLI progress is newline-delimited JSON on stderr; the
final result is JSON on stdout. One plan and source set serve the entire render,
and frames are written one at a time. The manifest records format, color space and alpha convention. PNG and OpenEXR
sequences use the same respective color and alpha rules as single-frame export;
no video encoding or audio is included.

MCP `render_sequence` takes `revision`, `path`, `start`, `end`, and optional `node`
and `region`, plus `format` (`png` or `exr`). It runs synchronously and returns the sequence manifest. The stdio
adapter does not yet expose mid-request cancellation or progress notifications.

## Blend controls

**Blend** combines foreground and background with Normal, Add, Multiply, Screen,
Darken, Lighten or Difference. Its opacity scales foreground RGB and alpha and
can be keyframed. All modes use source-over union alpha. Add sums premultiplied
RGB; its alpha remains in [0,1]. Blending operates in scene-linear Rec.709 without
RGB clamping, so HDR and negative values remain available downstream. Screen
can produce negative values for HDR inputs. Existing **Merge** nodes retain their
foreground-over-background behavior.

## Native editor

```sh
./target/release/veylvik open /tmp/example.veylvik
# Or start an unsaved demo:
./target/release/veylvik open
# Save the editable ribbon animation as a project:
./target/release/veylvik demo /tmp/ribbons.veylvik --motion
```

The viewer consumes GPU-resident frames on the same Vulkan device as the UI.
**View exposure** adjusts only the display by −16 to +16 stops. **Alpha** shows
coverage as grayscale, independent of exposure; **Reset view** restores normal
color viewing. These controls do not change the document, history or exports.
Scroll over the viewer to zoom around the pointer and drag to pan. **Fit** or
a double-click centers the whole frame; **100%** maps one image pixel to one
physical display pixel, including on scaled displays. Navigation changes only
the view; exports still use the complete requested frame. Live MCP inspection
reports viewer settings separately from the composition (`scale: null` means
fit mode; pan coordinates are logical UI points).
Select and arrange nodes, drag between output and input sockets to connect them,
add procedural nodes, and preview any node without changing composition output.
Socket drags work in either direction; Escape or dropping on empty space cancels.
Each accepted rewire is one undo step. Named inputs are also editable in the inspector. Inspector
changes are drafts until **Apply**; one apply creates one undo step. Invalid
connections fail atomically. **Revert** discards the draft. Apply or revert before
selecting another node or using undo/redo.

Scrub the timeline or play it to evaluate animation. Scalar controls can add a
key at the playhead; expanded key lists edit exact rational times and values or
remove keys. Choose Linear, Hold, or Ease in/out for each animated parameter.
**Make constant here** replaces a curve with its evaluated playhead value; Apply
and undo work as for other inspector edits.
Saving uses the same atomic persistence and source-image protection as MCP.
Open, import, save-as, and export use the desktop file portal. File selection and
image loading run on a bounded background worker. Save-as preserves the original
asset resolution; frame export uses the preview root and time captured when invoked.
Saving an opened project detects changed or deleted external content before
replacement and requires reopening or saving elsewhere. This check is not an
atomic filesystem transaction against unrelated simultaneous writers.
Closing asks about unsaved document or inspector changes.

**Sequence…** opens animation export settings. Choose inclusive first/last frames,
a new folder name, and an existing parent folder through the desktop portal.
Export captures the current preview root, composition, and loaded source pixels;
it runs in the background with a progress bar and **Cancel export** between
frames. Completed frames remain in the export folder if cancelled. Cancel the
folder picker itself if it is still open. Other project edits wait until export
finishes.

| Shortcut | Action |
|---|---|
| Space | Play / pause |
| Ctrl+O / Ctrl+I | Open composition / import PNG or OpenEXR |
| Ctrl+E | Export current preview to PNG or OpenEXR |
| Ctrl+Shift+E | Export PNG or OpenEXR sequence |
| Ctrl+S | Save |
| Ctrl+Shift+S | Save as |
| Ctrl+Z / Ctrl+Shift+Z | Undo / redo |
| Q | Close |
| ? | Show shortcuts |

The editor follows the current Omarchy accent, with a fallback when Omarchy is
absent. Node positions are saved with the project; each drag is one undo step.
Drag the graph background to pan, Ctrl+scroll to zoom, and use **Fit** to see
the whole graph. **Arrange** restores an automatic layout as one undoable edit. Source images load when the
project opens or an image is imported, within a 256 MiB host image budget. The
initial UI uses English labels. A working `org.freedesktop.portal.FileChooser`
service is required for interactive file selection; failures are shown in the
window. Headless file commands remain available without a desktop service.
The viewer filters scaled images bilinearly in linear premultiplied color,
clamps sampling to the requested crop, composites transparency over a checkerboard,
and applies an SDR display transform. A previous preview stays visible while rendering, explicitly marked
when it no longer matches the requested frame.

## Composition and rendering

Projects are versioned JSON containing nodes with stable UUIDs, named input
ports, and an output node. Validation, editing, and saving share a 16 MiB limit
measured as canonical pretty-printed JSON, preventing edits from exceeding the
format’s load limit. Undo/redo retains at most 64 snapshots and 128 MiB of serialized
snapshot data across both directions, dropping distant history first. Hosts may
request a smaller history budget. This accounting excludes allocator overhead
and the current document; it is not a whole-process RAM limit. MCP inspection
includes retained history statistics. Supported operations:

| Operation | Inputs | Parameters |
|---|---|---|
| Solid | none | Straight linear RGB plus alpha; converted to premultiplied pixels |
| Noise | none | Seed and cell size, 1–8192 pixels |
| Fractal noise | none | Smooth grayscale noise; seed, scale, octaves, roughness, evolution |
| Exposure | image | −32 to +32 stops |
| Transform | image | X/Y translation in pixels, bilinear sampling |
| Displace | image, map | Animated X/Y strength in pixels; red/green map channels, alpha-weighted |
| Blur | image | Box radius, 0–32 pixels, rounded to an integer |
| Merge | background, foreground | Premultiplied foreground-over-background |
| Image | none | Asset key; native image size, top-left anchor, transparent padding |
| Affine | image | Translation, signed X/Y scale, clockwise rotation, pixel-space pivot |
| Ellipse | none | Center, X/Y radii, and radial feather in pixels |
| Channel mask | image | Convert luminance, red, green, blue, or alpha into white mask coverage |
| Mask | image, mask | Multiply premultiplied image by mask alpha; ignore mask RGB |
| Path | none | Closed vector contours and fill rule |
| Blend | background, foreground | Blend mode and animated opacity |

Scalar parameters accept constants or `linear`, `step`, and `smooth` keyframes.
Step holds each value until the next exact key time. Smooth applies cubic
smoothstep (`3t² − 2t³`) within each interval, easing in and out without
overshooting the endpoint values. Curves contain 1–4096 keys. Outside
the key range they hold the nearest endpoint. Input boundaries are transparent
black. Image dimensions are 1–8192 per axis, subject to GPU limits and budget.
Affine transforms scale around the pivot, rotate clockwise, then translate;
integer coordinates address pixel centers. Negative scales flip the image and
scale magnitudes below 0.000001 produce transparent output. Ellipse feather uses
normalized radial distance scaled by the smaller radius, with a minimum one-pixel
transition; it is not an exact signed-distance feather on noncircular ellipses.
Zero ellipse radius produces transparent output. All these scalar controls can
be animated, and their registry supplies both inspector bounds and MCP schemas.

Intermediate images use scene-linear Rec.709 premultiplied RGBA32F. Values above
one are preserved. PNG export unpremultiplies, clips to SDR, and applies sRGB
encoding; it is not an HDR delivery format or a tone mapper.

The compiler validates the entire graph, prunes unreachable nodes, and assigns
explicit compute passes and reusable image-buffer slots by last use. Box blur
lowers to horizontal and vertical passes, with transparent boundaries. Intermediate pixels stay on the GPU;
export reads back only the final image. `render_gpu` instead returns an immutable
GPU frame, reading only a four-byte finite-value status flag. Held frames count
against subsequent render budgets and are never recycled as intermediates.
`read_frame` explicitly downloads one, including its requested crop. A GPU
consumer must retain the frame until its submission finishes. One device and queue own all resources.
Completed source buffers retain immutable image identities while idle in the
existing pool, avoiding repeated uploads during animation. Aliased asset names
sharing one image also share one upload and budget allocation. Ordinary buffer
reuse clears source identity; trimming or memory pressure may evict it. Resource
statistics report source uploads, bytes uploaded, and source-content reuse.
The default 512 MiB render budget covers declared GPU image buffers, readback,
uniforms, and each unique source image upload; it does not measure driver overhead
or bound total process RAM. `check` reports graph working bytes before source
uploads and also checks the reachable image inputs against the combined budget.

The renderer receives immutable `ImageInputs` from its host and never opens files.
Hosts can also insert a completed `GpuFrame` with `insert_gpu`. The image node
reads its buffer directly, using the frame's crop as a top-left source view;
there is no pixel download, repacking, or reupload. GPU sources must come from
that same renderer and remain retained by the input snapshot. Their allocations
already count as held GPU memory and are not charged again as uploads. Cache
keys include immutable frame identity. `ImageInputs::byte_size` continues to
count CPU pixels only; `contains_key` covers both CPU and GPU sources. This is a
render-library boundary, not yet a Klipp connection.

External GPU producers can render into a `TextureTarget` allocated by
`Renderer::create_texture_target` on the shared device, then call `import_texture`
to capture an immutable frame. RGBA8/BGRA8 unorm, RGBA16F and RGBA32F targets are
supported. The caller specifies linear or sRGB RGB and straight or premultiplied
alpha; the GPU converts to linear premultiplied Rec.709 without clipping HDR.
Only a four-byte validation flag is read back. Foreign-renderer targets are
rejected before GPU use; later producer writes do not change imported frames.
Producer texture memory remains caller-owned, outside the buffer pool budget;
each target allocation is capped, but the host must bound aggregate producer
allocations. Submit producer writes on the shared queue before importing, and
retain the target without modifying or destroying it until import returns.
`veylvik_document::vector::PathMask` stores closed contours with line, quadratic,
and cubic Bézier segments and nonzero/even-odd filling. `render_path_mask`
produces a GPU frame suitable for `insert_gpu` and the Apply mask node. Geometry
preparation uses Vello Hybrid on CPU; coverage rasterization uses Vulkan. The
coverage target is 8-bit, while the masked image retains scene-linear HDR color.
Paths are limited to 4096 commands, coordinates within ±32768, and dimensions up
to 4096 per axis. The Path mask node persists this geometry in the composition and exposes its
strict schema through MCP. Studio edits coordinates, fill rules, segments and
contours in the inspector, with Apply/Revert and shared undo. Animate the result
with an Affine node; path-point keyframes and on-canvas pen editing are not yet
implemented. Vello scratch memory is separate from the
compositor buffer budget; image atlases cannot grow and intermediate layers are
disabled. `release_vector_resources` frees its persistent pipelines and scratch
resources without invalidating retained output frames. Graph renders retain up
to 16 completed vector masks within a separate 32 MiB cache limit, keyed by exact
geometry, fill rule and canvas size. These buffers count against the same GPU
buffer budget as other held frames. Animation of downstream nodes reuses coverage;
changed geometry misses the cache. Memory pressure evicts retained masks even for
non-vector renders. `vector_cache_stats` reports reuse and `clear_frame_cache`
clears both final-output and vector-coverage caches.

`veylvik-text` shapes a single horizontal directional/script run from immutable
host-supplied font bytes, using HarfRust and vector font outlines. It returns
visual-order glyph IDs, UTF-8 clusters, advances, positions, missing-glyph flags,
and an optional `PathMask`. Ligatures, kerning and combining marks remain the
shaper's responsibility. Empty and whitespace-only runs have no outline. Font
files are capped at 32 MiB, text at 4096 UTF-8 bytes, positioned runs at 512 glyphs,
and combined outlines at the path limit. It performs no font discovery, fallback,
paragraph bidi layout, wrapping or bitmap/color-glyph rendering. This library
boundary does not yet provide a live editable Text node.

`shape_lines` adds explicit LF/CRLF line breaks, up to 64 lines, with left,
center or right alignment around the requested baseline x coordinate. Blank lines
retain their spacing. Each line must still be one directional/script run; this
is not automatic mixed-direction paragraph layout. The 4096-byte, 512-glyph and
4096-command limits apply to the entire block. Returned `advance` contains the
maximum line advance and the first-to-last baseline distance, not ink bounds.

In Studio, choose **Add node → Outline text…**, enter text with line breaks, and
choose a TTF/OTF/TTC font through the desktop picker. Conversion runs in the
background and inserts one undoable Path mask at the graph's insertion position.
Cancellation or an invalid font leaves the document unchanged and reopens the
text settings. The original font is read only.

Convert one text run into a portable composition with an explicit font:

```sh
cargo run -p veylvik-cli -- outline-text /path/to/font.ttf "Veylvik ffi Å" title.veylvik --size 64 --origin 40 100 --canvas 960 240
```

Use `--align left|center|right` and `--line-height N` for explicit line layout.
The default baseline spacing is 1.25 times font size; the native dialog provides
an editable pixel spacing. Newlines are actual characters in the text argument.

The output file must be new. `--rtl`, `--language TAG` and `--font-index N`
select run direction, language and a face within a font collection. Missing glyphs
and text without visible outlines are rejected. The resulting Path mask can be
edited or animated in Studio without keeping the source font installed. It is a
conversion to geometry, so changing the wording requires a new conversion.
Headless MCP exposes `outline_text` with the same inputs and a revision
precondition; it returns a Path operation for an explicit `apply` transaction and
does not change the document itself. Font paths resolve against the opened
composition directory.

The CLI/MCP adapter resolves image asset keys as PNG or OpenEXR paths relative to the opened
composition's directory (absolute paths also work). Save-as rebases relative paths
without mutating the live session. Save/export refuse to replace an input image.

PNG input supports 8/16-bit RGB, RGBA, grayscale, and expanded palettes. Untagged
images are interpreted as sRGB; standard sRGB tags are accepted. Unsupported ICC,
cICP, non-sRGB gamma/primaries, and animated PNG are rejected. Samples are converted
to linear light before premultiplication, preserving 16-bit precision. Input files
are limited to 64 MiB and each decoded float image to 128 MiB, also subject to the
remaining render budget. Images are reloaded for each request; there is no stale
image cache. Input orientation metadata is not applied.

## MCP

Launch a session against a saved composition:

```sh
./target/release/veylvik mcp /absolute/path/example.veylvik
```

The server uses newline-delimited JSON-RPC on stdio and MCP protocol version
2025-11-25. Initialize, then send `notifications/initialized` before using tools.
The tools are `inspect`, `nodes`, `apply`, `undo`, `redo`, `save`, `render`, and
`render_sequence`.
`nodes` provides defaults, parameter units/bounds, named ports, and operation JSON
schemas from the document registry. The same registry validates scalar parameters
and supplies input ports to the compiler. The `apply` tool embeds these schemas. Every mutation, save, or render
requires the current revision from `inspect`. A batch is validated atomically
and becomes one undo step. `apply` with `dry_run: true` changes nothing.

The `render` tool optionally accepts `node` (a UUID) and `region` (an object with
`x`, `y`, `width`, `height`). It returns the rendered region, path, and document
revision. An invalid preview target or region is rejected before GPU creation.

The headless session owns an independent in-memory document. Edits persist only through `save`;
external file edits are not live-synchronized. Save and render can replace the
explicit destination path, except for a referenced input image. The MCP client should expose those actions for user
review. Output currently returns the image path, not inline image content.

To control the open native editor instead, start it with a new private endpoint
directory, then point an MCP client's stdio command at the proxy:

```sh
./target/release/veylvik open example.veylvik --control-dir /tmp/veylvik-live
# MCP client command, while that editor remains open:
./target/release/veylvik mcp --studio /tmp/veylvik-live
```

Live control is opt-in and uses a local Unix socket, with directory permissions
0700 and socket permissions 0600. Existing endpoint directories are refused.
Normal editor shutdown removes its endpoint; after an abnormal exit, use a new
directory or remove the stale endpoint yourself. One client connects at a time.
The toolbar shows **MCP** when live control is enabled.

Live tools are `inspect`, `nodes`, `apply`, `undo`, `redo`, and `preview`.
`inspect` returns a project `session` UUID and `revision`; both are required for
edits, undo/redo, and preview. Opening a project creates a new session identity,
so an old command cannot target a newly opened project with the same revision.
Edits run on the UI thread, appear in the editor, and share its undo history.
`apply` supports dry runs and uses the same command schema as headless editing.
Unapplied inspector drafts, active node drags, and file jobs block remote changes.
Image assets must already be loaded through the editor. `preview` accepts an
absolute `frame` in 0–1000000 and optional `node`, pauses playback, and requests
an asynchronous GPU preview. Live tools do not save or export files; normal UI
file actions remain available. Both transports bound incoming messages to 4 MiB.

## Development

```sh
cargo fmt --all --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked
cargo test --workspace --locked -- --ignored --nocapture --test-threads=1
```

The final command is the mandatory Vulkan check. It fails if no adapter is
available. Default tests leave it explicitly ignored so document/control tests
can run on machines without graphics. CI runs it with Mesa's software Vulkan;
hardware validation remains a separate check.

The workspace has five boundaries:

- `veylvik-document`: serialization, exact time, curves, graph validation,
  transactional editing, undo/redo, and atomic persistence. No GPU dependencies.
- `veylvik-render`: immutable compiled plans, buffer lifetimes, compute kernels,
  headless rendering, and the SDR output transform. No UI dependencies.
- `veylvik-io`: PNG/OpenEXR decoding, source protection, and path-aware persistence.
- `veylvik-control`: shared MCP lifecycle/framing and bounded local transport.
- `veylvik-cli`: command-line and MCP adapters using those public APIs.
- `veylvik-studio`: native Rust UI, shared-device display, and document controls.

The executor is whole-frame. `PreviewScheduler` runs it on a dedicated worker,
keeps at most one active and one pending request, and replaces obsolete pending
work. Generation and document revision accompany every completion. Cancellation
suppresses old results; it does not interrupt submitted GPU work. A wake callback
can notify a UI without polling. Dropping the scheduler joins the worker.
`GpuPreviewScheduler` shares a host-owned Vulkan renderer and produces these
GPU-resident frames. `Renderer::from_device` integrates with a window host on
the same device and queue. Direct `Renderer` calls remain synchronous. Plans, pipelines, and GPU
buffers can be reused. A bounded pool retains up to 128 MiB of idle buffers by
default, within the render budget. Buffers return to the pool only after GPU
completion and readback unmapping. `Renderer::resource_stats` reports allocations,
reuse, evictions, and declared resident bytes; `trim_resources` releases idle
buffers. `render_gpu_cached` and the GPU preview scheduler additionally retain
completed output in a 64 MiB, at-most-16-frame LRU cache. Exact keys include
compiled dependencies, evaluated parameter bits, crop, and immutable source
identities. Label edits, unreachable nodes, and times evaluating to the same
values can reuse output. Changed sources and parameters invalidate it. Cache
retention counts toward the render budget and yields to memory pressure.
`cache_stats` reports hits/misses/evictions; `clear_frame_cache` releases cache
ownership. Direct CPU renders and intermediate node results are not cached. There is
no video decoder, text engine, plug-in loader, simulation, or Klipp adapter
in this version. No Klipp source is copied or linked.

### Displacement maps

Displace samples its image at `pixel - strength * (2 * map.rg - map.a)`.
Here `map.rg` is premultiplied and clamped between zero and map alpha:
straight map red/green values of 0.5 are neutral, 0 and 1 give negative and
positive displacement, and alpha attenuates the effect. Transparent maps are
neutral even when they contain emissive RGB. Positive strength moves image
features right/down where the map is white. X/Y strengths animate independently
within ±8192 pixels. Image sampling is bilinear in linear premultiplied RGBA,
with transparent borders; image HDR and negative values are preserved. Map
values are interpreted in scene-linear space, including imported PNGs after
sRGB decoding. Blur a Noise output for a smooth procedural distortion map.

### Animated fractal noise

Fractal noise produces opaque grayscale values in [0,1] from deterministic
three-dimensional value noise. Quintic interpolation joins lattice cells
smoothly. `scale` sets the first octave cell size in pixels (1–8192), `octaves`
rounds to 1–8 layers, and `roughness` (0–1) multiplies each successive layer's
amplitude. Each octave doubles frequency and the weighted sum is normalized.
`evolution` (−8192 to +8192) travels through the third dimension; keyframe it for
continuous texture animation. Evolution is not a looping phase. All controls
except the integer seed accept animation curves. High frequencies can alias;
this generator does not yet apply footprint-aware filtering. The original
colored cell Noise retains its existing output for saved compositions.

Connect Fractal noise to Displace's map port for smooth diagonal distortion,
or transform and combine maps upstream for other patterns. Its RGB is grayscale
and alpha is one; it is a color texture, not an alpha mask.

### Channels as masks

Channel mask converts an image channel into white premultiplied mask coverage.
Select Red, Green, Blue, Alpha, or scene-linear Rec.709 Luminance
(`0.2126 R + 0.7152 G + 0.0722 B`). The selected premultiplied value is clamped
to `[0, source alpha]` and written to all four output channels. This preserves
transparent boundaries and avoids division by tiny alpha. Negative values
become zero and HDR values saturate at source alpha. Connect Fractal noise to
Channel mask, then connect that mask to Apply mask to reveal an image through
an animated procedural texture. Alpha extraction preserves coverage but replaces
image color with white.

Compiled preview plans are reused across node renames, layout changes, frame-rate
changes, and edits to disconnected branches when the preview root, dimensions,
reachable operations, and input wiring are unchanged. The candidate document is
still fully validated. Parameter, topology, resolution, and preview-root changes
recompile; animation continues to evaluate at the current exact time. Key
comparison retains timestamp representation to preserve identical floating-point
evaluation. This complements the GPU frame cache, which compares evaluated render
inputs and therefore also reuses equal pixels across different times.

### OpenEXR image inputs

CLI import, native import, saved image nodes, and MCP rendering accept `.exr`
assets. The initial supported subset is single-part version-2 scanline EXR with
exactly R/G/B/A half- or full-float channels, no subsampling, matching origin-zero
data/display windows, and square pixels. Raw, RLE, ZIP1, and ZIP16 compression
are supported. Chromaticities must be Rec.709/D65, or absent (the EXR default).
Pixels are interpreted as scene-linear premultiplied RGBA and preserved without
SDR clipping. Nonfinite values, alpha outside [0,1], and nonzero RGB at zero
alpha are rejected by the image-input contract. Tiled, deep, multipart, other
color spaces, extra channels, and cropped data windows are not yet imported.

Files are bounded to 64 MiB, headers to 1 MiB/256 attributes, dimensions to
8192 per axis, and decoded RGBA32F to the smaller of the caller's budget and
128 MiB. Header checks precede pixel allocation; serial decoding uses the same
immutable file bytes that were checked. Original input files remain protected
from save/export overwrites, including EXR inputs. EXR export supports a wider
pixel contract, including emission at zero alpha, so not every exported frame
is currently accepted as an image input.

Expanding an animated parameter's key editor shows a read-only curve preview
with key markers and the current playhead. Linear, Hold, and Ease in/out draw
their actual interpolation shape; the horizontal span runs from first to last
key and the vertical range fits the key values. Invalid drafts omit the plot
until corrected. Editing still uses the exact time/value controls below the
plot, and Apply commits the complete inspector draft as one undo step.

### Renderer diagnostics

The native **Stats** window and MCP `diagnostics` tool report declared GPU
buffer residency, held frames, idle buffers, allocation/upload reuse, and frame
and vector cache hits, misses, sizes, and limits. Reading diagnostics never
waits for an active render: `snapshot` is null while the required renderer locks
are busy. The headless server reports `initialized: false` until its first render;
inspection alone does not initialize Vulkan. These counters exclude driver and
vector rasterizer scratch allocations and are not total process or VRAM usage.
Diagnostics are read-only and do not change the document revision or clear caches.

### Crash recovery

Studio writes a separate recovery copy of unsaved **applied** edits at most once
every ten seconds while its event loop runs. Copies live in
`$XDG_STATE_HOME/veylvik/recovery`, or `~/.local/state/veylvik/recovery` when the
state variable is unset. Each session has its own private file, published
atomically by a background worker with one active and one latest pending write.
No recovery copy replaces a project or an original image. Image paths become
absolute in the copy so recovery does not depend on the old project directory.

After an interrupted session, the recovery dialog offers available copies;
**Recover** reopens that list. Running editor sessions are excluded using OS
file locks, which release automatically on a crash. A restored composition is
unsaved and uses Save As. Saving or explicitly discarding it removes its recovery
copies. Failed restores leave the copy intact. Other sessions' copies are
retained. Copies retain the 16 MiB document limit; up to 256 available records
are listed, with no automatic deletion of older unsaved work.

Recovery does not include an unapplied inspector draft, undo history, source
image bytes, or edits made since the last completed snapshot. It supplements
normal saving. A recovery storage error is shown; normal project saving remains
available. If storage is unavailable at startup, restart after fixing it to
re-enable background recovery.

### New compositions and format settings

**New…** (`Ctrl+N`) creates a composition with one transparent Background node.
Choose canvas width/height and an exact frame-rate numerator/denominator before
creating it. Unsaved work is protected by the same Save/Discard/Keep editing
flow as opening another project. **Settings…** changes the current canvas and
frame rate together as one undo step. Resizing changes the canvas bounds; it does
not scale node coordinates. Animation keys retain their times in seconds, while
frame indices use the new frame rate. Format dialogs isolate their drafts and
block live MCP mutations until applied or closed.

MCP `apply` accepts `{"command":"frame_rate","frame_rate":{"ticks":30000,"rate":1001}}`
alongside the existing `resize` command. Both can be placed in a single atomic
batch. Numerator and denominator must be positive; invalid batches leave the
composition and history unchanged.
