# Planar tracking

Select the footage or image branch containing the surface, then choose **Track
surface** in the Inspector. The node palette's Planar track and Planar warp
entries also open this workflow.

1. Pause on a clear reference frame. Drag the four viewer handles around one
   rigid, textured surface. Keep moving foreground objects outside the region
   where possible.
2. Choose the end frame, or **To start** to track backward. **Tracking settings**
   exposes feature radius and search radius in each pyramid level’s pixels, and
   match tolerance in full-resolution source pixels.
3. Choose **Track surface**. The viewer follows completed frames live; after
   analysis, scrub the accepted frames. Green circles show
   matches supporting the plane; coral crosses show rejected matches. The window
   reports accepted feature count and reprojection error. These diagnostics are
   saved with the track; **Show feature matches** hides the markers.
4. **Save reusable track** keeps the analysis without changing the composition
   output. Alternatively choose a layer and **Create tracked layer**.

Tracking alone does not edit the project. Saving, updating or attaching is one
undoable transaction. Cancellation and loss of tracking retain the accepted
frames for explicit saving; they never silently extend a failed solve.

## Correcting a section

Reopen a saved track, seek to a clear frame and choose **Correct corners / retrack
section**. Drag its four corners to the intended surface, choose an endpoint,
then track forward or backward. Return to the correction frame if you scrub away
before starting. **Cancel correction** restores the previous analysis.

**Update reusable track** replaces only successfully analyzed frames. Frames
outside the section, including frames beyond a failed or cancelled solve, retain
their previous samples. The original reference frame and shared track node stay
unchanged, so existing Follow, Fit and Stabilize consumers remain connected.
Updating is one undo step. Inspect the joins between old and new sections: a
manual correction can introduce a visible jump there.

While the tracking window is open, the timeline shows a clickable quality strip:
green for strong matches, amber for fewer than 12 accepted features, under two
thirds acceptance, or error above 1.5 source pixels; red marks where the current
analysis lost the track. Grey means no sample. These are diagnostic hints, not a
guarantee of accuracy. The red stop marker belongs to the current analysis;
saved samples retain their feature and error diagnostics.

## Reusing motion

A **Planar track** passes its source image through unchanged and holds normalized
surface coordinates. Connect it to the `track` input of any number of **Planar
warp** nodes. Their `image` inputs can be text, masks, images, footage or effect
branches. Reopen a saved track (or its warp) with **Track surface** to attach
another layer or rerun analysis. Updating the track updates its consumers.

- **Follow surface** moves a layer relative to its placement on the reference
  frame. Position the layer there before attaching it.
- **Fit image to surface** maps the full composition-sized layer canvas into the
  four corners. Transparent margins are part of that canvas.
- **Stabilize surface** applies inverse motion to the tracked source. Transparent
  borders are expected where the original frame provides no pixels.

Creating a Follow or Fit layer also merges it over the tracking source and makes
that merge the composition output. Existing branches remain intact. Stabilize
makes the stabilized source the output. The underlying nodes remain editable.

## Processing and limits

Feature detection, perspective-aware patch correlation and image warping use
Vulkan compute. Only feature coordinates and correlation scores return to the
CPU for deterministic robust homography fitting; source pixels stay on the GPU.
Coordinates are resolution-independent. Correlation thresholds reject flat or
ambiguous patches; consensus rejects inconsistent motion. The detector distributes
up to 64 features across the chosen region and requires at least six reliable
matches. Tracking stops if no dominant plane remains. The reference texture is
fixed. Up to three area-filtered GPU pyramid levels search for larger motion,
then full-resolution matching and a second refinement solve the final perspective.
Only a successful full-resolution solve is accepted.

This is a rigid planar tracker, not a deforming mesh or an automatic occlusion
matte. Large frame-to-frame changes, strong blur, repetitive textures, changing
reflections and heavy occlusion can lose the track. Reframe the region or start
from a clearer frame. Source edits require retracking; saved motion is not
recomputed automatically. No lens calibration or nonrigid optical flow is applied.

The range is limited to 4096 frames and may stop sooner to preserve the project's
16 MiB size budget with diagnostics. Outside the accepted range, the warp holds
the nearest accepted surface; between samples it interpolates the corners.

The **Match surface appearance** option creates a Surface blend node for
lighting, texture, wear, displacement, softness and grain; see
[surface blending](surface-blending.md). Foreground occlusion remains an editable
outline workflow. Rejected feature matches diagnose tracking; they are not
an occlusion mask and do not automatically transfer cracks or wear to a layer.

## Motion models

**Motion** in Tracking settings limits how the surface may move between frames:
**Position**, **Position, rotation, scale**, **Affine (shear)**, **Perspective**, or
**Automatic** (the default). Simpler models need fewer matches (two to five accepted
instead of six) and cannot invent perspective the evidence does not support, which
keeps distant or off-screen corners steady on small or low-texture areas. Rotation and
scale are fitted in pixels, so portrait and landscape frames behave alike.
**Automatic** fits both perspective and affine motion on every frame and keeps the
affine result unless perspective explains clearly more matches or clearly smaller
errors; when too few matches remain for either, it falls back to similarity and then
translation. Choose **Perspective** to force full perspective on surfaces that turn
strongly towards or away from the camera. Headless tools take `motion` with the values
`auto`, `perspective`, `affine`, `similarity` and `translation`.

A track still ends when the pinned corners pass behind the camera, for example the near
end of a wall the camera walks past. That is the geometry of the shot, not a tracker
failure; keep the corners on the part of the surface that stays in front of the camera.

## Area refinement

Feature matches give a first estimate of the surface's motion on each frame. With
**Refine with area alignment** (on by default), the GPU then aligns every sampled
pixel of the tracking area directly against the reference frame: a damped
Gauss-Newton fit of the plane motion together with a brightness gain and offset,
robust to outliers and skipping pixels under the occlusion matte. Only small
normal-equation sums return to the CPU. The refined motion is kept only when it
lowers the photometric error clearly and stays within three match tolerances of the
feature solution; otherwise the feature result stands. Refinement applies to the
perspective and affine models. It costs roughly a tenth more analysis time and is
most visible on blurred, low-contrast or finely textured surfaces. Headless tools
take `refine: false` (or `--no-refine`) to compare.

## Occlusion

People, cars or hands crossing the surface pull matches away from the plane. Choose
an **Occlusion matte** in Tracking settings: any mask node whose alpha covers the
foreground object, such as an outline drawn with the pen and animated with the path
tools. The matte is rendered for every analyzed frame on the GPU; detection skips
patches touching it on the reference frame and matches whose patch lands on it are
rejected before the plane is fitted, so the surface stays locked behind the object.
Headless tools take the matte node as `occlusion`.

## Re-anchoring

The reference texture is fixed per anchor. On walking, zooming or turning shots the
surface changes scale and angle until too few patches match. With **Re-anchor when
matches thin out** (on by default in Tracking settings), the tracker re-detects
features on the latest accepted frame once fewer than 24 matches are accepted, and
continues from that frame. If a frame still fails, it retries once from a fresh
anchor on the previous accepted frame before stopping. Corners may leave the image;
detection then uses the visible part of the surface. Each anchor can add slight
drift, so inspect long tracks; turn the option off for the original single-reference
behaviour. Tracking still stops when the surface crosses behind the camera or too
little of it remains in view. The completion message reports how many times the
track re-anchored.

## Tracking area

By default the four corners are also the area analyzed. Enable **Separate tracking
area** to analyze a larger textured area of the same plane (amber handles) than the
corners the layer is pinned to (cyan). Corners can then sit on blank, small or partly
hidden parts of the surface, and re-anchoring detects inside whatever part of the
area remains visible. Headless tools accept the same area as `region`.

For floors, roads, sand and other planes that keep filling the same part of the frame
while the camera moves, set the area to follow the screen (`area_follows: "screen"`,
or `--screen-area` on the command line): every re-anchor then detects fresh texture in
that fixed part of the frame instead of in the area carried along with the surface.
Corners far outside the frame are extrapolated from the visible plane and can still
end the track when they cross the projective horizon.

## Headless tracking

The same analysis runs without Studio. From the command line:

```sh
veylvik track scene.veylvik tracked.veylvik SOURCE_UUID 160 300 "0.08,0.64 0.92,0.64 0.98,0.90 0.02,0.90"
```

`SOURCE_UUID` is the footage or image branch to analyze; frames are composition
frames, the first being the reference (an end before the start tracks backward); the
four corners are 0..1 composition coordinates in perimeter order on the reference
frame. The result is written to a new project with one added Planar track node, and a
JSON report gives the tracked range, why it stopped, re-anchors and worst error.

The headless MCP server exposes `track_surface` with the same inputs (`source`,
`surface`, `start`, `end`) plus optional `motion`, `refine`, `occlusion`, `region`, `area_follows`,
`feature_radius`, `search_radius`, `tolerance` and `reanchor`. It adds the Planar track node as one undoable
transaction; connect it to `planar_warp` nodes with `apply`. Both operations, their
data schema, and all three warp modes are also exposed through the node catalog and
the existing MCP editing commands.
