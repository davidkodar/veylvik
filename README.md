# Veylvik

A Rust composition engine with headless Vulkan rendering and an MCP interface.
This is an early foundation, not yet a desktop editor.

## Build and run

Requires Rust 1.95 or newer and a Vulkan driver. No display server is needed.

```sh
cargo build --release --locked
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

## Composition and rendering

Projects are versioned JSON containing nodes with stable UUIDs, named input
ports, and an output node. Supported operations:

| Operation | Inputs | Parameters |
|---|---|---|
| Solid | none | Straight linear RGB plus alpha; converted to premultiplied pixels |
| Noise | none | Seed and cell size, 1–8192 pixels |
| Exposure | image | −32 to +32 stops |
| Transform | image | X/Y translation in pixels, bilinear sampling |
| Blur | image | Box radius, 0–32 pixels, rounded to an integer |
| Merge | background, foreground | Premultiplied foreground-over-background |
| Image | none | Asset key; native image size, top-left anchor, transparent padding |

Scalar parameters accept constants or linearly interpolated keyframes. Outside
the key range they hold the nearest endpoint. Input boundaries are transparent
black. Image dimensions are 1–8192 per axis, subject to GPU limits and budget.

Intermediate images use scene-linear Rec.709 premultiplied RGBA32F. Values above
one are preserved. PNG export unpremultiplies, clips to SDR, and applies sRGB
encoding; it is not an HDR delivery format or a tone mapper.

The compiler validates the entire graph, prunes unreachable nodes, and assigns
explicit compute passes and reusable image-buffer slots by last use. Box blur
lowers to horizontal and vertical passes, with transparent boundaries. Intermediate pixels stay on the GPU;
only the final image is read back. One device and queue own all resources.
The default 512 MiB render budget covers declared GPU image buffers, readback,
uniforms, and each unique source image upload; it does not measure driver overhead
or bound total process RAM. `check` reports graph working bytes before source
uploads and also checks the reachable PNG inputs against the combined budget.

The renderer receives immutable `ImageInputs` from its host and never opens files.
The CLI/MCP adapter resolves image asset keys as PNG paths relative to the opened
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
The tools are `inspect`, `nodes`, `apply`, `undo`, `redo`, `save`, and `render`.
`nodes` provides defaults, parameter units/bounds, named ports, and operation JSON
schemas from the document registry. The same registry validates scalar parameters
and supplies input ports to the compiler. The `apply` tool embeds these schemas. Every mutation, save, or render
requires the current revision from `inspect`. A batch is validated atomically
and becomes one undo step. `apply` with `dry_run: true` changes nothing.

The `render` tool optionally accepts `node` (a UUID) and `region` (an object with
`x`, `y`, `width`, `height`). It returns the rendered region, path, and document
revision. An invalid preview target or region is rejected before GPU creation.

The session owns an in-memory document. Edits persist only through `save`;
external file edits are not live-synchronized. Save and render can replace the
explicit destination path, except for a referenced input image. The MCP client should expose those actions for user
review. Output currently returns the PNG path, not inline image content.

## Development

```sh
cargo fmt --all --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked
cargo test --workspace --locked -- --ignored --nocapture
```

The final command is the mandatory Vulkan check. It fails if no adapter is
available. Default tests leave it explicitly ignored so document/control tests
can run on machines without graphics. CI runs it with Mesa's software Vulkan;
hardware validation remains a separate check.

The workspace has three boundaries:

- `veylvik-document`: serialization, exact time, curves, graph validation,
  transactional editing, undo/redo, and atomic persistence. No GPU dependencies.
- `veylvik-render`: immutable compiled plans, buffer lifetimes, compute kernels,
  headless rendering, and the SDR output transform. No UI dependencies.
- `veylvik-cli`: command-line and MCP adapters using those public APIs.

The executor is whole-frame. `PreviewScheduler` runs it on a dedicated worker,
keeps at most one active and one pending request, and replaces obsolete pending
work. Generation and document revision accompany every completion. Cancellation
suppresses old results; it does not interrupt submitted GPU work. A wake callback
can notify a UI without polling. Dropping the scheduler joins the worker.
Direct `Renderer` calls remain synchronous. Plans, pipelines, and GPU
buffers can be reused. A bounded pool retains up to 128 MiB of idle buffers by
default, within the render budget. Buffers return to the pool only after GPU
completion and readback unmapping. `Renderer::resource_stats` reports allocations,
reuse, evictions, and declared resident bytes; `trim_resources` releases idle
buffers. Image results are not cached. There is
no GUI, video decoder, text engine, plug-in loader, simulation, or Klipp adapter
in this version. No Klipp source is copied or linked.
