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
