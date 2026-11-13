# Veylvik features

Everything Veylvik can do today, area by area. Veylvik is a native Rust node compositor for Linux that renders on Vulkan through wgpu, with the Studio editor, the `veylvik` command line, a headless MCP server and live MCP control of the open editor. The [reference](reference.md) has the exact behaviour, limits and formats behind each item.

## Node graph editing

- Connect nodes by dragging between output and input sockets, in either direction; Escape or dropping on empty space cancels.
- Each accepted rewire is one undo step; invalid connections fail atomically.
- Named inputs are also editable in the inspector.
- **Add node** button, **Add → Search nodes…** or **Tab**: searchable picker over all 36 node types, recipes and text outlines (by name or description); Up/Down, Enter to add, Escape cancels.
- New effects connect to the selected node; procedural nodes appear where the picker opened.
- Gradient endpoints and shape defaults follow the composition size.
- **Add** menu submenus: Generators (Solid, Noise, Fractal noise, Gradient, Procedural fire, Smoke, fog and clouds); Masks and shapes (Ellipse mask, Path mask, Channel mask, Grow/shrink mask, Combine masks); Transform and distort (Affine transform, Transform, Displace); Colour and filters (Exposure, Colour correct, Colorize, Blur); Compositing (Merge, Blend, Fade, Apply mask, Surface blend); Materials and light (Text surface, Tarnish and dirt, Glass surface, Surface reflection, Sun glints, Metallic shine); Keying (Chroma key, Luma key); Tracking (Planar track, Planar warp). Every node type is in exactly one submenu or has its own entry.
- **Add → Image or footage…**, **Add → Text**, **Add → Text outlines…**, **Add → Effect library…**.
- Editable recipes: **Glow effect**, **Outline effect**.
- **Find** / **Ctrl+F**: locate a node by name, operation type or UUID (all words, case-insensitive), Enter selects and previews it.
- Preview any node without changing composition output; **Set as output** / **Set selected node as output**.
- Shift-click to add/remove from selection; Shift-drag empty canvas to box-select; Ctrl+A selects all visible nodes.
- Drag a selection together (including hidden members of collapsed groups) as one undoable layout edit.
- **Duplicate node** (Ctrl+D): copies node with animation and input connections.
- **Duplicate branch** (Ctrl+Shift+D): copies node and all upstream nodes, shared ancestors copied once, groups/animation/asset keys preserved, placed outside existing bounds.
- **Copy selected nodes** (Ctrl+C) to the system clipboard as a versioned selection; **Paste nodes** (Ctrl+V) at the cursor.
- Paste preserves internal wiring, keyframes, text/fonts and complete groups; same-project links reconnect, unavailable/cross-project inputs become **Input: ...** placeholder nodes.
- Copy and paste works across projects; cut is not supported yet.
- Node context menu: preview, rename, bypass (single-input nodes), duplication, group controls, upstream arrangement, placement pinning, output selection, safe deletion.
- Referenced nodes and the output cannot be deleted from the context menu.
- Canvas context menu: **Add node**, arrangement and fit controls.
- Pan by dragging the background, Ctrl+scroll to zoom, **Fit** to see the whole graph.
- **Arrange**: lays out connected branches as one undoable edit, collapsed effects as one stage, unused branches separately; never deletes nodes.
- **Layout**: arrange the selected upstream branch; session-only placement pins.
- **Auto arrange**: session-only mode that adapts spacing and fits the view; dragging a node turns it off.
- Optional node role strips and wires with a role-colour legend; selected connections brighter; unused nodes labelled.
- Node positions saved with the project; each drag is one undo step.

## Viewer

- GPU-resident frames on the same Vulkan device as the UI, no CPU copy.
- Scroll to zoom around the pointer, drag to pan; **Fit** or double-click centres the frame; **100%** / **Actual pixels (100%)** maps one image pixel to one physical pixel.
- Displays dimensions and zoom; integer zoom aligns to physical pixels.
- **View exposure** −16 to +16 stops (display only).
- **Alpha** / **Show alpha** view of coverage as greyscale; **Reset view** / **Reset display settings**.
- Transparency over a checkerboard; bilinear filtering in linear premultiplied colour; SDR display transform.
- Previous preview stays visible while rendering, marked when stale.
- Playback skips ahead when rendering is slower than the frame rate; render errors stop playback.
- On-canvas handles (hidden during playback/stale previews, never exported):
  - Ellipse: centre and X/Y radii.
  - Gradient: start/end handles, radius in radial mode.
  - Affine transform: centre (translate), two axis handles (independent X/Y scale, flip through zero), square handle (rotation).
  - Transform: translation handle.
  - Text: source-coordinate handles, resizable layout box, optional safe-area guides and centre snapping.
  - Path mask: points, Bézier handles, pen, whole-mask transform (see Masks).
  - Planar tracking corners and tracking area; point tracking patch and search box.
- Handle gestures are inspector drafts until Apply; animated parameters get keys at the playhead.
- **View → Render statistics** window.

## Inspector and parameters

- Edits are drafts until **Apply** (Ctrl+Enter, also while editing a field); **Revert** discards; one Apply = one undo step.
- Apply/Revert stay visible above the scrolling inspector.
- Scalar controls: value, previous-key arrow, keyframe diamond, next-key arrow, reset icon, `...` for interpolation and detailed key editing.
- Red diamond removes the key at the playhead; outlined diamond adds one; amber outline marks animated between keys.
- Reset restores the registered default at the playhead, preserving other keys.
- **Pick color** for Solid and Colorize (0–1 range, alpha unchanged); numeric RGB keeps HDR and negative values; opening the picker never rewrites values.
- Image node **Replace source…** keeps identity, connections and position (one undo step).
- Footage inspector shows resolution, duration, frame count, average frame rate, colour assumptions and active decode backend.

## Animation

- Every scalar parameter can be constant or keyframed at exact rational times (`ticks/rate`).
- Interpolation: Linear, Hold (`step`), Ease in/out (`smooth`, cubic smoothstep), Auto smooth (`monotone`, no overshoot), and per-segment Bézier.
- Per-key outgoing segment override: Auto, Smooth, Linear, Hold or Bézier; **Use curve default** removes overrides.
- Curves hold their end values outside the keyed range; 1–4096 keys per curve.
- Inspector: **At playhead** edits or inserts a key, preserving interpolation.
- Expanded key list edits exact rational times and values, removes keys, shows a read-only curve preview.
- **Remove key at playhead**; removing the last key keeps its value as a constant.
- **Make constant here** replaces a curve with its value at the playhead.
- Timeline: play/pause, frame-step buttons and Left/Right arrows, scrubbing.
- Timeline **Prev key** / **Next key** and marker strip of the selected node's keys (gold diamonds, red at playhead); Alt+Left/Right.
- **Animation** workspace (beside **Nodes**):
  - **Keyframes** view with parameter lanes; **Curves** view with renderer-identical interpolation.
  - Click, Shift-click and box-select keys across one or more parameters; Shift-click parameters to keep a selection.
  - Drag keys: time and value in Curves, time only in Keyframes or multi-parameter selections; offset snaps to whole frames; Escape cancels.
  - **Copy keys** / Ctrl+C, **Paste at playhead** / Ctrl+V, Delete; collisions are rejected.
  - **Add key**, interpolation selector, **Fit composition**, **Fit keys**; scroll zooms, Shift-scroll pans time.
  - **Bézier** under **Selected outgoing segments** shows yellow tangent handles for acceleration and overshoot.
  - Group operations across up to 64 parameters; clipboard up to 4096 keys; every operation one undo step.
  - Resizable lower workspace.
- MCP: `set_key`, `remove_key`, `set_parameter` with full curves including Bézier segments.

## Masks and vector paths

- **Path mask**: closed vector contours of line, quadratic and cubic Bézier segments; Non-zero or Even-odd fill rule; antialiased white alpha.
- Pen tool in the viewer: click for corners, drag for curves, close on the first point to commit.
- Pen commits as a replacement shape or an additional contour; Escape cancels; Backspace/Ctrl+Z removes the latest unfinished point.
- Edit points and Bézier control handles directly in the viewer with live GPU draft preview.
- Right-click an anchor: insert a point after it (splits the segment without changing the curve), make smooth, make corner, delete (minimum three anchors).
- Precision coordinates, fill rule, segments and contours in the inspector.
- **Transform mask** mode: drag inside bounds to move all contours, corners to scale, sides to stretch, round handle to rotate about the centre.
- **Keep proportions** for corner scaling; Shift snaps rotation to 15 degrees.
- **Animate shape**: keyframe point positions directly; linear interpolation between matching shapes.
- Shape key diamond add/remove, previous/next navigation, keys shown on the timeline.
- Optional onion outlines: previous shape in blue, next in violet.
- Disabling shape animation freezes the current shape (undoable); topology changes and pen redraw are disabled while animated.
- Limits: 4096 commands, ±32768 coordinates, 256 shape keys, 65,536 keyed commands.
- **Ellipse mask**: centre, X/Y radii, radial feather (all animatable).
- **Grow/shrink mask**: signed radius −256 to +256 px (dilate/erode), square neighbourhood.
- **Combine masks** modes: Union, Intersect, Subtract (directional), Exclude overlap; soft-alpha.
- **Channel mask** channels: Luminance (Rec.709), Red, Green, Blue, Alpha.
- **Apply mask**: multiply an image by mask alpha (mask RGB ignored).
- Feathering by following a mask with **Blur**.
- **Place behind object** (text, Surface blend or Planar warp): pick a source layer, outline the foreground object, and Veylvik adds path, grow/shrink, blur, masked foreground and merge in one undoable step.
- Outline inspector for occlusion: edge expansion, softness, **View result**, **Redraw over source**.
- Occlusion outlines can be animated with the path animation tools; no automatic object detection.
- Animate a whole mask with an Affine transform node.

## Text and titles

- **Text** node: editable GPU vector text from bundled or embedded fonts; white alpha source for any material or colour.
- **Edit scene text…** finds titles even under material/effect groups.
- Double-click a title or **Edit text** for an in-viewer editor with caret, selection, clipboard and typing undo.
- Inspector tabs: **Text / Layout / Transform / Style / Animation**; sliders with numeric entry and per-control **Reset**; **Keys** opens animation controls.
- Editable: wording, font, alignment (left/center/right), colour, underline, wrapping, tracking, line spacing.
- Keyframable: size, position, rotation, horizontal/vertical scale.
- Mirror and flip buttons that preserve keys.
- Layout: box width/height, padding, top/middle/bottom alignment, shrink-to-fit, overflow report, **Layout box** in the viewer.
- Transform: nine-point anchor, linked scales, centre-in-frame.
- Language and explicit right-to-left direction (per line); OpenType feature tags in **Font options**.
- Selected character ranges: relative size and font overrides (bundled Sans styles); ranges follow edits.
- Style fills: Colour, Gradient, Material, Image/animation; **Use colour fill** / **Use gradient fill**; **Plain colour** bypasses a material.
- **Add material or effect…**, **Browse effects…**; changing material reuses its node and keeps animation.
- Attached effects can be disabled, reordered, removed and adjusted inline.
- Quick actions: outline, shadow, glow, shine, bevel relief.
- **Fill text with an image or effect**: clip another branch (still, animated procedural or material) to text alpha.
- **Saved text styles** in `$XDG_DATA_HOME/veylvik/title-styles` (typography and procedural fills/effects; image assets rejected).
- Text animation: none, typewriter, rise, scale, spin; by character, word, line or whole title; entrance/exit timing, stagger, easing; renders headless too.
- Font picker: **Bundled**, **System**, **Added** fonts by family, style selector, preview, favourites, recent fonts.
- Bundled fonts: Liberation Sans regular/bold/italic/bold-italic, Hack regular.
- System fonts via Fontconfig in the background; **Refresh fonts**.
- **Add font…** copies TTF/OTF/TTC into `$XDG_DATA_HOME/veylvik/fonts`; collections expose faces.
- **Use file once…** embeds a font in the current project only; selected system/added fonts are embedded (up to 8 MiB).
- Bitmap-only fonts rejected; named variable instances excluded.
- **Text outlines…** / CLI `outline-text` / MCP `outline_text`: convert text to an editable Path mask (alignment, line height, RTL, language, font index).
- Not implemented: mixed-direction paragraphs, automatic font fallback, variable font axes, independently coloured runs.

## Generators

- **Solid**: straight scene-linear RGBA (HDR allowed).
- **Noise**: deterministic cell noise; seed, scale 1–8192 px.
- **Fractal noise**: seed, scale, octaves 1–8, roughness, evolution (keyframe for animation).
- **Gradient**: Linear or Radial; animated start/end coordinates.
- **Ellipse mask**, **Path mask**, **Text** (see above).
- **Procedural fire**, **Smoke, fog and clouds** (see Atmosphere).
- **Image** and **Footage** sources (see Footage).

## Keying

- **Chroma key**: colour-difference key for green, blue or any coloured backdrop; brightness-independent, so unevenly lit, shadowed and HDR parts of a screen key alike.
- **Key colour** with one-click **Green screen** / **Blue screen** presets; the screen need not match exactly.
- **Pick** eyedropper on colour parameters (Key colour, Solid, Colorize): click to sample a 5 × 5 area or drag to average a rectangle, exact scene-linear HDR values read back off the UI thread; samples the node's input, so a chroma key picks the screen, not its result; Esc cancels.
- **Tolerance** (how much of the screen goes) and **Softness** (edge falloff), keyframable.
- Skin, greys and colours without the key's bias stay opaque; near-black pixels are kept.
- **Despill**: limits the key colour in what remains to the average of the other channels, removing fringes and bounce light.
- **Luma key**: **Low**/**High** luminance thresholds with a linear ramp; swap them to key out bright areas (white backdrops); HDR values allowed.
- Both keep input alpha and output premultiplied colour; inspect the matte with **Show alpha**; refine with Grow/shrink mask, Blur and Combine masks (garbage mattes).
- **Refine matte**: cuts a subject out with a coarse matte (such as a Subject matte) and snaps the edge to the image: opacity re-estimated near the edge from local subject and background colours, recovering fine detail and correcting soft or misplaced edges; reliability checks keep patterned subjects and background grain clean; the confident interior stays opaque.
- **Decontaminate** in Refine matte: blur-fusion foreground colour estimation removes the old background's colour from semi-transparent edges.
- **Radius**, **Detail** and **Decontaminate**, keyframable; all live on the GPU, so changing them needs no new analysis.
- **Add → Keying** submenu.

## Colour

- **Exposure**: −32 to +32 stops.
- **Colour correct**: Temperature, Tint, Lift, Gain, Contrast (around 18 % grey), Gamma, Saturation; fixed order; alpha kept; no clamping; every control keyframable; identity at defaults is skipped.
- **Colorize**: map luminance to Shadows/Highlights (low/high) RGBA colours, HDR or transparent.
- Duotone via the **Duotone Print** starter.
- Viewer colour picker never rewrites HDR values.

## Filters and distortion

- **Blur**: box blur radius 0–256 px, two-pass GPU.
- **Transform**: X/Y translation, bilinear.
- **Affine transform**: X/Y translate, signed X/Y scale (flip), clockwise rotation, pivot.
- **Displace**: map red/green displaces X/Y, animated strength up to ±8192 px, alpha-weighted.
- **Grow/shrink mask**, **Channel mask** as filters on alpha.

## Compositing and blending

- **Merge**: premultiplied foreground over background.
- **Blend** modes: Normal, Add, Multiply, Screen, Darken, Lighten, Difference; keyframable opacity.
- **Apply mask**.
- **Fade**: **Start**, **End** or **Until the end**, **Fade in**, **Fade out** (linear ramps), keyframable **Opacity**; edited in frames, stored as exact rationals.
- A layer faded fully out is neither rendered nor decoded.
- **Surface blend** (background = clean surface, foreground = layer): Opacity, Surface lighting, Surface texture, Wear / cracks, Dark-recess threshold, Surface distortion, Match softness, Match grain; all keyframable.
- Surface blend starting looks: **Painted**, **Weathered paint**, **Soft print**, **Clean decal**, Neutral.
- **Compare clean surface** (viewer-only); **Blend onto tracked surface** upgrades a simple merge in place.
- **Use as background** / **Use as foreground** for materials, with **Background placement · advanced**.
- All blending in scene-linear Rec.709 premultiplied, no RGB clamping.

## Materials and surfaces

- **Text surface** (`material`) kinds: gold, silver, aluminium, bronze, phosphor, wood, light oak, dark oak, marble, light brick, red brick, concrete, paper, aged paper, red/brown/tan/dark/black leather (19).
- Text surface controls: scale, bevel, roughness, lighting, grain, emission.
- **Tarnish and dirt** (`patina`): tarnish, verdigris or rust; amount, dirt, scale, wear, scratches, seed.
- **Glass surface**: clear or frosted; bevel, refraction, roughness, IOR, transmission, tint, dispersion; separate background input.
- **Surface reflection**: metal or glass; bevel, curvature, roughness, strength, rotation, scale, X/Y offset; separate reflection input (any image or animated branch).
- **Sun glints**: seed, size, intensity, threshold, evolution, density, angle.
- **Metallic shine**: alpha-preserving specular sweep; X/Y, width, angle, intensity.
- Image-based 2.5D shading, not ray tracing or 3D meshes.

## Atmosphere, fire and particles

- **Smoke, fog and clouds** styles: smoke, soot, fog, drifting fog, clouds; seed, position, size, evolution, density, detail, lighting.
- **Procedural fire**: seed, position, width/height, evolution, intensity, turbulence; transparent background; repeats every four units.
- Particle starters (Dust Motes, Fireflies, Embers, Snowfall, Starfield): 18 editable particles each from ellipse/animation/colour/blend nodes, staggered four-second lifetimes, seekable; controls for colour, glow, amount.
- Deterministic approximations; no fluid or particle simulation, no general emitter yet.

## Starter effects library (59 effects)

Opened with **Add → Effect library…**; searchable by name, description or category, with category counts. Cards play short examples; **Before / After**, pause and scrubbing; **Try example**; **Apply to my composition** with named input binding; **Connect into selected layer** option. Sizing: **Fit composition** (default), **Fill composition**, **Authored pixels**.

### Light (10)
- **Sun Glints**: tiny animated sun-mirror flashes on bright polished details.
- **Metallic Shine**: crisp diagonal reflection sweep over isolated text or logos.
- **Prismatic Glow**: cyan and rose light spill with a subtle chromatic offset.
- **Neon Outline**: luminance edges become luminous cyan contours with a halo.
- **Soft Bloom**: broad luminous bloom around bright details.
- **Anamorphic Streaks**: long horizontal light streaks from bright details.
- **Dream Focus**: soft screened diffusion retaining original detail.
- **Light Sweep**: moving shaft of light in a four-second loop.
- **Soft Vignette**: feathered elliptical shading toward the centre.
- **Spotlight**: soft pool of light travelling around the composition.

### Surface (8)
- **Paint on Surface**: blend a layer into a real surface with lighting, texture, wear, distortion and grain.
- **Edge Engrave**: directional luminance edges etched into a dark cyan drawing.
- **Duotone Print**: luminance mapped to an ink-and-paper palette with adjustable mix.
- **Silver Relief**: directional edge contrast as metallic relief.
- **Ink Bleed**: luminance spreads into a soft ink silhouette.
- **Film Grain**: fine animated procedural grain.
- **Scanlines**: fine horizontal lines for a screen texture.
- **Hologram**: cyan remapping, scanning lines and a drifting ghost image.

### Distortion (5)
- **Liquid Shimmer**: animated flowing distortion (four-second loop, protected edges).
- **Frosted Glass**: fine refraction and diffusion like textured glass.
- **Heat Haze**: tall flowing refraction with protected edges.
- **Underwater**: slow broad refraction and blue-green tint.
- **Glitch Slices**: horizontal slices shifting sideways in a repeatable glitch.

### Motion (3)
- **Chromatic Echo**: opposing cyan and rose echoes drift around the source.
- **Radial Echo**: expanding copies form a graphic tunnel.
- **Orbit Echo**: two tinted ghost images orbit the source.

### Particles (5)
- **Dust Motes**: soft floating dust with staggered fades.
- **Fireflies**: warm glowing points wandering and pulsing.
- **Embers**: orange sparks rising and fading.
- **Snowfall**: layered white flakes falling and drifting.
- **Starfield**: twinkling points in a looping star flight.

### Materials (23)
- **Polished Gold**, **Polished Silver**, **Brushed Aluminium**, **Aged Bronze**, **Phosphor** (emissive, with bloom), **Carved Wood**.
- **Light Oak**, **Dark Oak**, **White Marble**, **Light Brick**, **Red Brick**, **Rough Concrete**, **Clean Paper**, **Weathered Paper**.
- **Red Leather**, **Brown Leather**, **Tan Leather**, **Dark Leather**, **Black Leather**.
- **Surface Reflection**: reflect clouds, another object or an image in metal.
- **Clear Glass Material**: bevelled glass with refraction, dispersion and a reflection input.
- **Frosted Glass Material**: diffused glass with softened background and reflected light.
- **Metal Patina**: uneven tarnish, dirt, scratches; verdigris or rust variants inside.

### Atmosphere (5)
- **Light Smoke**, **Dense Dark Smoke**, **Fog Layer**, **Drifting Fog**, **Clouds**.

### Example scenes (Try example)
- Every starter opens a complete editable four-second scene.
- Showcases: **After hours** (Neon Outline), **Prism / identity** (Prismatic Glow), **Pink melt** (Liquid Shimmer), **Gilded inferno** (Embers, gold type, fire, floor reflection), **Polished gold** (Metallic Shine), **Toxic nectar** (Fireflies), **Sands of time** (Dust Motes), **Deep freeze** (Snowfall), **Nova ignition** (Starfield).
- Detailed examples render at 1920 × 1080; untouched examples do not mark the project modified.

## Effect groups and presets (.veyfx)

- **Group…** / **Edit → Create effect group…** collapses nodes feeding the selected output; uncheck sources to expose them as inputs.
- **Edit effect…**: rename, expose scalar/colour/seed parameters, name controls and inputs, capture defaults, optional scalar ranges.
- Collapsed group inspector edits exposed controls (drafts, keyframes, colours); **Reset to default**; **Show in animation**.
- **Open group / Close group**, **Duplicate effect** (Ctrl+D), **Ungroup** (pixels unchanged).
- Groups have one output, cannot overlap or nest; add no render passes.
- **Save to library** captures values, animation, defaults and wiring into `$XDG_DATA_HOME/veylvik/effects`.
- **Export preset…** writes a portable `.veyfx`; **Import preset…** validates and copies one into the library.
- Presets include procedural nodes and text outlines; image files must be named inputs (not bundled).
- Limits: 16 MiB per preset; library scans 256 files / 64 MiB; invalid entries reported.
- MCP `get_preset` (starter ID or group UUID) and `insert_preset` (input bindings, `sizing` fit/fill/original, position, dry run).
- MCP `apply` group commands: `put_group`, `ungroup`, `set_control`, `connect_group`.

## Footage and media

- **File → Import media…**: MP4, MOV, M4V, MKV, WebM, AVI, MPEG video (FFmpeg required), PNG, OpenEXR; first video stream imported.
- **File → Import numbered sequence…**: pick the first PNG/EXR frame; consecutive frames discovered; `.veyseq` manifest written beside it.
- Import also accepts `.veyseq` files and completed Veylvik `sequence.json` exports.
- **File → New from media…**: composition matched to source size and (for video/sequences) frame rate and duration.
- First mismatched import into an empty composition offers **Match composition** (optional frame rate and duration) or **Keep settings**.
- **Match composition to this media…** in the inspector or context menu, with optional proportional scaling of scene controls.
- **Footage** node timing: **Timeline start**, **Source in**, **Use source end** / exclusive **Source out**; frame fields with reset buttons.
- Placement modes: **Fit**, **Fill**, **Original size**, **Stretch**, **Native pixels (top-left)** (legacy).
- Transparent before start and after trimmed end; multiple Footage nodes can share a source with independent timing.
- Replacing a still with a video promotes it to Footage.
- Hardware decoding: NVIDIA NVDEC, then Intel/AMD VA-API, then software; each candidate is test-decoded; fallback reported in the inspector.
- Hardware failure during playback retries the frame through software.
- NVIDIA direct CUDA-to-Vulkan GPU copy (matching device UUIDs); otherwise host-staged planes.
- Supports NV12 and 10-bit P010 4:2:0 on the hardware path.
- Vulkan does chroma reconstruction, range/matrix conversion, rotation, transfer decoding and primaries conversion.
- Colour: explicit conversion to scene-linear Rec.709; SDR and PQ transfer (PQ at 100-nit reference, no tone mapping); incomplete tags reported; documented defaults for untagged sources.
- Variable frame rate and reordered frames handled via packet timestamps.
- Rejected with explicit errors: alpha WebM, HLG, constant-luminance BT.2020, unsupported primaries, anamorphic, interlaced.
- Decode at placed size (never larger than source), area-averaged in linear light; 256 MiB per-frame decode budget.
- Footage outside its range, or behind a zero-opacity Blend/Surface blend or a fully faded Fade, is not decoded.
- Sequences: lazy bounded loading, same-size frames, missing/changed frames reported as errors, endpoints held outside range; max 10,000 frames.
- PNG input: 8/16-bit RGB, RGBA, greyscale, palettes; sRGB or untagged; 64 MiB file / 128 MiB decoded limits.
- OpenEXR input: single-part scanline RGBA half/float, Raw/RLE/ZIP1/ZIP16, Rec.709/D65.
- **Reload source images** (Ctrl+R) in the background; failures retain previous inputs.
- Footage audio is not played in preview yet.

## Point tracking

- **Track motion…** in the source inspector; click a feature in the viewer (gold patch box, blue search box).
- **Track forward** / **Track backward** to an end frame; adjustable search radius.
- Works on image sequences, footage or any animated node output.
- Review the path; choose a target (text, mask or layer) and optional X/Y offsets.
- **Apply tracked movement** inserts a translation node with editable position keys (one undo step).
- **Update target's position keys** replaces keys in the retracked interval only.
- Cancellation keeps accepted samples; at least two samples needed.
- Translation only, integer-pixel precision, fixed reference patch; GPU matching.
- Starting point-tracking analysis is Studio-only.

## Planar tracking

- **Track surface** in the inspector (also from Planar track / Planar warp palette entries).
- Drag four viewer corners around a rigid textured surface on a reference frame.
- End frame or **To start** (backward); **Both directions** tracks from a mid-shot reference to both ends.
- **Tracking settings**: feature radius, search radius, match tolerance, grid 8–16 cells per side (64–256 features).
- **Motion** models: **Automatic** (default), **Position**, **Position, rotation, scale**, **Affine (shear)**, **Perspective**.
- **Refine with area alignment** (default on): GPU photometric alignment with gain/offset.
- **Occlusion matte**: any mask node (e.g. animated pen outline) excludes foreground objects.
- **Re-anchor when matches thin out** (default on; below 24 matches, one retry).
- **Separate tracking area** (amber) distinct from pinned corners (cyan); area can follow the screen for floors/roads/sand.
- Camera-plane clipping: tracking continues as the surface passes beside/behind the camera; warps clip at the camera plane.
- Live viewer follow; green circles (inliers), coral crosses (rejected); feature count and reprojection error; **Show feature matches**.
- Timeline quality strip: green, amber, red (lost), grey (no sample).
- **Save reusable track** (Planar track node) or **Create tracked layer**.
- **Correct corners / retrack section**, **Cancel correction**, **Update reusable track** (replaces only analysed frames).
- **Planar warp** modes: **Follow surface**, **Fit image to surface**, **Stabilize surface**; any number of warps per track.
- **Match surface appearance** creates a Surface blend with a starting look.
- Up to three GPU pyramid levels plus full-resolution refinement; up to 4096 frames.
- Headless: CLI `track` and MCP `track_surface` with motion, refine, back_to, occlusion, region, area_follows, radii, tolerance, reanchor.

## Subject segmentation

Details: [Subject segmentation](subject-segmentation.md).

- **Select subject…** (Inspector or **Tools**): left-click the subject in the viewer, right-click or Alt-click to exclude; instant blue mask preview per click; **Undo point**, **Clear**.
- **Track subject** with progress and **Stop** (keeps tracked frames); optional **Also track back to the start**; one undo step adds Subject matte and Subject cutout.
- **Detail pass for small parts** (default on, `--no-detail` in the CLI): a second, zoomed tracking pass around the subject at one constant scale, about 2.5 times the detail on a full-length portrait shot; completes feet, toes and fingers; skipped automatically when the subject fills the frame.
- One-click results: **Keep subject**, **Replace background** (new background colour, rewirable), **Put layers behind** (a title between shot and subject).
- **Tools → AI models…**: every local model with size, licence and status; one-click **Install** with progress and **Cancel**, **Uninstall**, **Enabled**; info button with hover summary and click-through pros, cons, speed, graphics memory and model page.
- Click-prompted subject mattes from a local AI model (EdgeTAM, Apache-2.0), tracked through the shot; about 30 ms per frame on an RTX 4080.
- Points that mark or exclude areas (1–32); forward, backward or both directions from the prompted frame; up to 4096 frames.
- Analyses the rendered source node at composition size, so placement, trims and timing are included.
- Writes a 16-bit matte sequence (`.veyseq`) beside the project and adds a **Subject matte** Footage node and a ready **Subject cutout** (Refine matte) wired to the source.
- Smooth edges: model logits are resampled before the sigmoid, never upsampled as a binary mask.
- Model manager: explicit install with pinned revision, size and SHA-256; uninstall; enable and disable; each model lists its uses, pros, cons, speed, memory and licence.
- Runs in an isolated worker process on CUDA, with no silent CPU fallback.

## Export

- **Export current preview…** (Ctrl+E): PNG (SDR sRGB, straight alpha) or OpenEXR (32-bit float RGBA, linear Rec.709, premultiplied, ZIP16).
- EXR keeps HDR, negatives and emission at zero alpha.
- **Export image sequence…** (Ctrl+Shift+E): inclusive frame range, new folder, PNG or EXR, background progress, **Cancel export**.
- Sequences: `frame-000000.png|exr`, `sequence.json` manifest with timing, region, format, colour space, alpha and status (`rendering`/`complete`/`cancelled`); 1–10,000 frames; negative frame indices allowed.
- **Export video…**: MP4 with H.264 or HEVC 10-bit, SDR Rec.709, no alpha.
- Size presets or even custom dimensions (2–8192), fitted and centred with a background colour; custom frame rate and range; up to 10,000 frames.
- Quality: draft, high (default), master.
- Encoders: NVIDIA NVENC first, then Intel/AMD VA-API; software only with **Allow software encoding if hardware is unavailable**.
- NVIDIA direct Vulkan-to-CUDA-to-NVENC GPU path; RAM-staged fallback reported; experimental DMA-BUF path for AMD/Intel (unverified).
- Audio: AAC 48 kHz stereo 192 kbit/s; sound of every Footage node feeding the output, at timeline position and trims.
- **Include footage audio** toggle; **Music** file with level −60 to +12 dB.
- Cancel discards the partial file; existing files are never replaced; published atomically.
- Crop (`--crop` / `region`) and preview-node (`--node`) rendering in CLI/MCP.

## Project safety

- Atomic saves that detect changed or deleted files on disk before replacing.
- Save/export refuse to overwrite input images.
- Undo/redo: 64 snapshots, 128 MiB; every transaction atomic.
- Crash recovery copies of applied edits every ten seconds in `$XDG_STATE_HOME/veylvik/recovery`; **File → Recover unsaved work…**.
- Unsaved-changes prompts on close, open and new.
- Missing media: projects still open, unavailable sources marked in the menu bar and inspector, repair via Replace source / Reload.
- **Collect project** (`veylvik collect`): copies images and supported video into `assets/` with relative paths (sequences not supported).
- 16 MiB document size limit.
- **File → New composition…** (Ctrl+N): width, height, frame count, exact frame-rate numerator/denominator.
- **File → Composition settings…**: canvas, frame count, frame rate as one undo step, optional control scaling.

## Automation

### CLI (`veylvik`)
- `open [FILE] [--control-dir DIRECTORY | --no-control]`: start Studio.
- `demo FILE [--motion|--procedural]`: write an example project.
- `import IMAGE.png|IMAGE.exr FILE`: create a project from an image.
- `outline-text FONT TEXT NEW_FILE [--size --origin --canvas --align --line-height --rtl --language --font-index]`.
- `collect FILE NEW_DIRECTORY`.
- `nodes`: node catalogue as JSON.
- `check FILE`: validate graph and assets, report working/source bytes.
- `render FILE OUTPUT.png|.exr [TICKS RATE] [--node UUID] [--crop X Y W H]`.
- `render-sequence FILE DIRECTORY START END [--format png|exr] [--node] [--crop]`.
- `render-video FILE OUTPUT.mp4 [--codec h264|hevc10] [--quality] [--size W H] [--fps N D] [--range A B] [--allow-software] [--no-audio] [--audio FILE] [--audio-gain DB]`.
- `track FILE NEW_FILE SOURCE_UUID START END "corners" ["area"] [--screen-area] [--motion ...] [--no-refine] [--back-to FRAME]`.
- `segment FILE NEW_FILE SOURCE_UUID FRAME END "x,y x,y,- ..." [--back-to FRAME] [--model ID] [--no-detail]`: subject matte sequence and node.
- `models [install|uninstall|enable|disable ID]`: AI model catalogue and installer.
- `mcp FILE` (headless) and `mcp --studio [SOCKET|DIRECTORY]` (live relay).
- JSON on stdout, JSON errors on stderr, non-zero exit on failure.

### Headless MCP tools
- `inspect`, `graph`, `get_node`, `nodes`, `diagnostics`, `outline_text`, `get_preset`, `insert_preset`, `apply`, `add_effect`, `duplicate_branch`, `track_surface`, `undo`, `redo`, `save`, `render`, `render_sequence`, `render_video`, `preview_image`.
- `apply` commands: `put`, `set_parameter`, `set_key`, `remove_key`, `remove`, `output`, `resize`, `frame_count`, `frame_rate`, `place`, `put_group`, `ungroup`, `set_control`, `connect_group`; atomic batches up to 4096, dry run, one undo step.
- Every mutation, save or render requires the current revision.

### Live MCP tools (open Studio)
- `inspect`, `graph`, `get_node`, `nodes`, `diagnostics`, `apply`, `duplicate_branch`, `add_effect`, `get_preset`, `insert_preset`, `undo`, `redo`, `preview`, `preview_image`, `new_project`, `open_project`, `new_from_media`, `import_media`.
- Edits appear in the editor and share its undo history; session UUID plus revision guard every edit.
- `new_project` (width, height, frame rate, frame count), `open_project`, `new_from_media` refuse unsaved changes unless `discard_changes`.
- `import_media` (video, PNG, EXR, `sequence: true`, `match_composition`).
- `preview` pauses and seeks; `preview_image` captures the completed frame as inline PNG.
- Live tools do not save or export files.

### Control socket
- `$XDG_RUNTIME_DIR/veylvik/control.sock` by default (`VEYLVIK_CONTROL_SOCKET` overrides); directory 0700, socket 0600.
- Stale sockets replaced; a second editor uses `control-<PID>.sock`; **MCP** tooltip in the menu bar shows the path.
- Relay answers `initialize` and `tools/list` itself, connects per call, picks up restarted editors; five-second busy timeout.
- 4 MiB message bound; MCP protocol 2025-11-25 over newline-delimited JSON-RPC.

## Performance and architecture

- GPU-only rendering on Vulkan; no CPU fallback.
- Scene-linear Rec.709 premultiplied RGBA32F, HDR and negatives preserved.
- Graph compiled into an immutable plan; unreachable nodes pruned; identity nodes aliased; identical operations shared.
- Bounded memory: 2048 MiB default GPU budget (`VEYLVIK_GPU_BUDGET_MIB`, 64–16384), 128 MiB idle pool, 64 MiB / 16-frame output cache, 32 MiB vector cache.
- Vector masks via Vello Hybrid geometry and Vulkan coverage.
- Preview scheduler on a worker; no blocking IO on the UI thread.
- **Render statistics** / MCP `diagnostics` report buffers, caches and reuse.
- **Help → About Veylvik…** with build, GPU, driver, RAM and **Copy details**.

## Platform and packaging

- Linux, Wayland and X11; headless commands need no display.
- Primary target x86_64 Arch/Omarchy; aarch64 supported by the package recipe.
- `bin/package` builds an Arch package: `veylvik`, `veylvik-studio`, launcher entry, SVG icon, **Open with Veylvik** for `.veylvik`.
- Desktop portal file dialogs; follows the Omarchy accent colour.
- Rust 1.98.1 pinned; `unsafe` forbidden; GPL-3.0-only.
