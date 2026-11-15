# Export video

Choose **File > Export video…** to configure an MP4 job, choose a new destination,
then click **Start queue** in the render queue. The **Render** button opens the queue.
Choose H.264 for broad compatibility or HEVC 10-bit for smoother gradients on
compatible players. Both are SDR Rec.709 without an alpha channel, with an AAC audio track (see Audio).
The composition's display transform applies: Filmic rolls highlights off with an ACES-style
curve, Standard clips at white (see the reference).

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

Progress identifies the encoder in use. **Cancel job** discards the partial
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

## Audio

Exports carry an AAC track (48 kHz stereo, 192 kbit/s). The sound of every Footage node
that feeds the output is mixed in at the clip's timeline position, honouring its source
in and out points, and clipped to the exported range; clips without sound are skipped.
Clips are summed without automatic level reduction, so footage keeps its recorded
loudness. The track always matches the video's length exactly. Audio follows footage
timing, not visibility: a clip that is in the graph but faded out by a Blend still plays,
so trim clips to the frames where they should be heard.

An extra track, such as music, can be added; it starts at the first exported frame and
plays at the chosen level (−60 to +12 dB). Mixing happens after encoding in one bounded
FFmpeg run that copies the encoded video stream unchanged, and the result is published
atomically like silent exports. Cancelling during **Mixing audio** leaves no file.

```sh
veylvik render-video scene.veylvik out.mp4 --audio music.mp3 --audio-gain -6
veylvik render-video scene.veylvik silent.mp4 --no-audio
```

In Studio, the export dialog has **Include footage audio**, a **Music** file path and its
level. The headless and live MCP tools accept the same choices as `settings.audio`
(`mute`, `music`, `music_gain_db`), and the result lists the mixed `audio` sources.

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

## Persistent render queue

**Render** or **File > Render queue…** opens the queue. Add video, image sequence
or current-preview still jobs. Each captures the composition, embedded fonts,
output node, frame/time range and export settings when added. You can switch
projects and keep editing while jobs render sequentially on the shared GPU.
Preview playback can slow down while the GPU is exporting.

Use **Start queue**, **Pause after current**, **Cancel job**, and **Move earlier /
Move later** to manage work. Cancel pending jobs or remove them before rendering.
Per-job progress reports frames, elapsed time, encoder and errors. The status bar
reopens the queue even when its window is hidden. **Clear completed** removes job
records, not exported files. **Retry to…** keeps the original snapshot and selects
a fresh destination; sequence retries use a new uniquely named subfolder and
preserve earlier partial frames. Failed jobs do not prevent later jobs rendering.

Jobs persist in `$XDG_STATE_HOME/veylvik/render-queue.json` (normally
`~/.local/state/veylvik/render-queue.json`), separate from project history. The
queue reopens paused; jobs active at an unexpected exit become **Interrupted**
and require explicit retry. Only one running Veylvik instance owns this queue.
Normal quitting requires finishing or cancelling the current render. The queue
is limited to 64 jobs and 64 MiB of saved snapshots; remove finished records to
free capacity.

External media, sequence frames, music and OCIO dependencies stay at their source
paths rather than being copied into the queue. Their size and modification time
are checked before rendering and retry; missing or changed files fail the job.
Keep sources unchanged while a render runs. To use edited media, enqueue a new
snapshot. This is not a portable project archive or a render farm. Queued outputs
never overwrite existing files or folders, including still frames. A cancelled
sequence retains completed frames and its incomplete manifest; a cancelled video
does not publish a partial MP4.
