# Veylvik reference

The complete behaviour reference: commands, limits, colour and alpha rules, every
node, the editor, and the MCP interface. For an overview, see the [README](../README.md).

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

`veylvik segment` produces a tracked subject matte from a local AI model, and
`veylvik models` installs, removes, enables and disables those models. Models
are never bundled or downloaded without an explicit install. See
[Subject segmentation](subject-segmentation.md).
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

**Fade** shows a layer only while it is on screen. **Start** is the first frame it
appears; **End** the first frame it is gone, or **Until the end** of the composition.
**Fade in** and **Fade out** ramp it linearly up after the start and down before the
end, and must fit between them. **Opacity** multiplies the result and can be keyframed
for anything beyond a simple ramp. Premultiplied colour and alpha scale together, and a
layer faded fully out at a frame is neither rendered nor decoded, so footage behind it
costs nothing. Times are exact rationals like footage timing; the inspector edits them
in frames.

**Chroma key** makes a green, blue or other coloured backdrop transparent with a
colour-difference key. It measures how strongly each pixel leans towards **Key colour**'s
dominant channel (for green, green minus the larger of red and blue) relative to the
pixel's own brightness, and compares that with the key, so shadowed, brightly lit and HDR
parts of an uneven screen key alike and the screen need not match the key exactly; the
**Green screen** and **Blue screen** buttons set typical screen colours. Pixels leaning at
least 1 − **Tolerance** as much as the key become transparent, and over a further
**Softness** they fade back to opaque. Skin, greys and colours without that bias stay
opaque, and near-black pixels, which carry no reliable colour, are kept. **Despill**
limits the key's channel in what remains to the average of the other two, removing
coloured fringes and bounce light. **Luma key** keys by Rec.709 luminance instead:
transparent at or below **Low**, opaque at or above **High**, with a linear ramp between;
set Low above High to key out bright areas such as a white backdrop. Both keep input
alpha, output premultiplied colour, work on HDR values and can be keyframed.

**Refine matte** cuts a subject out of **Image** with a coarse matte on **Matte** (its
alpha), such as the Subject matte from segmentation, and gives the matte the image's own
edges. Near the matte's edge it averages the colours of pixels the matte is sure are
subject and sure are background, within **Radius** pixels (default 6), and re-estimates each
pixel's opacity from where its colour lies between the two. That snaps a soft or slightly
misplaced edge to the real one and recovers fine detail. The estimate is used only where it
is reliable: both colours nearby and different enough (higher **Detail** accepts closer
colours), and the pixel's colour on the line between them, so a patterned subject such as a
striped shirt keeps the coarse edge. Where the matte was unsure the edge is re-estimated
fully; where it was sure of the background, only within about three pixels of the edge, so
background grains that happen to match the subject stay out. The confident interior always
stays opaque. **Decontaminate** then re-estimates the subject's colour at semi-transparent
edges by blur-fusion foreground estimation, so no trace of the old background shows over a
new one. Output is the premultiplied subject; HDR values are kept, and only negative colour
that the estimate would introduce at edges is clipped.

**Pick** beside a colour parameter (Chroma key's Key colour, Solid, Colorize's Shadows and
Highlights) is an eyedropper: click in the viewer to sample a 5 × 5 area, or drag to
average a rectangle; Esc or pressing Pick again cancels. It samples exact scene-linear
values, including HDR, rendered on the file worker so the editor never waits. Nodes with
an image input sample what feeds them, so a chroma key picks the screen even while the
viewer shows its keyed result; generators sample the viewed image. Coverage-weighted
averaging returns straight colour, and fully transparent areas are refused. The sample
becomes an inspector draft; Apply keeps it. Check a matte
with **View → Show alpha**, then refine it with **Grow/shrink mask** and **Blur**, or
combine it with a garbage **Path mask** through **Combine masks**.

**Colour correct** grades footage and renders in scene-linear light, in a fixed order:
white balance (**Temperature** warms or cools by up to a stop between red and blue,
**Tint** shifts towards magenta or green), **Lift** (offset) and **Gain** (multiply),
**Contrast** around 18 % grey, **Gamma**, then **Saturation** with Rec.709 luminance.
It works on unpremultiplied colour and multiplies coverage back afterwards, so alpha
is unchanged and transparent pixels stay transparent. Values are not clamped: HDR
highlights survive, and negative values keep their sign through contrast and gamma.
Every control can be keyframed. At the defaults (0, 0, 0, 1, 1, 1, 1) the node is an
exact identity and the renderer skips it. Use **Exposure** for whole-stop brightness
changes and **Colorize** for mapping luminance to colours.

**LUT** applies a 3D `.cube` LUT (Adobe/Resolve format, sizes 2 to 65, with optional
`DOMAIN_MIN`/`DOMAIN_MAX`; 1D LUTs are refused). Because LUTs work on encoded values and
Veylvik works in scene-linear light, **Input** names what the LUT expects and **Output**
what it produces, each a curve (Rec.709, sRGB, linear, PQ, HLG, Apple Log, S-Log3, LogC3,
LogC4, V-Log) and a gamut (Rec.709, Rec.2020, P3-D65, S-Gamut3, S-Gamut3.Cine, ARRI Wide
Gamut 3/4, V-Gamut); Auto means Rec.709. The node converts scene-linear colour into the
input gamut and curve, looks it up with trilinear interpolation, and decodes the result
from the output curve and gamut back to scene-linear Rec.709. A camera LUT such as
S-Log3 / S-Gamut3.Cine to Rec.709 therefore takes the camera's curve and gamut as Input
and Rec.709 as Output, applied to footage that Input colour has already decoded; a
creative look made for Rec.709 video takes Rec.709 both ways. **Mix** blends with the
original. It works on unpremultiplied colour and keeps alpha. **Choose .cube…** picks
the file; adding a LUT from the Add menu opens the chooser straight away. The `.cube`
file is a project asset like an image, so save-as, recovery, missing-media repair and
reloading treat it the same way.

## Preferences

Open **Edit → Preferences** or **Ctrl+,**. Settings are per user, stored atomically in
`$XDG_CONFIG_HOME/veylvik/preferences.json` (fallback `~/.config/veylvik/preferences.json`).
They do not modify the composition or its undo history. Apply saves; Cancel discards
changes, including the live theme preview. Each section has its own reset.

- **Playback:** optionally start from the playback range’s first frame every time Play/Space starts playback.
  Pausing keeps the current frame visible. Looping defaults on and can also be toggled
  with the timeline loop button. Without looping, playback stops at the final frame;
  starting again from that frame starts at the range’s first frame.
- **Audio:** play footage sound during playback (default on), the output device and the
  volume (0–200%). **System default** follows PipeWire's (or the sound server's) choice
  and is recommended; the list also offers PipeWire, PulseAudio and JACK, then hardware
  outputs by card name, marked *direct* because they bypass the sound server. ALSA
  plumbing (rate converters, up/downmixers) is hidden. **In use** shows the open device,
  its rate, channels and sample format and the backend (for example *ALSA via
  PipeWire*), or why there is no sound. A missing device stays silent rather than
  switching elsewhere. The device list is read in the background.
- **Appearance:** Dark (default), Light, Midnight Blue, Evergreen, or Follow system.
  Omarchy's active `colors.toml` is read from its state directory, with the
  legacy configuration path supported. Other desktops use their portal colour-scheme
  preference; unavailable settings fall back to Dark. Changes are polled every two
  seconds, without changing desktop configuration. Theme previews immediately;
  interface scale (75–200%) applies on Apply. These settings do not affect renders.
- **Workspace:** automatic arrangement for new/opened graphs. Changing this preference
  also updates the current graph; applying an unrelated preference preserves manual placement.
- **Performance:** animated library hover previews, default fast viewer filtering,
  and GPU buffer budget (64–16384 MiB, default 2048). The budget requires restart and
  covers renderer buffers, not total driver memory. `VEYLVIK_GPU_BUDGET_MIB` overrides
  it. CLI render budgets remain environment-controlled. Fast filtering never affects export.
- **Recovery:** enable recovery copies and choose a 10–300 second interval. Disabling
  pauses future writes; existing copies remain available under File → Recover unsaved work.

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
The timeline’s **Range**, **In** and **Out** controls limit playback to an inclusive
frame range. **I** and **O** set its boundaries at the playhead when no text field is
focused. Range settings belong to the current session; they do not change export
ranges or the composition. **Cache range** renders up to 10,000 frames sequentially,
then restores the playhead. Stop caching, seeking or editing interrupts it.

Completed previews have a separate playback cache, including temporal effects and
the preview colour transform. Preferences → Performance sets GPU retention (default
512 MiB, capped at half the renderer budget) and optional disk retention (default
zero/off). Memory pressure evicts frames; a range larger than the budget will only
be partly retained. At most 4,096 frame entries are retained. Green ruler marks
indicate GPU frames, blue marks disk frames; the count reports unique cached frames
in the range. The reset icon clears playback cache ownership.

**Preview: Full / Half / Quarter** changes the size of completed frames retained and
displayed. Effects still evaluate at full project resolution before GPU reduction;
this saves cache memory but does not reduce first-frame effect evaluation cost.
Project coordinates, text layout and export resolution are unchanged.

Optional disk caching transfers pixels from/to the GPU and compresses lossless
RGBA32F data on the preview worker. It uses session directories beneath
`$XDG_CACHE_HOME/veylvik/playback` (fallback `~/.cache/veylvik/playback`), removed on
normal shutdown; recognized directories from dead processes are cleaned on the next
disk-cache session. It is not a persistent project cache. Cache failures fall back
to rendering and appear as a Cache notice. Plan/output/source changes invalidate
retention; external footage and OCIO dependency metadata are checked every two
seconds. Reload still images after changing their files. Custom frame sources must
supply cache dependencies to opt in. Exports release playback GPU retention first.

**Capture A** holds the completed preview on the GPU as a session-only reference.
Choose **Compare: Wipe** (drag the divider; double-click it to centre) or
**Side by side**. A stays frozen while B follows edits, node selection and playback.
**Swap A/B** swaps sides, **Replace A** captures a new completed result, and **Clear A**
releases the reference. Off retains A for later comparison. Opening another project
clears it. Comparison uses shared zoom/pan, Fit/100%, exposure, alpha and filtering;
turn comparison off to manipulate objects in the viewer. Capture is disabled while
the preview is stale or an inspector draft is unapplied. A retains its capture-time
preview resolution and OCIO transform; B uses the current transform. Comparison
does not change exports or project history. One reference frame retains GPU memory
until cleared; no CPU pixel copy is needed. Colour scopes are described below.

**Hide editor** on the transport row collapses the node/animation canvas while
keeping playback and the time ruler visible. **Show editor**, **Nodes**, or
**Animation** reopens it at its previous height. **View → Node / animation editor**
provides the same toggle. This is session layout state and does not modify the project.

**Scopes** toggles a resizable panel with **Four-up** (default), Histogram, Waveform,
RGB parade and Vectorscope. Double-click a plot to expand it or return to Four-up.
Choose **B: live** or **A: reference**; A requires Capture A. Trace intensity only
changes drawing, never image pixels. Hiding the panel releases its retained result
and stops scope calculation. Scopes do not change project history or exports.

The measurement domain is **Display RGB, full range**: the completed preview's OCIO
transform, viewer exposure, filmic/clamped display transform, then sRGB encoding.
Colour is unpremultiplied; fully transparent and non-finite pixels are excluded.
Viewer zoom, checkerboard, guides and Alpha mode are not measured. This is not a
scene-linear/HDR exposure scope or a legal-range broadcast compliance check.
A retains its captured colour transform; shared viewer exposure affects A and B.

Scope presentation follows Klipp's conventions: 1024 levels, waveform columns
matched to the panel's physical width (256–4096), fractional level splats, and
source-column coverage. Four-up uses one row in wide panels and a 2 × 2 grid in
taller panels. Vectorscope measurement uses a 2048 × 2048 chroma grid, 75% RGB/CMY
targets, a hue ring and a 123-degree skin-tone reference. Scope drawing integrates
the bins covered by each screen pixel rather than stretching a small raster.

**10-bit**, **%**, and **IRE** scales use the same code mapping as Klipp. The trace
always spans 0–1023; IRE maps code 64 to 0 and code 940 to 100, with those reference
lines marked. Full-range endpoints therefore read about −7.3 and 109.5 IRE. These
markings do not convert the full-range signal to legal range.

Waveform/parade brightness follows a saturating density curve; sparse samples stay
dim and dense traces approach white. **Trace** uses Klipp's geometric intensity
scale: 0.5 is normal, 0 is six times dimmer, 1 six times brighter. **Low pass** (on by
default) applies a source-size-scaled Gaussian only to waveform/parade measurement.
Histogram/vector always measure original pixels. The histogram has a logarithmic
count axis, subdued fills, bright channel contours, four-code smoothing, and scale
normalisation that excludes endpoint clipping spikes.

Paused previews measure every source pixel up to a 16-million-sample safety limit.
Playback uses automatic power-of-two sampling while retaining at least roughly two
million samples when the source has that many; smaller previews are measured whole.
Measurements refresh at most about 15 times per second during playback, immediately
when paused or settings change. The current sample dimensions are visible. Preview
Half/Quarter settings affect the source; sampled scopes can miss isolated pixels.
The source remains display-referred, not a raw scene-linear/HDR exposure instrument.

Accumulation and drawing stay on Veylvik's shared GPU device without pixel readback.
The bounded scope grid uses roughly 20–80 MiB depending on waveform width, outside
the renderer working-buffer budget. Hiding scopes releases the retained result.
There is no runtime or build dependency on Klipp; measurement and presentation
conventions are aligned with its scope implementation.

**View exposure** adjusts only the display by −16 to +16 stops. **Alpha** shows
coverage as grayscale, independent of exposure; **Reset view** restores normal
color viewing. These controls do not change the document, history or exports.
**Select subject…** (Inspector or **Tools**) cuts out a person or object from a few
clicks in the viewer and tracks it through the shot; **Tools → AI models…** installs,
removes, enables and describes the local models it uses. See
[Subject segmentation](subject-segmentation.md).
Scroll over the viewer to zoom around the pointer and drag to pan. **Fit** or
a double-click centers the whole frame; **100%** maps one image pixel to one
physical display pixel, including on scaled displays. Navigation changes only
the view; exports still use the complete requested frame. Live MCP inspection
reports viewer settings separately from the composition (`scale: null` means
fit mode; pan coordinates are logical UI points).
Select and arrange nodes, drag between output and input sockets to connect them,
add procedural nodes, and preview any node without changing composition output.
The menu bar groups application commands under **File**, **Edit**, **Add**,
**View**, and **Help**, with shortcuts shown beside their commands. **Add** also
provides categorized node submenus and editable effect recipes. The project name
and unsaved indicator stay on the right; long names truncate without hiding menus.

**Help → About Veylvik…** shows the version, build number, source commit, build
profile, platform, OS, CPU, active Vulkan GPU and driver, system RAM, and the
app’s resident memory. **Copy details** copies these facts for a bug report.
Build numbers follow the source commit count, like Klipp; builds from modified
source are marked. The package builder carries this identity into source archives.
The GPU buffer budget is an application limit, not the GPU’s physical VRAM.

Opening the bundled demo starts clean. Playback, seeking, and preview selection
do not require a save on quit; document edits and unapplied inspector changes do.
You can still save the untouched demo explicitly with **File → Save**.

Contextual buttons remain beside their work: **Add node / Find / Fit / Arrange**
in the graph, **Apply / Revert** and selected-node actions in the inspector,
playback and key navigation on the timeline, and display controls in the viewer.

**Add node** in the graph, **Add → Search nodes…**, or **Tab** opens searchable node choices. Type part of a name or
description, use Up/Down to choose, and Enter to add; Escape cancels. New effects
connect to the selected node, and procedural nodes appear at the graph position
where the picker opened. Gradient endpoints and shape defaults follow the
composition size. **Text** adds an editable title over the composition. Image and outlined-text choices open their import dialogs.
Socket drags work in either direction; Escape or dropping on empty space cancels.
Each accepted rewire is one undo step. Named inputs are also editable in the inspector. Inspector
changes are drafts until **Apply**; one apply creates one undo step. Apply and
Revert stay visible above the scrolling inspector. **Ctrl+Enter** applies the
current draft, including while editing a parameter field. Invalid
connections fail atomically. **Revert** discards the draft. Apply or revert before
selecting another node or using undo/redo.

**Reusable effect groups:** **Add → Glow effect / Outline effect** creates a collapsed
node group with public controls. **Group…** in the graph or **Edit → Create effect
group…** groups nodes feeding the selected output. Uncheck source nodes to expose
their connections as inputs. Groups currently have one output and cannot overlap
or nest; every member must contribute to the output. To change group membership
in the editor, ungroup and regroup the desired nodes.

Select a collapsed group to edit its exposed controls in the inspector. Changes
remain drafts until **Apply**, including scalar keyframes and color edits. **Edit effect…**
renames the effect, exposes scalar/color/seed parameters, names controls and inputs,
and captures defaults. Optional scalar ranges must fit the underlying parameter
bounds and contain its current curve and default. These ranges apply to internal
node edits and automation too. **Reset to default** is an ordinary inspector edit.
**Show in animation** opens the underlying node in the animation workspace.

**Open group / Close group** reveals or hides internal nodes without changing the
project. A collapsed input socket reconnects all of its internal consumers together.
**Duplicate effect** (or Ctrl+D on a collapsed group) copies its internal nodes,
controls and animation while retaining external input connections. **Ungroup**
removes only the boundary; pixels and node parameters remain unchanged. Groups,
input names, defaults and controls save with the composition. Nested groups are not implemented yet.

**Add → Effect library…** opens searchable starter and saved effects. The bundled
**Prismatic Glow** adds cyan/rose light spill; **Neon Outline** adds luminous
contours; **Liquid Shimmer** adds a smooth four-second flow loop with protected
frame edges. The library contains **74 starter effects**, searchable by name,
description or category, with category counts in the left sidebar and a search field above the results. Every starter remains an editable group with public controls.

The expanded set includes:

- **Light:** Soft Bloom, Anamorphic Streaks, Dream Focus, Light Sweep, Soft Vignette,
  Spotlight and Metallic Shine (plus Prismatic Glow and Neon Outline).
- **Surface:** Edge Engrave, Duotone Print, Silver Relief, Ink Bleed, Film Grain,
  Scanlines and Hologram.
- **Distortion:** Frosted Glass, Heat Haze, Underwater and Glitch Slices (plus Liquid Shimmer).
- **Motion:** Chromatic Echo, Radial Echo and Orbit Echo.
- **Particles:** Dust Motes, Fireflies, Embers, Snowfall and Starfield.
- **Materials:** Gold, Silver, Aluminium, Bronze,
  Phosphor, Carved Wood, Light/Dark Oak, White Marble, Light/Red Brick,
  Rough Concrete, Clean/Weathered Paper, and Red/Brown/Tan/Dark/Black Leather.
  Surface Reflection, Clear Glass, Frosted Glass Material and Metal Patina add
  connected reflections, refraction and optional weathering.
- **Atmosphere:** Light Smoke, Dense Dark Smoke, Fog Layer, Drifting Fog and Clouds.

Material details also offer **Use as background** and **Use as foreground**.
**Use as background** replaces the scene backdrop by default, keeping the text,
animation and effects above it. Starter scenes identify their backdrop explicitly;
older graphs can use an unambiguous background chain. **Background placement ·
advanced** lets you choose a node/group output to replace, or place the material
behind the finished composition (visible only through transparent areas). Ambiguous
or flattened scenes require an explicit choice instead of silently hiding a layer.
Old backdrop nodes remain available, and the change undoes in one step.
**Use as foreground** composites a full-frame surface over the existing output;
reduce **Material layer composite** opacity to reveal the image below. Both modes
use the composition resolution and leave surface controls editable inside the group.
The optional node field `scene_background` records backdrop intent in saved scenes
and is also available through MCP's node schema.

Apply a material to an isolated text/logo image or a white shape mask. **Text surface**
replaces RGB using the source alpha. Metals reflect a filtered HDR studio environment
and optional rectangular area lights, using alpha-derived bevel normals on text and
shapes. Library spheres use the same material shader with spherical normals; a flat
text face will not reflect the studio in the same way as the sphere.

Besides the 19 original families there are glossy **plastic** (one colour under a
dielectric gloss with a faint orange-peel texture; roughness runs from enamel to
rubber), **car paint** (paint and flake colours; flakes tilt the clear coat so
highlights sparkle), **carbon fibre** (a 2×2 twill whose sheen follows the tow
direction) and **holographic foil** (a white mirror tinted by a rainbow that shifts
with surface angle and position and drifts with Reflection motion). **Glass** is a see-through family: with the shot on its Environment input it shows
what is behind it, bent by the bevel according to its refractive index (from the
look's optics: glass 1.5, ice 1.31, emerald 1.58, ruby and sapphire 1.77, diamond
2.42), tinted by its colour and weighted by Fresnel, with studio reflections and reflections of the
scene itself along the rims; thick, steep edges darken, and frosting adds a milky lift.
Clear glass reads the shot at full resolution; Roughness frosts it through the
shot's roughness levels; dense stones split colour at their edges. Applying a glass
look to text connects the composition's backdrop automatically. **Liquid metal**
swells and ripples as Reflection motion plays; **Pearl** adds an angle-dependent
thin-film sheen (a dark body gives an oil slick); **Slate** draws cleft layers;
**Woven fabric** a plain weave of warp and weft colours. Neon looks are glowing
phosphor recoloured: saturated tubes whose core warms towards white. Applied to text,
a glowing look adds a Neon glow (a gaussian blur added back over the surface) after
the material, and any other look removes it again.

The **Image texture** family takes its colour from the material's optional **Texture**
input: any image, logo or footage, sampled where it lies in the frame (so footage
plays inside text; transform the texture upstream to place or tile it). It is lit,
bevelled and reflective like any material; the texture's own detail tilts the
normal a little, scaled by Grain, and Roughness sets its gloss. A pack pass stores the
texture's straight colour beside the bevel height, so a textured material that also
reflects the shot still reads three images. Without a texture the surface is
black.

Every material also takes optional **PBR maps**, placed like the texture (sampled
where they lie in the frame) and usable with any family, for example a CC0 set from
ambientCG with its colour map on Texture:

- **Normal map** tilts the surface on top of the bevel (whiteout blending, so both
  survive). OpenGL maps (green up, `_NormalGL`) are the default; tick **DirectX
  normal map** (`normal_directx`) for green-down maps. **Normal strength** scales the
  tilt (0 switches the map off, 1 is as authored, up to 4).
- **Roughness map** sets roughness per pixel. Roughness 0.5 reads the map as
  authored; lower values make the whole surface glossier and higher ones rougher, so
  the Look slider still works.
- **Height map** adds relief: white is **Height depth** pixels high (default 4, up
  to 64) and its slope shades like a normal map. A height map and the normal map
  derived from it shade identically.

Maps are data, not colour: the pack pass undoes the sRGB decode that import
applies, recovering the stored 8/16-bit values. PNG import also accepts greyscale
PNGs with a pure-gamma grey ICC profile (common on roughness maps) and decodes them
with that gamma, so their darkest values can read up to about 0.03 high; other ICC
profiles are still refused. The three maps are packed into one image (normal xy,
roughness, height) that the material reads as a fourth image. Map controls appear in
the inspector once their map is connected. Nodes may have up to eight input ports;
a pass binds four images.

**Layered material** (`material_layers`) layers two materials of one shape: connect
two renders of the same title to **Under** (for example brushed steel) and **Over**
(for example red car paint). Over shows where the optional **Mask** input's alpha is
(everywhere without one), scaled by **Mix**. **Edge wear** wears Over away along the
Under input's edges to reveal Under: the shape's alpha blurred over **Wear width**
pixels measures how near each pixel is to an edge, and corners (where it falls
lowest) wear first. **Breakup** lets fractal noise of **Noise size** pixels and a
**Seed** carry chips further in and roughen their borders, and the paint's
thickness shades the bare surface just inside each chip. Wear 0 is clean paint;
light wear chips edges and corners; heavy wear leaves paint mainly in the middle of
strokes. Output colour and alpha mix like the inputs, so both should share a shape.

Seven animated recipes loop over four seconds as editable groups: **Chrome Shine
Sweep** (a shine travels across mirror chrome), **Glint Reveal** (rose-gold glints
flare, hold and fade), **Heat to Embers** (steel heats to embers and cools; thin parts heat first and
hotter: the title minus its own gaussian blur marks edges, corners and thin strokes,
which glow yellow-white and lead the heat, while thicker parts lag at a deep red)
**Rust Spreading** (rust patches grow over steel and recede), **Chipping Paint** (red
car paint chips off brushed steel from the edges and comes back, through Layered
material),
**Paper Burn** (embossed paper burns away along a ragged noise front, with a charred
band and a glowing ember line on the edge; one Burn progress control scales the noise
so every band moves together) and **Wet to Dry** (sandstone that starts dark, glossy
and beaded with water and dries matte). The library's
catalogue adds 48 looks built from these families as data (family, colours,
reflectance, roughness, grain, brushing); their swatches render on the GPU like
saved materials, and applying one to styled text brings its whole finish, while a
family starter still changes only the family.

**Colours** repaint a patterned surface without changing its pattern: wood and oak
have dark and light grain, marble stone and veins, brick mortar plus dark and light
brick, concrete, paper and leather one colour each, weathered paper light and dark
stains, phosphor its glow. Metals take their colour from the optical response
instead. Colours are linear Rec.709, 0–1, stored as the node's `colours` (three RGB
triples; `null` keeps the surface's own colours, so older projects render exactly as
before); **Reset colours** returns to them, and choosing another surface resets them
because its colour roles differ.

**Finish and strip lights.** Three settings shape a material's surface beyond its
roughness, grain and brushing, each 0 by default so older projects render exactly
as before. **Ridges** (0–1) are long shallow undulations along the grain that tip
parts of the surface towards the light and parts away, breaking reflections into
soft bands, as on worked metal. **Imperfections** (0–1) leave the plate not quite
flat, its polish uneven from place to place, and faintly scratched a little off the
grain, some scratches catching the light and some dull. **Brush angle** (degrees)
turns the grain that brushing, ridges and wear follow. They act in the surface
shading itself, so they also apply to library looks on 3D text and shapes. A flat
metal face mirrors only one direction of its surroundings, so it needs something
bright there to reflect: `StudioRig::strips` is a dim room (environment 0.35) with
eight long thin strip lights in front of the surface, tilted 14 degrees, in two
groups so their reflections cover the frame. A new Material node starts as gold in
that studio, brushed, with roughness 0.15, Brush angle 65, Ridges 0.7 and
Imperfections 0.3; the Gold, Silver, Bronze and Aluminium starters and the
library's metal looks take the strips, Brush angle 65, Ridges 0.6 and Imperfections
0.3 while keeping their own roughness, grain and brushing, so their named finish
(Polished, Satin, Matte, Brushed) stays. Projects saved earlier keep the studio
and finish they were saved with. Library samples of strip-lit metals are still
rendered in the plain studio, because on the round sample the strips show as a
small striped patch.

**Reflect the shot** (material inspector) connects the footage behind the layer to
the material's optional **Environment** input, so the surface sits in the shot
instead of a studio: metals reflect the footage with their own colour and Fresnel
response, roughness picks a blurred copy (a six-level box pyramid from half
resolution down, blended between levels), and dielectrics such as wood or leather
take their diffuse light from the shot's average colour. The shot stands in for the
world behind the camera: it is mirrored about the layer's position and shifted along
the reflected direction, so bevels pick up surrounding footage, and Reflection
motion slides it. **Shot reflection** (0–1, default 0.6) blends the studio's
reflections with the shot's; 1 is a pure mirror of the footage, which can vanish
into an evenly lit plate, and 0 is the studio look exactly. Environment strength
0.65 (the default) shows the shot at its own brightness. **Use studio** disconnects
it. Area lights still add their highlights. This is image-based 2.5D shading, not
ray tracing.

A **Light rig** node holds lights as a shared object: the studio environment, its
rotation, ambient light and up to eight area lights. It produces no image; its output
only connects to a **Lights** input (port kinds are checked, so a rig cannot feed an
image input and cannot be the composition output, and viewing it explains that).
A Text surface's optional **Lights** input replaces the surface's own rig with the
connected one, so several titles share one set of lights: moving a light, in the
inspector or by dragging its guide in the viewer, relights every connected surface
and the light-following shadows and condensation coatings that read them. In the
material inspector, **Share these lights** moves the surface's lights into a new
Light rig and connects it (one undo step); **Use own lights** disconnects the rig and
keeps its current lights on the surface, so the look does not jump.

A Light rig's optional **Track** input takes an enabled Planar track. Lights with
**Follow the track** then ride on the tracked point (the centre of the track's
reference quad carried by its motion, in the same scene units as the viewer guides)
plus their own X/Y offset, which is set to zero when following is turned on, and
they turn every frame to keep aiming at the subject. Dragging such a light in the
viewer edits its offset. The rig reads the track as data only, so footage under the
track is not decoded to light a title. Track a lamp, a window or the sun through a
shot and its light moves with it.

**Keyed colours and lights.** A Text surface's pattern colours and every light's
colour, intensity, position and rotation can be keyed. A colour or a position is
keyed as a whole: one key holds all three channels (a *track*), blended linearly by
default; a constant stays a plain `[x, y, z]` array in the project, and lights saved
before tracks existed (plain arrays and a plain intensity number) load unchanged.
In the inspector each colour, position and rotation row has the usual previous key,
key diamond and next key buttons; editing an animated value at the playhead sets a
key there, and dragging an animated light's guide in the viewer keys its position.
Keying a family's own colours first copies them, and **Reset colours** removes the
colour keys. Track keys show on the timeline with other keys and move, duplicate
and delete with them; removing a track's last key keeps its value. Following lights
add their keyed offset to the tracked point. **Match the shot** keys the key light
at the playhead when its properties are animated and replaces them otherwise.

Commands address these by path: `set_track_key` with `track` = `colours.0`..`colours.2`
(linear RGB 0–1) or `studio.lights.N.position`, `.rotation` (degrees) or `.colour`
and a `[x, y, z]` value; `set_key` with `parameter` = `studio.lights.N.intensity`;
`remove_key` with any of these paths. Every key is validated against the same bounds
as the constant values.

**Match the shot** (Light rig, or a surface's own lights) renders the shot behind
the surface at the playhead in the background: the surface's Environment input, else
an Environment of any surface the rig lights, else the composition's backdrop. It
analyses that frame on a coarse grid (at most 96 cells wide): the brightest 5% of
cells, weighted by how far they exceed that level, give the light's position and
colour, and their luminance over the frame's mean gives its contrast. The key light
(the rig's second card, as in the default rig) moves there (an eighth of the shorter
side per scene unit around the frame centre, 2.5 units in front), turns to face the
subject, takes the light's colour (normalised to its strongest channel) and an
intensity that grows with the contrast (1.2–12). The environment scales to the
shot's brightness (0.65 at a mean of 0.18). Soft, even light (contrast below 1.3)
keeps a soft key above instead of guessing a direction. The result is ordinary
editable values in one undo step; a black frame is reported, not applied.

Some node inputs are **optional**: they may stay unconnected and the node then
behaves exactly as before the port existed (the Material's Environment is the
first). The node catalog lists them under `optional`. In the graph an unconnected
optional socket is drawn hollow; drag an output onto it to connect and drag its link
off the socket to disconnect; the inspector's port list offers **None**. Bypass
passes the single required input through.

With a text layer selected, **Show on my text** (on by default) previews every card in
the Materials tab on that text's own first line and font, frozen and centred, instead
of a sphere or tile; glass is shown over a checkerboard it bends. The previews render
on the GPU two at a time in the background and stay cached until the words, font or
material change.

**Save as my material…** (material inspector, or **Save…** under text Appearance)
keeps the whole look — surface, colours, finish, optics and lighting rig — under a
name in **Library → Materials → My materials**. Saved materials are `.veymat` files
in `$XDG_DATA_HOME/veylvik/materials` (normally `~/.local/share/veylvik/materials`),
at most 1 MiB each and 512 listed; unreadable files are reported, not hidden. Their
swatches render on the GPU in the background. Applying one to text that already has
a material replaces the surface, colours, finish, optics and lighting but keeps the
layer's texture scale, bevel and reflection motion; a built-in changes only the
surface family and resets custom colours. Saved materials also work as backgrounds
and foreground layers, can be renamed or deleted (projects keep their own copy), and
any material can be starred; **Favourites** lists starred built-in and saved
materials (`favourites.json` in the same folder).

**Look** is one slider from polished to matte, in the material inspector, the text
Style panel and text Appearance. It sets roughness along a curve that gives the
glossy-to-satin middle more room (0.04 at polished, 0.85 at matte) and, for unbrushed
metals, a grain that rises gently with it; Roughness and Grain stay available for
fine-tuning. It reads the current roughness back, and while roughness is animated it
says so and sets one fixed value when moved.

For gold, silver, bronze and aluminium, **Finish** offers **Polished**, **Satin**,
**Matte** and **Brushed**. Find it in the material node inspector, on a collapsed
single-material group, or under text **Style → Material**. A finish sets roughness,
grain and directional brushing; it preserves bevel, optical response and lighting.
It replaces any roughness/grain animation with constants and undoes as one edit.
Manual settings or animated roughness/grain show **Custom**. **Directional brushing**
can also be toggled separately on any metal. Tarnish and dirt remain separate effects.

**Studio lighting** exposes environment strength and rotation, ambient illumination,
and up to eight rectangular lights with position, rotation, size, colour and intensity.
The default uses the photographic HDR environment with additional cards disabled.
**Reset studio lighting** applies that default to an existing material. **Surface
response** exposes optional conductor reflection colour or dielectric refractive index.
The studio environment is prefiltered offline on Vulkan and sampled by GPU shaders
at runtime. This is 2.5D image-based shading, not scene ray tracing. The separate Shadow node provides a flat receiving-plane approximation.

Texture scale, bevel, roughness, grain and reflection motion remain editable. Wood
combines warped growth rings and fine fibres; phosphor emits textured green light,
and its preset adds adjustable bloom.

Oak has elongated pores and growth rings; marble has warped mineral veins;
bricks have staggered joints and recessed mortar; concrete has pits and aggregate;
paper has fibres (plus stains and creases in its weathered variant); leather has
irregular pebbled grain. Their height fields drive directional surface shading.
These procedural materials are available through the node registry and MCP without
loading an example or preset. User-imported material definitions are not yet supported.

**Surface Reflection** accepts the material image and a separate reflection scene,
including animated clouds or any other image branch. Curvature, mapping, rotation,
roughness and strength are editable. This is image-based 2.5D reflection, not scene
ray tracing. **Clear Glass** and **Frosted Glass Material** expose separate shape,
background-to-refract and reflection inputs. Composite the glass result over that
same background. Frosting blurs that background with a smooth two-pass gaussian
whose reach is the glass Roughness in pixels (up to 256), extending the frame's edge
pixels so a full-frame background stays opaque; refraction and dispersion then
sample the frosted image. **Sun Glints** adds deterministic,
animated star-like highlights at bright surface points; threshold, density, size,
angle and intensity control them. **Metal Patina** independently adds tarnish,
verdigris or rust, dirt, edge wear and scratches; set amounts to zero for clean metal.
All these operations remain usable independently through the node registry and MCP.

Place **Metallic Shine** after the material and before the background merge. It adds
a narrow warm-white reflection core with a softer shoulder and preserves alpha
exactly, including antialiased edges. Edit **Sweep position** keys to change direction
or speed; **Shine width**, **Shine angle** and **Brightness** adjust its appearance.
The default pass travels left to right over four seconds, resetting off-canvas.
**Polished gold** is its dedicated example; **Gilded inferno** also includes a separate
editable Metallic Shine group on the lettering.

Atmosphere presets composite over their selected input; select a transparent source
for a standalone overlay. Their **Smoke, fog and clouds** generator has editable
position, size, density, flow, detail, illumination and seed. Pale smoke and dense
soot use rising plumes; fog offers a low layer and a drifting bank; clouds use
billowing density with directional interior shading. These are deterministic
procedural volume approximations, not combustion/weather simulations or scanned
materials. Surface and atmosphere presets include native-HD editable examples and work
with the existing host-resolution sizing policies.

Particle starters are deterministic overlays made from editable ellipse, animation,
color and blend nodes. Each has 18 particles with staggered four-second lifetimes;
public controls adjust color, glow and amount. Open the group to edit individual
paths, sizes and fades or duplicate particles. They render on transparent inputs
and seek reproducibly, without simulation state. These are authored particle
compositions, not yet a general emitter with forces, collisions or a density control.
Starter presets are authored at 960 × 540. Insertion defaults to **Fit composition**:
geometry, animated positions, blur and displacement scale to the target canvas,
including native 3840 × 2160 UHD. **Fill composition** preserves proportions and
crops to cover a different aspect ratio; **Authored pixels** retains absolute units.

Starter cards play short rendered examples. Select a card for **Before / After**,
pause and frame scrubbing. **Try example** opens a complete editable four-second
scene for every starter, including: **After hours** (neon title), **Prism / identity** (floating emblem), or
**Pink melt** (dripping liquid lettering). Unsaved work uses the normal save/discard/cancel
prompt. Playing an untouched example does not mark it modified; Save As creates
your own project. The shapes, palette, motion and effect controls are editable,
with no external fonts or media required.

The particle showcases now have distinct compositions: **Gilded inferno** pairs
Embers with extruded reflective gold type, layered procedural GPU fire and a floor
reflection; **Toxic nectar** pairs Fireflies with glossy green dripping letters;
**Sands of time** places Dust Motes over sandstone type and a dune; **Deep freeze**
combines Snowfall with crystalline lettering and mountains; **Nova ignition**
places Starfield over a turbulent plasma corona. **Pink melt** demonstrates Liquid
Shimmer on viscous pink lettering with falling droplets and a liquid pool.
These detailed examples and all material/atmosphere/shine examples render at
1920 × 1080; gallery thumbnails remain
320 × 180. The viewer renders the full composition and displays dimensions and
zoom. Use **100%** to inspect pixels without Fit enlargement; integer zoom aligns
the image to physical pixels. Fire uses deterministic turbulent emission and
absorption layers, not fluid simulation.
Search also matches scene titles and descriptions (for example, “gold” or “drip”).
Use **Try example** for these complete compositions. **Before / After** compares
only the selected library effect; **Apply to my composition** inserts that effect,
while the scene's additional flame, material, text and drip nodes stay in the example.

Choose **Apply to my composition**, select the image for each named input, then
**Add effect**. Saved custom effects open directly at input selection.
Insertion creates independent nodes and one undo step, selects its preview, and
leaves the composition output unchanged. Use **Set as output** when ready.
Sizing preserves exact animation times and does not stretch to the composition length.
Live and headless MCP `insert_preset` accept `sizing: "fit" | "fill" | "original"`.
The Klipp adapter must supply its project dimensions and frame rate; this does not
yet implement that adapter.
Shimmer has matching endpoints at zero and four seconds; loop a four-second
composition to repeat it, or edit its keys for another duration.

**Save to library** in a group's inspector captures its current values, animation,
control defaults and wiring as a new copy. The library lives in
`$XDG_DATA_HOME/veylvik/effects` (normally `~/.local/share/veylvik/effects`).
**Export preset…** writes a portable `.veyfx` file from the library's effect
configuration view; **Import preset…** validates and copies one into your library.
Existing export files are preserved; choose a new filename to export another copy.
Preset files are limited to 16 MiB; library scans show up to 256 files within a
64 MiB read budget and report invalid entries without hiding healthy effects.

Portable presets include procedural nodes and vector text outlines. File-based
Image nodes must be outside the group and connected as named inputs; presets do
not bundle or silently depend on external image files. The versioned format and
instantiation API live in `veylvik-document`, separate from the UI. Klipp needs a
host adapter before it can use these presets; compatibility is not implemented yet.
Headless and live MCP expose **get_preset** and **insert_preset**, with explicit
input bindings, revision guards and dry-run support. `get_preset` accepts either
a group UUID or a starter ID (`prismatic_glow`, `neon_outline`, `liquid_shimmer`).
Neither tool accesses files.

Headless and live MCP also expose the material library. **materials** lists every
material an agent can apply, with `id`, `label`, `category`, `description` (not for
saved ones), `source` and `favourite`: the family surfaces (`source: family`, such
as `gold_text` or `marble`), the catalogue's whole looks (`catalogue`, such as
`chrome` or `glass_frosted`) and the user's saved materials (`saved`, id
`saved:<file name>`, read from `$XDG_DATA_HOME/veylvik/materials`; unreadable files
come back as `warnings`). **apply_material** takes `revision` (and `session` live),
a `target` and a `material` id. A Text node gets a Text surface, or its existing one
restyled, exactly as when the library is used on text (neon looks bring their glow,
glass connects its backdrop); a Text surface node is restyled in place. A family
changes only the surface kind and keeps the target's finish and lighting; a
catalogue look or a saved material brings its whole look. It is one undo step, keeps
the output and wiring, supports `dry_run`, and live selects and previews the target.

Groups add no rendering passes. Exposed controls directly reference registered
node parameters; there is no second evaluator or duplicate value storage.
MCP **apply** accepts **put_group**, **ungroup**, **set_control** and
**connect_group** through the shared command schema. **inspect** includes group
metadata, and summary inspection reports the group count. `set_control` accepts
the parameter's existing schema, including complete animated scalar curves.
Group boundary violations and invalid controls reject the entire transaction.

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

**File → Export image sequence…** opens animation export settings. Choose inclusive first/last frames,
a new folder name, and an existing parent folder through the desktop portal.
Export captures the current preview root, composition, and loaded source pixels;
it runs in the background with a progress bar and **Cancel export** between
frames. Completed frames remain in the export folder if cancelled. Cancel the
folder picker itself if it is still open. Other project edits wait until export
finishes.

| Shortcut | Action |
|---|---|
| Space | Play / pause |
| Alt+Left / Alt+Right | Previous / next keyed frame on the selected node |
| Tab | Search and add a node |
| Ctrl+F | Find and preview an existing node |
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

With **Follow system** enabled, the editor automatically detects the active Omarchy
palette or the desktop light/dark preference. Node positions are saved with the project; each drag is one undo step.
**Duplicate node** (Ctrl+D) copies the selected node with its animation and input
connections, selects the copy, and creates one undo step. The composition output
stays unchanged. Apply or revert inspector drafts before duplicating.
**Duplicate branch** (`Ctrl+Shift+D`) copies the selected node and all upstream
nodes as one undo step. Connections are remapped entirely within the copy, and
shared ancestors are copied once. Complete groups and their controls, animation, node versions, and image asset
keys are preserved; source files and loaded image pixels are shared. The copied
branch keeps relative node positions and is placed outside existing graph bounds.
The copied root becomes the preview; the composition output stays unchanged.
Copying is rejected atomically if document size, node count, or graph coordinate
limits would be exceeded.

Right-click a node for preview, rename, bypass (single-input nodes), duplication,
group controls, upstream arrangement, placement pinning, output selection and
safe deletion. Actions target the clicked node. Referenced nodes and the output
cannot be deleted from this menu; disconnect consumers first. Right-click empty
canvas for **Add node**, arrangement and fit controls. **Add node**, the toolbar
button and **Tab** share the focused searchable picker; Up/Down selects, Enter
adds and Escape cancels. The canvas menu captures its opening position before
the pointer moves into the menu.

**Copy selected nodes** in the node context menu (or Ctrl+C over the canvas) writes
a versioned selection to the system clipboard. **Paste nodes** on empty canvas
(or Ctrl+V) inserts independent copies at that location, avoiding existing nodes.
Internal wiring, keyframes, text/fonts and complete groups are preserved. Same-project
external links reconnect to surviving source nodes; unavailable or cross-project
inputs become shared transparent nodes labelled **Input: ...** for reconnection.
Clipboard data is bounded and validated before one atomic, undoable paste; the
composition output is unchanged. Copied image references retain their original
base across projects; files are not read by paste. Use Reload images or Replace
source for unloaded images. Authored resolution and animation times are preserved,
not automatically scaled. Text fields and the animation editor keep their own
clipboard handling. Cut is not yet supported.

Shift-click adds or removes nodes from the selection. Shift-drag empty canvas
box-selects nodes without panning; Ctrl+A selects all visible nodes while the
pointer is over the graph. Drag any selected node to move the selection together,
including hidden members of collapsed groups, as one undoable layout edit.
The active inspector node has a stronger outline. A plain click selects one node;
clicking empty canvas clears the selection. Manual group movement also disables
Auto arrange. Node actions target the node you right-clicked; **Copy selected nodes** explicitly
uses the whole highlighted selection.

Drag the graph background to pan, Ctrl+scroll to zoom, and use **Fit** to see
the whole graph. **Arrange** lays out connected branches as one undoable edit, treating
collapsed effects as a single stage and placing unused branches separately. It never
deletes nodes. **Layout** offers arrangement of the selected upstream branch and
session-only placement pins. Optional node strips and wires share a role-colour
legend; wires follow their source node, and selected connections appear brighter.
Unused nodes are also labelled in text. **Auto arrange** adapts spacing and fits the
view when nodes or pane dimensions change. It is a session-only view mode and
resizing creates no document edits. Dragging a node turns it off and saves the
visible arrangement together with that move as one undo step. Manual placements
remain available while Auto arrange is off. Source images load when the
project opens or an image is imported. The host budget is 1/16 of physical RAM,
clamped to 256 MiB–2 GiB. Still images count against it in full. Video and image
sequences reserve nothing when they open: their decoded frames share one
least-recently-used host cache with that limit, at most two frames per clip, so
a project can hold any number of clips. A clip's frames are released when it closes.
**Reload images** (Ctrl+R) reads all current sources in the background and refreshes
preview after every file succeeds. A failed reload retains all previous inputs;
fix the source and retry. Reload pauses playback and requires a clean inspector
draft, but does not edit the composition or history. Each replacement snapshot
has the same bound; the old snapshot remains resident while it loads.
Loaded sources retained for undo/redo also count toward that bound and survive
reloads. Reopening the project releases sources from discarded editor history.
An Image node's **Replace source…** button chooses a new PNG or OpenEXR file in
the portal. It preserves the node's identity, connections and position as one
undoable edit. Canceling or failing to decode the file leaves the node unchanged.
Selecting the same asset path refreshes its pixels without adding a document edit.
Studio opens valid compositions even if image files are missing, unreadable or
unsupported. Available images load normally; unavailable sources are marked in
the menu bar and Image inspector, so their paths can be replaced or repaired and
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

**Display transform.** A composition's `display` setting decides how scene-linear light
becomes SDR values in the viewer, in PNG frames and sequences, in MCP preview images and
in exported video. **Standard** (the default) clips at white, which is exact for graphics
made in display terms. **Filmic** applies an ACES-style tone curve (the ACES 1.0 reference
rendering with the sRGB output transform, in Stephen Hill's fitted form): highlights roll
off smoothly instead of clipping, which suits HDR and camera log footage. It also maps mid
grey 0.18 to about 0.11 on screen, as ACES does, so footage usually wants a little
exposure. The graph itself and EXR output always stay scene-linear; the transform is
applied only at those SDR boundaries. Change it in **File → Composition settings…**, with
**View → Filmic display (ACES-style)**, or with the `display` command over MCP; each change
is one undo step, and projects without the setting are Standard.

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
| Metallic shine | image | Alpha-preserving specular streak; animated position, width, angle and brightness |
| Shadow | image, lighting | Shadow-only premultiplied output; manual offset or enabled material area lights; merge behind the source |
| Text surface | image | Nineteen procedural materials; bevel, grain, texture scale, roughness and lighting |
| Smoke, fog and clouds | none | Five transparent atmospheric volume styles with editable density and motion |
| Procedural fire | none | Layered turbulent flame emission; seed, position, size, evolution, intensity and turbulence |
| Fractal noise | none | Smooth grayscale noise; seed, scale, octaves, roughness, evolution |
| Exposure | image | −32 to +32 stops |
| Transform | image | X/Y translation in pixels, bilinear sampling |
| Displace | image, map | Animated X/Y strength in pixels; red/green map channels, alpha-weighted |
| Blur | image | Box radius, 0–256 pixels, rounded to an integer |
| Grow/shrink mask | image | Signed alpha radius, −256 to +256 pixels; square neighborhood |
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
the key range they hold the nearest endpoint.

A scalar can instead follow an **expression** (`{"kind": "expression",
"expression": "..."}`, at most 1000 characters and 256 steps): numbers (`1.5`,
`2e3`), `time` (seconds), `frame` (time × the frame rate), `pi`, `tau`,
`+ - * / %`, `^` (power, right-associative; unary minus binds looser, so `-2^2`
is −4), parentheses, and the functions `sin cos tan asin acos atan atan2 sqrt abs
floor ceil round fract sign exp ln log10 pow min max clamp mix step smoothstep mod
noise random radians degrees`. `noise(x)` is smooth gradient noise in −1..1, zero at
whole numbers; `random(x)` is a repeatable value in 0..1 that changes at whole
numbers (`random(frame)` flickers per frame). A link `[Node].parameter` reads
another node's animatable parameter at the same time, after its own keys or
expression; it may name the node by a unique label, and is stored by node id, so
renaming keeps it. Up to 16 links per expression and chains up to 16 deep;
loops, unknown nodes or parameters, and removing a node an expression reads are
refused with the reason. The result is kept within the parameter's range; a
result that is not a number (such as `1/0`) is an error for that frame. An
expression has no keys: setting a key is refused until it is removed. Render
plans include the nodes that expressions read, so changing them re-renders, even
when they are not connected. In Studio, a parameter's `...` menu offers **Use an
expression**, **Link to a parameter** (or **Add a link**) and **Remove expression**
(keeping the value at the playhead); the row then shows the formula with node
names and its current value. Input boundaries are transparent
black. Image dimensions are 1–8192 per axis, subject to GPU limits and budget.
Affine transforms scale around the pivot, rotate clockwise, then translate;
integer coordinates address pixel centers. Negative scales flip the image and
scale magnitudes below 0.000001 produce transparent output. Ellipse feather is
the width in pixels of the soft edge, centred on the outline (half inside, half
outside) and at least 1 pixel. It is measured along the smaller radius, so it is
not an exact signed-distance feather on noncircular ellipses.
Zero ellipse radius produces transparent output. All these scalar controls can
be animated, and their registry supplies both inspector bounds and MCP schemas.

Intermediate images use scene-linear Rec.709 premultiplied RGBA32F. Values above
one are preserved. PNG export unpremultiplies, clips to SDR, and applies sRGB
encoding; it is not an HDR delivery format or a tone mapper.

The compiler validates the entire graph, prunes unreachable nodes, and assigns
explicit compute passes and reusable image-buffer slots by last use. It visits
input branches in depth-first port order, keeping independent layers close to
their consumers instead of scheduling by UUID. Traversal is iterative for deep
graphs; this is a deterministic heuristic, not a minimum-memory optimizer. Blur
(box or gaussian filter; gaussian uses sigma = radius / 3 so the radius stays the
visible reach) lowers to horizontal and vertical passes, with transparent boundaries.
Lens blur boosts colour above its luminance threshold by 1 + highlights × (luminance −
threshold), then gathers over a disc on a golden-angle spiral of up to 400 samples;
when the disc is larger than the sample count, the image is first box-filtered to the
widest spiral gap, so even a single-pixel light spreads into a solid, evenly lit disc.
Discs up to about 11 px gather every pixel exactly. With a depth input (red channel),
each pixel's disc is radius × |1 − focus / depth|, capped at radius (depth ≤ 0 is
infinitely far); the box prefilter follows each pixel's own disc, and the gather
samples within it, counting a nearer sample only when that sample's own disc reaches
the pixel, so in-focus foreground keeps a sharp edge over blurred background (blurred
foreground does not spread past its silhouette). Material bevels are the gradient of
the coverage blurred with a gaussian (sigma = 0.35 × bevel) in two passes before
shading, so corners and acute vertices stay round. The
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
The default 2048 MiB render budget covers declared GPU image buffers, readback,
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
strict schema through MCP. Studio edits points and Bézier control handles directly in the selected-node
viewer, with live GPU preview, Apply/Revert and shared undo. Precision coordinates,
fill rules, segments and contours remain available in the inspector. The viewer pen draws corners with clicks and curves with drags. Close at the first
point to commit one undoable replacement or additional contour; Escape cancels and
Backspace/Ctrl+Z removes the latest unfinished point. The original mask remains
intact until the contour is closed. Right-click a viewer anchor to insert a point
after it, make it smooth or a corner, or delete it. Insertion splits the next
segment at its midpoint without changing the curve; deletion keeps at least three
anchors. These edits use live GPU draft preview and Apply/Revert with shared undo.
The viewer also offers a separate Transform mask mode: drag inside its bounding
box to move all contours, corners to scale, side handles to stretch, and the round
handle to rotate about the centre. Keep proportions controls corner scaling; Shift
snaps rotation to 15 degrees. These geometry edits retain the same Apply/Revert
workflow, and coordinate-limit violations leave the last valid shape intact.
Animate the result with an Affine node;
enable **Animate shape** in the mask inspector to keyframe its points directly.
Point and whole-mask edits author the current shape key. Matching point coordinates
interpolate linearly between keys, holding the first/last shape outside the keyed
range. The key diamond adds/removes a key, previous/next controls navigate keys,
and the selected-node timeline shows their frames. Optional neighbouring key
outlines show the previous shape in blue and next in violet. Disabling animation
freezes the current shape; Undo restores the track. Point-count/segment-type
changes and pen redraw are disabled while shape animation is enabled. Shape tracks
persist in `geometry.keys` and are available through the existing MCP geometry
schema. Tracks are bounded to 256 keys and 65,536 keyed path commands.

For text already connected to the composition, **Place behind object** in its
inspector guides foreground occlusion: choose a source layer without the selected
text, outline the object over that source in the viewer, and close the contour.
The action adds a path, grow/shrink, blur, masked foreground and final merge in one
undoable transaction. Existing text, effects and scene nodes are preserved.
The outline inspector exposes edge expansion and softness in composition pixels,
View result, and Redraw over source. Redrawing reuses the setup; Escape cancels
without adding nodes. The setup uses ordinary serialized graph nodes, so it stays
editable after reopening and through MCP. The foreground overlays the whole
composition; it does not perform automatic object detection or tracking.

Vello scratch memory is separate from the
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
paragraph bidi layout or bitmap/color-glyph rendering. The editable Text node
adds bounded word wrapping on top of this shaping layer.

`shape_lines` adds explicit LF/CRLF line breaks, up to 64 lines, with left,
center or right alignment around the requested baseline x coordinate. Blank lines
retain their spacing. Each line must still be one directional/script run; this
is not automatic mixed-direction paragraph layout. The 4096-byte, 512-glyph and
4096-command limits apply to the entire block. Returned `advance` contains the
maximum line advance and the first-to-last baseline distance, not ink bounds.

Use **Add → Text** for editable titles. **Edit scene text…** in the inspector
finds titles even underneath material/effect groups. Wording, font, alignment,
colour, underline, wrapping, tracking and line spacing remain editable; size,
position, rotation and horizontal/vertical scale support timeline keyframes.
Mirror and flip buttons preserve animation keys. Viewer handles manipulate the
text's source coordinates. Downstream transforms can change where that source
appears; preview the text node itself for direct source placement in that case.
The inspector keeps the wording and **Text / Layout / Transform / Style / Animation** tabs
above the scrolling controls. Sliders have precise numeric entry and individual
**Reset** buttons. Reset uses the registered default at the current frame while
preserving other animation keys; **Keys** opens detailed animation controls.
**Add material or effect…** opens Style, where choosing a material applies it
directly. Changing the material reuses its node and preserves its animation;
**Plain colour** bypasses it. **Browse effects…** opens the full effect library
after the current surface. Choosing an image fill replaces the surface and
reuses an existing fill node, keeping other effects connected.
Typing previews the draft and commits when editing finishes; drags commit after release.
**Apply** explicitly commits a draft and **Revert** discards one. Undo restores a
completed editing session. Changing text keeps connected materials and effects intact.
Double-click the selected title or choose **Edit text** for a readable editor overlay
in the viewer, with caret, selection, clipboard and typing undo. Escape or Done
leaves editing mode. The overlay is upright even when effects distort the result;
it is not a perspective-deformed caret on the final processed image.

**Layout** supports box width/height, padding, top/middle/bottom alignment and
shrink-to-fit. **Layout box** in the viewer enables a resizable box; the inspector
reports overflow. **Transform** includes a nine-point anchor, linked scales and
centre-in-frame. Viewer safe-area guides and centre snapping are optional.
**Style** separates Colour, Gradient, Material and Image/animation fills. Browsing
these tabs preserves the applied fill; explicit **Use colour fill** / **Use gradient fill**
actions replace it. Material
parameters stay in the text inspector. Attached single-source effects, including
compound groups, can be disabled, reordered, removed and adjusted inline. Shared
branches remain explicit in the node graph. Outline, shadow, glow, shine and bevel
relief have quick actions. **Saved text styles** stores typography and procedural
fills/effects in `$XDG_DATA_HOME/veylvik/title-styles`; applying one preserves wording
and position. Styles containing external image assets are rejected.

**Animation** provides typewriter, rise, scale and spin, grouped by shaped character
cluster, word, line or whole title. Entrance/exit timing, stagger and easing are
stored in the document and work in headless rendering too. Parameter diamonds
show keyed state; reset-at-frame and remove-animation are separate actions.
Selected character ranges support relative size and optional font overrides;
the inspector offers bundled Sans styles for selections. Ranges follow text edits.
The text source remains one mask, so a material/effect applies across the whole title.

The searchable font picker lists **Bundled**, **System**, and **Added** fonts by
family, with a separate style selector, selected-font preview, favourites and recent
fonts. Liberation Sans regular/bold/italic/bold-italic and Hack regular
are bundled. System fonts are discovered through Fontconfig in the background;
**Refresh fonts** picks up newly installed OS fonts without restarting Studio.
**Add font…** validates and copies a TTF/OTF/TTC font into Veylvik's own library
under `$XDG_DATA_HOME/veylvik/fonts` (normally `~/.local/share/veylvik/fonts`).
It is available to all Veylvik projects, without changing system fonts. Added
collections expose individual faces. **Use file once…** embeds a font only in the
current project. Selecting a system or added font embeds its bytes (up to 8 MiB),
so rendering does not depend on it staying installed. Font discovery and loading
run off the UI thread. Bitmap-only fonts are rejected on selection; named variable-font instances
are excluded from the list. The bundled Liberation font license is in
`crates/veylvik-text/assets/fonts/LICENSE`.
OpenType feature tags are in **Font options**; language and explicit
right-to-left direction are in **Layout**. Mixed-direction paragraphs, automatic font fallback, variable
font axes and independently coloured character runs are not implemented yet.
Missing characters and excessive outline complexity produce explicit errors.

The library's **Connect into selected layer** option preserves the existing
consumers and composition output when inserting an effect; disable it to create
a separate branch. **Fill text with an image or effect** clips another independent
branch to the text alpha. That branch can be a still, animated procedural image,
or material surface. Cyclic connections are excluded. Background and foreground
placement uses the same ordinary graph, with no preset-specific text renderer.

In Studio, choose **Add → Text outlines…**, enter text with line breaks, and
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

## 3D scenes

3D scenes travel through the same node graph on **Scene** ports (sockets named
`scene` or `scene_N`; validation rejects images there and scenes on image ports).
The frame is the composition's: x right and y down in pixels, z towards the viewer.
Every position is an offset from where a node naturally sits, the frame centre at
depth 0 or a title's own place, so a fresh 3D object lines up with its 2D layer.
Placement (position, rotation in degrees applied about X, then Y, then Z, and
scale) and colours are tracks, keyable as whole vectors through the inspector or
`set_track_key` with `placement.position`, `placement.rotation`, `placement.scale`
and `colour`.

- **3D shape**: cube, sphere, plane, cylinder, cone or torus, sized in pixels, with
  detail (segments, 3–256), colour, roughness and metallic. Shapes and 3D text
  also take a finish, each setting separate and 0 by default: **Brushed** (0–1)
  makes the surface scatter light across its grain, so reflections and
  highlights stretch that way; **Brush angle** (degrees from the object's
  horizontal) sets the grain's direction; **Grain** (0–1) shows the grain as
  fine streaks of brighter and duller polish, in the environment's and the area
  lights' reflections; **Ridges** (0–1) undulate the surface in long shallow
  ridges along the grain, which is what breaks a reflection into bands on a real
  plate; **Imperfections** (0–1) add wear: the polish varies in soft blotches
  and faint scratches catch or dull the light. They stay on the surface as it
  moves and apply to the plain surface, not to a library look. The finish's
  patterns and the softening of sharply curved edges are sized for a
  1920-pixel-wide frame and scale with the frame's width, so the same scene
  looks the same at 1080p and at 4K.
- **3D text**: extrudes the Text or Path node on its Outline input Depth pixels
  deep, front face where the 2D layer sits, turning about its own centre.
  **Bevel** (pixels, at most half the depth) rounds the front and back edges in four
  steps, or chamfers them at 45 degrees with **Round bevel** off: each outline is
  inset along its corner miters (stretched up to four times at sharp corners), the
  caps are triangulated from the innermost ring so faces, bevels and walls meet
  without gaps, and bevel normals stay smooth across gentle curves and crisp at
  corners, so metal titles catch highlights on their edges. New 3D text starts
  with a 6-pixel round bevel; projects saved before bevels keep square edges.
  Very thin strokes can fold under a bevel wider than half their width.
- **Image card**: any image on a flat card that covers the frame at depth 0.
- **3D transform**: moves, turns and scales a whole scene (cameras and lights too)
  about the frame centre. Bypassed, it passes its scene through.
- **Merge scenes**: combines up to eight scenes.
- **Camera**: untouched, it frames the composition exactly at depth 0, at the
  distance its focal length and sensor width imply (1920 × 50 / 36 = 2667 pixels for
  the defaults at 1080p); placement moves and turns it from there. **Aim at**
  names a 3D object (shape, 3D text, image card or model) the camera keeps
  looking at, upright, from wherever it is placed and as both move; the
  placement's rotation is then not used. A missing object hands the rotation
  back. Lights have the same setting.
- **Light**: point, spot (cone and softness), sun or **area**, pointing along −z
  before rotation, or at its **Aim at** object from wherever it is placed (a row
  of strips turns as one, about the row's middle; aiming is worked out after the
  whole scene is put together, so it holds under 3D transforms). An area light is a glowing panel of Width by Height pixels
  (a soft box or strip light): near a surface it lights it unevenly, and glossy
  surfaces show its reflection as a soft-edged shape. Its intensity is the
  panel's brightness. The reflection is found where the mirrored view ray meets
  the panel (held to its edges) and weighted by the surface's lobe, which is
  stretched across the grain on a brushed surface, and by the share of that lobe
  the panel fills, so a small panel on a rough or sharply curved surface is a
  faint sheen. With **Strips** above 1 (up to 32) the panel is split along its
  width into that many strip lights, a row of studio strips from one node;
  **Variation** (0–1) makes them uneven (lengths 0.5–1.5 times the even share,
  gaps up to half a share, heights and brightness up to ±40%, tilt up to ±4
  degrees) and **Seed** picks the arrangement, the same every time. With no
  variation the strips tile the panel exactly. An area light with Shadows on
  casts a soft shadow from the middle of its panel (a row of strips casts as
  one light, from the middle of the row), through a shadow map with a wide
  view; sun, spot and point lights take the four shadow maps first. Surfaces
  with a library look reflect and scatter area lights as plain ones do,
  without the finish settings. In Studio each panel is outlined in the viewer,
  every strip of a row, with a line showing which way it faces.
- **Environment**: what surrounds the scene. The image on its optional Image input
  is a panorama (equirectangular; an HDR or EXR sky, any image or footage) that
  lights and reflects in everything; without one the built-in studio is used.
  **Intensity** scales it (a panorama at 1 is used as is; the studio at 1 is the
  default rig's strength), **Rotation** turns it about the vertical, and
  **Background** also fills the sky behind the objects, softened by **Blur**. The
  first Environment in a scene is used. The panorama is converted on the GPU into
  the studio environment's own layout (a 1024 × 512 level and eight halvings, each
  a 2 × 2 average, plus the 64 × 32 diffuse layer), so plain surfaces and every
  library look light and reflect from it unchanged; the image wraps around
  horizontally and its alpha is divided out. An Image node placed with Fit
  letterboxes a non-16:9 panorama, so stretch it to the frame for a full sky.
- **Shadows**: every sun, spot and point light with **Shadows** on (the default)
  casts shadows, up to four lights per scene. Each renders a 2048 × 2048 depth map
  of the solids from where it shines: a sun an orthographic view fitted to the
  scene's bounds, a spot a perspective view down its cone, a point light a
  perspective view aimed at the scene. Surfaces test it with a 3 × 3 filter whose
  width follows the light's **Softness**, offset along their normal (and with a
  slope-scaled depth bias) so nothing shadows itself. Image cards and shadow
  catchers cast none. The maps are fitted to the casters alone, so a huge shadow-catching
  ground does not cost them detail; a point beyond a map's far end counts as
  behind every caster. Only scene lights shadow; the environment's light is not
  shadowed directly; **Occlusion** handles it.
- **Occlusion** (on Render scene, 0–1, default 1): contact shading of the
  environment's light, so objects sit on the floor and gaps and creases darken.
  The solids are rendered as 1024 × 1024 depth maps from 16 directions spread
  over the sphere (orthographic, fitted to the scene). For each of those
  facing it, a surface counts blockers within a tenth of the scene's size, fading
  with distance, weighted towards its normal; distant blockers do not count, so a
  large floor does not dim everything above it (full within half that reach, then fading out). It scales the environment's
  diffuse and reflected light and the look materials' colour, and adds to a
  shadow catcher's darkening. 0 skips the depth maps altogether.
- **Shadow catcher** (on a 3D shape): invisible except for the shadows falling on
  it. It renders black with the share of the direct light that is blocked as its
  alpha, weighed against the environment's ambient light, and draws after solid
  objects without hiding anything, so a plane laid on the ground of the footage
  darkens the real floor when the render goes over the clip.
- **Depth of field** (on a Camera): **F-stop** above 0 blurs what is out of
  focus like a real lens, treating one scene unit as a millimetre (so at home a
  1920-pixel-wide frame at depth 0 is about 1.9 m across). A point at distance d
  with the lens focused at s blurs by a radius of f² / N / (s − f) · |1 − s / d|
  on the sensor, scaled to pixels and capped at 64 pixels per 1920 of width.
  **Focus on** names a 3D object (shape, 3D text or image card) kept in focus:
  the distance is its bounding box centre along the camera's view, re-measured
  every frame, so it stays sharp as it and the camera move; a missing object
  falls back to **Focus distance**, in scene units from the camera, where 0
  focuses on whatever is at the frame centre. In Studio, the Camera inspector
  lists the objects, and **Focus** in the viewer's 3D toolbar (or **Pick in
  viewer**) outlines the object under the pointer (the smallest box wins, so a
  cube beats the floor under it) and sets it with a click on the scene camera,
  adding a camera if there is none and turning depth of field on at f/2.8 if it
  was off, as one undo step. Esc stops picking. The blur is a depth-aware gather after the resolve:
  out-of-focus neighbours spill onto a pixel as far as their own blur reaches,
  and a sharp object in front is not smeared by the background behind it. The
  free view in Studio always shows everything sharp.
- **Motion blur** (on Render scene): **Motion blur** is the shutter angle in
  whole degrees (0 off, 180 the film look, 360 the whole frame). The scene,
  cameras, lights, shadows and animated looks are evaluated and rendered at
  **Blur samples** (2–32, default 10) exact moments spread evenly across the
  shutter, centred on the frame, and averaged; render time grows with the
  samples.
- **Camera solve** (`veylvik solve-camera`, MCP `solve_camera`): recovers the
  real camera of a filmed shot so 3D sits in it. Features are detected over the
  whole frame (a 16 × 16 grid, up to 220 alive at once, replenished as they
  leave) and followed frame to frame on the GPU: a coarse-to-fine match gives
  each frame's overall motion, every point is matched on its own within ±16
  pixels (so parallax is kept), refined against the frame where the track was
  anchored within ±3 pixels (so matching errors do not add up into drift; a
  track re-anchors when its look has changed too much), and kept only when it
  correlates above 0.85 and comes back to within 0.6 pixels when matched
  backwards. Positions keep their sub-pixel part. The `veylvik-solve` crate then
  solves the camera (CPU, deterministic): keyframes, the pair with the most
  parallax (essential matrix by RANSAC), cameras added outward by resection,
  points triangulated (at least 1.5 degrees of parallax), and robust bundle
  adjustment (Levenberg-Marquardt with the Schur complement and Huber weights;
  sightings over 3 pixels are outliers). An unknown lens is searched over 0.5 to
  3.6 times the frame width and refined; a shot without parallax solves as a
  tripod pan (rotation only), and a tripod shot that turns under 3 degrees
  assumes a 36 mm-equivalent lens and reports `lens_measured: false`. The
  lens's radial distortion (k1, k2, as in **Lens distortion**) is measured in the
  final bundle adjustment and kept when it lowers the keyframe error by at least
  3%; when the straight-lens search finds only a pan, misses keyframes or leaves
  over 3 pixels of error, it searches again through lenses with k1 of −0.08,
  −0.16, −0.24 and 0.08, and a guess must at least halve the median error
  to be kept (a free moving camera can fit a pan's noise slightly better through a
  wrong lens). With distortion, every track is undistorted and the camera
  is placed in the ideal (straight) frame. The result
  is added as one undo step, leaving the output alone: **Solved camera** keyed on
  every frame (focal length in millimetres for the given sensor width, 36 mm by
  default), **Ground (shadow catcher)** when a plane facing up under the first
  camera holds at least 10% of the points (the world is levelled with it and
  faces the first camera's way), **Solved scene** (merge 3D objects into it),
  **Render solved scene**, **Footage lens** (a Lens distortion node putting the
  measured distortion on the render, only when there is some), **Footage
  grain** and **3D over footage** (that merged over the source). The median depth seen from the first frame lands at the home
  distance, so units match 2D pixels there. The report gives frames solved,
  tracks, points, rms and worst-frame error in pixels (under 0.5 is a good
  solve), the focal length, `lens_distortion` ({k1, k2} or null) and the new
  nodes.
- **Point cloud**: points in 3D (offsets from the frame centre at depth 0, at most
  20000) that draw nothing in renders; a camera solve adds its points as
  **Solved points**. In Studio the viewer shows them as dots (Guides > Points).
  Hovering a dot with a 3D shape, 3D text, image card or 3D model selected offers
  **Put … here**: a click moves its centre there (an upright object rests its
  bottom on the point), as one undo step. **Surface** in the 3D toolbar switches clicks to choosing points;
  with three or more, **Ground here** moves the scene's shadow catcher onto the
  least-squares plane through them (facing the camera) and **New surface** adds
  another shadow catcher there, merged next to the point cloud, so tables and
  walls catch shadows too. **Put … on it** turns the selected 3D text, shape, card or
  model to face that plane (its front towards the side the camera is on) and
  sets it there with its back resting on the surface: a title on a floor, a
  table or a wall, as one undo step.
- **Solve camera window** (Layer tools > Solve camera…): solves the selected
  footage in the background over a chosen frame range, with the lens found
  automatically or given (focal length and sensor width in millimetres), showing
  progress (following features, then solving) and Cancel. When it finishes the
  nodes are added as one undo step (if the project did not change meanwhile),
  the viewer shows 3D over footage, and the window gives a verdict on the average
  error (Excellent under 0.3 pixels, Good under 0.6, Usable under 1, otherwise
  Rough), the frames, tracks, points and lens, with notes for tripod shots, an
  unmeasured lens and a missing ground. **Add a test cube** stands a cube on the
  ground with a sun so it casts a shadow, to check that 3D stays put.
- **Match the shot (3D)** (3D toolbar): renders the footage behind the viewed 3D
  render at the playhead (the background of a Merge over it, else the scene's
  Environment image, else the composition's backdrop) off the UI thread and aims
  a sun from it, as one undo step. A light source seen in the frame (the
  brightest 0.5% of a coarse grid within 2% of the peak, in the upper half) is a
  sun beyond the subject along the camera's ray through it, at least 25 degrees
  up; otherwise the light comes from the bright side of the frame (the brightest
  5%), a little in front of the subject, at least 15 degrees up; flat light
  (contrast under 1.3) gives a soft sun from above. The sun takes the shot's
  light colour, an intensity from its contrast (2.5–6 for a seen source,
  1.5–6 otherwise, 1 for flat light) and shadows on; the scene's Environment
  takes 0.65 × mean / 0.18 (0.05–4). It sets the selected light, else the
  scene's first light, else adds a Shot light to the scene's Merge scenes.
  **Pick light** instead takes a click on the light in the frame (a sun, lamp
  or window) as that source.
- **Shine here** (3D toolbar, with a light selected): click a surface in the
  viewer and the light moves to where its reflection shows at that spot from
  the current view, and turns to face it: the view ray is mirrored about the
  surface's normal at the point clicked (the nearest solid surface under the
  pointer, found by a ray against the scene's meshes; cards and shadow
  catchers are passed through) and the light is put along it at its present
  distance from the point. A row of strips moves by its middle. Each click is
  one undo step and the tool stays on until Escape. The move is made in the
  world, so it is exact for a light that is not inside a turned or scaled 3D
  transform.
- **Gold title scene** (Studio's Library > Templates, the first card, added with a double-click; `veylvik demo FILE
  --gold-title`): adds, as one undo step, a 3D title in brushed gold (Brushed
  0.6, Brush angle 65, Grain 0.37, Ridges and Imperfections 0.12, roughness 0.15,
  a 6-pixel round bevel) in a dim even room, lit by three rows of strip lights
  (4 strips each, Variation 0.8), seven small bright panels, five point lights
  close to the sign for stars on its corners and two glancing suns from above
  and from the left that light the bevels facing them; a near-black wall with
  streaked haze, a tilted warm beam and a vignette; an optional subtitle; and
  Bloom with a short streak. It becomes the output and sets the display to
  Filmic, which the look relies on. Every measure scales with the composition's
  width. The title uses the built-in bold font; a heavier font from the font
  browser suits it better. The scene comes from the `veylvik-presets` crate,
  which builds scenes from document nodes only.
- **Grid warp** (`grid_warp`, input image): a mesh of `columns` by `rows`
  points (2 to 17 a side) spread evenly over the frame. `points` holds each
  point's offset in pixels from its rest position, row by row (at most 8192);
  the image under a point moves with it, and the offsets between points follow
  a Catmull-Rom surface across the mesh. **Amount** (0–1, keyable) scales every
  offset. Each output pixel finds its source q by solving q + offset(q) = pixel
  with eight fixed-point steps, which is exact while neighbouring points do not
  cross; sampling is bilinear and premultiplied, transparent beyond the frame.
  An untouched mesh, or Amount 0, passes the image through. The mesh reaches the
  kernel as a small data image bound after the node's inputs. In Studio, with
  the node viewed, the mesh is drawn over the picture and its points are
  dragged; the inspector changes the number of points (resampling the warp so
  far) and resets the mesh. Points are not keyable yet, and there are no tangent
  handles.
- **Particles** (`particles`, optional inputs emitter, target, field, obstacle,
  sprite and scene): soft dots or pictures on a transparent frame.
  - *Birth.* Without an emitter, **Rate** particles a second (at most 2000000
    alive) are born inside the box **Width** x **Height** centred on **X**,
    **Y** (0 x 0 is a point). With an emitter, they are born where that image
    has alpha, **Density** a second for every 100 x 100 pixels of it (the frame
    is divided into cells of 2 to 16 pixels with four particle threads each, so
    density saturates at about one a pixel). Emission begins at **Start** and
    lasts **Duration** seconds (0 is endless; a short duration is a burst).
  - *Motion.* Each lives **Life** seconds, launched at **Speed** along
    **Direction** (degrees, 0 right, 90 up) within **Spread** degrees, then
    accelerated by **Gravity** (down) and **Wind** (right) and carried by
    **Turbulence**, the curl of a noise field with swirls about **Turbulence
    scale** pixels across (so it never gathers or thins the cloud).
    **Variation** makes lives, speeds and sizes differ.
  - *Forces.* The field input pushes particles by the slope of its brightness
    (measured over 6 pixels): **Field push** from bright towards dark (negative
    the other way) and **Field flow** along the edges, so they stream round a
    shape. **Pull** draws them to the point **Pull X**, **Pull Y** (negative
    drives them away) and **Vortex** swings them round it, both easing to
    nothing at the point itself. The obstacle input is a matte: a particle
    about to enter it (alpha above 0.5) is turned back, reflected about the
    way out of the matte and keeping **Bounce** of its speed; each move is
    checked about every two pixels, so thin mattes hold.
  - *Depth.* Particles live in the 3D scenes' space: the frame at depth 0, in
    pixels, z towards the viewer. **Depth** spreads births that many pixels in
    front of and behind the frame, makes the launch spread a cone and lets the
    turbulence carry particles through depth; fields, the pull point, the
    obstacle and the target act on their position across the frame. They are
    filmed by camera number **Camera** of the scene connected to the scene
    input, counted as a Render scene node counts them (0, or no scene, is the
    view that sees the frame exactly), so they follow a 3D or solved camera
    with parallax. Near particles are drawn larger (radius at most 48 pixels);
    those behind the camera are not drawn. They are not hidden by the scene's
    objects.
  - *Target.* With a target connected each particle is given a place on it,
    drawn exactly in proportion to the target's alpha (a cumulative table over
    tiles of 16 pixels, 32 in frames above 2.5 megapixels, then a walk through
    the tile), so none goes without one however small the target. It is steered
    there over the last **Homing** share of its life, arriving as it dies.
  - *Look.* **Size** is the dot's radius (a gaussian of half that as sigma) and
    **Grow** its size at the end of the life as a factor of that. **Colour**
    runs to **End colour** over the life (linear RGBA; values above 1 are HDR
    and glow under Bloom), with a short fade-in and a fade-out over the last
    **Fade** share. **Inherit** tints a particle with the unpremultiplied
    colour of the emitter where it was born, turning to the target's as it
    lands. **Streak** draws it as up to 48 dots back along where it was that
    many seconds earlier (at most 96 pixels). With a sprite connected, that
    input's whole frame (averaged to 64 x 64) is drawn as each particle, Size
    wide either side and in the frame's proportions, tinted by the colours and
    turning at **Spin** degrees a second either way; Streak leaves five fading
    copies back along the path. **Colour variation** turns each particle's hue
    by its own random amount (1 is anywhere round the colour wheel).
    **Glow** 1 adds the particles' light (alpha clamped to 1); 0 lets them
    cover one another (alpha 1 - exp(-sum)).
  - *Time.* Nothing is simulated from frame to frame: a particle is a function
    of its thread, its generation and the frame's time, so frames render in any
    order and repeat exactly. Motion is integrated in 32 fixed steps of the
    life (96 with an obstacle). Keyed settings are followed: the settings are
    sampled 16 times across the last life, a particle is launched with those
    in force at its birth, and forces act as keyed while it flies. Life itself
    is read at the frame, and the target, field, obstacle and sprite images
    are read as they are at the frame. So is the emitter, unless `history`
    (**Follow a moving emitter** in Studio: Off, 4, 9 or 16 moments) is set:
    then the emitter's branch is rendered at that many moments, evenly back
    over the last life and each on a whole frame, into an atlas of tiles at a
    half, a third or a quarter of the frame's size, and a particle is born on
    the emitter as it was at the moment nearest its birth. Moving text or
    footage then leaves its particles behind. It costs that many more renders
    of the emitter's branch for every frame (earlier frames come from the
    frame cache when they are there), and the emitter's colour and edge are
    read at the tile's lower resolution.
  - Light is accumulated in fixed point (steps of 1/4096) with atomic adds,
    then resolved. The frame must be at least 32 x 32 pixels. In Studio the
    inspector shows the settings that apply to the node's wiring, **Start from
    a look** sets every setting for a ready-made effect scaled to the frame
    (Sparks, Embers, Snow, Rain, Confetti, Magic dust, Whirlpool, Disintegrate,
    Assemble),
    and the emitter box and the pull point are dragged in the viewer.
- **Shape warp** (`shape_warp`, input image): pulls the image so the start
  shape `from` lands on the end shape `to`. Both are closed paths in pixels
  (the Path mask's geometry format, keys allowed) with the same number of
  outlines, 1 to 8. Point counts may differ: each outline is resampled to 96
  points evenly along its length from its first point, both taken the same way
  round, and point k of the start goes to point k of the end. A mesh of offsets
  is built from those pairs (inverse-fourth-power weighting, so the outline is
  followed closely): everything inside the start shape follows fully, and
  outside it the pull fades smoothly to nothing **Reach** pixels (1–4096,
  default 200) from the outline. The mesh has a point about every 16 pixels
  (at least 33 a side in small frames, at most 129) and runs through the Grid
  warp's kernel, so detail finer than the mesh is smoothed, and a move longer
  than Reach folds the picture. **Amount** (0–1, keyable) scales the move.
  Matching shapes, or Amount 0, pass the image through. In Studio the inspector
  chooses which shape the viewer edits (Start shape or End shape); the other is
  drawn faintly with links between matching points, and **End shape like the
  start** resets the warp.
- **Custom shader** (`shader`, optional inputs image and second): runs the
  user's WGSL on every pixel. `code` defines `fn shade(at: vec2<f32>) ->
  vec4<f32>`, returning premultiplied scene-linear RGBA for the pixel centred at
  `at` (pixels, y down). `image(at)` and `second(at)` read the inputs with
  bilinear filtering and are transparent outside the frame or when
  unconnected; `size` is the frame's size and `time` the frame's time in
  seconds. Controls are declared in comments and read from `u`: `// slider NAME
  MIN MAX DEFAULT` gives `u.NAME` (f32) and `// colour NAME R G B` gives
  `u.NAME` (vec3, linear RGB, 0–64). Their values are the node's `p1` to `p8`
  in the order declared, a slider using one and a colour three (eight in all);
  each can be keyed or follow an expression, and Studio shows them under the
  declared names and ranges. To be safe on every pixel, the text may not use
  attributes (`@…`), global variables, `while`, `loop` or `continuing`, must be
  ASCII outside comments and at most 16000 characters, and may not redefine
  `image`, `second` or `custom`; `for` loops are allowed only inside `shade`,
  written `for (var i = 0; i < 16; i++)` (or `<=`, `+= N`) with whole-number
  bounds, a counter that is only read in the body, at most 1024 iterations
  each and when nested, and eight loops in all. These rules are checked when
  the node is stored; the full program (the node's bindings and helpers, the
  code, and an entry point that fills `u` and calls `shade`) is then parsed and
  validated with naga when the plan is compiled, so a mistake is an error
  naming the user's line and never reaches the GPU. Results are checked for
  non-finite values like every node's. Compiled shaders are cached by source
  (24 kept). In Studio the inspector has a code field that reports "Compiles"
  or the error as you type; **Use this code** runs it, starting newly declared
  controls at their defaults, and **Revert** returns to the running code.
- **Bloom** (`bloom`, input image): light brighter than **Threshold**
  (scene-linear luminance; **Knee** softens where it starts) spreads as glow in
  its own colour times **Tint**. The bright part is blurred with a wide gaussian
  of **Radius** pixels and a tight one of a fifth of it; 0.55 × wide + 0.45 ×
  tight, times **Intensity**, is added to the image, so lights keep a hot core
  with a long falloff. **Streak** adds a horizontal lens streak: the bright part
  blurred sideways by the radius and upwards by a twentieth of it, so it is a
  soft band and not a hard line along bright horizontal edges. Alpha is unchanged; put Bloom after the merge so a
  title lights its background. It works on HDR values, so it pairs with the
  Filmic display transform, which rolls highlights off to white.
- **Lens distortion** (`lens_distortion`, input image): radial Brown-Conrady
  distortion about the frame centre, with radii normalised by half the frame
  diagonal: a point at ideal radius r is seen at r (1 + **k1** r² + **k2** r⁴),
  both −1 to 1. Negative k1 is barrel distortion (wide lenses), positive is
  pincushion. By default it removes the distortion (each output pixel samples the
  source where the lens saw it), straightening footage; **Redistort** puts it on
  (the ideal radius is found by Newton's method), so 3D or graphics made in the
  ideal frame bend like the footage. Bilinear premultiplied sampling; pixels with
  no source, or beyond where the lens folds over, are transparent, so barrel
  distortion put on leaves the corners of a frame-sized render empty. **Fill the
  frame** (`fill`) enlarges the picture about the centre by the least that
  leaves no empty rim, found from the frame's edge, for a lens look on a finished
  picture; it is off by default because it moves every pixel, which a render
  that must line up with solved footage cannot have. Studio offers Remove the
  lens / Put the lens on and the Fill the frame box.
- **Match grain** (`match_grain`, inputs image and optional plate): adds fresh
  grain every frame to a clean image so it matches footage. The GPU makes unit
  gaussian noise in four channels (red, green, blue and a shared part), blurs it
  with the gaussian blur at **Size** (radius in pixels, sigma = radius / 3),
  undoes the blur's loss of spread, mixes the shared part in by **Colour**, and
  adds it to the unpremultiplied colour times the strength at the pixel's
  luminance (**Shadows**, **Midtones**, **Highlights** at 0.02, 0.18 and 0.9,
  linear in log2 luminance between), with **Red** and **Blue** against green,
  times **Amount**, then premultiplies again, so grain follows alpha. The plate is
  never rendered by it. **Measure from plate** (Studio) renders the plate at the
  playhead and measures its grain: the residual after a gaussian blur of radius
  6 in the flattest 30% of 16-pixel blocks of each brightness band (under 0.06,
  0.06–0.45, above), its standard deviation per channel, how much the channels'
  residuals correlate, and its neighbour correlation. Running Veylvik's own
  generator through the same estimator at sizes 0–6 picks the size and scales
  the strengths, so measured settings reproduce the plate. A camera solve
  measures the middle frame and puts **Footage grain** between the render and the
  merge over the footage.
- **3D model** (`model_3d`): an imported glTF 2.0 model (.glb, or .gltf with its
  buffers and images), by absolute path (Collect project does not copy it yet).
  Every triangle part of the default scene is read with its node transforms
  (a mirroring transform keeps faces the right way out) and its
  metallic-roughness material: base colour factor and texture (8-bit images,
  decoded to linear and box-reduced to at most 1024 pixels a side), metallic and
  roughness factors. Normal, metallic-roughness, occlusion and emissive maps,
  transparency, skins and animation are not read. glTF's y up becomes y down,
  and the model is centred on its box and scaled so its largest side is 1,
  then **Size** pixels; **Extent**, measured on import, gives the scene its box
  without reading the file. A library **Look** replaces every part's material.
  Limits: 4 million vertices in 1024 parts. Studio loads models with the
  project; Add > 3D > 3D model asks for the file, and **Choose model file…** in
  the inspector replaces it (one undo step). The CLI and MCP load a plan's
  models with its other inputs.
- **Render scene**: films a scene as an image with camera 1, 2, … in node order, or
  0 for the home view.

`veylvik-mesh` builds the geometry (extruded outlines and primitives, watertight
and bounded); `veylvik-scene` evaluates a scene graph at a frame into objects,
cameras and lights with world matrices, and projects points through cameras.

**Rendering.** `veylvik-render3d` draws Render scene on the GPU: 4× multisampled,
depth-tested (reversed depth, infinite far plane) and two-sided, into a half-float
target copied to the frame as premultiplied scene-linear Rec.709. Surfaces use a
metallic-roughness model (GGX specular, Lambert diffuse): the same prefiltered
studio environment that lights 2D materials, at the default rig's strength, plus
every point, spot and sun light in the scene (point and spot fall off with the
inverse square, normalised to 1000 pixels; spots blend from the inner to the outer
half-angle by softness). Where a surface curves faster than the pixel grid can
show, such as a tight bevel, its lobe is widened by the change of the normal
across the pixel, so a highlight thinner than a pixel becomes a soft glint
instead of a broken line. Image cards show their image's own premultiplied colour
unlit, sampled at its pixel centres, and draw after solid objects, far to near, so
an untouched card renders like the 2D image to half-float precision. Meshes stay
on the GPU between frames (up to 128). The frame cache keys a Render scene by what
its scene shows at that frame. The render crate holds no 3D code: it plans the
pass, keeps card images alive for it and calls the renderer the host registered
(`Renderer::set_scene_renderer`); Studio, the CLI and MCP register it. Without one,
Render scene reports that the 3D renderer is not loaded, and viewing a scene node
explains that it has no image of its own.

**Navigating a scene.** While the viewer shows a Render scene node, or an image
with one upstream (the nearest is used), a **3D** row
appears under the viewer toolbar and the mouse moves through the scene instead of
panning the 2D image (untick **Navigate** for 2D pan and zoom): drag to orbit
around the view's target, middle-drag or Shift-drag to pan, scroll to dolly, and
hold the right button to walk (W A S D to move along the view, Q and E down and up,
Shift faster, the mouse to look around, at half the view distance per second).
Double-click or **Home view** returns to the view that matches 2D. The first move
starts a **free view** from the scene's camera; it only films the preview (the
render request's `scene_view`, part of the frame cache key) and never changes the
project or exports. **Scene camera** looks through the scene's camera again.
**Camera from view** moves the scene's camera (the one Render scene films with, or
the scene's first) to the free view in one undo step, keyed at the playhead when
the camera is animated. **Record walk**, while playing, keeps the free view at every
played frame and, when stopped, writes them all as camera position and rotation
keys in one undo step. `veylvik_scene::FreeView` holds the view (target, yaw,
pitch, distance, lens) and its `placement` is exactly what a Camera node needs to
film the same picture.

**Scene guides and assisted camera work.** While navigating, the viewer draws
guides through the camera it looks through; they are overlays, never rendered.
**Guides** toggles each kind: cameras (a body, the cone to the frame it films and
that frame, where 2D layers sit; the camera Render scene uses in amber, others
grey, hidden while you look through it), lights in their own colour (a bulb with
rays; a spot's cone with its angle and aim line; a sun's parallel arrows), the 2D
frame as a dashed rectangle at depth 0 with a faint floor grid level with its bottom
edge and X/Y/Z axes at its centre, camera paths (an animated camera's route with a
dot at every key, cached per revision), and the selected object's box. Hovering a
guide names it; clicking selects its node. **View** jumps the free view to Front,
Top or Side around the same target, back to the scene **Camera**, or along the
selected **Light** (to aim a spot or sun).

**Frame (F)** brings the selected object, or everything, into view. **Keep aimed**
keeps looking at the selection while you walk. **Record walk** plays the range from
its start while you walk, draws the path live in red, and stops by itself when play
stops or wraps round; the walk is then smoothed (half a second, so hand shake goes;
the ends stay put; turns past 360 degrees stay continuous), thinned to a key every
four frames with smooth interpolation, and written in one undo step. **Moves**
writes a ready-made eased move over the play range from the current view, aimed at
the selection: **Orbit** (a quarter turn), **Push in**, **Pull out**, **Crane up**
and **Fly by** (keys every six frames). Recording, moves and **Camera from view**
add a Camera to the scene (merged in front of Render scene) when it has none.

**Moving things in the viewer.** The selected 3D shape, 3D text, image card, light
or camera gets handles; **Edit** (or W, E, R over the viewer) picks **Move**,
**Rotate** or **Scale**. Move shows red, green and blue arrows along X, Y (up on
screen) and Z, and a square that moves freely across the screen; Rotate shows a
ring per axis of the object's own frame, turning that rotation angle a degree per
two pixels across the ring; Scale shows axis handles that stretch and a square that
scales evenly (lights and cameras do not scale). A spot or sun also gets an amber
**aim** handle at the end of its beam: drag it and the light turns to point there.
Drags start only on a handle (anywhere else still orbits), work inside 3D
transforms (world motion is turned into the node's own frame), edit the inspector
draft with a live preview, key the placement at the playhead when it is animated,
and apply on release as one undo step.

**Library looks on 3D objects.** 3D text and 3D shapes take an optional **look**:
any library material, catalogue look or saved material (Library > Materials >
Apply to selection, or MCP `apply_material` with the 3D node as target). A look
replaces colour, roughness and metallic, whose sliders then hide; the inspector
shows the look's Look slider and keyable colours, and **Plain surface** removes it.
Applying a family to an object with a look keeps its finish, like a Text surface.
Looks are shaded by the material kernels' own `material_colour`, with the look's
parameters and studio lighting (`veylvik_render::material_uniforms` and
`MATERIAL_WGSL`), so they cannot drift from 2D: pattern coordinates come from the
mesh's pixel-sized uv (a title's caps use composition pixels, as the 2D title
does; primitives wrap theirs around their size), and each normal is turned into
camera space, where a face looking at the camera faces +z as a flat title does. A
face-on 3D title with a look therefore matches the 2D title with the same material
(no bevel) to within a few percent. Scene lights add GGX specular in the look's
reflectance and, for non-metals, diffuse in the colour the look shows. The 2D bevel
does not apply; flat metal faces mostly reflect their surroundings, as a flat 2D
title's face does. Not yet: geometric bevels, shadows, ambient occlusion, depth of
field and motion blur.

## MCP

Launch a session against a saved composition, or with no file for a new empty
one (1920 × 1080, 24 fps):

```sh
./target/release/veylvik mcp /absolute/path/example.veylvik
./target/release/veylvik mcp
```

The server uses newline-delimited JSON-RPC on stdio and MCP protocol version
2025-11-25. Initialize, then send `notifications/initialized` before using tools.
The tools are `inspect`, `graph`, `get_node`, `nodes`, `diagnostics`, `outline_text`,
`apply`, `add_node`, `gold_title`, `particle_look`, `duplicate_branch`, `add_effect`, `undo`,
`redo`, `save`, `render`, `render_sequence`, `render_video`, `preview_image`, `measure`,
`new_project`, `open_project`, `new_from_media` and `import_media`.
`new_project`, `open_project` and `new_from_media` replace the session's project
(revision 0) and refuse to drop changes made since it was opened or saved unless
`discard_changes` is true; `import_media` adds a Footage node and can match the
composition to the media. `measure` reads numbers back from a rendered frame:
displayed brightness percentiles, the scene-linear mean and peak, the share of
pixels above 1.0, the coverage and the box round everything that is not
transparent, for the output, one node or a region.

The live server (`mcp --studio`) has the same tools working on the open editor,
plus `preview`. There `save`, `render`, `render_sequence`, `render_video`,
`outline_text` and `solve_camera` run in the background: the call returns
`started`, renders go through the editor's render queue and start at once
(`inspect.render_queue`), and `inspect.job` reports the result of an outline or a
camera solve. When Veylvik is updated while a relay is running, its tool results
carry a note asking to restart the MCP server, because its tool list is from the
older build.
`nodes` provides defaults, parameter units/bounds, named ports, and operation JSON
schemas from the document registry. The same registry validates scalar parameters
and supplies input ports to the compiler. The tool list itself stays small (about
40 KB): `apply` and `insert_preset` name the operation types and point to `nodes`
for each type's settings, and every operation is still validated in full when
applied. `add_node` adds one node from a type's defaults with only the settings
you name written over them, wires its inputs and returns its id; `gold_title`
adds the Gold title scene and `particle_look` gives a Particles node one of the
ready-made looks, exactly as Studio does. The commands also cover `markers`,
`annotate` (a node's tag and note) and `backdrops`. `render_video` takes
`settings` naming only what differs from the composition's defaults, including
`audio` (`mute`, `music`, `music_gain_db`) and `replace`. Every mutation, save, or render
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

To control the open native editor instead, point an MCP client's stdio command at
the relay. Studio listens by default, so no extra flags are needed:

```sh
./target/release/veylvik open example.veylvik
# MCP client command; it can start before or after the editor:
./target/release/veylvik mcp --studio
```

Studio listens on `$XDG_RUNTIME_DIR/veylvik/control.sock`, or on the path in
`VEYLVIK_CONTROL_SOCKET` when that is set. The directory is created with
permissions 0700 and the socket with 0600; an existing directory is accepted only
if you own it and nobody else can read it. A socket left behind by an editor that
did not shut down cleanly is detected (nothing answers on it) and replaced, but a
live one is never taken over: a second editor listens on `control-<PID>.sock` in
the same directory, and the menu bar's **MCP** tooltip shows the path. Pass that
path (or a directory containing `control.sock`) as `veylvik mcp --studio PATH`.
`--control-dir DIRECTORY` puts the socket in another directory, and `--no-control`
turns live control off for that session. Socket paths are limited to 107 bytes by
Unix; a longer path is refused with an explanation. If the default socket cannot
be opened, the editor still starts, without live control, and the struck-through
**MCP** label explains why.

The relay answers `initialize` and `tools/list` itself, so it can be registered
permanently with an MCP client. Each tool call opens a fresh connection to the
editor, which keeps the editor's single client slot free between calls and picks
up a restarted editor automatically. When no editor is running, a call returns a
tool error saying so; when another client holds the editor, the call gives up after
five seconds and says the editor is busy.

Live tools are `inspect`, `graph`, `get_node`, `nodes`, `diagnostics`, `apply`,
`duplicate_branch`, `add_effect`, `get_preset`, `insert_preset`, `materials`,
`apply_material`, `undo`, `redo`,
`preview`, `preview_image`, `new_project`, `open_project`, `new_from_media`,
`import_media`, `track_surface` and `segment_subject`.

`track_surface` and `segment_subject` take the same arguments as the headless tools
(parsed by the same code), plus `session` and `revision`, and run in the open editor's
Track surface or Select subject window, so the user watches the progress. They return
`started`; `inspect` then reports `job` with the tool, `status` (`running` with
`progress.completed`/`total`, `done` with `result`, or `failed` with `error`, including
cancellation). A finished track is saved as a reusable Planar track node and a finished
segmentation adds Subject matte and Subject cutout, each one undo step. Live `back_to`
must be 0: the editor tracks backward to the first frame. One job runs at a time.

`new_project` replaces the open project with an empty composition (`width`, `height`,
optional `frame_rate` as `{ticks, rate}` and `frame_count`) and returns its session.
`open_project` and `new_from_media` take an absolute `path` and run on the editor's file
worker like the File menu: they return `started`, `inspect` reports `busy` until the job
finishes, and a new `session` once the project is open (`status` and `error` report
failures). All three refuse while the open project has unsaved changes unless
`discard_changes` is true; nothing is saved implicitly. `import_media` imports an
absolute `path` (video, PNG, OpenEXR, or a numbered sequence with `sequence: true`) into
the current session and revision, exactly like **Import media…**, and selects the new
Footage node when done. With `match_composition: true` an empty composition adopts the
media's size, frame rate and duration; the matching dialog is never shown for live
imports.
`inspect` returns a project `session` UUID and `revision`; both are required for
edits, undo/redo, and preview. Opening a project creates a new session identity,
so an old command cannot target a newly opened project with the same revision.
Edits run on the UI thread, appear in the editor, and share its undo history.
`apply` supports dry runs and uses the same command schema as headless editing.
Unapplied inspector drafts, active node drags, and file jobs block remote changes.
New media paths enter through `import_media` (or the editor). Existing asset references,
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
can run on machines without graphics. Run these checks locally; the repository
does not run GitHub Actions CI. Vulkan tests support software and hardware adapters.

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
buffers. `render_gpu_cached` additionally retains
completed output in a 64 MiB, at-most-16-frame LRU cache. Exact keys include
compiled dependencies, evaluated parameter bits, crop, and immutable source
identities. Label edits, unreachable nodes, and times evaluating to the same
values can reuse output. Changed sources and parameters invalidate it. Cache
retention counts toward the render budget and yields to memory pressure.
`cache_stats` reports hits/misses/evictions; `clear_frame_cache` releases cache
ownership. Direct CPU renders and intermediate node results are not cached. There is
no plug-in loader, simulation, or Klipp adapter in this version. Scope measurement and presentation conventions are aligned with Klipp; no Klipp crate is linked.

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
assets: single-part version-2 scanline EXR (long channel names allowed) with half-
or full-float channels, no subsampling, matching origin-zero data/display windows,
and square pixels, in any compression the decoder reads (raw, RLE, ZIP, PIZ,
PXR24, B44, B44A, DWAA, DWAB). Chromaticities must be Rec.709/D65, or absent (the
EXR default).

Multi-layer files (such as Blender's) group channels into layers by everything
before the last dot: `ViewLayer.Depth.Z` is channel Z of layer
`ViewLayer.Depth`. Only the chosen layer's channels are decoded. The main image is
the top-level R, G, B (and A), else a layer whose last part is `Combined`; it is
scene-linear premultiplied RGBA (alpha 1 without an A channel). A Footage node's
**layer** (`null` for the main image) picks any other layer of a still or of every
frame of an OpenEXR sequence; passes become data with alpha 1: R, G, B layers give
their colour, X, Y, Z layers (normals, positions, motion) fill R, G, B, two-channel
layers fill R and G, and one channel (such as depth Z) fills R, G and B. Studio's
footage inspector shows a **Layer** menu for multi-layer files, listed from the
file header in the background. Values that are not numbers, alpha outside [0,1],
and nonzero RGB at zero alpha are rejected. Tiled, deep, multipart, other colour
spaces and cropped data windows are not imported.

Files are bounded to 256 MiB, headers to 4 MiB/4096 attributes, dimensions to
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

The native **View → Render statistics** window and MCP `diagnostics` tool report declared GPU
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
**File → Recover unsaved work…** reopens that list. **Discard** removes an unwanted
stale copy; **Discard all** asks for confirmation before clearing available stale
copies. Neither action deletes saved projects or active sessions. Running editor sessions are excluded using OS
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

### Preferences: what is kept and how it is applied

Preferences has sections for Playback, Audio, Appearance, Workspace,
Performance and Recovery, and three for settings that are also switched
elsewhere in the interface: **Editing** (automatic keyframes; applying
inspector changes to all selected nodes of the same kind), **Viewer and
timeline** (timecode or frame numbers, layer bars, which viewer aids are left
on, thumbnails on nodes) and **Keyboard and layouts** (how many shortcuts were
changed, with a button to the shortcut editor, and how many layouts are
saved). Flipping one of these switches in a menu or panel saves it at once;
the viewer aids, the layer bars and editing several nodes at once are
remembered between sessions (`aids`, `layer_bars`, `spread_edits`).

**New projects** holds what a new composition starts as: its size (1 to 8192
on each side, with HD, UHD, Vertical and Square as shortcuts), its frame rate
(23.976, 24, 25, 29.97, 30, 50, 59.94 or 60) and its length, kept as a number
of frames and shown in seconds; changing the rate keeps the duration. New
composition starts from these, and the start screen's size cards use the rate
and length. A composition made from media still takes the media's size and
rate. **When Veylvik starts without a project** it shows the start screen (the
default), opens the project used most recently (the start screen if there is
none), or starts with an empty composition of the default size. The
**Projects folder** is where Open, Save and Import begin while the project has
no folder of its own, and the **Exports folder** is where frame, video and
sequence exports begin; each is typed or picked with **Choose...** in the
system's file dialog, used only if it is an existing folder, and left empty means the folder Veylvik was started in. These are kept
as `new_width`, `new_height`, `new_rate`, `new_frames`, `launch`,
`projects_folder` and `exports_folder`.

**Editing** also holds how changes are applied and how much is kept for undo.
An inspector change is applied **after a short pause** (the default, 300
milliseconds, adjustable from 100 to 3000) or only **when Apply is pressed**
(or Ctrl+Enter); the viewer shows the change at once either way, and a
pending change is still applied on moving to another frame or node.
**Undo steps kept** (8 to 256, default 64) and **Memory for undo** (16 to 128
MiB, default 128) bound the history; lowering either forgets the oldest steps
at once, and whichever is reached first applies. Kept as `apply_after_pause`,
`apply_delay_ms`, `undo_steps` and `undo_memory_mib`.

**Viewer and timeline** also holds the **preview quality** a session and each
new project starts with (Full, Half or Quarter; the transport's choice changes
it for the session), what the viewer shows **behind transparent areas**
(the checkerboard, black, mid grey or white; never part of a render), and the
**safe areas** as percentages of the frame (action 90 and title 80 by default,
each 50 to 100), used by the viewer's Safe areas and by the text guides. Guide
lines are drawn with a dark edge so they read on any picture. Kept as
`preview_quality`, `backdrop`, `safe_action` and `safe_title`.

**Editing** also sets the curve a setting's **first keyframe** makes: linear
(the default) or smooth (`smooth_keys`). It applies to keys added with the
key buttons; a curve's interpolation can still be changed afterwards.
**Viewer and timeline** sets the **timecode at the first frame**, a whole hour
from 00 to 23 (`timecode_hour`): position fields show and accept timecode
from that hour, a timecode typed without hours is taken to be within it, and a
plain number is still a frame. Lengths and the ruler's labels count from the
composition's start and do not move.

**System** holds: whether Studio **listens for live control** (MCP), which
takes effect at the next start and which `--control-dir` overrides
(`live_control`); the **cache folder** for the playback disk cache, typed or
picked with **Choose...**, used only if it is an existing folder (`cache_folder`), with a button
that clears the playback cache now; how many **recent projects** the start
screen lists, 1 to 30 (`recent_kept`), with a button that clears the list
(the projects are untouched); **Reset all preferences**, which sets every
section in the window back to its original values, changed shortcuts and
saved layouts included, and saves nothing until Apply; and **Open the
preferences folder**.

**Apply** saves only what was changed in the window since it opened. A
setting changed elsewhere while the window was open, such as a shortcut, a
saved layout or the timecode switch, keeps its new value; where both changed
the same setting, the window's value is used. Preferences files written by
earlier builds load, with the newer settings at their defaults.

### Layouts, a second viewer and interface scale

A **layout** is which of the library, the inspector and the node and
animation editor are open, and their sizes. **View > Layout** lists four that
come with Studio: **Standard** (all three; Alt+1), **Wide viewer** (no
library, a low editor; Alt+2), **Viewer only** (the viewer with the transport;
Alt+3) and **Graph** (no library, a tall editor; Alt+4), then the layouts
saved with **Save current layout as...**, each with **Remove**. Up to 16 are
kept in preferences as `layouts`, by name (up to 40 characters); saving under
an existing name replaces it, and a seventeenth pushes out the oldest. Sizes
are held within what each panel allows (library 230 to 420 points wide,
inspector at least 340, editor at least 130 and never taking the viewer's
220). A layout does not change the page, the project or what the viewer
shows.

**View > Viewer in its own window** (also a command) opens a second window
titled "Veylvik viewer" with the picture the viewer is showing, fitted to the
window on black, with the same exposure, channel and display transform. It
follows the main viewer frame by frame, including playback. It shows the
result only: no handles, guides or overlays, and no zoom or pan. Double-click
or F11 makes it full screen and Escape leaves that; closing the window
switches the option off. Where separate windows are not available it appears
as a floating panel inside the main window. It is not remembered between
sessions.

**Interface larger** (Ctrl+Plus) and **Interface smaller** (Ctrl+Minus) change
the interface scale in steps of 10% between 75% and 200%, the same setting as
Preferences > Interface scale, kept in preferences.

### Inspector tools

**Copy and paste settings.** Node actions > **Copy settings** (Alt+C) takes
the selected node's settings; **Paste settings** (Alt+V) puts them onto every
selected node in one undo step. Onto a node of the same kind every setting is
pasted. Onto another kind only the settings of the same name are, each tried
on its own and kept only if the node is still valid. Which file a node reads
(`asset`, `layer`) is never pasted, nor are a node's name, connections, tag or
note. The status line says how many settings went onto how many nodes, or why
nothing was pasted.

**Changing several nodes at once.** With several nodes selected in the graph,
a change made in the inspector is also applied to the other selected nodes of
the same kind as the one shown. Only the settings that the change touched are
carried over, with their new values (including keyframes), so the nodes keep
their other differences. The viewer previews all of them and the change is one
undo step. A line in the inspector says how many other nodes are reached and
has a checkbox to switch it off. Nodes of another kind in the selection are
left alone.

**Filter sliders.** The field at the top of the inspector hides the number
sliders whose names do not contain every word typed. Controls drawn by a node
type itself (colours, choices, buttons, text) are not filtered and stay.

Copy and paste shortcuts cannot use Ctrl+C or Ctrl+V in any combination:
those are taken for copying and pasting nodes and text before a command sees
them, and cannot be recorded in the shortcut editor either.

### Undo history and messages

Every undo step is named when it is made, from its commands and the
composition before them: "Add Glow", "Change softness of Shadow", "Key radius
of Blur", "Arrange 6 nodes", "Add a marker", "Change the length". A step of
several commands is named after the node it added, or its first change, with
"and N more"; names are cut at 80 characters. The names are for reading only
and are not saved with the project. **Edit > Undo history** (also a command)
lists the earliest kept state, the steps that can be undone in order, the
current state (marked) and the steps that can be redone (dimmed). Clicking a
row undoes or redoes as many steps as it takes to reach the composition as it
was after that step, stopping at the first step that cannot be done. Up to 64
steps are kept by default (Preferences > Editing sets the number and the
memory limit), and a new change forgets
the steps that could be redone, as before.

The status line shows the latest message only. The last 300 messages are kept
in **Help > Messages** (also opened by clicking the status line), each with how
long ago it appeared and errors in red; **Copy all** puts them on the
clipboard and **Clear** empties the list. Texts that only say work is under
way or finished ("Ready", "Rendering...") are not kept. While the window is
closed, the status line counts the errors that arrived since it was last open.
Messages are kept for the session and not written to disk.

### Automatic keyframes

A setting without keyframes is never animated by changing it: its value simply
changes. The first keyframe of a setting is always added by the artist, with
the key button beside it.

For a setting that already has keyframes, **Edit > Automatic keyframes**
(off by default, kept in preferences as `auto_key`, also a command without a
default shortcut) decides what a change means at a position where the setting
has no key:

- **Off**: the change is a try-out. The viewer shows it, and a bar at the top
  of the window says so and offers **Add keyframe**, **Discard** and **Always
  add keyframes** (which turns the option on and keeps this one). A try-out is
  not applied after the usual short pause. Going to another frame, playing,
  selecting another node, undoing or saving drops it and restores the
  animation as it was, with a note in the status line. **Add keyframe**, the
  inspector's **Apply** and Ctrl+Enter keep it as a keyframe in one undo step.
- **On**: the change adds a keyframe at the current position, and the status
  line says where.

In the inspector the name of an animated setting is drawn in the accent
colour, brighter at a position that has a key, and its hover text gives the
number of keyframes.

At a position that has a key the change edits that key, whichever way the
option is set. The key buttons (the diamond beside a setting, **Add key at
playhead**, **Reset to default** on an animated setting) always add or change
keys, because they are the artist's own request. The rule covers every slider,
number field and viewer handle of the selected node and of the effects
attached to a text, for numbers, colours, vectors and shapes alike: Studio
compares the keys in the pending edit with those of the saved nodes, and keys
that appeared without a key button are the try-out. Keys set in the curve
editor and on the timeline are direct edits and are not affected.

### Putting a layer's frame away

The frame and handles drawn in the viewer around the selected text or layer
are hidden by a click on the picture beside it, so the picture can be judged
clean, and shown again by a click on the text (for other layers, a click
anywhere on the picture) or by selecting the node in the graph. The click does
not change the selection or the project. Mask shapes keep their points, and
nothing is hidden while the pen, the eyedropper, a tracker or 3D navigation is
in use, where a click means something else.

### Commands and shortcuts

Studio's menu-level actions are one list of commands, each with a name, a
group (File, Edit, Add, Playback, View, Help) and a default key. **Find a
command** (Ctrl+K, or Help > Find a command) opens a search field: every word
typed must occur in a command's group or name, Up and Down choose, Enter runs
the chosen command and Escape closes. Commands without a key, such as the
viewer's channel views and overlays or showing and hiding panels, are reached
this way.

**Keyboard shortcuts** (Help menu, or the question mark) lists every command
with its key. Clicking a key and pressing a combination of Ctrl, Shift, Alt
and one key assigns it; Escape cancels. While a key is being recorded no
command runs. A combination that another command had is taken from it, and
the window says which command lost its shortcut. **Clear** leaves a command
without a key, **Reset** restores its default (taking it back from a command
that had been given it) and **Reset all** restores every default. Changes
apply at once and are kept in preferences as `shortcuts`, a map from command
id to key text such as `"save": "Ctrl+Shift+S"`, with an empty text for no
key; only changed commands are stored, and entries naming no command or no
valid key are ignored. Menus show the current keys. A command's key works
while no text is being typed and no dialog holds the keyboard; of two keys
that differ only in modifiers, the one with more modifiers is tried first.

Keys that belong to a tool while it is in use are fixed and not in the list:
Ctrl+Enter applies inspector changes, W, E and R move, rotate and scale in 3D,
F frames the selection, Ctrl+C and Ctrl+V copy and paste nodes, the pen and
text tools' keys, and the question mark.

### Graph tags, notes, backdrops and overview

These help read a graph and never change a render. A node may carry a colour
`tag` (`red`, `orange`, `yellow`, `green`, `teal`, `blue`, `purple`, `grey`)
and a `note` of up to 400 characters of plain text over any number of lines;
the command `{"command":"annotate","id":..,"tag":..,"note":..}` sets both. A
composition may hold up to 64 `backdrops`, each `{id, label, tag, nodes}`: a
name of up to 80 characters, an optional colour and one or more existing
nodes; `{"command":"backdrops","backdrops":[...]}` replaces the list. Removing
a node takes it out of its backdrops, and a backdrop left without nodes is
removed. Projects without any of these save exactly as before; projects with
them cannot be opened by builds from before they existed.

In Studio a node's menu has **Colour tag**, **Add a note...**, **Backdrop
around N selected node(s)** and **Take out of the backdrop**. A tag colours
a band behind the node's name and a bar down its side, and the node's tile in
the overview. A note is written under the node (once the graph is zoomed to
40% or more), marked by a folded corner at any zoom, and added to the node's
hover text. A backdrop is not a box with a position of its own: it is drawn
around its member nodes wherever they are, so it survives Auto arrange. A node
belongs to at most one backdrop made this way (framing it again moves it).
Clicking a backdrop's name selects its nodes, which can then be moved
together; its menu renames, recolours or removes it (the nodes stay). When
part of the graph is outside the panel, an **overview map** appears in the
panel's lower right corner with every node, tagged ones in their colour, and
the visible part outlined; clicking or dragging in it moves the view there.

**Thumbnails on nodes** (Nodes > **More** > **Thumbnails on nodes**, off by
default, kept in preferences as `node_thumbnails`) draw a small picture of
each image node's own output in the lower right of its tile, once tiles are
drawn at 45% zoom or more. Pictures are made one node at a time on a worker,
only while Studio is idle (not playing, no unapplied edit, the viewer up to
date, nothing being dragged) and after the current frame has stood still for
0.35 seconds. Each is a full render of that node's branch at the current
frame, reduced on the GPU before it is read back, at most 112 pixels on its
longer edge. A picture is kept until that node, anything upstream of it, the
composition's size or the frame changes; renaming, tagging or annotating a
node does not remake it. Nodes that output a scene, lights or data have none,
and neither has a collapsed group's tile. A node whose branch cannot render
shows no picture and is not tried again until it changes.

### Timeline: markers, sound and layer bars

**Markers** are named points in time stored in the composition (`markers`: a
list of `{time, label}`; at most 256, in strictly rising time, one at each
time, labels up to 80 characters without control characters, times within
0..86400 seconds). They never change a render. A composition without markers
saves exactly as before; one with markers cannot be opened by builds from
before markers existed. The command `{"command":"markers","markers":[...]}`
replaces the whole list in one undo step, also over MCP. In Studio, **M** or
Edit > **Add a marker here** adds one at the current position, each shows as a flag
with its name on the ruler, clicking a flag goes there, its right-click menu
renames or deletes it, and comma and period go to the previous and next
marker. Markers beyond the composition's last frame are kept but not shown.

**Sound** is drawn as a strip under the ruler when the composition's footage
has any: mixed the same way as export, on a worker, at 8 kHz, reduced to 2048
peak levels (the louder channel), and remixed only when a footage node, its
timing, or the composition's length or rate changes. Quiet material is drawn
up to four times larger so its shape can be read; the strip is a picture of
the sound, not a level meter. Only the first ten minutes are drawn.

**Layer bars** show, under the ruler, when each footage clip and each Fade is
on screen (the first eight; **View > Layer bars on the timeline** hides them, and they
are not shown while the editor is hidden). Dragging a bar slides the layer in
time; dragging its start or end trims it. Moves are in whole frames, previewed
while dragging and committed on release as one undo step. For footage,
sliding changes `start`; trimming the start moves `start` and `source_in`
together, so the end stays where it was; trimming the end sets `source_out`
(a clip with no `source_out` first gets one where the composition ends). For
a Fade, sliding moves `start` and `end` together and the ends move `start`
or `end`. A layer never starts before time zero, never becomes shorter than
one frame, and a Fade keeps room for its fade-in and fade-out. While
dragging, an edge that comes within six points of the playhead, a marker,
another bar's edge or the composition's start or end lands on it; holding Alt
drags freely. Clicking a bar selects its node. A bar with a lighter end is open-ended: it runs as long
as the media or the composition does.

### Viewer aids

The viewer's toolbar has a choice of what to look at: **Colour**, **Red**,
**Green**, **Blue**, **Luminance** or **Alpha**. A single channel and the
luminance (Rec.709 weights) are shown as grey after the view exposure and the
display transform; Alpha shows coverage directly, independent of exposure.
The **Overlays** menu draws aids over the picture: **Safe areas** (action safe
at 90% and title safe at 80% of the frame), **Centre**, **Thirds**, a **Grid**
in composition pixels whose spacing stays round (1, 2, 5, 10, 20, 25, 50, 100
and so on) and at least 48 points apart at any zoom, **Rulers** along the top
and left in composition pixels, and **Colour under the pointer**: the
position in composition pixels and the straight (unpremultiplied) scene-linear
red, green and blue with the alpha of the rendered pixel there, unclamped, with
a swatch. The value is read back one pixel at a time away from the interface
thread, of the frame as previewed, so at a reduced preview quality it is the
nearest rendered pixel. The aids follow zoom and pan, are hidden while the
viewer is full screen, are kept for the session only, and never change the
composition, a render or an export.

The node and animation editor below the viewer opens 420 points tall and can
be dragged taller, but the viewer always keeps at least 220 points of height.

### Pasting and dropping files

Files reach Studio in two more ways than its menus. **Paste**: with files
copied in a file manager, Ctrl+V while no text field is being typed in takes
them. The clipboard's text is read as files only when every line is an absolute
path or a `file://` address (percent-escapes decoded, an optional host name
dropped, and a leading `copy` or `cut` line ignored) and the file exists;
any other text is pasted as usual, for instance as nodes. **Drop**: files
dropped on the window are taken the same way, with a hint over the window
while they are held there. A `.veylvik` project opens (asking about unsaved
changes, and discarding the rest of the batch). A video (`mp4`, `mov`, `m4v`,
`mkv`, `webm`, `avi`, `mpg`, `mpeg`), a `png` or `exr` image, a `.veyseq` or a
`sequence.json` is imported at the graph's insertion point: the first may
offer to match an empty composition to it, the rest are placed as they are.
On the start screen a media file starts a new composition matched to it.
Anything else is refused with a message. Up to 32 files wait and are taken one
at a time, each through the ordinary open or import path. Drops arrive only
where the windowing system delivers them: on X11, and not on Wayland with the
windowing library Studio uses now (winit 0.30), where pasting is the way.

### Timecode

Positions in time are shown as timecode by default: the transport's position
field, the composition's length beside it, the In and Out fields of the
playback range, and the ruler's labels. Timecode is HH:MM:SS:FF counted at the
composition's frame rate rounded to a whole number. At 30000/1001 and
60000/1001 it is drop-frame, written HH:MM:SS;FF: frame numbers ;00 and ;01
(;00 to ;03 at 59.94) are skipped at the start of every minute except each
tenth, so the reading follows elapsed time and one hour is exactly 01:00:00;00.
Other fractional rates (24000/1001) count at their nominal rate without
dropping, so their reading runs slightly slow against the clock. A position
field accepts a full timecode, the shorter forms MM:SS:FF and SS:FF, or a plain
frame number, and still scrubs when dragged. The ruler labels whole seconds
when there is room, leaving out leading zero hours and minutes. The
**Timecode** / **Frames** button in the transport switches every one of these
to frame numbers and back; the choice is kept in preferences (`time_display`).
The cache counter, key tooltips and the curve editor's ruler still count
frames.

### Start screen

Studio started without a file opens on the start screen, below the top bar;
**File → Start screen** shows it again, and **Go to the editor**, a page tab or
opening anything leaves it. **New composition** cards make an empty composition
at once (HD 1920 × 1080, 4K UHD, Vertical 1080 × 1920 or Square, at 24 frames a
second and ten seconds); **Custom** opens the New composition dialog and **From
a video or image** matches a composition to a chosen file. **Recent projects**
lists up to twelve projects opened or saved here, newest first, each with a
picture, its folder and how long ago; clicking one opens it, **Open a project**
asks for a file, and the context menu removes an entry. Projects whose file is
gone are dropped. **Templates** offers the Gold title in a new HD composition
and the library's Templates tab. Any choice that replaces the open project asks
about unsaved changes first. The list is `recent.json` in
`$XDG_STATE_HOME/veylvik` (`~/.local/state/veylvik`), and the pictures are PNGs
of at most 480 pixels in `$XDG_CACHE_HOME/veylvik/thumbnails`
(`~/.cache/veylvik/thumbnails`), named by a hash of the project's path. A
picture is the frame in the viewer, taken when the project is first shown and
again after each save, read back and scaled on a worker thread.

### Pages

The top bar switches the window between four arrangements of the same panels;
nothing in the project changes. **Compose** (`Ctrl+1`) is the full editor:
library, viewer, inspector and the node or animation editor. **Animate**
(`Ctrl+2`) shows the animation editor in a taller panel of its own size and
keeps the library on the left. **3D** (`Ctrl+3`) hides the editor (the
transport stays); its left panel has a **Scene** / **Library** switch: Scene
lists the document's 3D nodes as Objects, Cameras, Lights and Structure, and
clicking one selects it; Library is the same library as on Compose, so
materials and templates are in reach (collapsed to its rail it keeps a Scene
button). **Export**
(`Ctrl+4`) hides the editor and replaces the inspector with the list of exports
(the render queue: what is rendering with its progress, waiting and done, with
Start queue, Pause after current, Clear completed, and Cancel, Retry and Remove
on each job), an **Export** button and **This frame**. Entering the page, the
Export button, **File → Export video…** and **Export image sequence…** open the
**Export** dialog: Video or Image sequence, the delivery cards (or the two
image formats) with the current choice selected, and **More settings**, which
expands the dialog in place with format and quality, size, frame rate, range,
sound, background and software encoding for a video, or range and folder name
for a sequence (**Fewer settings** folds them again). A line shows what will be
made. **Export** asks where to save and then renders: a job queued from this
page is started at once. **Cancel**, Escape or closing the dialog exports
nothing. A video saved over an existing file replaces it when the save window's
overwrite question was answered yes: the render is written beside it and takes
its place only when it has finished, so a failed or cancelled render leaves the
old file. Image sequences and single frames still need a new name.
Leaving Compose remembers whether the editor was shown and
whether it showed nodes or curves, and returning restores that. **File → Export
video…**, **Export image sequence…** (`Ctrl+Shift+E`) and the **Render** button
open the Export page.

### New compositions and format settings

**File → New composition…** (`Ctrl+N`) creates a composition with one transparent Background node.
Choose a format card (HD 1920 × 1080, 4K UHD, Vertical 1080 × 1920, Square, HD 720,
2K cinema 2048 × 1080, 4K cinema 4096 × 2160, Vertical 4K), a frame rate (23.976,
24, 25, 29.97, 30, 50, 59.94 or 60, stored as exact fractions such as 24000/1001)
and a length (5, 10, 15 or 30 seconds, 1 or 2 minutes). **Custom** in each row
reveals the manual fields: width and height up to 8192 pixels (with Swap), frames
every so many seconds, and a length in seconds or frames. The length is kept in
seconds, so changing the frame rate changes the frame count, not the duration. A
summary line shows the result before creating it. The same dialog serves
Composition settings, opening on Custom wherever the current values are not a
preset. Dialogs open centred on the window and can be dragged. File dialogs come from
the desktop's file chooser; Studio passes its window as their parent, so they
open as dialogs over it at their own size even when Studio is fullscreen. Unsaved work is protected by the same Save/Discard/Keep editing
flow as opening another project. **File → Composition settings…** changes canvas, frame count, and
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
512 and accepts 1..2048; images are never enlarged. A picture whose PNG would not
fit one message (2.9 MB) is refused with advice to lower `max_edge` or to ask
for a `region`, which shows part of the frame at full size. Area filtering happens in
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
animates from −256 to +256 pixels and rounds to the nearest integer, with half
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


## Branch duplication over MCP

Both headless and live servers expose `duplicate_branch`. Supply `revision` and
`node`; live requests also require `session`. The result includes the copied
`root`, `copied_nodes`, and a `mapping` object from original UUIDs to copied UUIDs.
The operation is one atomic undo step and preserves composition output, animation,
shared ancestors, and asset references. It writes no source files.

`dry_run: true` validates the proposed transaction without changing revision,
history, selection, playback, or preview. Returned IDs are provisional: a later
commit generates fresh IDs, so use the mapping returned by that committed call.
Live copies are placed outside existing graph bounds and selected for preview.
Headless copies retain stored positions; use `place` commands or Studio's Arrange
when a different layout is needed. Live mutations require applied/reverted
inspector drafts; dry runs inspect the applied document.


## GPU working-memory budget

CLI renders, headless MCP rendering, and Studio share the optional
`VEYLVIK_GPU_BUDGET_MIB` environment setting. The default is 2048 MiB; accepted
values are decimal integers from 64 to 16384. For example:

```sh
VEYLVIK_GPU_BUDGET_MIB=1024 ./target/release/veylvik open project.veylvik
VEYLVIK_GPU_BUDGET_MIB=1024 ./target/release/veylvik render project.veylvik frame.png
```

Set it when starting the process. Studio applies it before sharing the renderer
with workers; headless MCP reads it when rendering first initializes Vulkan.
Invalid settings fail rendering explicitly, while headless document inspection
and editing remain available without initializing the GPU. Live MCP diagnostics
and the Stats window report the effective limits.

Smaller budgets also reduce the idle pool to at most one-quarter, frame cache to
one-eighth, and vector cache to one-sixteenth of the budget. Larger budgets retain
the usual cache caps. The budget covers declared GPU buffers, including held
frames; it excludes driver allocations, vector rasterizer scratch, and UI
textures. Device buffer-size limits and existing image-decoding limits still
apply. Increasing this setting does not change rendering quality or pixel values.


`check` validates the graph and decodes reachable image assets one at a time,
without initializing Vulkan or applying a GPU budget. Its JSON reports
`working_bytes` for the compiled plan and `source_bytes` for decoded inputs.
A valid check confirms document and asset validity; rendering still enforces the
configured budget, held-frame accounting, and device limits.


## Editable glow and outline recipes

Search **Glow effect** or **Outline effect** in Add node. Each recipe expands
around the selected image into ordinary, editable nodes and one undo step. The
new composite becomes the preview; the composition output stays unchanged.
Nodes are placed together without overlapping the existing graph.

- **Glow:** an 8-pixel Blur, +1-stop Exposure, and additive Blend with the original.
  Edit radius and exposure to control spread and brightness. This blurs the full
  image rather than selecting only highlights; scene-linear RGB can exceed one.
- **Outline:** a 4-pixel Grow/shrink mask, Subtract of the original alpha,
  white Colorize, and Merge over the original. Edit width and the Colorize
  highlights color to customize it. It follows alpha coverage and uses a square
  neighborhood; a fully opaque canvas has no outside space for an outline.

MCP `nodes` includes a `recipes` catalog. Both servers expose `add_effect` with
`revision`, `input` UUID, and `recipe` (`glow` or `outline`); live calls also need
`session`. Optional `dry_run` validates without changing the document or editor.
The returned `root`, `added_nodes`, and `roles` identify the new graph. Glow roles
are `blur`, `brightness`, and `composite`; outline roles are `grow`, `outline`,
`color`, and `composite`. Use those IDs with `get_node` and `set_parameter` to
edit controls. Dry-run IDs are provisional; use committed IDs for later edits.
Headless recipe nodes use automatic layout; live nodes receive explicit positions.


Solid and Colorize controls include **Pick color** beside the numeric RGB/alpha
fields. The picker chooses standard colors in the 0–1 range and leaves alpha
unchanged, including zero alpha. Numeric RGB still supports HDR and negative
values. Merely opening the picker does not clamp or rewrite those authored
values; choosing a color replaces RGB in the inspector draft. Apply/Ctrl+Enter,
Revert, and undo behave as for numeric edits.


Use **Find** in the graph header or **Ctrl+F** to locate an existing node by name,
operation type, or UUID. Searches match all typed words, ignore case, and include
the full authored label even when a long display label is shortened. Up/Down
select a result, Enter selects and previews it, and Escape cancels. Choosing a
node brings its graph card into view without changing composition output,
document revision, or undo history. Apply or revert any inspector draft first.
The result list lays out only visible rows, keeping large graphs navigable.

Inspector scalar controls show a value, previous-key arrow, keyframe diamond,
next-key arrow and (where a default is available) reset icon. Red diamonds remove
the current key when clicked; outlined diamonds add one. Amber outlines indicate
an animated parameter between keys. The arrows navigate that parameter's keys;
`...` opens interpolation and detailed key editing. Reset restores the default at
the playhead while preserving other keys. Text inspector tabs keep these controls
beside their parameters, with a compact overview in Animation.

The playback strip's frame-step buttons and Left/Right arrows move one frame and
pause playback. Keyboard stepping is suppressed while editing text. Frame stepping
and parameter-key navigation commit valid inspector edits before seeking.

The timeline’s **Prev key** / **Next key** buttons and marker strip navigate the
selected node’s applied scalar keys in a dedicated full-width row. Gold diamonds
mark keyed frames; a red diamond marks a key at the playhead, and the vertical
playhead line remains visible between keys. Click or drag on the strip to seek the
nearest keyed frame; navigation pauses playback and does not edit the document.
Keys shared across parameters appear once. Subframe keys seek the nearest output
frame (half frames round up), while authored times remain unchanged. Keys before
frame zero or after the composition’s final frame are hidden. Unapplied inspector
changes appear in the strip after **Apply**.

Choose **Animation** beside **Nodes** to open the animation workspace. Expand a
node to select a scalar parameter. **Keyframes** shows parameter lanes for that
node; **Curves** plots the selected parameter with the same interpolation used by
the renderer. Click a key to select it, Shift-click to select several keys within
one or more parameters, and drag to move them. Drag empty space across lanes to
box-select keys; hold Shift to add to the selection. Shift-click a parameter in
the tree to retain a selection while switching tracks. Curve dragging changes
time and value;
keyframe dragging changes time only. Multi-parameter selections always change
time only, preserving values with different units. Horizontal movement snaps the
*offset* to
whole frames, preserving any existing subframe spacing. Escape cancels a drag.

**Copy keys** / Ctrl+C copies the selection; **Paste at playhead** / Ctrl+V anchors
its first key at the current frame. Delete removes selected keys. Removing all
keys retains the curve's evaluated value as a constant. Paste and drag reject
colliding timestamps rather than overwriting existing keys. Each operation is
one undo step, committed immediately; the inspector's Apply button is not needed.
During dragging only the curve/key preview changes; the GPU viewer updates on
release. Pause playback and apply or revert inspector drafts before editing.

Use **Add key**, the interpolation selector, **Fit composition**, and **Fit keys**
for common operations. Scroll over the plot to zoom; Shift-scroll pans time.
Scroll bars navigate parameter lanes. Fit keys includes keys outside the rendered
composition; the playhead itself stays within the composition. Resize the lower
workspace by dragging its top divider for a larger curve view.

Select keys and choose **Bézier** under **Selected outgoing segments** to reveal
yellow tangent handles. Drag these to adjust acceleration and overshoot. Each key
can use Auto, Smooth, Linear, Hold, or Bézier for the segment following it;
**Use curve default** removes selected overrides. The main interpolation selector
resets all segment overrides in the active parameter. Converting Linear, Smooth,
or Auto to Bézier preserves the curve to handle precision; Hold starts with flat
handles. Handle times cannot cross, and control values must remain inside the
parameter's allowed range. Group easing changes, retiming, paste, and delete are
atomic undoable transactions across up to 64 parameters. Group copy preserves
relative timing across tracks; group paste targets those original parameters in
the current composition. The clipboard accepts up to 4096 keys total.

Keys may include an optional outgoing `segment` object, for example
`{"mode":"bezier","outgoing":{"time":0.25,"value":1},"incoming":{"time":0.25,"value":0}}`.
Handle times are nonnegative fractions of the adjacent key interval and sum to at
most one. Values are offsets from the respective left/right endpoint. Omitted
segments inherit the scalar interpolation, so existing compositions retain their
original timing and pixels. Plots, GPU parameter evaluation, CLI exports, and MCP
all use the same bounded normalized-time evaluator.

Edits use revision-checked `set_parameter` transactions, also available through
MCP; no separate animation format or renderer is involved.

To move or archive an editable project, choose **File > Project files…** and
**Choose parent and collect…**, or use the CLI:

```sh
veylvik collect scene.veylvik portable-scene
veylvik verify-collection portable-scene
veylvik open portable-scene/project.veylvik
```

Collection includes every referenced still, video, LUT and image-sequence frame,
including disconnected nodes. It preserves source bytes, deduplicates canonical
aliases and writes relative references. Sequence manifests are normalized to
portable `.veyseq` lists. Fonts are bundled or embedded in the project already;
text outlines, tracking curves and other document data travel with it.

Custom OCIO conversions become lossless resolved CTF transforms with local OCIO
configs. They retain the used conversion (including resolved external LUTs and
context), not the original config's full colour-space/display catalogue. Builtin
OCIO config identities remain version-pinned. This is not an approximate LUT bake.

The collector streams large video files, reports progress and supports cancellation.
The destination must not exist. Missing/invalid sources fail collection; normal
failure/cancellation removes only the new collection. A process crash can leave an
incomplete folder. Original files and the current project stay unchanged. Limits
are 1 TiB / 100,000 dependency files; existing media decoder limits still apply.
Rendered outputs, render-queue jobs/music and application caches are not project
assets and are not included.

`collection.json` stores relative paths, sizes and SHA-256 hashes, including the
project. Verification rereads the copies and checks that every project file
reference resolves inside the collection, including sequence frames and colour
LUTs. It never falls back to originals. Editing a collected project or media makes
its saved verification hashes stale; collect it again for a new verified archive.

**Project files** also offers **Search folder…** and **Locate…** for unavailable
media and custom OCIO configs. The unavailable-media indicator opens this window.
Search proposes same-name files without following symlink directories, bounded to
100,000 entries / 32 levels. A unique match to a collection's SHA-256 is preselected;
name-only suggestions and ties require your choice. It does not yet implement
Klipp's reel/timecode/transcode matching for ordinary Veylvik projects, which do
not store that identity metadata. Review paths and click **Relink selected sources**.
Validation precedes one undoable transaction updating all uses, preserving node
IDs, connections, timing and animation. Project changes invalidate an old search.

Bundled library previews are generated from the editable showcase factories using
the Vulkan renderer (48 frames at 12 fps, downsampled to 320 × 180). Regenerate
them after changing a showcase or starter effect (an optional final starter ID
regenerates just that entry):

```sh
cargo run -p veylvik-io --example showcase_previews -- crates/veylvik-studio/assets/showcase
```

Gallery textures load only for visible cards and use a six-atlas cache (about 64 MiB
of pixel data), independent of the number of effects in the library.


### Lighting-driven shadows and viewer guides

**Contact shadow** (`contact_shadow`, and the **Contact shadow** button among a
title's attached effects) makes a layer sit on what is behind it. It outputs black
premultiplied occlusion to merge behind the layer: a ring of taps on the layer's
sharp alpha gives dense, tight darkening right at the edge and in counters, and a
gaussian of the alpha over **Reach** pixels (up to 256) a soft falloff that fades to
nothing at the reach. Both are read from a point shifted **Lean** pixels towards the
light, so the occlusion leans away from it: with a material on the optional
**Lighting** input, from that material's enabled lights (weighted by power, in the
same frame as cast shadows); otherwise towards **Angle** (degrees, 90 = down).
**Strength** scales it. The title button sizes Reach and Lean to the composition
(32 and 6 pixels at 1080p) and connects Lighting to the title's surface. Use it with
or instead of a cast Shadow.

New text **Shadow** actions create a Shadow node followed by a merge. **Follow scene
lighting** defaults off. When enabled, the nearest upstream Material on the Lighting
input supplies its area lights; breadth-first traversal follows input-name order to
resolve ties. Connect Lighting directly to a material for unambiguous control.
Light 0 uses all enabled emitters; 1–8 chooses one. Lights behind the receiver or with
zero intensity contribute nothing. An HDR environment has no positional emitters and
does not generate a directional shadow. In the text inspector, use **Style → Attached effects → Text shadow → Follow scene
lighting**; the checkbox is visible even with Controls collapsed. Existing offset/blur
shadow groups retain their original behaviour until **Enable light-following shadow**
is clicked in the text Style tab or group inspector. That one-step undoable upgrade
keeps the output identity, strength and softness, resets manual offsets, and connects
the same material branch for lighting. Shared internal nodes are protected from removal.

Distance is object separation from a flat receiving plane in pixels. The renderer
samples rectangular emitters with a deterministic 32-sample pattern and uses a local
parallel-light projection. Larger emitters and greater separation broaden the shadow;
X/Y, additional softness and strength remain artistic adjustments. Multiple emitters
contribute intensity/colour-luminance-weighted coverage. This is not geometric ray
tracing, depth reconstruction or shadow reception on arbitrary 3D objects.

Opening **Studio lighting** shows optional editor-only light guides. Their diagram
uses the viewer centre as the rig origin and one eighth of the fitted image's shorter
side per scene unit. Enabled lights have vector handles; the selected emitter has an
outline and line to the origin. Drag moves X/Y; inspector controls retain Z, size and
rotation. Off-screen markers stay at the viewer edge. Closing the lighting section or
unchecking **Show light guides in viewer** hides them. Environment rotation remains
separate, with no invented markers for photographic highlights.

### Renderer shutter sampling API

`Renderer::render_gpu_shutter` evaluates a compiled output at 1–32 stratified
subframe times and averages scene-linear premultiplied RGBA32F on the GPU.
`Shutter` specifies angle (0–360 degrees), midpoint phase (-0.5–0.5 frames),
sample count and rational frame rate. Offset rounding is limited to nanoseconds;
absolute timestamps retain rational arithmetic. The caller can select a branch
with `Plan::compile_output`. Footage is requested at each sample time; there is no
optical-flow interpolation. The API does not yet expose a Studio motion-blur node.

A cancellation callback is checked between samples; cancellation returns `None`,
never a partial frame. One sample and one accumulator are retained, both within
the renderer budget. Temporal outputs do not enter the ordinary frame cache.


### Material-edge and viewer antialiasing

Material nodes default `smooth_edges` to true, including documents where the field
is absent. **Surface response → Smooth bevel highlights** switches between continuous
Gaussian-derivative normals with coverage-weighted subpixel shading and the legacy
integer-tap bevel. Source alpha is preserved exactly; this does not blur or enlarge
the text silhouette. The material setting is saved and applies to exports as well
as previews. Smoothing changes the bevel's reflection profile and adds GPU work.

Viewer minification integrates the source-pixel footprint with up to 8 × 8 bilinear
samples before exposure/display conversion, in linear premultiplied colour. At 100%
and higher zoom it retains the normal bilinear reconstruction. **Fast view** disables
footprint filtering without modifying the composition or exports. Antialiasing cannot
recover detail missing from a low-resolution composition enlarged beyond 100%.

## Temporal effects and colour management

**Motion blur** samples its upstream branch at fractional times on Vulkan. Place it after animated transforms/warps and before merging a background. Shutter angle and phase are animatable; preview/final sample counts are separate, capped at 32. Nested temporal evaluation is limited to 256 evaluations per request and eight boundary levels. Superseded previews cancel between samples. Existing camera blur is not removed.

**Time remap** evaluates `source seconds = timeline seconds × rate + source time`. Negative rate reverses; zero freezes. Set rate to zero and keyframe Source time for explicit speed ramps. These image nodes do not retime audio. Clip trims and out-of-range transparency still apply.

**Optical flow retime** evaluates adjacent source-cadence frames and interpolates using bidirectional pyramidal Lucas–Kanade motion fields on Vulkan. When adding a node, a single consistent upstream clip cadence is detected, otherwise composition cadence is used. Set Source frame rate explicitly for mixed-rate or already-retimed branches. Fast/Balanced/Fine cap final analysis at 512/1024/1536 pixels on the long edge; Balanced/Fine preview caps are 768/1024. The output retains full composition resolution. Confidence combines image texture, residual and forward/backward agreement; occluded or unreliable areas fall back to the nearest source, and major detected cuts fall back completely. Show confidence renders a green/red diagnostic **into the node output, including export**. A bounded one-pair motion-field cache supports repeated interpolation. Textureless regions, complex deformation, large motion or illumination changes can still fail; this is not learned optical flow or vector motion blur.

**Colour management** under View separates preview and output OCIO transforms. Preview never changes exported pixels; enabled output transforms affect all exports, including EXR. An OCIO node supplies explicit branch conversions. Configurations can be built-in `ocio://` URIs or local config files with their LUTs. Native OpenColorIO 2.5 generates the GPU transform; unsupported dynamic uniforms fail explicitly. Config/LUT changes invalidate cached processors. External assets must accompany custom configurations; portable presets reject external config references.

The default working domain is linear Rec.709. ACEScg project mode converts compositing, grading and spatial passes to AP1 with D60 adaptation; materials keep their authored Rec.709 interpretation through explicit boundaries. Public frames and EXR remain linear Rec.709 unless an explicit output transform changes them. AgX requires a config that supplies it, such as Blender's; it is a view transform, not a working colour space.
