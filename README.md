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

Scalar parameters accept constants or linearly interpolated keyframes. Outside
the key range they hold the nearest endpoint. Input boundaries are transparent
black. Image dimensions are 1–8192 per axis, subject to GPU limits and budget.

Intermediate images use scene-linear Rec.709 premultiplied RGBA32F. Values above
one are preserved. PNG export unpremultiplies, clips to SDR, and applies sRGB
encoding; it is not an HDR delivery format or a tone mapper.

The compiler validates the entire graph, prunes unreachable nodes, and assigns
reusable image-buffer slots by last use. Intermediate pixels stay on the GPU;
only the final image is read back. One device and queue own all resources.
The default 512 MiB render budget covers declared GPU image buffers, readback,
and uniforms; it does not measure driver overhead or bound total process RAM.

## MCP

Launch a session against a saved composition:

```sh
./target/release/veylvik mcp /absolute/path/example.veylvik
```

The server uses newline-delimited JSON-RPC on stdio and MCP protocol version
2025-11-25. Initialize, then send `notifications/initialized` before using tools.
The tools are `inspect`, `nodes`, `apply`, `undo`, `redo`, `save`, and `render`.
`nodes` provides operation examples and limits. Every mutation, save, or render
requires the current revision from `inspect`. A batch is validated atomically
and becomes one undo step. `apply` with `dry_run: true` changes nothing.

The session owns an in-memory document. Edits persist only through `save`;
external file edits are not live-synchronized. Save and render can replace the
explicit destination path. The MCP client should expose those actions for user
review. Output currently returns the PNG path, not inline image content.

## Development

```sh
cargo fmt --all --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked
cargo test -p veylvik-render --locked -- --ignored --nocapture
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

The current executor is synchronous and whole-frame. Plans and pipelines can
be reused, but frame allocations are recreated and image results are not cached.
Box blur is a correctness baseline, not an optimized production blur. There is
no GUI, media decoder, text engine, plug-in loader, simulation, or Klipp adapter
in this version. No Klipp source is copied or linked.
