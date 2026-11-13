# Subject segmentation

Click a person or object and Veylvik produces its matte, tracked through the
shot by a local AI model. The analysis runs once, like planar tracking, and the
result is an ordinary matte sequence that any node can use. Nothing leaves the
computer.

Available today: the `veylvik segment` command, the model manager, and the
**Refine matte** node for the edge. The Studio workflow (clicking in the viewer,
**Track subject**, and one-click background replacement) and edge-band matting
with a dedicated matting model follow.

## Models

Models are not bundled. Install one explicitly:

```sh
veylvik models                      # catalogue as JSON: status, size, licence, uses, pros and cons
veylvik models install edgetam      # download and verify
veylvik models disable edgetam      # keep the files, stop offering the model
veylvik models enable edgetam
veylvik models uninstall edgetam    # remove the model's files
```

| Model | Use | Download | Licence |
| --- | --- | --- | --- |
| `edgetam` | Click-to-mask and video tracking: about 30 ms per frame and 1.5 GB of graphics memory on an RTX 4080 | 65 MB | Apache-2.0 |

Every file is pinned to a fixed upstream revision, size and SHA-256. Files are
fetched by `curl` over HTTPS only, verified, and only then renamed into place;
a failed or cancelled download installs nothing. Models live in
`$XDG_DATA_HOME/veylvik/models/<id>/` (or `~/.local/share/veylvik/models`), or
in `VEYLVIK_MODELS_DIR` when set. Switched-off models are listed in
`settings.json` in that directory. Uninstalling removes only the catalogue's own
file names and leaves anything else in place.

## Requirements

Inference runs on an NVIDIA GPU through ONNX Runtime's CUDA provider, with no
silent CPU fallback. On Arch, install `onnxruntime-cuda` and `cudnn`. Veylvik
loads `/usr/lib/libonnxruntime.so`, or the library named by `ORT_DYLIB_PATH`.

## Running an analysis

```sh
veylvik segment FILE NEW_FILE SOURCE_UUID FRAME END "X,Y X,Y,- ..." [--back-to FRAME] [--model ID]
```

- `SOURCE_UUID` is the node whose image is analysed, usually a Footage node.
  It is rendered at composition size for every frame, so placement, trims and
  timing are already applied.
- `FRAME` is the prompted frame and `END` the frame tracking runs to, before or
  after it. Use the same frame for a single-frame matte. `--back-to` also tracks
  backward from `FRAME` to an earlier frame.
- Points are in 0..1 frame coordinates from the top left. A point marks the
  subject; a point ending in `,-` excludes that area. Give 1 to 32 points, with
  at least one marking the subject. A few points spread over the subject (head,
  body, legs) select a whole person more reliably than one.
- An analysis covers up to 4096 frames.

The mattes are written next to `NEW_FILE` in
`Subject mattes/<source id>/matte_NNNNN.png`, with a `subject.veyseq` manifest
at the composition frame rate. `NEW_FILE` gets two new nodes: a **Subject matte**
Footage node that plays the sequence from the first analysed frame, and a
**Subject cutout** (Refine matte) that cuts the subject out of the source with
refined, decontaminated edges. Merge the cutout over a new background, or use
the matte as a mask. The command prints
a JSON report with the frame range, frames where the subject was absent, and
milliseconds per frame.

Mattes are 16-bit grey-and-alpha PNGs: white, with the matte as alpha. They
therefore read both as a premultiplied white matte and as an alpha channel.
Frames where the model judges the subject absent are empty.

## How it works

- **Isolated worker.** The model runs in a `veylvik segment-worker` child
  process that owns ONNX Runtime and the GPU sessions. The host talks to it over
  pipes, with size-bounded messages and hard timeouts, and kills it when done.
  A runtime or driver fault therefore cannot take down Studio or the CLI. The
  worker preloads cuDNN, which the packaged CUDA provider needs.
- **Model input.** Each scene-linear frame is area-averaged to the model's
  1024 × 1024 input in linear light, then encoded as sRGB, as the model was
  trained. HDR values are clamped for analysis only; the matte is applied to the
  untouched image.
- **Tracking.** EdgeTAM is SAM 2 with an efficient memory. The prompted frame
  conditions every later frame, together with the six most recent frames and up
  to sixteen object pointers. Tracking backward restarts from the prompted frame,
  as SAM 2 does.
- **Refined edges.** The Subject cutout runs on the GPU at render time, so
  its controls can change without a new analysis and the footage keeps its HDR
  values. See **Refine matte** in the [reference](reference.md). On a real
  shot, it makes skin edges against sand opaque right up to the edge, with the
  one mixed pixel partly transparent and carrying the subject's colour instead of
  the sand's.
- **Edges.** The model's mask logits are resampled bilinearly to the
  composition size before the sigmoid. The edge is never upsampled from a
  binary mask, so it stays smooth instead of stair-stepped.
