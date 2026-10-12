# Sawhorse user guide

Sawhorse is pre-alpha. This page covers what works today (lane 5): looking around, drawing, push/pull, measuring, guides and dimensions, the transform tools (Move, Rotate, Scale, Offset, Follow Me), groups and components, tags and scenes. Paint is on the tool rail but says "arrives in a later lane"; the Cut list pane says it is not built yet.

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
| Space | Select | Click; double-click selects the whole solid, or opens a group or component; drag left to right for a window, right to left for a crossing box; Shift adds, Ctrl toggles, Esc clears | - |
| E | Erase | Click an edge, a line drawn on a face, an imprinted face, a sketch region, a guide or a dimension; drag across several (one undo step) | - |
| L | Line | Click, click, click; stops when it closes, when a face forms, or on Esc | a length along the direction shown |
| R | Rectangle | Click two corners, or click one and type `w, d`; with nothing clicked, it starts at the origin | `w, d` (a negative value goes the other way) |
| R again | Rotated Rectangle | Click both ends of one edge, then the width | the edge's length, then the width |
| C | Circle | Click the centre, then the radius | a radius; `48s` sets segments (24 by default) |
| C again | Polygon | As Circle | a radius; `6s` sets sides (6 by default) |
| A | Arc | Click both ends, then pull out the bulge | a bulge length, `12r` radius, `12s` segments |
| A again | 3-Point Arc | Click the start, a point it passes through, then the end | - |
| P | Push/Pull | Press on a face and drag, then release; or click, move, click. Double-click repeats the last distance. Hold Ctrl when you let go to pull out a new face. | a distance (negative flips it) |
| M | Move | Click a solid (or select first), move, click; or drag. Ctrl copies. With faces, edges or points selected, moves that part (Alt: autofold). | a distance; after a copy `x3`, `3x`, `*3` or `/4` |
| Q | Rotate | Click the centre on the protractor's plane, the start of the angle, then the end. Ctrl copies. | an angle in degrees (`90`, `-22.5`, `30deg`); after a copy `x3` or `/4` |
| S | Scale | Drag a grip on the selection's box (or click it, move, click). Shift: uniform. Ctrl: about the centre. | a factor (`2`, `0.5`, `-1`), three factors (`2,1,0.5`), or a size with a unit (`30in`) |
| F | Offset | Click a face or region (or near selected edges), move inward or outward, click | a distance (negative goes the other way) |
| W | Follow Me | Select the path first (edges, or a face for its edge), then click the profile | - |
| T | Tape | Click two points to measure. Click an edge, then move away from it, to place a parallel guide. | the offset, or a length that drops a guide point |
| D | Dimension | Click two points (or one edge), then click where the dimension line goes | - |
| Shift+Z | | Zoom extents | |
| Ctrl+Z / Ctrl+Y | | Undo / redo (Ctrl+Shift+Z also redoes) | |
| Ctrl+A | | Select all (in the open group or component) | |
| Ctrl+G | | Make Group from the selection | |
| Ctrl+Shift+G | | Explode the selected groups and components | |
| Delete | | Erase the selection | |
| Esc | | Cancels the gesture, then clears the selection, then closes the open group or component | |

**Axis locks** (any tool drawing from a start point):
- Right arrow locks the red axis, Left arrow the green, Up arrow the blue.
- Down arrow locks the parallel or perpendicular direction currently shown.
- Pressing the same arrow again unlocks.
- Holding Shift locks whatever direction is showing, until you release it.
- With Rectangle, arrow keys pressed before the first click choose the plane: level, upright facing red, or upright facing green.

**Ctrl and Alt taps.** In Move and Rotate a tap of Ctrl switches copying on and off (holding it on the last click works too). In Scale, Ctrl switches between scaling about the opposite grip and about the centre. In Move, Alt switches autofold.

**Value Control Box.** Type a value at any point in a gesture. The units are your display units unless you type one: `3/4`, `1 1/2`, `1-1/2`, `18"`, `1'6"`, `457mm`, `45.7cm`, `0.457m`. Click the unit label to cycle through fractional in, decimal in, ft-in, mm, cm and m. Esc clears what you typed, and Ctrl+Z still works while you type. Angles (`30°`, or a bare number in Rotate) and the array forms (`x3`, `3x`, `*3`, `/4`) belong to Rotate and Move; other tools say so. A value typed right after a move, turn, scale or offset changes it (one undo step).

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

## Moving, turning and scaling

**Move (M).** With nothing selected, Move picks up the solid you click. Select first to move several solids, or to move part of one solid: a face, an edge or a point.
- Click where to pick it up (a corner snaps exactly), move, and click where it goes.
- Or type the distance: it goes the way the cursor went.
- The arrow keys lock the red, green or blue axis. Shift holds the direction that is showing.

Copies:
1. Tap Ctrl (the preview turns green), then move and click.
2. Straight after, type `x3`: you get three copies, each the same distance further on.
3. Or type `/4`: you get four copies dividing the distance.

Another count or distance replaces the last one. Ctrl+Z takes the whole array back to before the copy.

**Moving part of a solid.** Select a face (or an edge, or a corner), press M and move it. Faces that stay flat follow along: lifting a top face makes the box taller, and sliding a top edge slopes the sides. If a face would have to bend, Sawhorse refuses and says which face. Tap Alt for autofold: the bent face then folds along new edges.

**Rotate (Q).** The protractor appears on the face under the cursor, or level on the ground.
- Right, Left and Up lock it upright facing red, upright facing green, or level. Hold Shift to keep the plane it is on.
- Click the centre, click where the angle starts, then click where it ends.
- Or type the angle. The cursor snaps to the protractor's 15 degree ticks; a typed angle is exact.
- With nothing selected, the solid under the centre click turns.
- Ctrl copies; `x3` and `/4` work as in Move. A ring of four chair legs is one copy at 90 degrees, then `x3`.

**Scale (S).** Grips appear on the box around the selection (or the solid under the cursor):
- a face-centre grip stretches one way, an edge grip two ways, and a corner grip three;
- Shift makes the scale uniform, and Ctrl scales about the centre instead of the opposite grip;
- type `2` for twice the size, `-1` to mirror, or `30in` for an absolute length along the grip.

Sawhorse refuses a scale that would flatten the solid, turn it inside out or fold it into itself.

**A tapered leg.**
1. Draw a 2 x 2 rectangle and pull it up 28.
2. Select the top face and press S.
3. Tap Ctrl (about the centre), click a corner grip and type `0.6`. The leg tapers to the top.
4. To put the taper at the foot, turn the leg end for end: Q, press Right, click the middle of a side edge, and type `180`.

## Offset and Follow Me

**Offset (F).** Click a face, then move toward its middle and type `1/2`. A loop 1/2 in inside the edge is imprinted, and Push/Pull (P) raises it straight away as a tenon.
- Moving outward instead draws the bigger loop on a new sketch on that plane, because it reaches off the solid.
- A sketch region offsets inside its own sketch.
- With edges selected, Offset offsets that chain of edges to the side the cursor is on.
- Corners stay sharp. A face with a hole offsets as one shape.

**Follow Me (W).**
1. Select the path: edges joined end to end (Shift-click to add them), or one face to go round its edge.
2. Press W. The path shows in blue.
3. Click the profile: a face, or a region of a sketch.

The profile travels along the path and turns on a miter at every corner. A region becomes a new solid. A face adds to its solid, or cuts into it when the path runs into it.

**A moulding round the top of a box.**
1. Look at one end of the box (Right).
2. With Line, draw from the top edge, 2 in in, down to the front edge, 2 in down. That cuts off a chamfer triangle.
3. Select the top face, press W and click the triangle. The chamfer is routed all the way round.

Paths with arcs come later.

## Groups and components

**Groups.** Select solids (or other groups) and press Ctrl+G (Edit > Make Group). A group selects as one: a click on any of its solids selects the whole group, and Move, Rotate and Scale move it as a unit. Double-click it to open it for editing: the rest of the model fades, a dashed box marks the group, and the status bar shows where you are ("Edit: Model › Carcass"). Inside, you edit its solids as usual; strokes you draw only stick to strokes drawn in the same group, never to the ones outside. Esc closes it again (after clearing the selection). Clicking something outside the open group closes it too and selects what you clicked. Ctrl+Shift+G (Edit > Explode) takes a group apart; its solids stay exactly where they are.

**Components.** Edit > Make Component asks for a name and turns the selection into a component: a definition (the shared parts) and one copy of it, called an instance. Copies you make of an instance with Move and Ctrl, or with a Move array (`x3`, `/4`), are more instances of the same component. Open any instance and change it (push a face, draw on it, add a part) and every instance changes with it. To change only one, right-click it in the Outliner and choose **Make Unique**: it gets its own copy of the definition ("Shelf (2)"). A cut that splits a solid in two inside a component does that by itself, so the other instances keep their solid pieces. Copies of plain solids stay plain solids.

**A bookshelf in components.**
1. `R`, type `3/4, 11 1/4`, Enter; `P`, type `72`, Enter. Ctrl+A, then Edit > Make Component, name it `Side`.
2. `M`, tap Ctrl, click the side's front bottom corner, press the Right arrow (red), type `35 1/4`, Enter. Two sides, one component.
3. `R`, type `34 1/2, 11 1/4`; `P`, `3/4`. Double-click the new board to select it, `M`, click it, Right arrow, `3/4`: it sits between the sides. Make Component `Shelf`.
4. `M`, tap Ctrl, click a shelf corner, Up arrow (blue), type `23 3/4`, Enter, then type `x3`: four shelves.
5. Double-click one shelf, click its top face, `P`, type `1/4`: all four shelves are 1 in thick now. Esc, Esc.
6. Right-click the top shelf in the Outliner, Make Unique, double-click it and push its top back `-1/4`: only that one changes.

**The Outliner** (right panel) is the model's tree: groups, component instances ("component · 2 of 4") and solids, folded except around the group you have open (marked "editing"). Click a row to select it (Shift adds, Ctrl toggles); selecting something inside a closed group opens what holds it. The eye hides and shows, the padlock locks (a locked group cannot be opened, moved, exploded or deleted). Double-click a name to rename it; renaming a component renames it everywhere. Right-click for Edit inside, Make Group, Make Component, Make Unique, Explode, Hide, Lock, Rename and Delete.

## Tags

Tags control what you see, nothing else: geometry stays where it is and in its group. The Tags pane lists **Untagged** first (everything starts there), then your tags with their colour and how many things are on each. **New tag** adds one and lets you type its name. Select solids, groups or components and click a tag's label button to put them on it (Untagged's takes them off). The eye shows or hides everything on the tag, including what is inside a tagged group. Click the swatch to change the colour, double-click the name to rename (scenes follow), the bin deletes the tag (what was on it becomes untagged).

## Scenes

A scene remembers the camera, which tags are hidden and which group or component is open. The tabs above the view are your scenes: **+** saves the current view as a new one, a click goes to it, double-click renames it, and right-click updates it to the current view, renames or deletes it. View > Add Scene does the same as +. Going to a scene is an undo step for the tags it switches (the camera is not).

## When a cut goes all the way through

A push, or a Follow Me cut, that cuts a board in two leaves two solids. The piece that keeps the board's smallest face id keeps its name ("solid 3"); the other gets the next number. Both stay in the board's group. Inside a component used more than once, the cut makes that instance unique first, so the other instances are not cut.

## Unsaved work

Sawhorse keeps your work in this browser as you go. If the tab closes or reloads, the status line offers **Restore unsaved work?** with Restore and Discard; Edit > Restore Unsaved Work does the same while the offer stands. Restoring brings the undo history back too. Real files (.shz) arrive with their lane.
