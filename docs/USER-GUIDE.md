# Sawhorse user guide

Sawhorse is pre-alpha. This page covers what works today (lane 3): looking around, drawing, push/pull, measuring, guides and dimensions. Move, Rotate, Scale, Offset, Follow Me and Paint are on the tool rail but say "arrives in a later lane"; the Outliner, Tags and Cut list panes still show the design mock's example content.

## The first ten minutes

You will make a board, cut a dado across it, and set a shelf in the dado. Everything is keyboard and mouse; nothing needs a dialog.

**Minute 0 to 1: look around.** The screen is the approved layout: menus and view buttons along the top, the tool rail on the left, the 3D view in the middle, a panel on the right, and the **Value Control Box** (VCB) at the bottom right. Middle-drag to orbit, right-drag (or Shift+middle-drag) to pan, and roll the wheel to zoom toward the cursor. The buttons Iso, Top, Front, Right and Persp jump to the standard views. Shift+Z fits everything in view. The red, green and blue lines are the X, Y and Z axes; their dashed halves are the negative sides. The grid follows your units: 1 in and 1 ft lines in inches, 10 mm and 100 mm lines in metric.

**Minute 1 to 3: a board.** Press `R` (Rectangle). Type `3/4, 11 1/4` and press Enter: a 3/4 in by 11 1/4 in rectangle appears on the ground at the origin. You don't need to click in the box first; typing goes straight into it. Press `P` (Push/Pull), type `30` and press Enter: the rectangle you just drew rises 30 in into a solid board. The badge at the top left of the view shows the solid count, face count, volume and whether the solid is watertight.

**Minute 3 to 5: a guide and a dado.** Press `Right` (the view button) so you face the board's inner side, then Shift+Z.
1. Press `T` (Tape). Click on the bottom edge of that face. The point turns amber (On Edge).
2. Move the cursor up, type `12` and press Enter. A dashed guide line now runs across the face 12 in up.
3. Press `R`. Hover where the guide crosses the board's front edge: the point turns magenta (Intersection). Click there.
4. Move the cursor up and into the face, type `11 1/4, 3/4` and press Enter. The rectangle crosses the face from edge to edge, so it cuts the face into a strip.
5. Press `P`, type `-3/8` and press Enter. The strip sinks 3/8 in. That's a dado.

**Minute 5 to 8: a shelf.** Press `Iso`.
1. Press `R`. Hover the dado's front bottom corner, at the back of the groove: it turns lime (Endpoint). Click.
2. Move the cursor away from the board, level with the corner. Type `24, 11 1/4` and press Enter. The rectangle starts on a corner rather than inside a face, so its plane follows the cursor. Here that is the level plane through the corner.
3. Press `P`, type `3/4` and press Enter. The shelf sits in the dado.

**Minute 8 to 10: measure, annotate, undo.**
- Press `T`, click two corners, and the status line reads the distance and its x, y and z parts.
- Press `D` (Dimension), click two corners (or one edge), then click where the dimension line goes.
- Press Ctrl+Z to step back and Ctrl+Y to step forward; every step is one undo.
- Press `E` (Erase) and click the guide to remove it. You can also use Edit > Delete Guides to clear them all.
- Press Space (Select) and drag a box across the shelf, then press Delete. A box dragged left to right selects only what is wholly inside it. A box dragged right to left also selects anything it touches.

If an operation can't be done (for example, erasing an edge would open a solid), nothing changes and the status line says why.

## Keys

| Key | Tool | Gesture | VCB takes |
|---|---|---|---|
| Space | Select | Click; double-click selects the whole solid; drag left to right for a window, right to left for a crossing box; Shift adds, Ctrl toggles, Esc clears | - |
| E | Erase | Click an edge, a line drawn on a face, an imprinted face, a sketch region, a guide or a dimension; drag across several (one undo step) | - |
| L | Line | Click, click, click; stops when it closes, when a face forms, or on Esc | a length along the direction shown |
| R | Rectangle | Click two corners, or click one and type `w, d`; with nothing clicked, it starts at the origin | `w, d` (a negative value goes the other way) |
| R again | Rotated Rectangle | Click both ends of one edge, then the width | the edge's length, then the width |
| C | Circle | Click the centre, then the radius | a radius; `48s` sets segments (24 by default) |
| C again | Polygon | As Circle | a radius; `6s` sets sides (6 by default) |
| A | Arc | Click both ends, then pull out the bulge | a bulge length, `12r` radius, `12s` segments |
| A again | 3-Point Arc | Click the start, a point it passes through, then the end | - |
| P | Push/Pull | Press on a face and drag, then release; or click, move, click. Double-click repeats the last distance. Hold Ctrl when you let go to pull out a new face. | a distance (negative flips it) |
| T | Tape | Click two points to measure. Click an edge, then move away from it, to place a parallel guide. | the offset, or a length that drops a guide point |
| D | Dimension | Click two points (or one edge), then click where the dimension line goes | - |
| Shift+Z | | Zoom extents | |
| Ctrl+Z / Ctrl+Y | | Undo / redo (Ctrl+Shift+Z also redoes) | |
| Ctrl+A | | Select all | |
| Delete | | Erase the selection | |
| Esc | | Cancels the gesture, then clears the selection, then leaves the group | |

**Axis locks** (any tool drawing from a start point):
- Right arrow locks the red axis, Left arrow the green, Up arrow the blue.
- Down arrow locks the parallel or perpendicular direction currently shown.
- Pressing the same arrow again unlocks.
- Holding Shift locks whatever direction is showing, until you release it.
- With Rectangle, arrow keys pressed before the first click choose the plane: level, upright facing red, or upright facing green.

**Value Control Box.** Type a value at any point in a gesture. The units are your display units unless you type one: `3/4`, `1 1/2`, `1-1/2`, `18"`, `1'6"`, `457mm`, `45.7cm`, `0.457m`. Click the unit label to cycle through fractional in, decimal in, ft-in, mm, cm and m. Esc clears what you typed, and Ctrl+Z still works while you type. Angles (`30°`) and the array forms for Move (`x3`, `3x`, `*3`, `/4`) parse today but belong to Rotate and Move in lane 4; until then they are refused with a message.

## Inference colours

When the cursor lands on something the tool can use, a coloured point and a tip show it. These colours are Sawhorse's own:

| Colour | Means |
|---|---|
| Lime | Endpoint |
| Teal | Midpoint |
| Amber | On Edge |
| Violet | On Face |
| Magenta | Intersection (two edges, or a guide crossing an edge) |
| Blue | Centre of a circle, arc or polygon |
| Indigo | Parallel or Perpendicular to an edge (a dotted line runs from your start point) |
| Grey | From Point: lined up with a point you hovered earlier (a dotted line runs to it) |
| Tan | On Guide or Guide Point |
| Sage | Grid point (on the ground, when nothing nearer wins) |
| Red / Green / Blue | On the red, green or blue axis direction from the start point |

The legend under the view lists the four most common colours.

How the snapping chooses:
- Points beat lines, and lines beat faces.
- Hidden geometry doesn't snap.
- The snap distance stays the same number of pixels at any zoom.
- Along an axis or a locked direction, a length drawn with the mouse snaps to whole grid steps (1 in or 1 ft; 10 mm or 100 mm) once those steps are at least 8 pixels apart on screen. Type a value for anything finer.
- To line up with a corner that isn't connected to your shape, hover it for a moment and then move away. A grey From Point line stays available from it.

## Drawing on solids

A shape drawn on a solid's face becomes part of that face:
- A closed loop inside the face imprints a new face that you can push or pull.
- A line, an arc or a rectangle that crosses the face from edge to edge splits the face in two.
- A line that stops inside the face waits as a preview until it reaches an edge.

A shape that reaches past the solid (for example, a top board drawn from one side's corner to the other's) becomes a new sketch on that plane.

Push/Pull on a face moves it along its normal and keeps the solid closed. Pushing past the far side cuts through.
