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

On Arch/Omarchy, build a local desktop package from the checkout:

```sh
./bin/package
sudo pacman -U target/packages/veylvik-0.1.0-1-x86_64.pkg.tar.zst
```

`bin/package` creates a source archive with a verified SHA-256 checksum, builds
locked release binaries through `makepkg`, and writes the package under
`target/packages/`. It uses your configured Git name and email for package
metadata. Set `CARGO_NET_OFFLINE=true` to build from an existing dependency cache.
The recipe supports native x86_64 and aarch64 builds; the filename reflects the
host architecture. Required build tools are Cargo/Rust, Arch `base-devel`, and
`desktop-file-utils`.

The package installs `veylvik` and `veylvik-studio`, a **Veylvik** launcher entry,
and an SVG icon, and registers `.veylvik` compositions for **Open with Veylvik**.
The launcher uses the same Wayland app ID as the editor. File dialogs require a
working desktop portal backend. Remove the package with `sudo pacman -R veylvik`;
project files and recovery data remain in their original locations.

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
with straight alpha. Other frame extensions are rejected. The supported OpenEXR input subset is described below.

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

**Colorize** maps input luminance to **Shadows** and **Highlights** colors.
The colors are straight scene-linear RGBA, including HDR or negative RGB. The
node interpolates their premultiplied values and multiplies by source coverage.
Input luminance is clamped to black–white before mapping; fully transparent input
stays transparent. Endpoint alpha can turn a noise field into colored coverage.
Colors are static parameters; place animated nodes before or after the mapping.

## Native editor

```sh
./target/release/veylvik open /tmp/example.veylvik
# Or start an unsaved demo:
./target/release/veylvik open
# Save the editable ribbon animation as a project:
./target/release/veylvik demo /tmp/ribbons.veylvik --motion
# Or an asset-free procedural energy animation (960 × 540, six seconds):
./target/release/veylvik demo /tmp/energy.veylvik --procedural
./target/release/veylvik open /tmp/energy.veylvik
```

The viewer consumes GPU-resident frames on the same Vulkan device as the UI.
Playback lets each requested preview finish, then skips ahead to the current
clock time if rendering is slower than the composition frame rate. Scrubbing
and explicit edits still replace obsolete requests. Render errors stop playback.
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
**Add node** or **Tab** opens searchable node choices. Type part of a name or
description, use Up/Down to choose, and Enter to add; Escape cancels. New effects
connect to the selected node, and procedural nodes appear at the graph position
where the picker opened. Gradient endpoints and shape defaults follow the
composition size. Image and outlined-text choices open their import dialogs.
Socket drags work in either direction; Escape or dropping on empty space cancels.
Each accepted rewire is one undo step. Named inputs are also editable in the inspector. Inspector
changes are drafts until **Apply**; one apply creates one undo step. Apply and
Revert stay visible above the scrolling inspector. **Ctrl+Enter** applies the
current draft, including while editing a parameter field. Invalid
connections fail atomically. **Revert** discards the draft. Apply or revert before
selecting another node or using undo/redo.

Scrub the timeline or play it to evaluate animation. Scalar controls can add a
key at the playhead. For animated parameters, **At playhead** edits an existing
key or inserts one at the current frame while preserving interpolation. Equivalent
rational times update the existing key without changing its stored timestamp.
These edits pause playback and remain drafts until Apply. Expanded key lists edit exact rational times and values or
remove keys. **Remove key at playhead** deletes an exact-time key; removing the
last key keeps its value as a constant. Choose Linear, Hold, Ease in/out, or Auto smooth for each animated parameter.
**Make constant here** replaces a curve with its evaluated playhead value; Apply
and undo work as for other inspector edits.
When the selected ellipse is the displayed preview, orange viewer handles edit
its center and horizontal/vertical radii. The outline follows the draft; the
rendered image updates on **Apply**. **Revert** discards the gesture, and an Apply
is one undo step. Constants remain constant; animated parameters insert or update
keys at the playhead. Handles follow viewer zoom and pan, are hidden during
playback or stale previews, and never appear in exported images.

**Affine transform** has a center handle for translation, two circular axis
handles for independent X/Y scaling, and a square handle for clockwise rotation.
Scaling along a rotated axis preserves the other axis; dragging through zero
allows flipping. The pivot stays unchanged and remains editable in the inspector.
The outline shows the transformed composition bounds, not detected image content.
**Transform** has a translation handle at the translated composition center.
These gestures use image coordinates at any viewer zoom, preserve animation
modes, and remain drafts until Apply. Pan by dragging away from a handle.

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
| Tab | Search and add a node |
| Ctrl+O / Ctrl+I | Open composition / import PNG or OpenEXR |
| Ctrl+E | Export current preview to PNG or OpenEXR |
| Ctrl+Shift+E | Export PNG or OpenEXR sequence |
| Ctrl+S | Save |
| Ctrl+Shift+S | Save as |
| Ctrl+Z / Ctrl+Shift+Z | Undo / redo |
| Ctrl+D | Duplicate selected node |
| Ctrl+Shift+D | Duplicate selected node and its upstream branch |
| Ctrl+Enter | Apply inspector draft |
| Ctrl+R | Reload source images |
| Q | Close |
| ? | Show shortcuts |

The editor follows the current Omarchy accent, with a fallback when Omarchy is
absent. Node positions are saved with the project; each drag is one undo step.
**Duplicate node** (Ctrl+D) copies the selected node with its animation and input
connections, selects the copy, and creates one undo step. The composition output
stays unchanged. Apply or revert inspector drafts before duplicating.
**Duplicate branch** (`Ctrl+Shift+D`) copies the selected node and all upstream
nodes as one undo step. Connections are remapped entirely within the copy, and
shared ancestors are copied once. Animation, node versions, and image asset
keys are preserved; source files and loaded image pixels are shared. The copied
branch keeps relative node positions and is placed outside existing graph bounds.
The copied root becomes the preview; the composition output stays unchanged.
Copying is rejected atomically if document size, node count, or graph coordinate
limits would be exceeded.

Drag the graph background to pan, Ctrl+scroll to zoom, and use **Fit** to see
the whole graph. **Arrange** restores an automatic layout as one undoable edit. Source images load when the
project opens or an image is imported, within a 256 MiB host image budget.
**Reload images** (Ctrl+R) reads all current sources in the background and refreshes
preview after every file succeeds. A failed reload retains all previous inputs;
fix the source and retry. Reload pauses playback and requires a clean inspector
draft, but does not edit the composition or history. Each replacement snapshot
has the same 256 MiB bound; the old snapshot remains resident while it loads.
Loaded sources retained for undo/redo also count toward that bound and survive
reloads. Reopening the project releases sources from discarded editor history.
An Image node's **Replace source…** button chooses a new PNG or OpenEXR file in
the portal. It preserves the node's identity, connections and position as one
undoable edit. Canceling or failing to decode the file leaves the node unchanged.
Selecting the same asset path refreshes its pixels without adding a document edit.
Studio opens valid compositions even if image files are missing, unreadable or
unsupported. Available images load normally; unavailable sources are marked in
the toolbar and Image inspector, so their paths can be replaced or repaired and
reloaded. Outputs depending on unavailable images report a render error; the graph
and its saved structure remain editable. This also applies to recovery copies.
Headless rendering continues to reject unavailable required inputs.
The initial UI uses English labels. A working `org.freedesktop.portal.FileChooser`
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
| Gradient | none | Animated start/end coordinates, linear or radial grayscale ramp |
| Fractal noise | none | Smooth grayscale noise; seed, scale, octaves, roughness, evolution |
| Exposure | image | −32 to +32 stops |
| Transform | image | X/Y translation in pixels, bilinear sampling |
| Displace | image, map | Animated X/Y strength in pixels; red/green map channels, alpha-weighted |
| Blur | image | Box radius, 0–32 pixels, rounded to an integer |
| Grow/shrink mask | image | Signed alpha radius, −32 to +32 pixels; square neighborhood |
| Combine masks | mask, other | Union, intersect, subtract, or exclude soft alpha coverage |
| Colorize | image | Straight HDR low/high colors mapped through luminance and source alpha |
| Merge | background, foreground | Premultiplied foreground-over-background |
| Image | none | Asset key; native image size, top-left anchor, transparent padding |
| Affine | image | Translation, signed X/Y scale, clockwise rotation, pixel-space pivot |
| Ellipse | none | Center, X/Y radii, and radial feather in pixels |
| Channel mask | image | Convert luminance, red, green, blue, or alpha into white mask coverage |
| Mask | image, mask | Multiply premultiplied image by mask alpha; ignore mask RGB |
| Path | none | Closed vector contours and fill rule |
| Blend | background, foreground | Blend mode and animated opacity |

Scalar parameters accept constants or `linear`, `step`, `smooth`, and `monotone` keyframes.
Auto smooth (`monotone`) uses cubic Hermite interpolation with shared weighted
harmonic interior slopes and adjacent secant slopes at the ends. It carries
velocity through intermediate keys, flattens at reversals and plateaus, and stays
between neighboring key values. Two keys produce a linear segment; values hold
outside the authored range. Time differences are computed exactly before float
conversion, including near the timestamp limits.
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
explicit compute passes and reusable image-buffer slots by last use. It visits
input branches in depth-first port order, keeping independent layers close to
their consumers instead of scheduling by UUID. Traversal is iterative for deep
graphs; this is a deterministic heuristic, not a minimum-memory optimizer. Box blur
lowers to horizontal and vertical passes, with transparent boundaries. The
horizontal pass shares 64-pixel rows and their halo in 2 KiB of workgroup memory;
radii up to two pixels use direct sampling. Summation order stays unchanged.
Intermediate pixels stay on the GPU;
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
The tools are `inspect`, `graph`, `get_node`, `nodes`, `diagnostics`, `outline_text`,
`apply`, `undo`, `redo`, `save`, `render`, and `render_sequence`.
`nodes` provides defaults, parameter units/bounds, named ports, and operation JSON
schemas from the document registry. The same registry validates scalar parameters
and supplies input ports to the compiler. The `apply` tool embeds these schemas. Every mutation, save, or render
requires the current revision from `inspect`. A batch is validated atomically
and becomes one undo step. `apply` with `dry_run: true` changes nothing.

Use focused commands when changing an existing effect. `set_parameter` replaces
one registered parameter with the value shape shown in the `nodes` catalog:

```json
{"command":"set_parameter","id":"UUID","parameter":"stops","value":{"kind":"constant","value":1}}
```

`set_key` inserts or updates a scalar key at an exact rational time:

```json
{"command":"set_key","id":"UUID","parameter":"stops","time":{"ticks":24,"rate":24},"value":2}
```

Key edits preserve existing interpolation and other keys; equivalent timestamps
update the existing key without rewriting its time. A constant becomes a
single-key linear curve. Static parameters such as color and seed use
`set_parameter`. These commands preserve node identity, label, position and wiring,
validate the resulting graph, and participate in the same atomic batch and undo
step. Unknown parameters, wrong value shapes and out-of-range values reject the
batch. Changing a scalar with `set_parameter` replaces its complete curve.

`remove_key` deletes the key at an exact rational time, preserving interpolation
and the remaining timestamps. Removing the last key makes the scalar constant
with that key's value. A missing key rejects the entire transaction.

```json
{"command":"remove_key","id":"UUID","parameter":"stops","time":{"ticks":1,"rate":1}}
```

For larger projects, `inspect` with `{"summary":true}` returns `summary` instead
of `composition`: format, output UUID, node count and operation counts. Both
headless and live sessions also support these read-only queries:

- `graph` with `{"offset":0,"limit":32}` returns node metadata in UUID order,
  without operation parameters. `limit` is 1–128; `next_offset: null` ends the
  list. Labels are capped at 256 Unicode characters with `label_truncated` set
  when needed. Compare returned revisions between pages and restart if changed.
- `get_node` with `{"id":"UUID"}` returns the complete authored `node`.
- `get_node` with `{"id":"UUID","parameter":"stops"}` returns that registered
  parameter's exact `value`, including its curve when animated, without the rest
  of the node. Parameter names come from the `nodes` catalog.

These queries return the current `revision` (and live `session`), preserve
selection/history, and do not evaluate frames or initialize a GPU. Default
`inspect` behavior still returns the complete composition.

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

Live tools are `inspect`, `graph`, `get_node`, `nodes`, `diagnostics`, `apply`,
`undo`, `redo`, and `preview`.
`inspect` returns a project `session` UUID and `revision`; both are required for
edits, undo/redo, and preview. Opening a project creates a new session identity,
so an old command cannot target a newly opened project with the same revision.
Edits run on the UI thread, appear in the editor, and share its undo history.
`apply` supports dry runs and uses the same command schema as headless editing.
Unapplied inspector drafts, active node drags, and file jobs block remote changes.
New image paths must be imported through the editor. Existing asset references,
including unavailable sources, can remain in live edits; `inspect.unavailable_assets`
reports their load errors. `preview` accepts an
absolute `frame` in 0–1000000 and optional `node`, pauses playback, and requests
an asynchronous GPU preview. Live inspection reports `preview.displayed_time`
for the last successfully displayed frame, separately from the requested frame;
`preview.rendering` is false after either completion or failure.
Live tools do not save or export files; normal UI
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

The workspace has seven crates:

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

Execution plans freeze scalar animation into immutable `CompiledScalar` snapshots.
Compilation validates keys and precomputes monotone segments; frame evaluation
uses exact rational comparisons and binary search without revalidating every key.
Authoring changes require a new compiled snapshot. The authored curves remain in
the document for editing and serialization; compiled curve storage is CPU memory
and is separate from the GPU resource budget.

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
Choose canvas width/height, frame count, and an exact frame-rate numerator/denominator
before creating it. Unsaved work is protected by the same Save/Discard/Keep editing
flow as opening another project. **Settings…** changes canvas, frame count, and
frame rate together as one undo step. Resizing changes the canvas bounds; it does
not scale node coordinates. Animation keys retain their times in seconds, while
frame indices use the new frame rate. Format dialogs isolate their drafts and
block live MCP mutations until applied or closed.

MCP `apply` accepts `{"command":"frame_rate","frame_rate":{"ticks":30000,"rate":1001}}`
and `{"command":"frame_count","frame_count":144}` alongside `resize`. These
commands can be placed in a single atomic batch. Numerator and denominator must be positive; invalid batches leave the
composition and history unchanged.

Saved `frame_count` is in 1–1000000 and defines playback frames 0 through count−1.
Older documents without the field default to 240 frames. Changing length preserves
keys outside that interval; explicit headless renders and live previews can still
request them. The first sequence-export dialog uses the composition range, then
retains any custom range for that editor session. The 10000-frame limit per export
still applies, so longer compositions require a smaller export range. The motion
example is 96 frames and the procedural example is 144 frames, both at 24 fps.

## Gradients

**Gradient** generates opaque grayscale values from black to white. Linear mode
projects each pixel onto the line from `(start_x, start_y)` to `(end_x, end_y)`;
radial mode uses the start as its center and the endpoint distance as its radius.
Coordinates refer to integer pixel centers, matching transforms and ellipse masks.
Values clamp to 0..1 outside the ramp. Coincident endpoints, or a span shorter
than one millionth of a pixel, produce opaque black. All four coordinates animate.

Connect **Colorize** to map the ramp to HDR or transparent colors. For a glow,
use a bright low color and a dark or transparent high color. Connect **Channel
mask** in Luminance mode to convert the ramp into white alpha coverage before
using it as a mask; the generator's own alpha is always one.

When directly previewing the selected gradient, drag its start/end handles in
the viewer. Radial mode also draws the radius. As with ellipse handles, these
are inspector drafts until Apply, preserve existing animation modes, and remain
separate from pan/zoom and exported pixels.

## Inline MCP preview images

The headless MCP server exposes `preview_image` for visual feedback without a
file export. Supply the current `revision` and exact `ticks`/`rate`; optional
`node` and `region` select the same render root and crop as `render`.

```json
{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"preview_image","arguments":{"revision":0,"ticks":24,"rate":24,"max_edge":512}}}
```

The result includes a standard MCP PNG image content block and small structured
metadata containing revision, node, time, image dimensions, and source region.
Image data is excluded from the text/structured metadata. `max_edge` defaults to
512 and accepts 1..1024; images are never enlarged. Area filtering happens in
linear premultiplied color before conversion to straight-alpha sRGB PNG. HDR is
clipped at the existing SDR export boundary, and transparency is preserved.

This size limit applies to the returned image. The graph still renders at its
composition resolution, with the renderer's existing memory limits. The tool
creates no files and changes neither document revision nor undo history. The
live editor's `preview` tool remains an asynchronous seek/display operation.

The live editor also exposes `preview_image`, requiring `session` and `revision`
from `inspect` and accepting the same `max_edge` bound. First use `preview` to
pause/seek, then wait until `inspect.preview.rendering` is false. Capture rejects
playback, incomplete previews, and an older frame left visible after a render
failure. `inspect.preview.playing` reports playback explicitly.

Live capture reads an immutable completed GPU frame on a background worker with
one active and one queued job. Editing remains available while readback and PNG
encoding run. Returned session, revision, frame, and time identify the captured
snapshot even if the project changes afterward. The image contains applied
composition pixels, excluding inspector drafts, viewer exposure/alpha settings,
and overlays; metadata reports `inspector_draft` and
`viewer_settings_applied: false`. No file is created, and capture does not change
selection, playhead, document history, or viewer settings.


## Growing and shrinking masks

**Grow/shrink mask** (`morphology`) reads input alpha and produces a white
premultiplied mask. Positive radius expands coverage using the maximum alpha in
a square neighborhood; negative radius contracts it using the minimum. Radius
animates from −32 to +32 pixels and rounds to the nearest integer, with half
values rounded away from zero. Samples outside the canvas are transparent, so
erosion also contracts shapes touching the canvas boundary.

At radius zero the result is still white alpha coverage, not the original RGB.
Use **Channel mask** first when coverage comes from image luminance or another
color channel. Follow with **Blur** to soften the edge, **Colorize** to tint it,
or connect it to **Mask** to limit another image. The square neighborhood is
implemented as two separable GPU passes; corners are square rather than circular.


## Combining masks

**Combine masks** (`combine_mask`) combines alpha from `mask` and `other`,
ignores their RGB, and outputs white premultiplied coverage. It supports soft
edges rather than thresholding them. With input alpha `a` and `b`:

| Mode | Coverage |
|---|---|
| Union | `a + b − a*b` |
| Intersect | `a*b` |
| Subtract | `a*(1−b)` |
| Exclude overlap | `a*(1−b) + b*(1−a)` |

Subtract is directional: it removes `other` from `mask`. These are soft-alpha
operations; for example intersecting two half-covered pixels gives one-quarter
coverage, and subtracting a soft mask from itself can retain fractional edges.
For an outside outline, connect **Grow/shrink mask** with a positive radius to
`mask` and the original shape to `other`, then choose Subtract. Follow with
**Colorize** for a colored outline or **Blur** for a halo. Combine masks can also
intersect moving shapes or cut holes in image-derived mattes.
