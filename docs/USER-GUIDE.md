# Sawhorse user guide

Sawhorse is pre-alpha. This page covers what works today (lane 11): looking around, drawing, push/pull, measuring, guides and dimensions, the transform tools (Move, Rotate, Scale, Offset, Follow Me), groups and components, tags and scenes, the cut list, materials, paint, styles and shadows, and plugins.

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
| E | Erase | Click an edge, a line drawn on a face, an imprinted face, a sketch region, a guide or a dimension; drag across several (one undo step). Ctrl softens and smooths the edges instead, Shift hides them, Ctrl+Shift unsoftens them | - |
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
| B | Paint | Click a face to paint it with the active material; a closed group or component takes it as its default; Ctrl+click paints the whole solid; click something selected to paint the whole selection. Alt+click picks a material up. Double-click a textured face to move, turn or scale its texture with the handles. | - |
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

A scene remembers the camera, which tags are hidden, which group or component is open, and the style and shadows (Styles pane), and which section planes are active and hidden. The tabs above the view are your scenes: **+** saves the current view as a new one, a click goes to it, double-click renames it, and right-click updates it to the current view, renames or deletes it. View > Add Scene does the same as +. Going to a scene is an undo step for the tags it switches (the camera is not).

## Materials and paint

The Materials pane (right panel, **Materials**) shows the active material at the top, the materials in your model (with how many faces show each), and the library below: twelve woods (White oak, Red oak, Walnut, Cherry, Hard maple, Ash, Birch, Poplar, Pine, Douglas fir, Mahogany, Western red cedar), eight sheet goods (Baltic birch ply, Maple ply, Red oak ply, MDF, Hardboard, White melamine, OSB, Particle board), sixteen colours, and glass and metal. Type in the search box ("oak", "ply") or pick a category. Click a swatch and Paint (B) starts with it; a library material goes into your model with the first coat.

**Painting.**
- Click a face of a plain solid, or of a solid inside the group or component you are editing: that face is painted.
- Click a closed group or component: it is painted as a whole. Its faces that have a material of their own keep it; the rest show the new one. Each copy of a component can be painted differently this way. Paint faces inside a component (double-click it first) to paint them in every copy.
- Ctrl+click a face: its whole solid. Select several faces, or several solids, groups and components, and click one of them: all of them, one undo step.
- Hover shows what a click would paint, in the material's colour.
- Alt+click a face to make the material it shows the active one (sampling).
- Painting what already shows the material does nothing (no undo step), and says so.
- Painting a group (or a solid) whose parts have paint of their own: those parts keep it, and the status line counts them. **Edit > Clear Paint** with the group selected takes the paint off everything inside it (faces, solids, nested groups), so the group's shows everywhere: one undo step. A component's parts belong to its definition: open it (double-click) and clear them there.
- A face shows its own material, else its solid's, else the nearest group or component around it that has one. Back faces of open imported meshes show the default back-face colour. Faces that a push or a cut makes start unpainted; a face you split, or draw a shape on, keeps its paint on every piece.

**Editing a material.** Click a material of your model to see its editor: rename it, change its colour, its texture's real size (type a width or a height in your units: the other follows, keeping the image's shape) and rotation, and what it is as stock. **Delete** takes it off everything (what was painted with it shows what is around it). **Import image...** makes a new material from a PNG or JPEG, one foot across until you give it its real size.

**Stock.** A material can say what it is: **Solid lumber** or **Sheet goods**, and the species ("White oak", "3/4 Baltic birch ply"). The library's woods and sheet goods say so already. The cut list then lists a part painted with it by that species, as lumber or sheet goods, unless you named the part's stock yourself.

**Positioning a texture.** Double-click a textured face with Paint (or select one face and click **Position texture** in the pane). Three handles appear on the face: drag the square in the middle to slide the texture, the round one to turn it about the middle, the small square to scale it. The texture follows as you drag; letting go is one undo step. Enter or Esc finishes. By default a texture's grain runs along the longer side of each face, so boards read right without positioning.

## Styles and shadows

The Styles pane (right panel, **Styles**) changes how the model is drawn, never the model.

- **Presets**: Sawhorse (textures, plain edges), Shop drawing (hidden line on paper, heavy outlines, edges running a little past their ends), Shaded (colours without textures), X-ray, Clay (one colour, heavy outlines and endpoints) and Outdoors (textures under a sky, on the ground).
- **Faces**: shaded with textures, shaded, monochrome, hidden line (faces in the background colour: only the edges you could see), wireframe (every edge, no faces), X-ray (see-through faces, every edge).
- **Edges**: on or off; **Profiles** draws outlines heavier; **Extensions** runs edges a little past their ends, like pencil lines; **Endpoints** draws the ends of edges heavier. SVG view drawings (File > Export) follow the edge style.
- **Background**, **Sky** and **Ground** colours. The horizon follows the camera as you tilt it.
- **Sections**: section cuts, section planes and section fill on or off, and the cut's colours and line width (see Section planes below).
- **Shadows**: tick **Show shadows**, then pick the date, the time (local sun time) and the latitude; **Light** and **Dark** set how bright the sun and the shade are; **On the ground** and **On faces** choose where shadows fall. North is along green.

Sliders show their effect as you drag and are one undo step when you let go. Styles and shadows are saved with the model, and a scene saved after changing them brings them back when you go to it.

### Soft and smooth edges

A round part is drawn as flat faces: a dowel of 24 sides, a rounded slab, a turned leg. Make it read as one round surface by softening the edges between those faces: a **soft** edge is not drawn as a line (where the surface turns away from you it is still drawn, as its outline), and a **smooth** edge lets the shading run across it. A **hidden** edge is not drawn at all.

- **Soften edges** in the Styles pane: select the solids, set **Faces turning by at most** (20° rounds a 24-sided cylinder, whose sides turn 15° each; its rims turn 90° and stay sharp), tick **Soften** and / or **Smooth** and click **Soften selected**. Every edge of those solids within the angle is softened and smoothed and every other edge goes hard: one undo step. To soften the parts of a group or component, open it first (double-click); every copy of a component follows.
- **With the Eraser** (E), edge by edge or dragged across several: **Ctrl** softens and smooths, **Shift** hides, **Ctrl+Shift** unsoftens (soft, smooth and hidden all cleared). Nothing is erased. Hovering an edge tells you if it is soft, smooth or hidden. Soft and hidden edges cannot be clicked by accident; holding Ctrl+Shift reaches them.
- **Soft and hidden edges** in the Edges list draws them dashed, so you can see where they are.
- SketchUp files keep their soft, smooth and hidden edges when imported. Saved `.shz` files keep them. GLB exports carry the smooth shading, OBJ exports carry it as smoothing groups, SVG drawings leave soft and hidden edges out; STL and 3MF have no edges to mark.

## Section planes

A section plane cuts the model open so you can see inside it: the shelves and dados of a cabinet, the joint in a frame. It never changes the model; it only hides what is on one side of it, and fills the cut where it passes through solid wood.

**Placing one.** Pick **Section** in the tool rail (or Tools > Section Plane; it has no key). Hover a face: the plane's outline lies on it, with arrows pointing into the model, at what stays. Click (or press Enter) to place it. A plane laid on a face cuts nothing until you move it in, so the usual move is: place it on the end of the cabinet, then Move (M) it along the cabinet to where you want the cut, typing the distance. Before you click:

- The arrow keys lock the plane square to an axis: Right for red, Left for green, Up for blue; Down unlocks. Away from any face the plane stands square to the view's main axis.
- A tap of Alt reverses it (the arrows point the other way); Alt+click places it reversed.

The new plane is **active** (it cuts) and selected. It is placed in the group or component you have open, and then it cuts only what that group holds (a plane in one copy of a component belongs to that copy). Each context has at most one active plane; activating another switches the first off.

**Working with planes.** Planes are listed in the Outliner ("Section 1 · section · active"). Right-click a plane in the Outliner for **Activate Cut** / **Deactivate Cut**, **Reverse** (keep the other side), **Create Group from Slice** (the cut's outline as faces in a new group on the plane, ready to push into a template or a profile), **Export Slice (SVG / DXF)**, Hide, Rename and Delete; with a plane selected, the Edit menu has the first three too. Move, Rotate, Scale and copies move planes like anything else; select a plane and solids together and they move as one undo step. File > Export > Section slice saves the selected (or active) plane's cut: SVG filled, DXF as one closed polyline (LWPOLYLINE) per outline and per hole, never loose LINE pieces, so a CAM program sees each part as a closed shape with its holes; both 1:1 in millimetres. A plane that cuts nothing (laid on a face and keeping the side the whole solid is on, or past every solid) has nothing to export, and the status line says so: move it into the solid, or Reverse it.

**How the cut looks** is part of the style (Styles pane, **Sections**, or View > Section Planes / Section Cuts / Section Fill):

- **Section cuts**: active planes cut. Off, the whole model shows and the planes stay.
- **Section planes**: the planes are drawn (a light frame with arrows). Off, they are hidden but still cut.
- **Section fill**: the cut faces are filled, exactly, holes and all. **Fill colour**, **Cut line colour** and **Cut line width** set how the cut is drawn; untick a colour to use the theme's.

**Scenes** remember which planes are active and which are hidden. Save a scene with the cut on (say the Right view, Shop drawing style, the plane through the middle) and going to it brings the cut back; a scene saved with no plane active shows the model whole. That is how a sheet shows a section next to whole elevations.

## Cut list

The Cut list pane (right panel, **Cut list**) turns the model into the list of boards you cut: one row per part, with how many, its finished size in lumber terms and its material, two tiles for board feet and sheets, and the files a shop needs.

**What is a part.**
- A component is a part, and its quantity is the number of its copies. Copies inside other components multiply: a Drawer used three times, holding two Drawer side copies, gives Drawer side x 6.
- A plain solid is a part of its own, named as the Outliner names it ("Solid 3", or the name you gave it).
- A group of solids (a glue-up) is one part.
- A component that holds other components or groups is an assembly: its pieces are listed, not the assembly.

So make the boards you repeat components (select, Edit > Make Component): six identical sides become one row, "Side, 2".

**Parts that are not solids.** Most furniture drawn in SketchUp is not watertight: a bench or a desk arrives as groups of open faces (Sawhorse tints them amber and calls them meshes). A group or component holding such faces is still a part. Sawhorse first tries to close it (it welds gaps under 0.1 mm, turns faces that point the wrong way and drops stray slivers); if the faces still do not close, the part is sized from the smallest box around them, exactly like a solid, and flagged **not solid (sizes from bounds)** in the row, the CSV, the report, the PDF, the labels and the nesting booklet. Its board feet, its sheet or lumber reading and its place on the sheets all use that size. Check a flagged part's size against what you meant to build: a box around open faces is right for a board, generous for anything with a slanted or rounded edge. Two kinds of part are listed but **not counted** (no board feet, no area, no sheet, not nested; the row is greyed and says why): a **flat face** with no thickness (a label or a decal drawn on the model), and open faces whose box is **thicker than any board** (4 in, or the thickest board in your stock library), which is almost always a whole piece of furniture drawn as one group: a bench 14 in thick is not a board. Group each board's faces on its own (select them, Make Group) and they count.

Faces drawn loose, outside any group or component, are never parts. The line above the table counts them ("312 faces outside parts"); **Make parts from loose faces** puts each connected piece of them into a group of its own (named after the file: "benches 1", "benches 2"...), one undo step, nothing moved, and each becomes a part. It acts on the faces where you are: at the top level, or inside the open group or component. When the loose faces sit inside an assembly instead (a cupboard whose doors are groups but whose carcass is loose faces), the line says where ("171 faces outside parts (in Cupboard)") and offers **Open Cupboard**; once it is open, Make parts from loose faces takes them. When every face in the file is loose, the empty list says so instead of only "No parts yet".

**Leaving a part out.** Every row has a box before its name. Untick it (or right-click, **Leave out of the totals**) to keep a part listed but out of the board feet, the sheet count, the labels and the nesting: a cushion, a lamp, the hardware. Tick it again to count it. It is one undo step and is saved with the model.

**Sizes** are finished sizes: thickness, width and length (T is the smallest, L the largest). **Round to** starts at **Follow units**: the nearest 1/16 in when your units are fractional inches, decimal inches or ft-in, the nearest 0.5 mm (sizes in mm) when they are metric; the menu says which ("Follow units (1/16 in)"). It also offers 1/16 in, 1/32 in and 0.5 mm outright. With decimal-inch or ft-in units the sizes are written as decimal inches (11.25 rather than 11 1/4), in the pane and in every file. A rounding you pick lasts for this session (a reload keeps it); click **Pin** beside it to keep it for next time too, and click Pin again (or go back to Follow units) to stop. Rounding only changes how sizes are written: whether a part is sheet goods, its board feet, its area and the sheet count all come from its exact size, so no rounding or unit changes a total. A part made of rectangles (a board, a dadoed side, a tenon) is measured exactly. A part that is not (a tapered leg, a moulding) is noted **shaped** and shows the box it comes out of. A mirrored copy counts in the same row, noted **handed** ("1 of 2 handed" when only some are).

**Board feet are counted on the rough lumber you buy, not the finished part.** Hardwood is sold rough by the quarter inch (4/4 is 1 in, 5/4 is 1 1/4 in, 8/4 is 2 in) and planed down: 4/4 surfaced four sides (S4S) is 3/4 in. So a 3/4 in shelf 11 x 34 1/2 in is 2.64 board feet (1 x 11 x 34 1/2 / 144), not 1.98. The sizes in the table stay the finished sizes; each lumber part's board feet use the thinnest board in the stock library at least as thick as the part (4/4 for 3/4 in, 5/4 for 1 in, 8/4 for 1 1/2 in), or the board its stock or its material names when that is thick enough; a part thicker than every board counts its own thickness. Hover a part's T to see which ("board feet on 4/4 (1 in rough)"); the CSV has it in the **Board feet on** column, and the pane, the report and the PDF carry the footnote. Sheet goods are counted by area, never in board feet.

**Grain** runs along the length; a small arrow beside L shows it. While the pane is open the view draws a grain arrow on every part. To change it, right-click a row: **Grain along the length**, **Grain across (along the width)** (the arrow moves to W, and the view's arrow turns), or **No grain** (MDF, a painted part). Files write the grain as "along L", "across W" or "none", and a grain you set is listed in the row's notes ("grain across W", "no grain").

**Material and stock.** The Material column shows the part's material (the one covering most of its first solid's faces, as painted), or a dash; a material that says what stock it is shows its species and decides lumber or sheet goods. Right-click a row and choose **Stock / material...** to name what it is cut from ("White oak", "1/4 birch ply") and whether it counts as solid lumber, sheet goods, or (the default) whichever its name and thickness say. Names with ply, MDF, HDF, OSB, particle board, melamine or hardboard in them are sheet goods, and so is anything 1/4 in thick or thinner (its exact thickness: a 0.26 in panel is lumber even though it shows as 1/4). **Mark as sheet goods** / **Mark as solid lumber** switches the row; **Use the material's name** undoes the naming. Stock belongs to the component, so it is the same in every copy.

**Many rows at once.** Click a row, then Shift+click another to pick every row between them, or Ctrl+click to add or drop one row (Ctrl+Shift+click adds a range). Right-click any picked row and the menu (titled "3 parts: Side, Shelf, Top") acts on all of them: grain, **Stock / material...** (one dialog, "Stock for 3 parts"), sheet goods or lumber. It is one step: one Ctrl+Z puts them all back. Right-clicking a row that is not picked acts on that row only. Ctrl+Z and Ctrl+Y work while the pane's controls (Round to, Visible only, the buttons) have focus; only a text field you are typing in keeps them for itself.

**Narrow pane.** The table fits the 300 px panel at every rounding: long part names and stock end in "..." and show in full when you hover them; the right-click menu shows the row's name at the top, and the row stays marked while the menu is open.

**The tiles.** Board feet = T x W x L / 144 (inches) for each lumber part times its quantity, from its exact size (not the rounded one), with the species when there is one. Sheet goods are counted as a rough number of 4 x 8 sheets (their area divided by a sheet's, rounded up), with the stock and thickness. That count is a rough estimate; **Nesting** (below) lays the parts out on real sheets and boards, kerf included. Edge banding is not included.

**Hidden parts** (the Outliner's eye, a hidden tag, a hidden group) stay in the list with a "hidden" note. Tick **Visible only** to leave them out of the rows and the totals.

**A row click** selects that part's pieces in the model (Shift+click picks a range of rows, Ctrl+click adds or drops one). When they all sit inside one closed group or component, it opens that first, as the Outliner does; picked rows in several places select the groups or components that hold them.

**Files.**
- **CSV** saves a spreadsheet: Part, Qty; T, W, L as shown (fractions or mm); T, W, L again as decimal numbers in inches or mm (for formulas); Material, Grain, Stock (Lumber or Sheet), Board feet, Area, Notes, Summary, Board feet on (the nominal board a lumber part's board feet are counted on, "4/4 (1 in rough)"); and a Total row (pieces, board feet, sheet area, and the 4 x 8 sheet count in Summary). Empty cells are empty. A shown size with a fraction is written as `="3/4"` so Excel, LibreOffice and Google Sheets keep it as text instead of turning 3/4 into a date or 11 1/4 into a number; a plain CSV reader sees the `="..."` around it, and the decimal columns are ordinary numbers.
- **PDF** saves the cut list report as a PDF (Letter pages: the table, the totals and the note). Its columns are Part, Qty, T, W, L, Material, Grain (along L, across W or none), Bd ft / area (board feet for lumber, area for sheet goods) and Notes (grain set on the part, shaped, handed, hidden, scaled); long names and notes wrap instead of being cut off.
- **Print** prints the same report, with the same columns, on its own, without the window around it (the print preview shows only the report).
- **Labels** saves a PDF of labels, one per part, laid out for Avery 5160-size sheets (30 to a page, 2 5/8 x 1 in): name and quantity, size, stock and the grain as words and an arrow.

Each file is offered through the same save dialog as an export, named after your model ("Bookshelf cut list.csv", "Bookshelf labels.pdf").

**The bookshelf as components.** This builds the bookshelf the cut list tests use. Sizes in inches; typed values need no click first.
1. The back panel, while there is room: `R`, `34 1/2, 71 1/4`, Enter; `P`, `1/4`, Enter. Double-click it, `Q`, Right arrow, click its corner at the origin, click along its long edge, type `90` (or `-90` if it turned down into the ground): it stands up. Move it (`M`) 3/4 along red and back along green to 11. Edit > Make Component, `Back panel`.
2. Sides: `R`, `3/4, 11 1/4`; `P`, `71 1/4`. Double-click it, Make Component `Side`. `M`, tap Ctrl, click its front bottom corner, Right arrow, `35 1/4`.
3. Top: `R`, `36, 11 1/4`; `P`, `3/4`; double-click it, `M`, Up arrow, `71 1/4`. Make Component `Top`.
4. Toe kick: `R`, `34 1/2, 3/4`; `P`, `3 1/2`; move it 3/4 along red and 2 along green. Make Component `Toe kick`.
5. Shelves: `R`, `34 1/2, 11`; `P`, `3/4`; move it 3/4 along red and 3 1/2 up. Make Component `Shelf`. `M`, tap Ctrl, click a corner, Up arrow, `18`, then `x3`.

The Cut list then reads: Side 2, 3/4 x 11 1/4 x 71 1/4; Top 1, 3/4 x 11 1/4 x 36; Toe kick 1, 3/4 x 3 1/2 x 34 1/2; Shelf 4, 3/4 x 11 x 34 1/2; Back panel 1, 1/4 x 34 1/2 x 71 1/4 (sheet). That is 2,735.15625 cubic inches of finished lumber, all of it 3/4 in, so it comes from 4/4 boards: 3,646.875 cubic inches of rough 1 in lumber, **25.33 board feet**, and **1 sheet** of 1/4 in stock. Right-click the Top, Grain across: its arrow turns. Right-click the Back panel, Stock, `1/4 birch ply`, Sheet goods. Then CSV, PDF, Print and Labels.

## Nesting (cutting diagrams)

**Nesting ›** at the bottom of the Cut list opens the Nesting section under the list: every piece of every part laid out on real sheets and boards, the cuts that free them, and what to buy. It works itself out again a moment after every change you make, and it is the same in the browser and on the desktop: the same model always gives the same layout.

**What it is cut from.** Each part's stock is chosen in this order:
1. The part's own stock (right-click its row, **Stock / material...**), when that names a stock in the library ("4 x 8 MDF, 3/4"). A part named for a stock of the wrong thickness is listed as not placed.
2. Its material's stock, set in the **Stock...** dialog ("Oak" from "4/4 x 8 x 96").
3. Otherwise automatically. Sheet goods come from the sheet of their thickness (within 1/16 in, so a part drawn 3/4 in comes from 23/32 in "3/4" ply). Solid lumber comes from boards of the thinnest nominal thickness that covers it (4/4 for 3/4 in parts); all the parts of one material and thickness share one board size, the narrowest board as wide as the widest of them, so a narrow part rides on the offcuts of the wide ones.

Parts of different materials never share a sheet or a board.

**The library** (the **Stock...** button) starts with 4 x 8 plywood sheets sold as 3/4, 1/2 and 1/4 in (actually 23/32, 15/32 and 7/32 in, as a yard sells them), 5 x 5 Baltic birch at 18 and 12 mm (sold as 3/4 and 1/2), 4 x 8 MDF at a true 3/4 in (no grain), 4/4 boards (3/4 in S4S) 4, 6, 8 and 12 in wide, and 5/4 (1 1/16 in), 6/4 (1 5/16 in), 8/4 (1 3/4 in) and 12/4 (2 3/4 in) boards 6 in wide, all 8 ft long. Every stock has a **nominal** thickness (what the yard calls it) and an **actual** one (what it measures): parts are matched to stock by the actual thickness (within 1/16 in, and a part drawn at the nominal 3/4 in still finds "3/4" ply), boards are bought in board feet on the nominal. In the dialog: pick each material's stock (or Automatic); **Edit** or **Delete** a stock; add one (name, sheet or board, width, length, nominal, actual thickness, grain along the length or none). Typing a standard nominal (3/4, 1/2, 1/4, 5/8, 3/8 for sheets; 4/4 to 16/4 for boards) fills in the actual thickness; type over it when your supplier's differs (your 3/4 ply measures 0.709 in, your 4/4 is skip-planed to 13/16) and it stays as you typed it. **Built-in stock** puts everything back. Lengths run along the grain. Type sizes as anywhere else ("48", "5'", "1219mm", `1-1/2"`, `1 1/2`, `1.5"`, `1½`). Every change is one undo step and is saved with the model.

**Saw, kerf and trim.** **Saw: Table saw** (the default) lays sheets out so they come apart in straight cuts from edge to edge, the way a table saw or panel saw cuts. **Tightest (CNC)** packs parts as tightly as their rectangles allow; some of its layouts need a track saw, a jigsaw or a CNC (the diagram says "not all straight cuts"). **Kerf** is the width of every cut (1/8 in to start; type 3/32 for a thin-kerf blade). **Trim** is taken off every edge of a sheet and both ends of a board before anything else (factory edges, checked ends). Both commit when you press Enter, leave the field, or stop typing.

**Grain.** A part's grain runs along each sheet's and board's length, so a part is never turned across the grain; it turns 90 degrees only when it, or the stock (MDF), has no grain. Set a part's grain across (right-click its row) and it lies across the sheet.

**Reading it.**
- Three tiles: sheets (and which), boards (by nominal size, with the board feet you buy), and the overall yield: the parts' area over the stock's.
- **Parts on no stock**, if any, with why: no stock of its kind is that thick, the stock it names is the wrong thickness, it is larger than the stock, or it would fit only with its grain across. Nothing is ever dropped silently.
- A diagram per sheet or board, to scale, length left to right. Each part carries its tag: its row number in the cut list, and a letter when the row has more than one piece ("4c" is the third Shelf). Hover a part for its name and size; it lights up in the model too (and the part under the pointer in the view lights up in its diagram). Click it to select it in the model.
- Under each diagram, the cuts in order ("Rip to 11 1/4 in", "Crosscut to 34 1/2 in": set the fence to that), and the parts on it.

**Files.** **SVG** saves one drawing per sheet or board, 1:1 in millimetres (it prints or projects to scale): every part outlined and labelled with its tag, name and size, the overall dimensions, every cut numbered where it starts, and the cut sequence written underneath. Each diagram's own **SVG** link saves just that one. **DXF** saves every sheet and board in one file for a CNC router or a laser, in millimetres: a closed polyline per part on a layer per sheet or board (SHEET-01, BOARD-02, ...), the stock outlines on layer STOCK and the tags as text on layer LABELS, so the CAM program can cut one layer and ignore the rest. **DXF R12** is the same with old-style polylines, for programs that read only R12. **PDF** saves a booklet: a first page with what to buy (each stock and material, how many, the yield, board feet), the settings, any parts on no stock and an index; then a page per sheet or board with its diagram to scale, its parts and its cut sequence.

**The bookshelf.** Build the bookshelf above and name its back panel 1/4 birch ply, then open Nesting: the carcass, top, shelves and toe kick (3/4 in, no material) come from **five 4/4 x 12 x 96 boards** (each side takes a board, with 24 5/8 in left that nothing fits; the top and the toe kick share one; the four shelves go two to a board) and the back from **one 4 x 8 sheet of 1/4 in**: 6,105 sq in of parts from 10,368 sq in of stock, **58.9% yield**. Name the carcass "3/4 birch ply" too and it becomes three sheets: two of 3/4 in (four 11 1/4 in strips fit across a sheet, but nothing 34 1/2 in long fits beside a 71 1/4 in side) and the back's.

## Drawing sheets

Sheets turn the model into shop drawings on paper: elevations and plans at a real scale, a section, dimensions that read the model, notes, the cut list and a title block, printed or saved as PDF, SVG, DXF or PNG. What you see on the sheet is exactly what prints.

**The Model / Sheets switch** at the start of the tab row above the view changes the window between the model and its sheets. In Sheets mode the tabs are your sheets (**+** adds one, a click shows it, double-click renames, right-click to rename, move left or right, or delete), the rail on the left holds the sheet tools, and the right panel is the **Sheet** pane. A new sheet is Letter landscape in inches (A4 when you work in metric) with half-inch margins and a title block.

**Moving around.** The wheel zooms about the pointer; drag with the middle button, or hold Space and drag, to pan; **Fit** shows the whole sheet. **Grid snap** snaps what you place and move to the sheet's grid (1/8 in to start).

**The tools.**
- **Select**: click an item (Shift adds), drag a box round several, drag to move, drag a corner to resize, drag a dimension to stand it off (a click on one only selects it). **Alt+drag inside a viewport slides its drawing** without moving the frame (to bring the bottom shelves into a detail). The arrow keys nudge the selection by the grid (Shift: ten times), Delete removes it, Ctrl+A selects everything, Esc clears the selection.
- **View**: drag a rectangle to place a viewport. The pane's **Shows** picks what it shows first: a scene (its camera, hidden tags, style and section planes), a standard view (Front, Back, Left, Right, Top (plan), Bottom, Isometric) or the 3D view's camera. A new viewport takes the largest standard scale its drawing fits at, and a caption ("FRONT ELEVATION 1:12").
- **Dim**: pick the kind in the pane (Horizontal, Vertical, Aligned, Angle, Radius, Diameter), click two points of a viewport's drawing (the dot shows the corner, end or middle it snaps to; three for an angle, the corner first, or for an arc), then click where the dimension line goes. The number is the model's true length, written as the sheet says (fractions to 1/16 in to start), whatever the viewport's scale. **A dimension on the model follows it**: placed on a solid's corners, edge midpoints or edges, it stays on them when you push/pull, move, rotate or scale the solid, so widening a 30 in cabinet to 36 in makes its dimension say 36" and stretches its line to match. An end placed on a sketch line, an imported mesh or a section cut is a fixed point and stays where you put it, and so does an end whose corner an edit took away (the pane says so when a dimension is selected); so do the ends of dimensions in sheets saved before this. Delete and redraw such a dimension to put it back on the model.
- **Label**: click what it points at (a point of a drawing, or the paper), click where the text goes, type, Enter. A leader on a drawing follows it when the viewport changes, and when it points at a solid's corner or edge, when the solid changes.
- **Table**: drag a rectangle: the cut list, live. The pane picks its title, its columns, **Decimal sizes** and **Only what is shown** (hidden parts left out).
- **Image**: a PNG or JPEG (a logo, a photo of the piece) in a frame.
- **Line** (Shift: square to the page) and **Rect**: plain marks on the paper.

**The Sheet pane** shows the selected item's properties, then the paper, then the title block, then printing and exporting.
- A **viewport**: what it shows, its **Scale** (1:1, 1:2, 1:4, 1:8, 1:12, 1:16, 1:24, 1:48, and the architect scales 3", 1 1/2", 1", 3/4", 1/2" and 1/4" = 1'-0"), its caption, hidden lines dashed, its frame, **Fit scale** (the largest scale that fits), **Recentre**, **Use 3D view**, its position and size, and its order. An isometric or perspective view is never to scale: it fits its frame and is marked NTS. A caption Sawhorse wrote follows the view and the scale; one you typed stays.
- **Paper**: the name, the size (Letter, Legal, Tabloid, A4, A3, A2 or a custom size), portrait or landscape, the units (inches or millimetres), the margins, the grid, and how dimensions write lengths (fractions to 1/8, 1/16 or 1/32 in, decimal inches, feet and inches, millimetres).
- **Title block** (bottom right): title, project, sheet ("1 of 3" by itself when left empty), scale (left empty, it is the viewports' scale when they share one, "As noted" when they differ), date (**Today's date** fills it), drawn by (left blank), revision. Untick **Show the title block** to leave it off.
- **Print and export**: **Print...** (the system print dialog at 100%, the page at the paper's size; it can also save a PDF), **PDF** (vector, the real paper size), **PDF, all sheets** (a page each), **SVG** (paper inches or millimetres), **DXF** (paper size, in the sheet's units: lines, outlines and text on a layer per kind; dimension values are text), **PNG 150 dpi** and **PNG 300 dpi**. File > Print Sheets... prints every sheet.

Every change on a sheet is one undo step, and sheets are saved in the `.shz`.

**Scales on paper.** At 1:8 a 30 in cabinet is 3 3/4 in wide on the paper; at 1/4" = 1'-0" it is 5/8 in. A dimension on it still says 30".

**The bookshelf on a sheet.** With the bookshelf above: put a section plane down its middle (with Section, hover a shelf 18 in from the end, press Right to stand the plane square to red, click), Reverse it if the wrong half is kept, go to the Right view, pick the Shop drawing style, add a scene and call it "Section A", then deactivate the cut. Switch to Sheets and add a sheet. With View, drag a tall frame on the left for the Front view (it comes in at 1:12, since 72 in at 1:8 would not fit on Letter), a wide one at the top right for Top (plan), set to 1:8, and a short one under it showing Section A, set to 1:4 and slid down to the toe kick with Alt+drag. Dimension the width (36") under the elevation, the height (72") beside it and a shelf spacing (17 1/4"); label a fixed shelf; drag a cut-list table under the section; fill in the title block. Save it as PDF: the dimensions are text in the file, at their true values.

## Plugins

Plugins add tools, panels and commands. Each runs in its own sandbox, apart from the app, and can do only what it asked for when you enabled it: read the model, change it (every change is one undo step), add to the window, keep its settings, save or open files through the usual dialogs, or reach the sites it lists. A plugin that stops responding is reported in the status line (with **Stop**) and stopped after 15 seconds; the app keeps working.

**The Plugins pane** is the puzzle-piece tab at the end of the right panel. It lists every plugin with its state (Off, Running, Not responding, Stopped, Crashed, Cannot load), what it asks for and its last error, and the panels of the plugins that are running, each in a section under its name (click the name to fold it).

- **Enable...** shows what the plugin asks for; Enable starts it, now and every time Sawhorse starts. What you allow is for those exact files: a new version, or another plugin with the same id (even one taking a sample's place, which the dialog points out), stays **Off** until you enable it again and see what it asks for. **Disable** stops it and takes its panels, commands and tools away.
- **Reload** restarts it (a plugin loaded from a folder is read again first).
- **Developer: console** shows what it logged; useful when something does not work.
- **Uninstall** removes a plugin you installed; **Remove** takes a plugin loaded for this session or from a folder off the list (the folder stays). The samples cannot be uninstalled, only disabled.
- **Open folder** (desktop) shows where it is on disk.

**Getting a plugin.** Plugins > Install from .zip... (or the button under the list) takes a plugin's .zip. On the desktop it goes into the app's plugin folder; in the browser, into this browser. In the browser, Load from .zip for this session runs one until you close the tab; on the desktop, Load unpacked folder runs one from a folder (for people writing plugins). Install only plugins you trust, and read what they ask for before enabling them.

**Using them.** A plugin's commands are in the Plugins menu under its name (with its shortcut, if it has one), its panels in the Plugins pane, and its tools in the last group of the tool rail. In a panel, Enter takes the value you typed and hands the keyboard back to the model, so the next tool key (R, P ...) picks a tool; after ticking a box, tool keys work straight away. Changes a plugin makes are undone with Ctrl+Z like your own; the status line names them ("Undone: Generate cabinet").

**The samples** come with Sawhorse, off until you enable them:

- **Cabinet generator**: type a width, height, depth, stock thickness, number of shelves, back panel and toe kick (in your units), then **Generate cabinet**: a frameless cabinet whose parts are named components (Left side, Right side, Top, Bottom, Shelf 1 ..., Back, Toe kick), so the Cut list reads it straight away. With that cabinet selected the button reads **Update selected cabinet** and replaces it in place. A 30 x 72 x 12 in cabinet with three shelves lists Side 2 (3/4 x 12 x 72), Top, Bottom and Shelf 3 (3/4 x 11 3/4 x 28 1/2), Back (1/4 x 28 1/2 x 68, sheet goods) and Toe kick.
- **Shelf pin holes**: pick **Pins** in the rail, hover a face and a column of holes shows; click to drill it. Type `count, pitch, diameter` (8, 32mm, 5mm to start). Esc clears the preview; one Ctrl+Z takes a column back. To drill a cabinet side, open the cabinet and the side first (double-click, or the Outliner's Edit inside): both sides get the holes, since the left one is the right one mirrored.
- **Model report**: a live report of the model (counts of solids, groups, components, instances, faces and edges, the overall size, the volume, board feet by material, warnings) with **Export report** to Markdown and CSV.

Writing plugins: `docs/PLUGINS.md`.

## When a cut goes all the way through

A push, or a Follow Me cut, that cuts a board in two leaves two solids. The piece that keeps the board's smallest face id keeps its name ("solid 3"); the other gets the next number. Both stay in the board's group. Inside a component used more than once, the cut makes that instance unique first, so the other instances are not cut.

## Settings

**Edit > Settings…** holds one setting today: the **render service**. Renders (and the Credits chip and the Render pane) are not part of this alpha: they appear only when a render service address is saved here and that service answers when the app starts. Without one, the line under the field says "Renders are off" and nothing about credits is shown. Nothing else in Sawhorse needs a network or an account.

## Known limits

How large a model has been tried, which SketchUp files do not open, and the hard limits: [LIMITS](https://github.com/sawhorse3d/sawhorse/blob/main/docs/LIMITS.md).

## Unsaved work

Sawhorse keeps your work in this browser as you go. If the tab closes or reloads, the status line offers **Restore unsaved work?** with Restore and Discard; Edit > Restore Unsaved Work does the same while the offer stands. Restoring brings the undo history back too. Real files (.shz) arrive with their lane.
