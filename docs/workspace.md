# Editing workspace

The docked **Library** sits on the left, the viewer and node workspace in the
centre, and the Inspector on the right. Resize the panel dividers to suit your
screen. Collapse the Library with its header arrow; restore it through
**View > Library panel**. Narrow windows use a compact library button.

Use the Library tabs to browse effects, materials, example templates or imported
media. Search filters the current tab. Materials also have category filters.
Selecting a card does not modify the composition. With editable text selected,
**Apply to selected text** changes its material in one undoable transaction.
**Try example** opens the corresponding editable starter scene; **More options**
provides composition inputs and background/foreground placement.

Material cards use bundled GPU-rendered samples of the same surface shader used
in compositions. Curved samples show metals and leather; flat samples show wood,
brick, paper and other repeating surfaces. Sample lighting and geometry differ
from your composition. Effect cards use still frames from their rendered examples;
open More options for animated before/after previews.

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
Transform, to preserve proportions. Double-click the text to edit its wording.
Layout-box editing remains a separate Text tools option for wrapping paragraphs.

The keyframe strip uses labelled frame ticks that adapt to the available width
and composition duration. Click or drag anywhere on the ruler to scrub; clicking
near a keyframe snaps to it. The previous/next key buttons still navigate the
selected node's applied keys.
