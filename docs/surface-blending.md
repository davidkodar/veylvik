# Blending layers into surfaces

**Surface blend** is a reusable GPU compositor with two inputs:
`background` is the clean surface; `foreground` is the positioned or tracked
text, image, mask or effect layer. It does not require planar tracking.

## Quick start

- In **Track surface**, leave **Match surface appearance** enabled, choose a
  starting look, then **Create tracked layer**. The new surface controls are
  selected automatically. Disable the checkbox for a normal composite.
- For an existing output Planar warp, choose **Blend onto tracked surface** in
  the Inspector. A simple existing merge is upgraded without changing its ID or
  connections. More complex downstream graphs require explicitly connecting a
  Surface blend node so existing effects are not discarded.
- In **Add → Effect library → Surface**, open **Paint on Surface**. Use its
  example to compare clean lettering against weathered paint on a brick wall,
  or apply the effect with separate clean-surface and foreground bindings.
  The example uses authored camera motion shared by the wall and lettering;
  it is not a recorded tracking analysis.

Starting looks are **Painted**, **Weathered paint**, **Soft print**, and **Clean
decal**. They only set editable parameter values. In the node Inspector, choosing
another look or Neutral creates an unapplied draft; Apply commits it as one Undo
step. Neutral restores ordinary source-over compositing. Starting looks replace
parameter animation too, so use individual resets to preserve other controls.

## Controls

| Control | Behavior |
| --- | --- |
| Opacity | Visibility of the inserted layer. Zero returns the background exactly. |
| Surface lighting | Transfers locally averaged surface color and brightness. |
| Surface texture | Transfers finer luminance variation relative to the local average. |
| Wear / cracks | Removes paint coverage in dark local recesses, revealing the clean surface. |
| Dark-recess threshold | Contrast needed before wear starts removing coverage. |
| Surface distortion | Small displacement derived from local surface gradients, in composition pixels. |
| Match softness | Softens the inserted layer, including alpha edges, in composition pixels. |
| Match grain | Adds deterministic time-varying grain to the inserted layer only. |

All eight values support keyframes and individual resets. **Compare clean
surface** changes only the viewer preview, not the composition or Undo history.
Lighting, texture, wear and grain are dimensionless. The texture sampling window
adapts to composition resolution; softness and displacement are explicit pixels
and scale when a preset is inserted with Fit or Fill sizing.

Lighting is an image-based approximation, using twice the local scene-linear RGB
as its illumination estimate, bounded to a factor of four. Texture compares local
luminance against a neighborhood average. These are adjustable compositing aids,
not recovered 3D lighting or automatic material identification. Dark paint,
shadows and cracks can all contribute to wear. Use the threshold and wear amount
to control the interpretation. Grain is synthetic, not measured from footage.

## Objects in front of the layer

Select the Surface blend, Planar warp, or editable text node and choose **Place
behind object**. The tracked source is preselected where available. Draw an
outline around the foreground object in the viewer; closing it creates editable
outline, expansion, softness, source cutout and merge nodes in one undoable step.
The untouched source inside the outline is restored over the composition.

Use the existing path animation controls for a moving outline. This is an explicit
mask workflow: dark-crack wear does not identify foreground objects, and planar
tracking does not automatically segment them. Expand a collapsed library effect
to select its Surface blend node for this workflow.

All image processing runs in Vulkan compute with scene-linear, premultiplied
RGBA. Unaffected pixels remain the clean background; softened edges stay
premultiplied and HDR values are not clipped to SDR. The node, parameter schema,
and portable library preset are also available through existing MCP tools.
