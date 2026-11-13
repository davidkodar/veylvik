# Image sequences and point tracking

## Import footage

Choose **File > Import numbered sequence…** and select the first PNG or OpenEXR
frame (for example `shot0001.png`). Veylvik discovers consecutive numbered files
and uses the composition's frame rate. It writes a small `.veyseq` manifest beside
the first frame; keep that manifest with the frames when moving the shot. Existing
manifests are never overwritten. A missing number ends discovery.

**File > Import image or sequence…** also accepts `.veyseq` files and completed
Veylvik `sequence.json` exports. The Image node displays the appropriate source
frame at the current composition time, starting at time zero. Frames outside the
sequence hold the nearest endpoint. Sources retain their native pixel dimensions;
use an affine transform to resize or place footage in a larger composition.

All frames must have the same dimensions. Loading is lazy and bounded; a missing,
changed or invalid frame reports an error rather than displaying another frame
as if it were correct. Reload sources after changing files on disk. Source decoding
runs off the interface thread; compositing and effects remain on Vulkan.

Direct video import is available through **File > Import media…**; see
[Video footage](video-footage.md) for formats, timing controls and decoding limits.
Project collection currently rejects sequence assets; copy the sequence directory
with its manifest manually.

## Track a feature

1. Select the source node and click **Track motion…** in the inspector.
2. Click a distinct feature in the viewer. The gold box is the feature patch;
   the blue box is the search area around the preceding accepted position.
3. Choose an end frame, then **Track forward** or **Track backward**. A larger
   search radius accommodates more movement between frames. Tracking stops when
   the patch is flat, too different, or has an ambiguous match.
4. Review the path. Choose a text, mask or layer as the target and optionally set
   X/Y offsets. **Apply tracked movement** inserts and connects a translation node
   with ordinary editable position keys. It is one undoable transaction.

A source may be an image sequence or any animated node output. Feature matching
uses Vulkan compute and reads back only candidate scores, not full image pixels.
This first tracker follows translation at integer-pixel precision using a fixed
reference patch. It does not estimate rotation, scale, perspective, deforming
silhouettes or hidden features; for surfaces that turn or change scale, use
[planar tracking](planar-tracking.md).

Cancellation keeps accepted samples available for explicit application; it does
not modify the graph. At least two accepted samples are needed. Composition edits
invalidate pending results, preventing an old track from being attached silently.

To correct or continue a track, reposition the feature at the desired frame and
track again. Choose an existing translation transform and enable **Update target's
position keys** to replace keys in that interval while retaining keys outside it.
The target's position at the new reference frame anchors the replacement. Inspect,
edit or delete the resulting X/Y keys using the normal animation tools. Applying
to other node types adds a separate transform, preserving their existing animation.

## Automation and format

Image nodes continue to use the standard `asset` field. A `.veyseq` asset contains:

```json
{
  "schema": 1,
  "frame_rate": { "ticks": 24000, "rate": 1001 },
  "files": ["shot0001.png", "shot0002.png"]
}
```

Paths are relative to the manifest directory and must stay inside it. There are
at most 10,000 frames per source, a 1 MiB manifest limit, and the existing image
size limits apply to each frame. CLI, MCP rendering, Studio preview and sequence
export use the same time-aware source loader. Tracking results are standard
transform keys, available to MCP through normal document inspection and editing;
starting the tracking analysis is currently a Studio action.
