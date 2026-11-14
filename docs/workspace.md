# Editing workspace

The docked **Library** sits on the left, the viewer and node workspace in the
centre, and the Inspector on the right. Resize the panel dividers to suit your
screen. Collapse the Library with its header arrow; restore it through
the persistent Effects, Materials, Templates or Media buttons on the left rail.
The Inspector also has a collapse arrow and a persistent button on the right.
Both panels can be toggled from the View menu.

Use the Library tabs to browse effects, materials, example templates or imported
media. Search filters the current tab. Materials also have category filters.
Selecting a card does not modify the composition. With editable text selected,
**Apply to selected text** changes its material in one undoable transaction.
**Try example** opens the corresponding editable starter scene; **More options**
provides composition inputs and background/foreground placement.

Material cards use bundled GPU-rendered samples of the same surface shader used
in compositions. Curved samples show metals and leather; flat samples show wood,
brick, paper and other repeating surfaces. Sample lighting and geometry differ
from your composition. Effect and template cards play a rendered loop while hovered and return to a
still frame when the pointer leaves. Only the hovered card animates. Open More
options for before/after comparison and playback controls.

The Inspector uses tabs for text, layout, transform, style and animation. Parameter
rows keep values, keyframe navigation and reset controls together. Right-click a
text parameter label for interpolation details. Other scalar rows have a details
menu. Layer tools collects tracking, masking and surface workflows.

Veylvik bundles Inter Regular and Semibold for its interface. Inter 4.1 is from
https://github.com/rsms/inter and is distributed under the SIL Open Font License
1.1, included alongside the font assets and in the installed package's licences.
User text fonts remain independent of the interface font.

Selected text has an outline box in the viewer. Drag its centre cross to move,
its eight square handles to resize, or the round handle above it to rotate.
Resizing keeps the opposite handle fixed. Hold Shift, or enable linked scale in
Transform, to preserve proportions. Rotation gently snaps within three degrees
of horizontal, vertical and diagonal (45-degree) angles. Hold Alt for free
rotation, or Shift for exact 15-degree increments. Double-click the text to edit
its wording.
Layout-box editing remains a separate Text tools option for wrapping paragraphs.

The frame ruler uses labelled ticks that adapt to the available width and
composition duration. Its scope defaults to **Layer**, including connected effects
and group members up to composition merge boundaries. Choose **Selected** for one
node or **All** for every node. Inspector draft keys appear immediately; pause and
finish/apply the edit before retiming. Previous/next key buttons follow this scope.

Click a diamond to select it and seek; drag empty space to scrub. A number beside
a diamond indicates several parameter keys at that frame. Hover to see their
nodes, parameters and values. Drag a diamond to move those keys together, Shift-click
to select several frames, or Alt-drag to duplicate. Delete removes selected keys
only while the ruler has keyboard focus. Right-click to duplicate at the playhead,
delete, or open a node in Animation. Retime gestures preview through the renderer
and commit as one undo step on release; Escape cancels. Collisions are rejected
without overwriting existing keys, and subframe spacing is preserved.

Auto-arrange is enabled by default and adapts the node canvas to its available
space. Manually moving a node switches it off so your arrangement stays yours.

Library cards support both double-click and a right-click menu. Double-click a
material to fill selected text; effects open their composition setup, and template
cards open an example scene through the normal unsaved-changes flow. Drag a card
onto the viewer or node canvas for explicit placement choices. Materials offer
text fill, background and foreground; other effects offer composition setup or
an example. Cancelling a drop leaves the composition unchanged.
