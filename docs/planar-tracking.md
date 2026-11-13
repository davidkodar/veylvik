# Planar tracking

Select the footage or image branch containing the surface, then choose **Track
surface** in the Inspector. The node palette's Planar track and Planar warp
entries also open this workflow.

1. Pause on a clear reference frame. Drag the four viewer handles around one
   rigid, textured surface. Keep moving foreground objects outside the region
   where possible.
2. Choose the end frame, or **To start** to track backward. **Tracking settings**
   exposes feature radius, search radius and match tolerance in source pixels.
3. Choose **Track surface**, then scrub the accepted frames. Green circles show
   matches supporting the plane; coral crosses show rejected matches. The window
   reports accepted feature count and reprojection error. These diagnostics are
   saved with the track; **Show feature matches** hides the markers.
4. **Save reusable track** keeps the analysis without changing the composition
   output. Alternatively choose a layer and **Create tracked layer**.

Tracking alone does not edit the project. Saving, updating or attaching is one
undoable transaction. Cancellation and loss of tracking retain the accepted
frames for explicit saving; they never silently extend a failed solve.

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
fixed, with a second matching pass refining each frame's perspective prediction.

This is a rigid planar tracker, not a deforming mesh or an automatic occlusion
matte. Large frame-to-frame changes, strong blur, repetitive textures, changing
reflections and heavy occlusion can lose the track. Reframe the region or start
from a clearer frame. Source edits require retracking; saved motion is not
recomputed automatically. No optical-flow pyramid or lens calibration is applied.

The range is limited to 4096 frames and may stop sooner to preserve the project's
16 MiB size budget with diagnostics. Outside the accepted range, the warp holds
the nearest accepted surface; between samples it interpolates the corners.

Dirt, lighting, foreground occlusion, displacement, blur and grain are separate
compositing operations. Rejected feature matches diagnose tracking; they are not
an occlusion mask and do not automatically transfer cracks or wear to a layer.

Both operations, their data schema, and all three warp modes are exposed through
the node catalog and existing MCP editing commands. Analysis currently starts in
the Studio workflow; there is no dedicated headless tracking tool yet.
