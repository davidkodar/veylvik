# Export video

Choose **File > Export video…** to export the composition output as an MP4.
Choose H.264 for broad compatibility or HEVC 10-bit for smoother gradients on
compatible players. Both are SDR Rec.709, with no audio or alpha channel.

The panel starts with the composition resolution, frame rate and entire frame
range. Choose a size preset or enter even dimensions. The composition is fitted
and centred without stretching; unused space and transparency use the selected
background colour. Export settings do not resize or retime the project.

Frame ranges are zero-based and inclusive. Changing export frame rate samples
the animation at the new rate, preserving the source duration to within one
output frame. This does not interpolate motion in imported video. Exports are
limited to 10,000 output frames and dimensions from 2 to 8192, subject to GPU
memory and encoder limits.

The encoder is tested at the requested size and bit depth before export:
NVIDIA NVENC first, then supported Intel/AMD VA-API devices. Software encoding
is disabled unless **Allow software encoding if hardware is unavailable** is
checked. Export failures never silently restart on the CPU.

Progress identifies the encoder in use. **Cancel export** discards the partial
file. Choose a new filename: existing files are never replaced, and the final
MP4 is published only after encoding finishes successfully. Export takes a
snapshot of the composition; it does not add an undo step.

## Colour and GPU processing

Vulkan performs resizing, linear-light background compositing, Rec.709 transfer,
limited-range YCbCr conversion and NV12/P010 packing. The encoder receives
4:2:0 video. HDR values are clipped to SDR; no tone mapping is applied.
Use PNG sequences for transparency or EXR sequences for scene-linear HDR.

On compatible NVIDIA devices, Vulkan transfers prepared NV12/P010 planes to
CUDA and in-process NVENC entirely on the GPU. Device UUIDs must match. The
export trial checks the full transfer, encode and MP4 path at the requested
settings before selecting it. No uncompressed pixels are mapped or staged in
host memory on this path. Compressed packets still pass through the CPU muxer.

This is a GPU-to-GPU copy, not zero-copy aliasing. Host synchronization waits
and per-frame external allocation remain. If the direct bridge is unavailable,
NVENC/VA-API can still encode via the explicit RAM-staged path. A bridge failure
once export starts aborts rather than restarting the job. The completion message
identifies GPU transfer or RAM staging. Hardware decode and export transport
are separate capabilities.

Native GPU/encoder calls run in-process. Cancellation is checked between
frames and before publication; a stalled driver call cannot be forcibly killed
without stopping the app.

## CLI and MCP

For example:

```sh
veylvik render-video scene.veylvik output.mp4 --codec hevc10 --size 3840 2160 --fps 30000 1001 --range 0 95 --quality high
```

Quality is `draft`, `high` (default) or `master`; master is not lossless.
Use `--allow-software` to permit CPU encoding if hardware cannot encode the
requested format. Progress goes to stderr and the JSON result goes to stdout.

The headless `veylvik mcp scene.veylvik` server exposes `render_video`, with
`path` and `revision` required. Omitting `settings` uses composition defaults.
An explicit settings object requires `width`, `height`, `frame_rate`
(`ticks`/`rate`), `start` and `end`. Optional fields are `codec`, `quality`,
`allow_software` and `matte` (linear RGB). Rendering is synchronous. The live
Studio MCP connection does not yet expose this export tool.

The result identifies the encoder, whether it is hardware, the frame count,
the published path, cancellation and host staging. Additional fields
`direct_gpu_transfers` and `staged_frames` count production frames (excluding
the capability trial). `interop_fallback` contains the reason when the direct
bridge was not selected. A direct NVIDIA export reports `host_staging: false`
and zero staged frames.

## AMD/Intel DMA-BUF status: experimental

A direct Vulkan → DMA-BUF → VA-API encoder path is implemented for AMD/Intel,
but has **not yet been verified on AMD or Intel hardware**. Its progress and
completion messages identify experimental DMA-BUF use; JSON results set
`experimental_interop: true`. This applies to export, not hardware decode.

The path uses linear NV12/P010 buffers with aligned rows and plane offsets.
It requires Vulkan external-memory DMA-BUF support, foreign-queue ownership,
DRM device identification, and VA-API direct DRM PRIME import on the same
render device. Tiled/compressed modifiers and cross-device copies are not
implemented. No CPU pixel mapping/upload is part of the direct path.

Vulkan completes and releases ownership before VA-API imports the buffer.
FFmpeg references retain the FD and Vulkan allocation through encoding; those
allocations remain charged against the renderer budget. Export trials include
import, encode and MP4 finalization. If a trial fails, export falls back to the
existing staged hardware path and reports the reason through `interop_fallback`.
A successful trial checks capability, not final visual correctness on an
untested driver. Failures after production encoding starts abort the export.

### Hardware validation

On a machine with a matching AMD/Intel Vulkan adapter and VA-API render node:

```sh
VEYLVIK_TEST_VAAPI_DEVICE=/dev/dri/renderD128 CARGO_BUILD_JOBS=2 cargo test -p veylvik-io --test video_export vaapi_dma_import_encode_and_retention_on_real_hardware -- --ignored --nocapture
```

Use the node corresponding to the selected Vulkan GPU. The test deliberately
fails if the device is unavailable or the direct bridge cannot run; it does not
silently skip or fall back. It checks padded, 1080p and 4K output in NV12/P010,
exact pixel parity after direct import, retained-buffer lifetime, repeated
frames, frame rate/colour metadata and complete video decoding. Diagnostic
readback occurs only in this test. A codec unsupported by the card may fail
that portion; record the GPU, driver, format and error when reporting results.

Afterward, test native Studio export and cancellation, watch for corruption,
and confirm `host_staging: false`, nonzero `direct_gpu_transfers`, and zero
`staged_frames` in CLI/MCP output. Keep this path marked experimental until
actual AMD and Intel results have been reviewed.
