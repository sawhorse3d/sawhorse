//! Groups, components, the editing context, tags and scenes (lane 5).
//!
//! The model is a tree. Its inner nodes ([`Node`]) are **groups** and **component
//! instances**; its leaves are solids, imported non-solid meshes and sketches. Every leaf
//! stays in world coordinates (so picking, inference, push/pull and the exporters keep
//! working on plain solids), and records where it sits in [`Membership`]: its parent node,
//! its slot in a component definition, and its hidden / locked flags.
//!
//! - A **group** is a named container with its own editing context. Its `xform` is
//!   metadata (where the group's axes are): moving a group moves everything in it and
//!   composes the move into the group's `xform`; face ids inside do not change.
//! - A **component definition** ([`Definition`]) is shared geometry and structure in its own
//!   coordinates: template solids, meshes, nested groups and nested instances, each under a
//!   **slot** number. An **instance** is a node with a definition and a placement `xform`
//!   (definition coordinates to world). Its world solids are *derived*: each is exactly the
//!   template solid under the instance's `xform` ([`Object::mapped`]), bit for bit, which
//!   the validator checks. The derived solids keep their ids and display numbers across
//!   edits (they are matched to template members by slot).
//! - The **editing context** is the open group or instance (`None`: the model). Only what is
//!   directly inside it can be edited, moved, grouped or deleted; strokes only stick to
//!   sketches of the same context (scoped stickiness, ADR-0001). A stroke drawn on a face or
//!   sketch outside the context goes into the context's own sketch on that plane.
//!
//! **Propagation.** Every op records which containers it changed ([`Dirty`]). After the op,
//! the innermost instance above each changed container is captured back into its
//! definition (only the solids the op touched are re-read from the world; everything else
//! keeps its exact template), and every instance of that definition is re-derived. So an
//! edit inside one instance updates all of them. A push/pull or sweep that severs a solid
//! inside a shared instance first makes that instance unique ("Make Unique"), so the cut
//! never spreads to the other copies.
//!
//! **Tidying.** After every op, groups and instances left with nothing in them disappear
//! (except the open context and its ancestors), unused definitions are purged, and scenes
//! that pointed into a deleted node forget it.

use std::collections::{BTreeMap, BTreeSet};

use crate::error::{DocumentError, KernelError, StructureError, TransformError};
use crate::ids::{DefId, MeshId, NodeId, ObjectId, SketchId, from_public, to_public};
use crate::meshbuild::LooseMesh;
use crate::model::{Model, OpOutcome, SceneDef};
use crate::object::Object;
use crate::transform::Xform;

/// Where a solid, mesh or node sits in the tree.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Membership {
    /// The group or instance it is in (`None`: the model itself).
    pub parent: Option<NodeId>,
    /// Inside a component instance: the definition member it is derived from. Slots are
    /// numbered per definition and never reused.
    pub slot: Option<u32>,
    /// Hidden (the Outliner's eye): not drawn, not picked, not exported by default.
    pub hidden: bool,
    /// Locked: cannot be edited, moved, entered, exploded or deleted.
    pub locked: bool,
}

impl Membership {
    pub fn parent(&self) -> Option<u64> {
        self.parent.map(to_public)
    }
}

/// A group, or an instance of a component definition.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NodeKind {
    Group,
    Instance(DefId),
}

/// An inner node of the model's tree.
#[derive(Debug, Clone, PartialEq)]
pub struct Node {
    /// Display name (`None`: "Group", or the definition's name for an instance).
    pub name: Option<String>,
    pub kind: NodeKind,
    /// Local axes to world. For an instance: definition coordinates to world (the
    /// placement). For a group: where its axes are (metadata; its contents are in world
    /// coordinates already).
    pub xform: Xform,
    pub tag: Option<String>,
    pub member: Membership,
}

/// A component definition: what its instances share.
#[derive(Debug, Clone)]
pub struct Definition {
    pub name: String,
    /// Template members by slot, in definition coordinates.
    pub members: BTreeMap<u32, Member>,
    /// The next slot number to hand out.
    pub next_slot: u32,
}

/// One member of a definition: a solid, a mesh, a nested group or a nested instance.
#[derive(Debug, Clone)]
pub struct Member {
    /// The slot of the nested group it is in (`None`: directly in the definition).
    pub parent: Option<u32>,
    pub name: Option<String>,
    pub tag: Option<String>,
    pub hidden: bool,
    pub locked: bool,
    pub kind: MemberKind,
}

#[derive(Debug, Clone)]
pub enum MemberKind {
    /// A solid in definition coordinates (its material is part of it; its name and tag are
    /// the member's).
    Solid(Box<Object>),
    /// An imported non-solid mesh in definition coordinates.
    Mesh(Box<LooseMesh>),
    /// A nested group with its axes in definition coordinates.
    Group(Xform),
    /// A nested instance of another definition, placed in definition coordinates.
    Instance(DefId, Xform),
}

/// What an op may refer to: a solid, or a group or component instance (public ids). JSON:
/// `{"object": 3}` or `{"node": 2}`.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, serde::Serialize, serde::Deserialize,
)]
#[cfg_attr(feature = "arbitrary", derive(arbitrary::Arbitrary))]
#[serde(rename_all = "snake_case")]
pub enum Item {
    Object(u64),
    Node(u64),
}

/// What the last op changed, for propagation (transient: not hashed, not saved).
#[derive(Debug, Clone, Default)]
pub(crate) struct Dirty {
    /// Containers whose contents (or the properties of something directly in them) changed.
    pub containers: BTreeSet<NodeId>,
    /// Solids whose geometry changed.
    pub touched: BTreeSet<ObjectId>,
    /// Meshes whose geometry changed.
    pub touched_meshes: BTreeSet<MeshId>,
    /// Containers in which a solid came apart into pieces.
    pub severed: BTreeSet<NodeId>,
}

/// A world item of the tree.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum Thing {
    Node(NodeId),
    Object(ObjectId),
    Mesh(MeshId),
}

/// The tree's child lists, built once per query: per container, nodes (key order), solids
/// (creation order), meshes (import order); and the sketches.
#[derive(Debug, Default)]
pub(crate) struct Kids {
    pub things: BTreeMap<Option<NodeId>, Vec<Thing>>,
    pub sketches: BTreeMap<Option<NodeId>, Vec<SketchId>>,
}

impl Kids {
    fn of(&self, n: Option<NodeId>) -> &[Thing] {
        self.things.get(&n).map_or(&[], |v| v.as_slice())
    }
}

/// The default name a new definition gets.
pub const DEFAULT_COMPONENT_NAME: &str = "Component";

/// Tag names that mean "no tag".
pub fn is_untagged_name(t: &str) -> bool {
    let t = t.trim();
    t.is_empty() || t.eq_ignore_ascii_case("untagged")
}

fn check_name(n: &str) -> Result<(), DocumentError> {
    if n.trim().is_empty() || n.chars().count() > 64 {
        Err(DocumentError::BadName)
    } else {
        Ok(())
    }
}

fn check_tag_name(n: &str) -> Result<(), KernelError> {
    check_name(n)?;
    if is_untagged_name(n) {
        return Err(StructureError::ReservedTag.into());
    }
    Ok(())
}

fn structure(msg: impl Into<String>) -> KernelError {
    StructureError::BadStructure(msg.into()).into()
}

/// A mesh under an affine map (mirrors keep faces outward).
pub(crate) fn map_mesh(m: &LooseMesh, x: &Xform) -> Result<LooseMesh, TransformError> {
    x.check()?;
    if *x == Xform::IDENTITY {
        return Ok(m.clone());
    }
    let a = x.to_affine();
    let mut out = m.clone();
    for p in &mut out.positions {
        *p = a.transform_point3(*p);
        if !(p.is_finite() && p.abs().max_element() <= crate::tolerance::MAX_COORD) {
            return Err(TransformError::OutOfBounds);
        }
    }
    if x.determinant() < 0.0 {
        for f in &mut out.faces {
            for r in f {
                r.reverse();
            }
        }
    }
    Ok(out)
}

/// A solid as a template member: in definition coordinates, without world-only data.
fn template_solid(o: &Object, inv: &Xform) -> Result<Object, TransformError> {
    let mut t = o.mapped(inv)?;
    t.name = None;
    t.tag = None;
    t.number = 0;
    t.source = None;
    t.member = Membership::default();
    Ok(t)
}

fn template_mesh(m: &LooseMesh, inv: &Xform) -> Result<LooseMesh, TransformError> {
    let mut t = map_mesh(m, inv)?;
    t.tag = None;
    t.member = Membership::default();
    Ok(t)
}

/// Same member apart from solid / mesh geometry (which the caller tracks).
fn same_member(a: &Member, b: &Member) -> bool {
    let kind = match (&a.kind, &b.kind) {
        (MemberKind::Solid(x), MemberKind::Solid(y)) => x.material == y.material,
        (MemberKind::Mesh(x), MemberKind::Mesh(y)) => x.material == y.material && x.name == y.name,
        (MemberKind::Group(x), MemberKind::Group(y)) => x == y,
        (MemberKind::Instance(d, x), MemberKind::Instance(e, y)) => d == e && x == y,
        _ => false,
    };
    kind && a.parent == b.parent
        && a.name == b.name
        && a.tag == b.tag
        && a.hidden == b.hidden
        && a.locked == b.locked
}

/// One node of the tree as the UI sees it (`Model::graph`).
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct GraphNode {
    pub id: u64,
    /// `"group"` or `"instance"`.
    pub kind: &'static str,
    /// Its own name, if it has one.
    pub name: Option<String>,
    /// What the Outliner shows: the name, else "Group" or the definition's name.
    pub label: String,
    /// The definition's public id (instances).
    pub definition: Option<u64>,
    pub parent: Option<u64>,
    pub tag: Option<String>,
    pub hidden: bool,
    pub locked: bool,
    /// Drawn: not hidden, its tag and every ancestor shown.
    pub visible: bool,
    /// Local axes to world (instances: the placement).
    pub xform: Xform,
    /// Nested inside a component instance (part of its definition).
    pub in_component: bool,
}

/// A solid's place in the tree.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct GraphLeaf {
    pub id: u64,
    pub parent: Option<u64>,
    pub hidden: bool,
    pub locked: bool,
    pub visible: bool,
    /// Part of a component definition (inside an instance).
    pub in_component: bool,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct GraphDefinition {
    pub id: u64,
    pub name: String,
    /// World instances of it.
    pub instances: usize,
    /// Solids in it (directly or in its nested groups).
    pub solids: usize,
    /// Its size in its own axes (min, max), metres: what a cut list measures.
    pub min: [f64; 3],
    pub max: [f64; 3],
}

/// The model's tree for the UI: nodes, where every solid, mesh and sketch is, the
/// definitions, the open context and the path to it.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct Graph {
    pub context: Option<u64>,
    /// From the outermost open node to the context.
    pub path: Vec<u64>,
    pub nodes: Vec<GraphNode>,
    pub objects: Vec<GraphLeaf>,
    pub meshes: Vec<GraphLeaf>,
    /// `(sketch, parent)`.
    pub sketches: Vec<(u64, Option<u64>)>,
    pub definitions: Vec<GraphDefinition>,
    /// Tag colours (`#rrggbb`), for tags that have one.
    pub tag_colors: BTreeMap<String, String>,
}

pub fn hex_color(c: [u8; 3]) -> String {
    format!("#{:02x}{:02x}{:02x}", c[0], c[1], c[2])
}

impl Model {
    // ---- queries ---------------------------------------------------------------------

    /// Groups and instances with their public ids, in id order.
    pub fn nodes(&self) -> Vec<(u64, &Node)> {
        self.nodes.iter().map(|(k, n)| (to_public(k), n)).collect()
    }

    pub fn node(&self, id: u64) -> Option<&Node> {
        self.nodes.get(from_public::<NodeId>(id))
    }

    /// Definitions with their public ids, in id order.
    pub fn definitions(&self) -> Vec<(u64, &Definition)> {
        self.defs.iter().map(|(k, d)| (to_public(k), d)).collect()
    }

    pub fn definition(&self, id: u64) -> Option<&Definition> {
        self.defs.get(from_public::<DefId>(id))
    }

    /// The public id of a definition key.
    pub fn definition_id(&self, d: DefId) -> u64 {
        to_public(d)
    }

    /// The open group or instance (`None`: the model).
    pub fn context(&self) -> Option<u64> {
        self.context.map(to_public)
    }

    /// Tag colours by tag name (tags without one are absent).
    pub fn tag_colors(&self) -> &BTreeMap<String, [u8; 3]> {
        &self.tag_colors
    }

    /// The parent of a thing.
    pub(crate) fn parent_of(&self, t: Thing) -> Option<NodeId> {
        match t {
            Thing::Node(n) => self.nodes.get(n).and_then(|x| x.member.parent),
            Thing::Object(k) => self.objects.get(k).and_then(|x| x.member.parent),
            Thing::Mesh(k) => self.meshes.get(k).and_then(|x| x.member.parent),
        }
    }

    fn membership_mut(&mut self, t: Thing) -> Option<&mut Membership> {
        match t {
            Thing::Node(n) => self.nodes.get_mut(n).map(|x| &mut x.member),
            Thing::Object(k) => self.objects.get_mut(k).map(|x| &mut x.member),
            Thing::Mesh(k) => self.meshes.get_mut(k).map(|x| &mut x.member),
        }
    }

    fn membership(&self, t: Thing) -> Option<&Membership> {
        match t {
            Thing::Node(n) => self.nodes.get(n).map(|x| &x.member),
            Thing::Object(k) => self.objects.get(k).map(|x| &x.member),
            Thing::Mesh(k) => self.meshes.get(k).map(|x| &x.member),
        }
    }

    /// From the outermost ancestor down to `n` itself (empty for the model).
    pub(crate) fn path_of(&self, n: Option<NodeId>) -> Vec<NodeId> {
        let mut out = Vec::new();
        let mut at = n;
        while let Some(k) = at {
            if out.contains(&k) || out.len() > self.nodes.len() {
                break; // a cycle: the validator reports it
            }
            out.push(k);
            at = self.nodes.get(k).and_then(|x| x.member.parent);
        }
        out.reverse();
        out
    }

    /// The first instance from `c` upward (`c` included).
    pub(crate) fn instance_of(&self, c: NodeId) -> Option<NodeId> {
        self.path_of(Some(c))
            .into_iter()
            .rev()
            .find(|&k| matches!(self.nodes[k].kind, NodeKind::Instance(_)))
    }

    /// Every container's children (one pass over the model).
    pub(crate) fn kids(&self) -> Kids {
        let mut k = Kids::default();
        for (n, node) in &self.nodes {
            k.things
                .entry(node.member.parent)
                .or_default()
                .push(Thing::Node(n));
        }
        for &o in &self.object_order {
            k.things
                .entry(self.objects[o].member.parent)
                .or_default()
                .push(Thing::Object(o));
        }
        for &m in &self.mesh_order {
            k.things
                .entry(self.meshes[m].member.parent)
                .or_default()
                .push(Thing::Mesh(m));
        }
        for &s in &self.sketch_order {
            k.sketches
                .entry(self.sketches[s].parent)
                .or_default()
                .push(s);
        }
        k
    }

    /// What instance `i`'s definition covers in the world: everything below `i` down to (and
    /// including) nested instance nodes, but not inside them.
    pub(crate) fn scope_in(&self, kids: &Kids, i: NodeId) -> Vec<Thing> {
        let mut out = Vec::new();
        let mut stack = vec![i];
        while let Some(c) = stack.pop() {
            for &t in kids.of(Some(c)) {
                out.push(t);
                if let Thing::Node(n) = t
                    && self.nodes[n].kind == NodeKind::Group
                {
                    stack.push(n);
                }
            }
        }
        out.sort();
        out
    }

    /// Everything below `n` (nodes, solids, meshes).
    pub(crate) fn subtree_things(&self, kids: &Kids, n: NodeId) -> Vec<Thing> {
        self.subtree(kids, n).0
    }

    /// Everything below `n` (nodes, solids, meshes) and the sketches in it.
    fn subtree(&self, kids: &Kids, n: NodeId) -> (Vec<Thing>, Vec<SketchId>) {
        let (mut things, mut sketches) = (Vec::new(), Vec::new());
        let mut stack = vec![n];
        while let Some(c) = stack.pop() {
            sketches.extend(kids.sketches.get(&Some(c)).into_iter().flatten().copied());
            for &t in kids.of(Some(c)) {
                things.push(t);
                if let Thing::Node(k) = t {
                    stack.push(k);
                }
            }
        }
        (things, sketches)
    }

    /// World instances of a definition, in id order.
    pub(crate) fn instances_of(&self, d: DefId) -> Vec<NodeId> {
        self.nodes
            .iter()
            .filter(|(_, n)| n.kind == NodeKind::Instance(d))
            .map(|(k, _)| k)
            .collect()
    }

    fn tag_shown(&self, tag: Option<&str>) -> bool {
        tag.is_none_or(|t| self.tags.get(t).copied().unwrap_or(true))
    }

    /// Shown: not hidden, its tag visible, and every ancestor shown.
    pub fn node_visible(&self, id: u64) -> bool {
        self.node_shown(from_public(id))
    }

    pub(crate) fn node_shown(&self, n: NodeId) -> bool {
        self.path_of(Some(n)).iter().all(|&k| {
            let x = &self.nodes[k];
            !x.member.hidden && self.tag_shown(x.tag.as_deref())
        })
    }

    pub(crate) fn member_shown(&self, m: &Membership, tag: Option<&str>) -> bool {
        !m.hidden && self.tag_shown(tag) && m.parent.is_none_or(|p| self.node_shown(p))
    }

    /// A sketch is drawn unless it sits in a hidden group or instance.
    pub fn is_sketch_visible(&self, sketch: u64) -> bool {
        self.sketch(sketch)
            .is_some_and(|s| s.parent.is_none_or(|p| self.node_shown(p)))
    }

    /// The group or instance a sketch belongs to.
    pub fn sketch_parent(&self, sketch: u64) -> Option<u64> {
        self.sketch(sketch).and_then(|s| s.parent).map(to_public)
    }

    /// Solids whose parent is the open context (what tools may edit), in creation order.
    pub fn context_objects(&self) -> Vec<u64> {
        self.object_order
            .iter()
            .filter(|&&k| self.objects[k].member.parent == self.context)
            .map(|&k| to_public(k))
            .collect()
    }

    /// Nodes whose parent is the open context, in id order.
    pub fn context_nodes(&self) -> Vec<u64> {
        self.nodes
            .iter()
            .filter(|(_, n)| n.member.parent == self.context)
            .map(|(k, _)| to_public(k))
            .collect()
    }

    /// Sketches of the open context, in creation order.
    pub fn context_sketches(&self) -> Vec<u64> {
        self.sketch_order
            .iter()
            .filter(|&&k| self.sketches[k].parent == self.context)
            .map(|&k| to_public(k))
            .collect()
    }

    /// The display label of a node.
    pub fn node_label(&self, n: &Node) -> String {
        if let Some(name) = &n.name {
            return name.clone();
        }
        match n.kind {
            NodeKind::Group => "Group".into(),
            NodeKind::Instance(d) => self
                .defs
                .get(d)
                .map_or_else(|| DEFAULT_COMPONENT_NAME.into(), |x| x.name.clone()),
        }
    }

    /// Bounds of a definition in its own axes (min, max); zero for an empty one.
    pub fn definition_bounds(&self, d: DefId) -> ([f64; 3], [f64; 3]) {
        let mut lo = glam::DVec3::splat(f64::INFINITY);
        let mut hi = glam::DVec3::splat(f64::NEG_INFINITY);
        self.extend_bounds(d, &Xform::IDENTITY, &mut lo, &mut hi, 0);
        if lo.x > hi.x {
            return ([0.0; 3], [0.0; 3]);
        }
        (lo.to_array(), hi.to_array())
    }

    fn extend_bounds(
        &self,
        d: DefId,
        x: &Xform,
        lo: &mut glam::DVec3,
        hi: &mut glam::DVec3,
        depth: usize,
    ) {
        let Some(def) = self.defs.get(d) else { return };
        if depth > 64 {
            return;
        }
        let a = x.to_affine();
        for m in def.members.values() {
            match &m.kind {
                MemberKind::Solid(o) => {
                    for (_, v) in o.brep().vertices() {
                        let p = a.transform_point3(v.pos);
                        *lo = lo.min(p);
                        *hi = hi.max(p);
                    }
                }
                MemberKind::Mesh(mesh) => {
                    for &v in &mesh.positions {
                        let p = a.transform_point3(v);
                        *lo = lo.min(p);
                        *hi = hi.max(p);
                    }
                }
                MemberKind::Group(_) => {}
                MemberKind::Instance(e, l) => {
                    self.extend_bounds(*e, &x.after(l), lo, hi, depth + 1)
                }
            }
        }
    }

    /// The tree for the UI (see [`Graph`]).
    pub fn graph(&self) -> Graph {
        let solids_in = |d: DefId| {
            self.defs[d]
                .members
                .values()
                .filter(|m| matches!(m.kind, MemberKind::Solid(_)))
                .count()
        };
        let leaf = |id: u64, m: &Membership, visible: bool| GraphLeaf {
            id,
            parent: m.parent(),
            hidden: m.hidden,
            locked: m.locked,
            visible,
            in_component: m.slot.is_some(),
        };
        Graph {
            context: self.context(),
            path: self
                .path_of(self.context)
                .into_iter()
                .map(to_public)
                .collect(),
            nodes: self
                .nodes
                .iter()
                .map(|(k, n)| GraphNode {
                    id: to_public(k),
                    kind: match n.kind {
                        NodeKind::Group => "group",
                        NodeKind::Instance(_) => "instance",
                    },
                    name: n.name.clone(),
                    label: self.node_label(n),
                    definition: match n.kind {
                        NodeKind::Instance(d) => Some(to_public(d)),
                        NodeKind::Group => None,
                    },
                    parent: n.member.parent(),
                    tag: n.tag.clone(),
                    hidden: n.member.hidden,
                    locked: n.member.locked,
                    visible: self.node_shown(k),
                    xform: n.xform,
                    in_component: n.member.slot.is_some(),
                })
                .collect(),
            objects: self
                .objects()
                .into_iter()
                .map(|(id, o)| leaf(id, &o.member, self.is_visible(id)))
                .collect(),
            meshes: self
                .meshes()
                .into_iter()
                .map(|(id, m)| leaf(id, &m.member, self.is_mesh_visible(id)))
                .collect(),
            sketches: self
                .sketch_order
                .iter()
                .map(|&k| (to_public(k), self.sketches[k].parent.map(to_public)))
                .collect(),
            definitions: self
                .defs
                .iter()
                .map(|(k, d)| {
                    let (min, max) = self.definition_bounds(k);
                    GraphDefinition {
                        id: to_public(k),
                        name: d.name.clone(),
                        instances: self.instances_of(k).len(),
                        solids: solids_in(k),
                        min,
                        max,
                    }
                })
                .collect(),
            tag_colors: self
                .tag_colors
                .iter()
                .map(|(t, c)| (t.clone(), hex_color(*c)))
                .collect(),
        }
    }

    // ---- editing rules ---------------------------------------------------------------

    pub(crate) fn node_key(&self, id: u64) -> Result<NodeId, StructureError> {
        let k = from_public::<NodeId>(id);
        if self.nodes.contains_key(k) {
            Ok(k)
        } else {
            Err(StructureError::NoSuchNode(id))
        }
    }

    fn check_editable(&self, m: &Membership) -> Result<(), StructureError> {
        if m.parent != self.context {
            return Err(StructureError::OutsideContext);
        }
        if m.locked {
            return Err(StructureError::Locked);
        }
        Ok(())
    }

    /// A solid can be edited: directly in the open context, and not locked.
    pub(crate) fn editable_object(&self, k: ObjectId) -> Result<(), StructureError> {
        self.check_editable(&self.objects[k].member)
    }

    pub(crate) fn object_is_editable(&self, k: ObjectId) -> bool {
        self.objects
            .get(k)
            .is_some_and(|o| self.check_editable(&o.member).is_ok())
    }

    pub(crate) fn editable_mesh(&self, k: MeshId) -> Result<(), StructureError> {
        self.check_editable(&self.meshes[k].member)
    }

    /// A sketch can be drawn in or erased: it belongs to the open context.
    pub(crate) fn editable_sketch(&self, k: SketchId) -> Result<(), StructureError> {
        if self.sketches[k].parent != self.context {
            return Err(StructureError::OutsideContext);
        }
        Ok(())
    }

    pub(crate) fn sketch_is_editable(&self, k: SketchId) -> bool {
        self.sketches
            .get(k)
            .is_some_and(|s| s.parent == self.context)
    }

    pub(crate) fn editable_node(&self, n: NodeId) -> Result<(), StructureError> {
        self.check_editable(&self.nodes[n].member)
    }

    /// Record that a solid's geometry changed (and its container's contents with it).
    pub(crate) fn touch(&mut self, k: ObjectId) {
        if let Some(o) = self.objects.get(k) {
            self.dirty.touched.insert(k);
            if let Some(p) = o.member.parent {
                self.dirty.containers.insert(p);
            }
        }
    }

    /// Record that a container's contents changed.
    pub(crate) fn mark(&mut self, c: Option<NodeId>) {
        if let Some(c) = c {
            self.dirty.containers.insert(c);
        }
    }

    pub(crate) fn mark_severed(&mut self, c: Option<NodeId>) {
        if let Some(c) = c {
            self.dirty.severed.insert(c);
            self.dirty.containers.insert(c);
        }
    }

    /// Resolve items to things, refusing unknown ids and duplicates are dropped.
    fn things(&self, items: &[Item]) -> Result<Vec<Thing>, KernelError> {
        let mut out = Vec::new();
        for &it in items {
            let t = match it {
                Item::Object(id) => {
                    let k = from_public::<ObjectId>(id);
                    if !self.objects.contains_key(k) {
                        return Err(DocumentError::NoSuchObject(id).into());
                    }
                    Thing::Object(k)
                }
                Item::Node(id) => Thing::Node(self.node_key(id)?),
            };
            if !out.contains(&t) {
                out.push(t);
            }
        }
        Ok(out)
    }

    /// Items that are being regrouped: directly in the context and unlocked.
    fn editable_things(&self, items: &[Item]) -> Result<Vec<Thing>, KernelError> {
        if items.is_empty() {
            return Err(StructureError::NothingSelected.into());
        }
        let things = self.things(items)?;
        for &t in &things {
            let m = self.membership(t).expect("resolved");
            self.check_editable(m)?;
        }
        Ok(things)
    }

    // ---- ops -------------------------------------------------------------------------

    /// [`Op::SetContext`](crate::model::Op::SetContext).
    pub(crate) fn set_context(&mut self, node: Option<u64>) -> Result<OpOutcome, KernelError> {
        let k = match node {
            None => None,
            Some(id) => {
                let k = self.node_key(id)?;
                if self
                    .path_of(Some(k))
                    .iter()
                    .any(|&a| self.nodes[a].member.locked)
                {
                    return Err(StructureError::Locked.into());
                }
                Some(k)
            }
        };
        self.context = k;
        Ok(OpOutcome::ContextChanged {
            context: self.context(),
        })
    }

    /// [`Op::LeaveContext`](crate::model::Op::LeaveContext): one level up.
    pub(crate) fn leave_context(&mut self) -> Result<OpOutcome, KernelError> {
        let c = self.context.ok_or(StructureError::NothingToLeave)?;
        self.context = self.nodes[c].member.parent;
        Ok(OpOutcome::ContextChanged {
            context: self.context(),
        })
    }

    /// Make Group: the items (directly in the context) go into a new group there.
    pub(crate) fn make_group(
        &mut self,
        items: &[Item],
        name: Option<&str>,
    ) -> Result<OpOutcome, KernelError> {
        if let Some(n) = name {
            check_name(n)?;
        }
        let things = self.editable_things(items)?;
        let g = self.nodes.insert(Node {
            name: name.map(|n| n.trim().to_string()),
            kind: NodeKind::Group,
            xform: Xform::IDENTITY,
            tag: None,
            member: Membership {
                parent: self.context,
                ..Default::default()
            },
        });
        for t in things {
            self.membership_mut(t).expect("resolved").parent = Some(g);
        }
        self.mark(self.context);
        Ok(OpOutcome::GroupCreated {
            group: to_public(g),
        })
    }

    /// Make Component: the items go into a new definition with one instance (placed with
    /// the identity, so the definition's axes are the world's and nothing moves).
    ///
    /// A single group becomes the instance itself (Model › Side › Solid, never Model › Side ›
    /// Group › Solid): it keeps its id, tag and flags, its axes become the placement (so the
    /// definition is in the group's own axes), and its contents become the definition.
    pub(crate) fn make_component(
        &mut self,
        items: &[Item],
        name: &str,
    ) -> Result<OpOutcome, KernelError> {
        check_name(name)?;
        let things = self.editable_things(items)?;
        let d = self.defs.insert(Definition {
            name: name.trim().to_string(),
            members: BTreeMap::new(),
            next_slot: 1,
        });
        let i = match things.as_slice() {
            [Thing::Node(g)] if self.nodes[*g].kind == NodeKind::Group => {
                let g = *g;
                let node = &mut self.nodes[g];
                node.kind = NodeKind::Instance(d);
                node.name = None;
                g
            }
            _ => {
                let i = self.nodes.insert(Node {
                    name: None,
                    kind: NodeKind::Instance(d),
                    xform: Xform::IDENTITY,
                    tag: None,
                    member: Membership {
                        parent: self.context,
                        ..Default::default()
                    },
                });
                for &t in &things {
                    self.membership_mut(t).expect("resolved").parent = Some(i);
                }
                i
            }
        };
        // They leave whatever definition they were part of and join the new one.
        let kids = self.kids();
        for t in self.scope_in(&kids, i) {
            self.membership_mut(t).expect("in scope").slot = None;
            if let Thing::Object(k) = t {
                self.dirty.touched.insert(k);
            }
            if let Thing::Mesh(k) = t {
                self.dirty.touched_meshes.insert(k);
            }
        }
        self.capture(i)?;
        self.rederive(i)?;
        self.mark(self.context);
        Ok(OpOutcome::ComponentCreated {
            instance: to_public(i),
            definition: to_public(d),
        })
    }

    /// A definition name not used yet: "Shelf", then "Shelf (2)", "Shelf (3)"...
    fn unique_def_name(&self, base: &str) -> String {
        let stem = base
            .strip_suffix(')')
            .and_then(|b| b.rsplit_once(" ("))
            .filter(|(_, n)| n.parse::<u32>().is_ok())
            .map_or(base, |(s, _)| s)
            .to_string();
        let taken = |n: &str| self.defs.values().any(|d| d.name == n);
        if !taken(&stem) {
            return stem;
        }
        (2..)
            .map(|k| format!("{stem} ({k})"))
            .find(|n| !taken(n))
            .expect("unbounded")
    }

    /// Give instance `i` its own copy of its definition. Returns the new definition.
    fn fork(&mut self, i: NodeId) -> DefId {
        let NodeKind::Instance(d) = self.nodes[i].kind else {
            unreachable!("fork of a group")
        };
        let mut def = self.defs[d].clone();
        def.name = self.unique_def_name(&def.name);
        let nd = self.defs.insert(def);
        self.nodes[i].kind = NodeKind::Instance(nd);
        self.mark(self.nodes[i].member.parent);
        nd
    }

    /// Make Unique: this instance gets its own copy of the definition.
    pub(crate) fn make_unique(&mut self, node: u64) -> Result<OpOutcome, KernelError> {
        let i = self.node_key(node)?;
        let NodeKind::Instance(d) = self.nodes[i].kind else {
            return Err(StructureError::NotAnInstance.into());
        };
        if self.nodes[i].member.locked {
            return Err(StructureError::Locked.into());
        }
        if self.instances_of(d).len() < 2 {
            return Err(StructureError::AlreadyUnique.into());
        }
        let nd = self.fork(i);
        Ok(OpOutcome::MadeUnique {
            definition: to_public(nd),
        })
    }

    /// Explode: the node's contents move up into its parent; the node goes away. An
    /// instance's solids stay where they are, as plain solids.
    pub(crate) fn explode(&mut self, node: u64) -> Result<OpOutcome, KernelError> {
        let n = self.node_key(node)?;
        self.editable_node(n)?;
        let kids = self.kids();
        let parent = self.nodes[n].member.parent;
        if matches!(self.nodes[n].kind, NodeKind::Instance(_)) {
            for t in self.scope_in(&kids, n) {
                self.membership_mut(t).expect("in scope").slot = None;
            }
        }
        let (mut objects, mut nodes) = (Vec::new(), Vec::new());
        for &t in kids.of(Some(n)) {
            self.membership_mut(t).expect("child").parent = parent;
            match t {
                Thing::Object(k) => objects.push(to_public(k)),
                Thing::Node(k) => nodes.push(to_public(k)),
                Thing::Mesh(_) => {}
            }
        }
        for &s in kids.sketches.get(&Some(n)).into_iter().flatten() {
            self.sketches[s].parent = parent;
        }
        self.nodes.remove(n);
        self.mark(parent);
        Ok(OpOutcome::Exploded { objects, nodes })
    }

    /// Remove a thing and everything inside it (sketches in a removed node go with it).
    pub(crate) fn remove_thing(&mut self, kids: &Kids, t: Thing) {
        match t {
            Thing::Object(k) => {
                self.objects.remove(k);
                self.object_order.retain(|&o| o != k);
            }
            Thing::Mesh(k) => {
                self.meshes.remove(k);
                self.mesh_order.retain(|&m| m != k);
            }
            Thing::Node(n) => {
                let (things, sketches) = self.subtree(kids, n);
                for t in things {
                    match t {
                        Thing::Node(k) => {
                            self.nodes.remove(k);
                        }
                        other => self.remove_thing(kids, other),
                    }
                }
                for s in sketches {
                    self.sketches.remove(s);
                    self.sketch_order.retain(|&k| k != s);
                }
                self.nodes.remove(n);
            }
        }
    }

    /// Delete a group or instance with everything in it.
    pub(crate) fn delete_node(&mut self, node: u64) -> Result<OpOutcome, KernelError> {
        let n = self.node_key(node)?;
        self.editable_node(n)?;
        let parent = self.nodes[n].member.parent;
        let kids = self.kids();
        self.remove_thing(&kids, Thing::Node(n));
        self.mark(parent);
        Ok(OpOutcome::NodeDeleted)
    }

    /// Rename a solid, group or instance (`None` clears the name).
    pub(crate) fn rename(
        &mut self,
        item: Item,
        name: Option<&str>,
    ) -> Result<OpOutcome, KernelError> {
        if let Some(n) = name {
            check_name(n)?;
        }
        let name = name.map(|n| n.trim().to_string());
        let t = self.things(&[item])?[0];
        match t {
            Thing::Object(k) => self.objects[k].name = name,
            Thing::Node(k) => self.nodes[k].name = name,
            Thing::Mesh(_) => unreachable!("items are objects or nodes"),
        }
        self.mark(self.parent_of(t));
        Ok(OpOutcome::Renamed)
    }

    pub(crate) fn rename_definition(
        &mut self,
        definition: u64,
        name: &str,
    ) -> Result<OpOutcome, KernelError> {
        check_name(name)?;
        let d = self
            .defs
            .get_mut(from_public::<DefId>(definition))
            .ok_or(StructureError::NoSuchDefinition(definition))?;
        d.name = name.trim().to_string();
        Ok(OpOutcome::Renamed)
    }

    /// Hide / show, or lock / unlock, items anywhere in the tree.
    pub(crate) fn set_flag(
        &mut self,
        items: &[Item],
        locked: bool,
        value: bool,
    ) -> Result<OpOutcome, KernelError> {
        if items.is_empty() {
            return Err(StructureError::NothingSelected.into());
        }
        let things = self.things(items)?;
        let path = self.path_of(self.context);
        for &t in &things {
            if locked && value && matches!(t, Thing::Node(n) if path.contains(&n)) {
                return Err(StructureError::OnContextPath.into());
            }
        }
        for t in things {
            let m = self.membership_mut(t).expect("resolved");
            if locked {
                m.locked = value;
            } else {
                m.hidden = value;
            }
            let p = m.parent;
            self.mark(p);
        }
        Ok(if locked {
            OpOutcome::LockSet
        } else {
            OpOutcome::VisibilitySet
        })
    }

    /// Put items (solids, groups, instances) on a tag, or take them off it.
    pub(crate) fn tag_items(
        &mut self,
        items: &[Item],
        tag: Option<&str>,
    ) -> Result<OpOutcome, KernelError> {
        if items.is_empty() {
            return Err(StructureError::NothingSelected.into());
        }
        let tag = match tag {
            Some(t) if is_untagged_name(t) => None,
            Some(t) => {
                check_name(t)?;
                Some(t.trim().to_string())
            }
            None => None,
        };
        let things = self.things(items)?;
        if let Some(t) = &tag {
            self.tags.entry(t.clone()).or_insert(true);
        }
        for t in things {
            match t {
                Thing::Object(k) => self.objects[k].tag = tag.clone(),
                Thing::Node(k) => self.nodes[k].tag = tag.clone(),
                Thing::Mesh(k) => self.meshes[k].tag = tag.clone(),
            }
            self.mark(self.parent_of(t));
        }
        Ok(OpOutcome::TagSet)
    }

    pub(crate) fn create_tag(
        &mut self,
        name: &str,
        color: Option<[u8; 3]>,
    ) -> Result<OpOutcome, KernelError> {
        check_tag_name(name)?;
        let name = name.trim().to_string();
        if self.tags.contains_key(&name) {
            return Err(StructureError::TagExists(name).into());
        }
        self.tags.insert(name.clone(), true);
        if let Some(c) = color {
            self.tag_colors.insert(name, c);
        }
        Ok(OpOutcome::TagSet)
    }

    /// Apply `f` to every tag reference: world items, definition members, scenes.
    fn retag(&mut self, f: &dyn Fn(&Option<String>) -> Option<Option<String>>) {
        let fix = |t: &mut Option<String>| {
            if let Some(n) = f(t) {
                *t = n;
            }
        };
        for o in self.objects.values_mut() {
            fix(&mut o.tag);
        }
        for m in self.meshes.values_mut() {
            fix(&mut m.tag);
        }
        for n in self.nodes.values_mut() {
            fix(&mut n.tag);
        }
        for d in self.defs.values_mut() {
            for m in d.members.values_mut() {
                fix(&mut m.tag);
            }
        }
    }

    pub(crate) fn rename_tag(&mut self, tag: &str, name: &str) -> Result<OpOutcome, KernelError> {
        check_tag_name(name)?;
        let name = name.trim().to_string();
        let visible = *self
            .tags
            .get(tag)
            .ok_or_else(|| StructureError::NoSuchTag(tag.to_string()))?;
        if name == tag {
            return Ok(OpOutcome::TagSet);
        }
        if self.tags.contains_key(&name) {
            return Err(StructureError::TagExists(name).into());
        }
        self.tags.remove(tag);
        self.tags.insert(name.clone(), visible);
        if let Some(c) = self.tag_colors.remove(tag) {
            self.tag_colors.insert(name.clone(), c);
        }
        let (old, new) = (tag.to_string(), name.clone());
        self.retag(&move |t| (t.as_deref() == Some(old.as_str())).then(|| Some(new.clone())));
        for sc in &mut self.scenes {
            for h in &mut sc.hidden_tags {
                if h == tag {
                    *h = name.clone();
                }
            }
        }
        Ok(OpOutcome::TagSet)
    }

    pub(crate) fn set_tag_color(
        &mut self,
        tag: &str,
        color: Option<[u8; 3]>,
    ) -> Result<OpOutcome, KernelError> {
        if !self.tags.contains_key(tag) {
            return Err(StructureError::NoSuchTag(tag.to_string()).into());
        }
        match color {
            Some(c) => self.tag_colors.insert(tag.to_string(), c),
            None => self.tag_colors.remove(tag),
        };
        Ok(OpOutcome::TagSet)
    }

    /// Delete a tag: what was on it becomes untagged (and stays where it is).
    pub(crate) fn delete_tag(&mut self, tag: &str) -> Result<OpOutcome, KernelError> {
        if self.tags.remove(tag).is_none() {
            return Err(StructureError::NoSuchTag(tag.to_string()).into());
        }
        self.tag_colors.remove(tag);
        let old = tag.to_string();
        self.retag(&move |t| (t.as_deref() == Some(old.as_str())).then_some(None));
        for sc in &mut self.scenes {
            sc.hidden_tags.retain(|h| h != tag);
        }
        Ok(OpOutcome::TagSet)
    }

    fn check_scene(&self, sc: &SceneDef) -> Result<(), KernelError> {
        check_name(&sc.name)?;
        let finite = sc
            .eye
            .iter()
            .chain(&sc.target)
            .chain(&sc.up)
            .all(|v| v.is_finite())
            && sc.fov.is_finite();
        if !finite {
            return Err(StructureError::BadScene.into());
        }
        if let Some(c) = sc.context {
            self.node_key(c)?;
        }
        Ok(())
    }

    pub(crate) fn add_scene(&mut self, scene: &SceneDef) -> Result<OpOutcome, KernelError> {
        self.check_scene(scene)?;
        let mut sc = scene.clone();
        sc.name = sc.name.trim().to_string();
        if self.scenes.iter().any(|s| s.name == sc.name) {
            return Err(StructureError::SceneExists(sc.name).into());
        }
        self.scenes.push(sc);
        Ok(OpOutcome::SceneSet)
    }

    pub(crate) fn update_scene(
        &mut self,
        name: &str,
        scene: &SceneDef,
    ) -> Result<OpOutcome, KernelError> {
        self.check_scene(scene)?;
        let k = self
            .scenes
            .iter()
            .position(|s| s.name == name)
            .ok_or_else(|| StructureError::NoSuchScene(name.to_string()))?;
        let mut sc = scene.clone();
        sc.name = sc.name.trim().to_string();
        if sc.name != name && self.scenes.iter().any(|s| s.name == sc.name) {
            return Err(StructureError::SceneExists(sc.name).into());
        }
        self.scenes[k] = sc;
        Ok(OpOutcome::SceneSet)
    }

    pub(crate) fn delete_scene(&mut self, name: &str) -> Result<OpOutcome, KernelError> {
        let k = self
            .scenes
            .iter()
            .position(|s| s.name == name)
            .ok_or_else(|| StructureError::NoSuchScene(name.to_string()))?;
        self.scenes.remove(k);
        Ok(OpOutcome::SceneSet)
    }

    /// Switch to a scene: its tag visibility (tags it does not list as hidden are shown)
    /// and its editing context. The camera is the UI's.
    pub(crate) fn activate_scene(&mut self, name: &str) -> Result<OpOutcome, KernelError> {
        let sc = self
            .scenes
            .iter()
            .find(|s| s.name == name)
            .cloned()
            .ok_or_else(|| StructureError::NoSuchScene(name.to_string()))?;
        for (t, v) in self.tags.iter_mut() {
            *v = !sc.hidden_tags.contains(t);
        }
        self.context = sc
            .context
            .map(from_public::<NodeId>)
            .filter(|&k| self.nodes.contains_key(k))
            .filter(|&k| {
                !self
                    .path_of(Some(k))
                    .iter()
                    .any(|&a| self.nodes[a].member.locked)
            });
        Ok(OpOutcome::SceneActivated {
            context: self.context(),
        })
    }

    // ---- transforms and copies of nodes ----------------------------------------------

    /// Move / rotate / scale a node as a unit: its `xform` composes the map; a group's
    /// contents move with it (face ids unchanged); an instance is re-derived.
    pub(crate) fn transform_node(&mut self, n: NodeId, x: &Xform) -> Result<(), KernelError> {
        let node = &mut self.nodes[n];
        node.xform = x.after(&node.xform);
        node.xform.check()?;
        let parent = node.member.parent;
        self.mark(parent);
        match self.nodes[n].kind {
            NodeKind::Instance(_) => {
                self.rederive(n)?;
                // Strokes drawn inside the instance (not part of its definition) move too.
                let kids = self.kids();
                let mut inside: Vec<NodeId> = vec![n];
                inside.extend(self.scope_in(&kids, n).into_iter().filter_map(|t| match t {
                    Thing::Node(k) if self.nodes[k].kind == NodeKind::Group => Some(k),
                    _ => None,
                }));
                for c in inside {
                    for &s in kids.sketches.get(&Some(c)).into_iter().flatten() {
                        self.sketches[s] = self.sketches[s].mapped(x)?;
                    }
                }
                Ok(())
            }
            NodeKind::Group => {
                let kids = self.kids();
                for t in kids.of(Some(n)).to_vec() {
                    match t {
                        Thing::Object(k) => {
                            let o = self.objects[k].transformed(x)?;
                            self.objects[k] = o;
                            self.touch(k);
                        }
                        Thing::Mesh(k) => {
                            let m = map_mesh(&self.meshes[k], x)?;
                            self.meshes[k] = m;
                            self.dirty.touched_meshes.insert(k);
                            self.mark(Some(n));
                        }
                        Thing::Node(k) => self.transform_node(k, x)?,
                    }
                }
                for &s in kids.sketches.get(&Some(n)).into_iter().flatten() {
                    // Strokes inside a moved group move with it.
                    self.sketches[s] = self.sketches[s].mapped(x)?;
                }
                Ok(())
            }
        }
    }

    /// A copy of node `n` under `x`, placed in `parent`. Returns the new node.
    pub(crate) fn copy_node(
        &mut self,
        n: NodeId,
        x: &Xform,
        parent: Option<NodeId>,
    ) -> Result<NodeId, KernelError> {
        let src = self.nodes[n].clone();
        let copy = self.nodes.insert(Node {
            xform: x.after(&src.xform),
            member: Membership {
                parent,
                slot: None,
                hidden: src.member.hidden,
                locked: false,
            },
            ..src.clone()
        });
        self.nodes[copy].xform.check()?;
        match src.kind {
            NodeKind::Instance(_) => self.rederive(copy)?,
            NodeKind::Group => {
                let kids = self.kids();
                for t in kids.of(Some(n)).to_vec() {
                    match t {
                        Thing::Object(k) => {
                            let mut o = self.objects[k].transformed(x)?;
                            o.source = None;
                            o.member = Membership {
                                parent: Some(copy),
                                slot: None,
                                hidden: o.member.hidden,
                                locked: false,
                            };
                            self.insert_object(o);
                        }
                        Thing::Mesh(k) => {
                            let mut m = map_mesh(&self.meshes[k], x)?;
                            m.member = Membership {
                                parent: Some(copy),
                                slot: None,
                                hidden: m.member.hidden,
                                locked: false,
                            };
                            let id = self.meshes.insert(m);
                            self.mesh_order.push(id);
                        }
                        Thing::Node(k) => {
                            self.copy_node(k, x, Some(copy))?;
                        }
                    }
                }
            }
        }
        Ok(copy)
    }

    // ---- propagation -----------------------------------------------------------------

    /// After an op: capture each changed instance into its definition and re-derive every
    /// instance of it (innermost first), making severed shared instances unique first.
    pub(crate) fn propagate(&mut self) -> Result<(), KernelError> {
        // Rounds: making an instance unique changes the definition around it, which the
        // next round captures.
        for _ in 0..=64 {
            let containers = std::mem::take(&mut self.dirty.containers);
            let severed_in = std::mem::take(&mut self.dirty.severed);
            if containers.is_empty() {
                break;
            }
            let mut todo: Vec<NodeId> = containers
                .iter()
                .filter(|c| self.nodes.contains_key(**c))
                .filter_map(|&c| self.instance_of(c))
                .collect();
            todo.sort();
            todo.dedup();
            // Innermost first: an outer capture must see its nested instances settled.
            todo.sort_by_key(|&i| std::cmp::Reverse(self.path_of(Some(i)).len()));
            let severed: BTreeSet<NodeId> = severed_in
                .iter()
                .filter(|c| self.nodes.contains_key(**c))
                .filter_map(|&c| self.instance_of(c))
                .collect();
            for i in todo {
                if !self.nodes.contains_key(i) {
                    continue;
                }
                let NodeKind::Instance(mut d) = self.nodes[i].kind else {
                    continue;
                };
                if severed.contains(&i) && self.instances_of(d).len() > 1 {
                    d = self.fork(i);
                }
                if self.capture(i)? {
                    for j in self.instances_of(d) {
                        self.rederive(j)?;
                    }
                }
            }
        }
        self.dirty = Dirty::default();
        Ok(())
    }

    /// Read instance `i`'s world contents back into its definition. Solids and meshes the
    /// op did not touch keep their exact templates; nodes keep their local placement unless
    /// they moved. Returns whether the definition changed.
    pub(crate) fn capture(&mut self, i: NodeId) -> Result<bool, KernelError> {
        let NodeKind::Instance(d) = self.nodes[i].kind else {
            return Ok(false);
        };
        let x = self.nodes[i].xform;
        let inv = x.inverse();
        inv.check()?;
        let kids = self.kids();
        let scope = self.scope_in(&kids, i);
        let mut def = self.defs[d].clone();
        let old = std::mem::take(&mut def.members);
        let mut changed = false;
        for &t in &scope {
            let m = self.membership(t).expect("in scope");
            let fresh = match m.slot {
                None => true,
                Some(s) => {
                    // A slot this definition does not have (moved in from elsewhere), or one
                    // already taken by an earlier item: a new member.
                    !old.contains_key(&s)
                        || scope
                            .iter()
                            .take_while(|&&u| u != t)
                            .any(|&u| self.membership(u).and_then(|x| x.slot) == Some(s))
                }
            };
            if fresh {
                let s = def.next_slot;
                def.next_slot += 1;
                self.membership_mut(t).expect("in scope").slot = Some(s);
                changed = true;
            }
        }
        for &t in &scope {
            let m = self.membership(t).expect("in scope").clone();
            let slot = m.slot.expect("assigned");
            let parent = match m.parent {
                Some(p) if p == i => None,
                Some(p) => Some(
                    self.nodes[p]
                        .member
                        .slot
                        .ok_or_else(|| structure("a nested group has no slot"))?,
                ),
                None => return Err(structure("a scope item has no parent")),
            };
            let prev = old.get(&slot);
            let member = match t {
                Thing::Object(k) => {
                    let o = &self.objects[k];
                    let geom = match prev.map(|p| &p.kind) {
                        Some(MemberKind::Solid(g)) if !self.dirty.touched.contains(&k) => {
                            let mut g = (**g).clone();
                            g.material = o.material.clone();
                            g
                        }
                        _ => {
                            changed = true;
                            template_solid(o, &inv)?
                        }
                    };
                    Member {
                        parent,
                        name: o.name.clone(),
                        tag: o.tag.clone(),
                        hidden: m.hidden,
                        locked: m.locked,
                        kind: MemberKind::Solid(Box::new(geom)),
                    }
                }
                Thing::Mesh(k) => {
                    let mesh = &self.meshes[k];
                    let geom = match prev.map(|p| &p.kind) {
                        Some(MemberKind::Mesh(g)) if !self.dirty.touched_meshes.contains(&k) => {
                            let mut g = (**g).clone();
                            g.material = mesh.material.clone();
                            g.name = mesh.name.clone();
                            g
                        }
                        _ => {
                            changed = true;
                            template_mesh(mesh, &inv)?
                        }
                    };
                    Member {
                        parent,
                        name: Some(mesh.name.clone()),
                        tag: mesh.tag.clone(),
                        hidden: m.hidden,
                        locked: m.locked,
                        kind: MemberKind::Mesh(Box::new(geom)),
                    }
                }
                Thing::Node(k) => {
                    let node = &self.nodes[k];
                    let local = |prev_local: Option<&Xform>| match prev_local {
                        Some(l) if x.after(l) == node.xform => *l,
                        _ => inv.after(&node.xform),
                    };
                    let kind = match (node.kind, prev.map(|p| &p.kind)) {
                        (NodeKind::Group, Some(MemberKind::Group(l))) => {
                            MemberKind::Group(local(Some(l)))
                        }
                        (NodeKind::Group, _) => MemberKind::Group(local(None)),
                        (NodeKind::Instance(e), Some(MemberKind::Instance(_, l))) => {
                            MemberKind::Instance(e, local(Some(l)))
                        }
                        (NodeKind::Instance(e), _) => MemberKind::Instance(e, local(None)),
                    };
                    Member {
                        parent,
                        name: node.name.clone(),
                        tag: node.tag.clone(),
                        hidden: m.hidden,
                        locked: m.locked,
                        kind,
                    }
                }
            };
            if !prev.is_some_and(|p| same_member(p, &member)) {
                changed = true;
            }
            def.members.insert(slot, member);
        }
        if def.members.len() != old.len() {
            changed = true;
        }
        for m in def.members.values() {
            if let MemberKind::Instance(e, _) = m.kind
                && self.def_reaches(e, d)
            {
                return Err(StructureError::Cycle.into());
            }
        }
        self.defs[d] = def;
        // Settled: a later round (an outer instance) must not read these again.
        for t in scope {
            match t {
                Thing::Object(k) => {
                    self.dirty.touched.remove(&k);
                }
                Thing::Mesh(k) => {
                    self.dirty.touched_meshes.remove(&k);
                }
                Thing::Node(_) => {}
            }
        }
        Ok(changed)
    }

    /// True when definition `from` is `target` or contains an instance of it (at any depth).
    pub(crate) fn def_reaches(&self, from: DefId, target: DefId) -> bool {
        let mut seen = BTreeSet::new();
        let mut stack = vec![from];
        while let Some(d) = stack.pop() {
            if d == target {
                return true;
            }
            if !seen.insert(d) {
                continue;
            }
            if let Some(def) = self.defs.get(d) {
                for m in def.members.values() {
                    if let MemberKind::Instance(e, _) = m.kind {
                        stack.push(e);
                    }
                }
            }
        }
        false
    }

    /// Rebuild instance `i`'s world contents from its definition, keeping the ids (and
    /// display numbers) of everything that matches a template member by slot.
    pub(crate) fn rederive(&mut self, i: NodeId) -> Result<(), KernelError> {
        self.rederive_at(i, 0)
    }

    fn rederive_at(&mut self, i: NodeId, depth: usize) -> Result<(), KernelError> {
        if depth > 64 {
            return Err(StructureError::Cycle.into());
        }
        let NodeKind::Instance(d) = self.nodes[i].kind else {
            return Ok(());
        };
        let x = self.nodes[i].xform;
        let def = self
            .defs
            .get(d)
            .ok_or(StructureError::NoSuchDefinition(to_public(d)))?
            .clone();
        let kids = self.kids();
        let scope = self.scope_in(&kids, i);
        let fits = |m: &Member, t: Thing| match (&m.kind, t) {
            (MemberKind::Solid(_), Thing::Object(_)) | (MemberKind::Mesh(_), Thing::Mesh(_)) => {
                true
            }
            (MemberKind::Group(_), Thing::Node(n)) => self.nodes[n].kind == NodeKind::Group,
            (MemberKind::Instance(..), Thing::Node(n)) => {
                matches!(self.nodes[n].kind, NodeKind::Instance(_))
            }
            _ => false,
        };
        let mut by_slot: BTreeMap<u32, Thing> = BTreeMap::new();
        let mut stale: Vec<Thing> = Vec::new();
        for t in scope {
            match self.membership(t).and_then(|m| m.slot) {
                Some(s)
                    if !by_slot.contains_key(&s)
                        && def.members.get(&s).is_some_and(|m| fits(m, t)) =>
                {
                    by_slot.insert(s, t);
                }
                _ => stale.push(t),
            }
        }
        let invalid = |e: String| -> KernelError { StructureError::InstanceInvalid(e).into() };
        let mut nested = Vec::new();
        for (&s, m) in &def.members {
            let member = Membership {
                parent: Some(i),
                slot: Some(s),
                hidden: m.hidden,
                locked: m.locked,
            };
            match &m.kind {
                MemberKind::Solid(g) => {
                    let mut o = g.mapped(&x).map_err(|e| invalid(e.to_string()))?;
                    o.name = m.name.clone();
                    o.tag = m.tag.clone();
                    o.source = None;
                    o.member = member;
                    match by_slot.get(&s) {
                        Some(&Thing::Object(k)) => {
                            o.number = self.objects[k].number;
                            self.objects[k] = o;
                        }
                        _ => {
                            let k = self.insert_object(o);
                            by_slot.insert(s, Thing::Object(k));
                        }
                    }
                }
                MemberKind::Mesh(g) => {
                    let mut mesh = map_mesh(g, &x).map_err(|e| invalid(e.to_string()))?;
                    if let Some(n) = &m.name {
                        mesh.name = n.clone();
                    }
                    mesh.tag = m.tag.clone();
                    mesh.member = member;
                    match by_slot.get(&s) {
                        Some(&Thing::Mesh(k)) => self.meshes[k] = mesh,
                        _ => {
                            let k = self.meshes.insert(mesh);
                            self.mesh_order.push(k);
                            by_slot.insert(s, Thing::Mesh(k));
                        }
                    }
                }
                MemberKind::Group(l) | MemberKind::Instance(_, l) => {
                    let kind = match m.kind {
                        MemberKind::Instance(e, _) => NodeKind::Instance(e),
                        _ => NodeKind::Group,
                    };
                    let xform = x.after(l);
                    xform.check().map_err(|e| invalid(e.to_string()))?;
                    let node = Node {
                        name: m.name.clone(),
                        kind,
                        xform,
                        tag: m.tag.clone(),
                        member,
                    };
                    let k = match by_slot.get(&s) {
                        Some(&Thing::Node(k)) => {
                            self.nodes[k] = node;
                            k
                        }
                        _ => {
                            let k = self.nodes.insert(node);
                            by_slot.insert(s, Thing::Node(k));
                            k
                        }
                    };
                    if matches!(kind, NodeKind::Instance(_)) {
                        nested.push(k);
                    }
                }
            }
        }
        // Parents, now that every member has a world item.
        for (&s, m) in &def.members {
            let p = match m.parent {
                None => i,
                Some(ps) => match by_slot.get(&ps) {
                    Some(&Thing::Node(k)) => k,
                    _ => return Err(structure(format!("member {s} is in a missing group"))),
                },
            };
            let t = by_slot[&s];
            self.membership_mut(t).expect("made").parent = Some(p);
        }
        // What the definition no longer has (its kept contents were moved out above).
        if !stale.is_empty() {
            let kids = self.kids();
            for t in stale {
                if let Thing::Node(n) = t {
                    // Strokes drawn in a vanished nested group stay with the instance.
                    for &sk in kids.sketches.get(&Some(n)).into_iter().flatten() {
                        self.sketches[sk].parent = Some(i);
                    }
                }
                self.remove_thing(&kids, t);
            }
        }
        for k in nested {
            self.rederive_at(k, depth + 1)?;
        }
        // Every derived solid must be a valid solid (a copy scaled so the change folds it,
        // for example): refuse the edit with a typed error rather than commit it.
        for &t in by_slot.values() {
            if let Thing::Object(k) = t {
                self.objects[k]
                    .validate()
                    .map_err(|e| invalid(crate::transform::typed(&e).to_string()))?;
            }
        }
        Ok(())
    }

    /// The definition and slot a node inside an instance stands for.
    fn member_key(&self, n: NodeId) -> Option<(DefId, u32)> {
        let slot = self.nodes[n].member.slot?;
        let i = self.instance_of(self.nodes[n].member.parent?)?;
        match self.nodes[i].kind {
            NodeKind::Instance(d) => Some((d, slot)),
            NodeKind::Group => None,
        }
    }

    /// Remove empty groups and instances (not the open context or its ancestors, nor their
    /// counterparts in other copies of a component), purge unused definitions, forget scene
    /// contexts that are gone. Propagates what it changed.
    pub(crate) fn tidy(&mut self) -> Result<(), KernelError> {
        loop {
            let kids = self.kids();
            let path = self.path_of(self.context);
            // An open group inside a component is the same member in every copy: its empty
            // counterparts elsewhere stay too (removing one would remove it from the
            // definition, and so the open one).
            let open: BTreeSet<(DefId, u32)> =
                path.iter().filter_map(|&k| self.member_key(k)).collect();
            let empty: Vec<NodeId> = self
                .nodes
                .keys()
                .filter(|k| !path.contains(k))
                .filter(|&k| self.member_key(k).is_none_or(|m| !open.contains(&m)))
                .filter(|k| kids.of(Some(*k)).is_empty())
                .filter(|k| {
                    kids.sketches
                        .get(&Some(*k))
                        .is_none_or(|v| v.iter().all(|&s| self.sketches[s].edge_count() == 0))
                })
                .collect();
            if empty.is_empty() {
                break;
            }
            for n in empty {
                let parent = self.nodes[n].member.parent;
                self.remove_thing(&kids, Thing::Node(n));
                self.mark(parent);
            }
        }
        self.propagate()?;
        // Definitions reachable from the world.
        let mut used = BTreeSet::new();
        let mut stack: Vec<DefId> = self
            .nodes
            .values()
            .filter_map(|n| match n.kind {
                NodeKind::Instance(d) => Some(d),
                NodeKind::Group => None,
            })
            .collect();
        while let Some(d) = stack.pop() {
            if !used.insert(d) {
                continue;
            }
            if let Some(def) = self.defs.get(d) {
                for m in def.members.values() {
                    if let MemberKind::Instance(e, _) = m.kind {
                        stack.push(e);
                    }
                }
            }
        }
        self.defs.retain(|k, _| used.contains(&k));
        let nodes = &self.nodes;
        for sc in &mut self.scenes {
            if sc
                .context
                .is_some_and(|c| !nodes.contains_key(from_public::<NodeId>(c)))
            {
                sc.context = None;
            }
        }
        Ok(())
    }

    // ---- validation ------------------------------------------------------------------

    /// The tree's rules: parents exist and form no cycle; instances name live definitions
    /// and hold exactly their definition's members, each derived bit for bit; tags are
    /// listed; the context is live and unlocked; definitions are acyclic and all used.
    pub(crate) fn check_structure(&self) -> Result<(), String> {
        for (k, n) in &self.nodes {
            if let Some(p) = n.member.parent
                && !self.nodes.contains_key(p)
            {
                return Err("a node's parent is missing".into());
            }
            let path = self.path_of(Some(k));
            if path
                .first()
                .and_then(|&r| self.nodes[r].member.parent)
                .is_some()
            {
                return Err("the node tree has a cycle".into());
            }
            if let NodeKind::Instance(d) = n.kind
                && !self.defs.contains_key(d)
            {
                return Err("an instance's definition is missing".into());
            }
            if n.tag.as_deref().is_some_and(|t| !self.tags.contains_key(t)) {
                return Err("a node's tag is not listed".into());
            }
            if n.xform.check().is_err() {
                return Err("a node's placement is not a valid transform".into());
            }
        }
        let live = |p: &Option<NodeId>| p.is_none_or(|p| self.nodes.contains_key(p));
        for o in self.objects.values() {
            if !live(&o.member.parent) {
                return Err("a solid's parent is missing".into());
            }
        }
        for m in self.meshes.values() {
            if !live(&m.member.parent) {
                return Err("a mesh's parent is missing".into());
            }
        }
        for s in self.sketches.values() {
            if !live(&s.parent) {
                return Err("a sketch's parent is missing".into());
            }
        }
        if let Some(c) = self.context {
            if !self.nodes.contains_key(c) {
                return Err("the editing context is missing".into());
            }
            if self
                .path_of(Some(c))
                .iter()
                .any(|&a| self.nodes[a].member.locked)
            {
                return Err("the editing context is locked".into());
            }
        }
        for t in self.tag_colors.keys() {
            if !self.tags.contains_key(t) {
                return Err("a tag colour names a missing tag".into());
            }
        }
        for (d, def) in &self.defs {
            if self.instances_of(d).is_empty()
                && !self.defs.values().any(|other| {
                    other
                        .members
                        .values()
                        .any(|m| matches!(m.kind, MemberKind::Instance(e, _) if e == d))
                })
            {
                return Err("an unused definition was not purged".into());
            }
            for (&s, m) in &def.members {
                if s >= def.next_slot {
                    return Err("a definition slot is at or above its counter".into());
                }
                if let Some(p) = m.parent
                    && !matches!(
                        def.members.get(&p).map(|x| &x.kind),
                        Some(MemberKind::Group(_))
                    )
                {
                    return Err("a member's parent is not a group of the definition".into());
                }
                if m.tag.as_deref().is_some_and(|t| !self.tags.contains_key(t)) {
                    return Err("a member's tag is not listed".into());
                }
                match &m.kind {
                    MemberKind::Instance(e, l) => {
                        if !self.defs.contains_key(*e) {
                            return Err("a nested instance's definition is missing".into());
                        }
                        if self.def_reaches(*e, d) {
                            return Err("a definition contains itself".into());
                        }
                        if l.check().is_err() {
                            return Err("a nested placement is not a valid transform".into());
                        }
                    }
                    MemberKind::Group(l) if l.check().is_err() => {
                        return Err("a nested group's axes are not a valid transform".into());
                    }
                    MemberKind::Solid(o)
                        if o.material()
                            .is_some_and(|x| !self.materials.contains_key(x)) =>
                    {
                        return Err("a member's material is not defined".into());
                    }
                    _ => {}
                }
            }
        }
        // Every instance holds exactly its definition, derived bit for bit.
        let kids = self.kids();
        let mut slotted = BTreeSet::new();
        for (i, n) in &self.nodes {
            let NodeKind::Instance(d) = n.kind else {
                continue;
            };
            let def = &self.defs[d];
            let scope = self.scope_in(&kids, i);
            if scope.len() != def.members.len() {
                return Err(format!(
                    "an instance holds {} items, its definition {}",
                    scope.len(),
                    def.members.len()
                ));
            }
            let slot_node: BTreeMap<u32, NodeId> = scope
                .iter()
                .filter_map(|&t| match t {
                    Thing::Node(k) => self.nodes[k].member.slot.map(|s| (s, k)),
                    _ => None,
                })
                .collect();
            let mut seen = BTreeSet::new();
            for t in scope {
                slotted.insert(t);
                let m = self.membership(t).expect("in scope");
                let Some(s) = m.slot else {
                    return Err("an item in an instance has no slot".into());
                };
                if !seen.insert(s) {
                    return Err("two items of an instance share a slot".into());
                }
                let Some(member) = def.members.get(&s) else {
                    return Err("an item's slot is not in its definition".into());
                };
                let want_parent = match member.parent {
                    None => Some(i),
                    Some(ps) => slot_node.get(&ps).copied(),
                };
                if m.parent != want_parent || m.hidden != member.hidden || m.locked != member.locked
                {
                    return Err("an instance item does not match its definition member".into());
                }
                match (t, &member.kind) {
                    (Thing::Object(k), MemberKind::Solid(g)) => {
                        let o = &self.objects[k];
                        let want = g.mapped(&n.xform).map_err(|e| e.to_string())?;
                        if crate::hash::object_hash(o) != crate::hash::object_hash(&want)
                            || o.next_face_id != want.next_face_id
                            || o.material != g.material
                            || o.name != member.name
                            || o.tag != member.tag
                        {
                            return Err("an instance's solid is not its definition's".into());
                        }
                    }
                    (Thing::Mesh(k), MemberKind::Mesh(g)) => {
                        let mesh = &self.meshes[k];
                        let want = map_mesh(g, &n.xform).map_err(|e| e.to_string())?;
                        if mesh.positions != want.positions || mesh.faces != want.faces {
                            return Err("an instance's mesh is not its definition's".into());
                        }
                    }
                    (Thing::Node(k), MemberKind::Group(l)) => {
                        let node = &self.nodes[k];
                        if node.kind != NodeKind::Group || node.xform != n.xform.after(l) {
                            return Err("an instance's group is not its definition's".into());
                        }
                    }
                    (Thing::Node(k), MemberKind::Instance(e, l)) => {
                        let node = &self.nodes[k];
                        if node.kind != NodeKind::Instance(*e) || node.xform != n.xform.after(l) {
                            return Err(
                                "an instance's nested instance is not its definition's".into()
                            );
                        }
                    }
                    _ => return Err("an instance item is the wrong kind".into()),
                }
            }
        }
        // Outside instances, nothing carries a slot.
        for (k, n) in &self.nodes {
            if n.member.slot.is_some() && !slotted.contains(&Thing::Node(k)) {
                return Err("a node outside any instance has a slot".into());
            }
        }
        for (&k, o) in self.object_order.iter().map(|k| (k, &self.objects[*k])) {
            if o.member.slot.is_some() && !slotted.contains(&Thing::Object(k)) {
                return Err("a solid outside any instance has a slot".into());
            }
        }
        for &k in &self.mesh_order {
            if self.meshes[k].member.slot.is_some() && !slotted.contains(&Thing::Mesh(k)) {
                return Err("a mesh outside any instance has a slot".into());
            }
        }
        for sc in &self.scenes {
            if sc
                .context
                .is_some_and(|c| !self.nodes.contains_key(from_public::<NodeId>(c)))
            {
                return Err("a scene names a missing context".into());
            }
        }
        Ok(())
    }
}
