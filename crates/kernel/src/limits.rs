//! Resource limits (lane 16): how much one model, one import and one plugin batch may hold,
//! so that a file or a plugin cannot drive the kernel out of memory (WebAssembly has 4 GB,
//! and the kernel keeps a copy of the model per edit and per undo step) or into hours of
//! work. Each is a typed refusal before anything is committed; `docs/LIMITS.md` lists them
//! with their reasons.
//!
//! Counts are checked where the work they bound starts: an import before its soups are
//! built, a copy or array before its copies are made, a file's components before they are
//! derived, and the whole model again before every edit commits.

use crate::model::Model;

/// Solids and imported meshes in one model. Every edit validates and copies them.
pub const MAX_OBJECTS: usize = 100_000;
/// Groups and component instances in one model.
pub const MAX_NODES: usize = 100_000;
/// Component definitions in one model.
pub const MAX_DEFINITIONS: usize = 20_000;
/// Faces of all solids and meshes together (a face is about 0.5 KB with its edges).
pub const MAX_FACES: usize = 5_000_000;
/// Points of all solids and meshes together.
pub const MAX_POINTS: usize = 10_000_000;
/// Faces one imported file may bring in (all its items and component copies together):
/// building a solid takes about 2 KB per triangle at its peak.
pub const MAX_IMPORT_FACES: usize = 1_500_000;
/// Points one imported file may bring in.
pub const MAX_IMPORT_POINTS: usize = 3_000_000;
/// Items (solids or meshes) one imported file may bring in.
pub const MAX_IMPORT_ITEMS: usize = 50_000;
/// Sketch strokes one imported file may bring in (DXF lines and arcs, SVG paths): each
/// stroke is inserted into its sketch, which is checked whole.
pub const MAX_IMPORT_STROKES: usize = 5_000;
/// Points of all strokes of one import together.
pub const MAX_IMPORT_STROKE_POINTS: usize = 250_000;
/// Edges in one sketch: every edit to a sketch checks every pair of its edges.
pub const MAX_SKETCH_EDGES: usize = 20_000;
/// Pixels of one texture image (4096 x 4096): the browser decodes it to 4 bytes a pixel
/// and keeps mipmaps.
pub const MAX_TEXTURE_PIXELS: u64 = 4096 * 4096;
/// Bytes of all texture images in one model together.
pub const MAX_TEXTURE_TOTAL: usize = 256 * 1024 * 1024;
/// Pixels of all texture images in one model together (128 million: eight images of
/// 4096 x 4096). A PNG of one colour is a few kilobytes at any size, so the bytes do not
/// bound what the browser decodes (4 bytes a pixel, and mipmaps): sixteen legal 4096 x 4096
/// images in a file of a few hundred kilobytes were a gigabyte decoded.
pub const MAX_TEXTURE_PIXELS_TOTAL: u64 = 128 * 1024 * 1024;
/// Faces one solid may have: a solid is checked whole (self-intersection) whenever it
/// changes; a shell with more faces stays a mesh, with a note.
pub const MAX_SOLID_FACES: usize = 250_000;

// Lane 16b: the model's other entities. Each is a few dozen bytes in a file, so a file's
// size bounds none of them: 2,000,000 guides in a 5 MB `.shz` took the tab down, and
// 1,000,000 materials took the browser to 6.2 GB. Readers count them before parsing them
// (`sawhorse_shz` counts what `scene.json` declares as it inflates, and stops at the first
// past its limit), imports and plugin batches before building, and every edit again
// before it commits (`check_model`).

/// Guide lines and points in one model: each is drawn across the whole view and offered as
/// an inference on every pointer move.
pub const MAX_GUIDES: usize = 50_000;
/// Dimensions in one model: each is laid out (leaders, text) whenever the camera moves.
pub const MAX_DIMENSIONS: usize = 50_000;
/// Tags in one model; also the tags with a colour, and the tags one scene hides. The Tags
/// pane lists each, and every scene holds the visibility of each.
pub const MAX_TAGS: usize = 10_000;
/// Materials in one model: the Materials pane draws a swatch for each, and each is a
/// material on the GPU while a face shows it. At most `looks::MAX_TEXTURES` (512) of them
/// have a texture image of the model's own.
pub const MAX_MATERIALS: usize = 4_096;
/// Saved scenes (views) in one model: each is a tab with its own camera, style and hidden tags.
pub const MAX_SCENES: usize = 1_000;
/// Section planes in one model: each active one adds a clipping set to every material it reaches.
pub const MAX_SECTIONS: usize = 1_000;
/// Drawing sheets in one model, and items on one sheet (each item a drawing of the model):
/// the limits `crate::sheets` has kept since lane 12, stated here with the others.
pub const MAX_SHEETS: usize = crate::sheets::MAX_SHEETS;
pub const MAX_SHEET_ITEMS: usize = crate::sheets::MAX_ITEMS;

#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum LimitError {
    #[error("the model would hold {0} solids and meshes; at most {MAX_OBJECTS}")]
    Objects(usize),
    #[error("the model would hold {0} groups and component instances; at most {MAX_NODES}")]
    Nodes(usize),
    #[error("the model would hold {0} component definitions; at most {MAX_DEFINITIONS}")]
    Definitions(usize),
    #[error("the model would hold {0} faces; at most {MAX_FACES}")]
    Faces(usize),
    #[error("the model would hold {0} points; at most {MAX_POINTS}")]
    Points(usize),
    #[error("a sketch would hold {0} edges; at most {MAX_SKETCH_EDGES}")]
    SketchEdges(usize),
    #[error("{what}: {count} is more than the {max} one import may bring in")]
    Import {
        what: &'static str,
        count: usize,
        max: usize,
    },
    #[error("the model's texture images would take {0} MB; at most {max} MB", max = MAX_TEXTURE_TOTAL / 1024 / 1024)]
    TextureBytes(usize),
    #[error("the model's texture images would hold {0} million pixels; at most {max} million (eight images of 4096 x 4096)", max = MAX_TEXTURE_PIXELS_TOTAL / 1024 / 1024)]
    TexturePixels(u64),
    /// Lane 16b: guides, dimensions, tags, materials, scenes, section planes, sheets.
    #[error("the model would hold {count} {what}; at most {max}")]
    Count {
        what: &'static str,
        count: usize,
        max: usize,
    },
    /// Lane 16b: a file declares more of something than a model may hold (counted as the
    /// file is read, before any of it is parsed).
    #[error("the file holds more than {max} {what}; a model holds at most {max}")]
    Declared { what: &'static str, max: usize },
    #[error("{0}")]
    Other(String),
}

/// What a model holds, counted the way the limits are stated.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Counts {
    pub objects: usize,
    pub nodes: usize,
    pub definitions: usize,
    pub faces: usize,
    pub points: usize,
    pub guides: usize,
    pub dimensions: usize,
    pub tags: usize,
    pub tag_colors: usize,
    pub materials: usize,
    pub scenes: usize,
    /// The most tags one scene hides.
    pub scene_hidden_tags: usize,
    pub sections: usize,
    pub sheets: usize,
    /// The most items on one sheet.
    pub sheet_items: usize,
}

impl Counts {
    pub fn of(m: &Model) -> Counts {
        let mut c = Counts {
            objects: m.objects.len() + m.meshes.len(),
            nodes: m.nodes.len(),
            definitions: m.defs.len(),
            faces: 0,
            points: 0,
            guides: m.guides.len(),
            dimensions: m.dimensions.len(),
            tags: m.tags.len(),
            tag_colors: m.tag_colors.len(),
            materials: m.materials.len(),
            scenes: m.scenes.len(),
            scene_hidden_tags: m
                .scenes
                .iter()
                .map(|s| s.hidden_tags.len())
                .max()
                .unwrap_or(0),
            sections: m.sections.len(),
            sheets: m.sheets.len(),
            sheet_items: m.sheets.values().map(|s| s.items.len()).max().unwrap_or(0),
        };
        for (_, o) in &m.objects {
            c.faces += o.brep.faces.len();
            c.points += o.brep.vertices.len();
        }
        for (_, x) in &m.meshes {
            c.faces += x.faces.len();
            c.points += x.positions.len();
        }
        c
    }
}

impl Counts {
    /// Raise each count to `other`'s where that is more (the peak over a run).
    pub fn raise(&mut self, other: &Counts) {
        let pairs = [
            (&mut self.objects, other.objects),
            (&mut self.nodes, other.nodes),
            (&mut self.definitions, other.definitions),
            (&mut self.faces, other.faces),
            (&mut self.points, other.points),
            (&mut self.guides, other.guides),
            (&mut self.dimensions, other.dimensions),
            (&mut self.tags, other.tags),
            (&mut self.tag_colors, other.tag_colors),
            (&mut self.materials, other.materials),
            (&mut self.scenes, other.scenes),
            (&mut self.scene_hidden_tags, other.scene_hidden_tags),
            (&mut self.sections, other.sections),
            (&mut self.sheets, other.sheets),
            (&mut self.sheet_items, other.sheet_items),
        ];
        for (a, b) in pairs {
            *a = (*a).max(b);
        }
    }

    /// The lane 16b counts with their limits: (what, count, limit).
    pub fn bounded(&self) -> [(&'static str, usize, usize); 10] {
        [
            ("guides", self.guides, MAX_GUIDES),
            ("dimensions", self.dimensions, MAX_DIMENSIONS),
            ("tags", self.tags, MAX_TAGS),
            ("tag colours", self.tag_colors, MAX_TAGS),
            ("materials", self.materials, MAX_MATERIALS),
            ("scenes", self.scenes, MAX_SCENES),
            ("tags hidden by one scene", self.scene_hidden_tags, MAX_TAGS),
            ("section planes", self.sections, MAX_SECTIONS),
            ("sheets", self.sheets, MAX_SHEETS),
            ("items on one sheet", self.sheet_items, MAX_SHEET_ITEMS),
        ]
    }
}

/// Refuse a model over any model-wide limit.
pub fn check_model(m: &Model) -> Result<(), LimitError> {
    let c = Counts::of(m);
    if c.objects > MAX_OBJECTS {
        return Err(LimitError::Objects(c.objects));
    }
    if c.nodes > MAX_NODES {
        return Err(LimitError::Nodes(c.nodes));
    }
    if c.definitions > MAX_DEFINITIONS {
        return Err(LimitError::Definitions(c.definitions));
    }
    if c.faces > MAX_FACES {
        return Err(LimitError::Faces(c.faces));
    }
    if c.points > MAX_POINTS {
        return Err(LimitError::Points(c.points));
    }
    check_counts(&c)?;
    for (_, s) in &m.sketches {
        if s.edge_count() > MAX_SKETCH_EDGES {
            return Err(LimitError::SketchEdges(s.edge_count()));
        }
    }
    check_textures(m.textures.values())
}

/// One count against its limit (lane 16b).
pub fn count(what: &'static str, count: usize, max: usize) -> Result<(), LimitError> {
    if count > max {
        Err(LimitError::Count { what, count, max })
    } else {
        Ok(())
    }
}

/// Refuse guides, dimensions, tags, materials, scenes, section planes and sheets over their
/// limits (lane 16b).
pub fn check_counts(c: &Counts) -> Result<(), LimitError> {
    for (what, n, max) in c.bounded() {
        count(what, n, max)?;
    }
    Ok(())
}

/// Refuse texture images over the model's limits on bytes and pixels in all.
pub fn check_textures<'a>(
    images: impl IntoIterator<Item = &'a crate::looks::TextureImage>,
) -> Result<(), LimitError> {
    let (mut bytes, mut pixels) = (0usize, 0u64);
    for t in images {
        bytes += t.data.len();
        pixels += u64::from(t.width) * u64::from(t.height);
    }
    if bytes > MAX_TEXTURE_TOTAL {
        return Err(LimitError::TextureBytes(bytes / 1024 / 1024));
    }
    if pixels > MAX_TEXTURE_PIXELS_TOTAL {
        return Err(LimitError::TexturePixels(pixels.div_ceil(1024 * 1024)));
    }
    Ok(())
}

/// Refuse `count` of `what` over `max` for one import.
pub fn import(what: &'static str, count: usize, max: usize) -> Result<(), LimitError> {
    if count > max {
        Err(LimitError::Import { what, count, max })
    } else {
        Ok(())
    }
}
