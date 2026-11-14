# Subject segmentation

Click a person or object and Veylvik produces its matte, tracked through the
shot by a local AI model. The analysis runs once, like planar tracking, and the
result is an ordinary matte sequence that any node can use. Nothing leaves the
computer.

Edge-band matting with a dedicated matting model, for fine hair, follows.

## MCP

The headless MCP server (`veylvik mcp FILE`) exposes `segment_subject` with the
same inputs as the command: `source`, `points` (objects with `x`, `y` and
`positive`), `frame`, optional `end` (default: the last composition frame, so
a prompt tracks through the rest of the shot), `back_to`, `model`, `detail` and
`matting`, plus the document `revision`. It adds the Subject matte and
Subject cutout as one undo step and returns the same report, including both
node IDs.

## In Studio

1. Select the footage node and choose **Select subject…** in the Inspector, or
   **Tools → Select subject…**. Pick the **Model**: EdgeTAM is fastest, SAM 2.1
   Base+ and Large are more precise. The window offers to install the chosen
   model if needed.
2. Click the subject in the viewer. Each click shows a blue preview mask within
   a fraction of a second. Right-click or Alt-click excludes an area. A few
   clicks spread over a person (head, body, legs) select them whole. **Undo
   point** and **Clear** step back. Points belong to the frame they were placed
   on; clicking on another frame starts a new selection.
3. Choose the frame to track to, optionally **Also track back to the start**,
   keep **Detail pass for small parts** on for feet and hands and **Hair and
   soft edges** on (it offers to install ViTMatte, 104 MB, if needed), and
   **Track subject**. Progress shows in the window; **Stop** keeps the
   frames tracked so far.
4. The analysis adds a **Subject matte** and a **Subject cutout** as one undo
   step and shows the cutout. Then, with one click:
   - **Keep subject** makes the cut-out subject the output.
   - **Replace background** puts the subject over a new background colour; edit
     it, or connect any image or footage in its place.
   - **Put layers behind** adds a title between the shot and the subject, the
     classic text-behind-the-person look; edit it or connect any other layer.

The model runs in its own process while the window is open and is shut down
when it closes, releasing its graphics memory. Mattes are stored beside a saved
project, or in Veylvik's data folder for an unsaved one.

## AI models window

**Tools → AI models…** manages every local model. Each row shows what the
model is for, its download size, licence and status, with one-click
**Install** (with progress and **Cancel**), **Uninstall**, and **Enabled**.
Hover over the info button for a summary, or click it for the pros, cons,
speed, graphics memory and a link to the model's page.

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
| `sam2.1-base-plus` | Click-to-mask and video tracking with SAM 2.1 (Hiera Base+, half precision): more precise and more consistent over time than EdgeTAM; about 65 ms per frame and 2.9 GB | 183 MB | Apache-2.0 |
| `sam2.1-large` | The same with Hiera Large, the most precise tracker here; about 80 ms per frame and 3.2 GB | 475 MB | Apache-2.0 |
| `vitmatte` | Hair and soft edges: matting of the band around the tracked outline, about 55 ms per frame for a full-length person at 1080 × 1920 | 104 MB | Apache-2.0 weights; trained on research matting datasets |

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
veylvik segment FILE NEW_FILE SOURCE_UUID FRAME END "X,Y X,Y,- ..." [--back-to FRAME] [--model ID] [--no-detail] [--matting|--no-matting]
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
- `--model` picks the tracker: `edgetam` (default), `sam2.1-base-plus` or
  `sam2.1-large`.
- `--no-detail` skips the detail pass (see below) for about half the time.
- Edge matting (see below) runs when the `vitmatte` model is installed and
  switched on; `--no-matting` skips it, and `--matting` requires it.

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
- **Detail pass.** The model sees the whole frame squeezed into 1024 × 1024,
  so on a full-length shot a foot or hand covers only a few dozen model pixels.
  By default the analysis therefore runs twice. The first pass finds the subject
  on every frame. The second tracks again inside a crop around the subject, with
  one crop size for the whole shot (the subject's largest extent plus a margin),
  so the model sees a constant scale. The crop's centre follows the subject,
  smoothed over time. On a walking person in a portrait frame this gives about
  2.5 times the detail, which completes toes and fingers and stops patches of
  ground sticking to feet. The pass is skipped when the subject already fills
  most of the frame (less than 1.3 times the detail to gain). It roughly doubles
  the analysis time; the report gives the zoom used.
- **Hair and soft edges.** Segmentation models draw a smooth outline, which
  loses flyaway hair, fur and motion blur. With ViTMatte installed, each frame's
  tracked matte becomes a trimap: surely subject, surely background, and an
  uncertain band around the outline about 2% of the subject's diagonal wide
  (6 to 48 pixels), plus wherever the matte is already soft. ViTMatte resolves
  only that band, from the image at native resolution (crops larger than
  1536 pixels are scaled down for it), and everything outside the band stays
  exactly as tracked. On the beach shot the bun gains its real shape and
  flyaway strands, and the ears and neck follow their real contours.
  ViTMatte's weights are Apache-2.0, but like practically every matting model
  it was trained on academic matting datasets (Composition-1k) whose terms
  are research-oriented; check them before commercial use.
- **Confident interiors.** On the prompted frame, where the model has no
  memory yet, its mask is often under-confident and would come out partly
  transparent. Each frame's logits are therefore scaled, up to eight times, until
  the median inside the subject is clearly opaque; confident frames are left
  as they are, and the outline (logit zero) never moves.
- **SAM 2.1.** The SAM 2.1 graphs have a fixed-size memory bank (seven frames
  and sixteen object pointers). Until a shot has filled it, the prompted frame's
  memory and pointer repeat into the unused slots; object pointers carry their
  temporal positions as SAM 2.1 does. On the beach shot SAM 2.1 kept its reading
  of the prompt consistent through the shot where EdgeTAM changed its mind about
  the shoulder bag.
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
