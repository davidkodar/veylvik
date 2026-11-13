# Video footage

Choose **File > Import media…** to open MP4, MOV, M4V, MKV, WebM, AVI or MPEG
footage. FFmpeg must be installed; the Arch package declares it as a dependency.
The first video stream is imported. Audio playback and export are not implemented.

New media imports, including still images, create a **Footage** node. The inspector shows resolution,
duration, frame count, average frame rate and colour assumptions. **Timeline start**
places the clip in the composition; **Source in** skips the beginning of the file.
Uncheck **Use source end** to set an exclusive **Source out**. Frame fields use the
composition frame rate, while the document stores exact rational times. Each field
has a vector reset button. Edits use normal Apply/Revert and Undo behavior.

Footage is transparent before its timeline start and after its trimmed end.
Source pixels retain native dimensions. **Placement** fits them to the canvas on
the GPU before clipping: **Fit** shows the whole image with transparent padding,
**Fill** covers the canvas and crops edges, **Original size** centres native pixels,
and **Stretch** fills both dimensions independently. **Native pixels (top-left)**
preserves older documents. Use a downstream transform for further positioning. Multiple Footage nodes can share a source with independent timing. Existing
Image nodes retain their earlier endpoint-holding behavior. Replacing a still
image with video promotes it to a Footage node.

The same source and timing code serves Studio previews, point tracking, CLI/MCP
rendering and exports. Tracking works on the rendered source, including its trim
and placement. Choose a tracking range within the visible clip.

## Canvas size

**File > New from media…** creates a composition with the source dimensions and,
for video or sequences, its frame rate and duration. Still images use 24 fps and
the normal default duration.

The first mismatched import into an empty composition offers **Match composition**
or **Keep settings**. Matching frame rate and duration are separate, optional
checkboxes. Keep settings fits and centres the source in the existing canvas.
Cancel leaves the document unchanged. Import and matching undo together.

Imports into an existing scene keep its settings. To change them, choose
**Match composition to this media…** in the source inspector or node context menu.
The optional scene-control scaling proportionally fits authored spatial controls
into the new canvas. Otherwise only canvas bounds change. Media retains its chosen
placement mode, and keyframe times remain in seconds. The same scaling choice is
available in **File > Composition settings…**. These edits are undoable.

## Decoding and colour

Video decoding tries NVIDIA NVDEC first, then native Intel/AMD VA-API devices,
and finally software decoding. A hardware candidate must successfully decode a
hardware frame before it is selected; unsupported codecs, pixel formats or drivers fall
back explicitly. The inspector identifies the active backend and any fallback.
A hardware failure during playback retries the requested frame through software.

Hardware decoding runs inside Veylvik through the FFmpeg libraries. Clips share
reference-counted device contexts (the primary CUDA context on NVIDIA), while
each clip owns its demuxer and decoder. Hardware frames retain their device and
frame pool until released, including when a decoder closes or seeks.

The hardware path supports even-sized 4:2:0 NV12 and 10-bit P010 sources. It
transfers these compact planes through host staging, then performs chroma
reconstruction, range/matrix conversion, rotation, transfer decoding and primary
conversion on the renderer's Vulkan device. Linear RGBA frames remain on that
device for effects and display. This is hardware decoding with a staged transfer,
not zero-copy decode-to-Vulkan interoperability. Software fallback retains FFmpeg
matrix/range conversion, with transfer and primary conversion on Vulkan.

Indexing reads packet presentation timestamps rather than decoding every frame
on the CPU or assuming a constant frame rate. Sources must have distinct packet
presentation timestamps; remux sources with missing or duplicate timestamps.
Seeking handles reordered frames and variable-frame-rate files. Sequential
hardware requests reuse the decoder; random seeks flush and reposition it using
stream timestamps without reopening the device. Skipped/preroll hardware frames
are discarded on the GPU before host staging. The software compatibility path
uses a bounded FFmpeg process and restarts it for distant/backward seeks.
Plane caches are bounded. Indexing and decoding run on
workers, and inspector metadata never waits for a decoder or GPU fence.
The cache budget does not include FFmpeg's internal buffers or conversion scratch.
Demux operations have a 30-second interrupt deadline. In-process GPU driver calls
cannot be forcibly terminated like a child process.

Callers explicitly requesting CPU pixels still have a CPU realization path.
Normal Studio, CLI/MCP rendering and tracking use the render-device path.
The Rust `Video::diagnostics()` API reports hardware selection, host staging,
fallback reason, in-process status, shared device context ID, native seek count,
GPU conversion count and CPU realization count.
`Video::open_with_policy` permits automatic, software-only or hardware-required
selection for integrations and diagnostics. Hardware-required mode returns an
error rather than silently falling back.

Decoded colour is converted explicitly to scene-linear Rec.709 using source transfer
and primary tags, then premultiplied by alpha exactly once. SDR and PQ transfer
functions are supported; PQ uses a 100-nit reference without tone mapping.
Incomplete colour tags are reported in the inspector. Defaults assume Rec.709
primaries, sRGB transfer for RGB video or BT.709 transfer for YUV video, with
BT.709 matrix for HD YUV and BT.601 matrix for SD YUV. Untagged RGB/JPEG-range video
uses full range; other untagged YUV uses limited range.

Alpha WebM currently requires conversion to a PNG/EXR sequence or RGBA FFV1.
Unsupported primaries, HLG, constant-luminance BT.2020, anamorphic or interlaced
video produce explicit import errors. Convert those sources to supported footage
or linear EXR first. Frame dimensions, source memory and index size are bounded:
individual decoded frames use the existing 128 MiB image limit, manifests/indexes
are capped, and video indexing admits at most 250,000 frames with a 30-second
process timeout. Long shots may need splitting. Changes on disk require source
reload instead of silently mixing old and new frames.

Save-as, recovery and cross-project clipboard operations preserve asset paths.
Project collection copies supported video files within the existing 64 MiB
per-file / 1 GiB total collection limits. Image-sequence collection remains
unsupported; keep its manifest and frame directory together.

## Automation

The `footage` operation is exposed by the node catalog and document schema:

```json
{
  "type": "footage",
  "asset": "shot.mp4",
  "placement": "fit",
  "timing": {
    "start": { "ticks": 2, "rate": 1 },
    "source_in": { "ticks": 1, "rate": 2 },
    "source_out": null
  }
}
```

This places the clip at composition second 2, starting at source second 0.5.
`placement` accepts `fit`, `fill`, `original`, `stretch`, or `legacy`. Omitted
placement defaults to `legacy` for compatibility. Both `placement` and `timing` can be updated through ordinary revision-checked `set_parameter`
transactions. `source_out` is exclusive; null uses the source duration.

Building from source requires the FFmpeg development libraries, libclang and
pkg-config. The Arch package declares these build/runtime dependencies.
