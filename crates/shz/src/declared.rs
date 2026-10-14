//! What `scene.json` declares, counted as it is inflated (lane 16b).
//!
//! Before lane 16b the reader parsed `scene.json` whole and the kernel counted what it held
//! afterwards. Guides, dimensions, tags, materials and scenes are a few dozen bytes each in
//! the file, so a 5 MB `.shz` could declare 2,000,000 guides: parsing them built
//! 2,000,000 guides (and the app drew them) before any limit was asked, and the tab died.
//!
//! This pass reads the format and version (what the old head pass read) and counts every
//! list and map the model's limits bound, without building any of it (each element is
//! skipped as it is read). It stops at the first element past a limit, so such a file is
//! refused after inflating and skipping as many elements as a model may hold, and the rest
//! of it is never inflated.

use std::cell::Cell;
use std::fmt;
use std::io::Read;

use sawhorse_kernel::limits::{self as l, LimitError};
use serde::de::{self, DeserializeSeed, Deserializer, IgnoredAny, MapAccess, SeqAccess, Visitor};

/// The two fields a reader checks before parsing the rest.
#[derive(Debug, Default)]
pub(crate) struct Head {
    pub format: Option<String>,
    pub version: Option<serde_json::Number>,
}

/// Why the pass stopped.
#[derive(Debug)]
pub(crate) enum Stop {
    /// More of something than a model may hold.
    Limit(LimitError),
    /// Not JSON, or not an object at the top.
    Json(serde_json::Error),
}

/// Read `scene.json` from `r` far enough to know its head and that nothing it lists is past
/// a limit (to its end, unless something is).
pub(crate) fn check(r: impl Read) -> Result<Head, Stop> {
    let over = Cell::new(None);
    let mut de =
        serde_json::Deserializer::from_reader(std::io::BufReader::with_capacity(64 * 1024, r));
    let head = de.deserialize_map(Top { over: &over });
    match (head, over.into_inner()) {
        (_, Some(e)) => Err(Stop::Limit(e)),
        (Ok(h), None) => Ok(h),
        (Err(e), None) => Err(Stop::Json(e)),
    }
}

/// What a top-level key holds, and its limit.
fn limit_of(key: &str) -> Option<(&'static str, usize, Inner)> {
    Some(match key {
        "guides" => ("guides", l::MAX_GUIDES, Inner::Skip),
        "dimensions" => ("dimensions", l::MAX_DIMENSIONS, Inner::Skip),
        "tags" => ("tags", l::MAX_TAGS, Inner::Skip),
        "tag_colors" => ("tag colours", l::MAX_TAGS, Inner::Skip),
        "materials" => ("materials", l::MAX_MATERIALS, Inner::Skip),
        "scenes" => ("scenes", l::MAX_SCENES, Inner::Scene),
        "sections" => ("section planes", l::MAX_SECTIONS, Inner::Skip),
        "sheets" => ("sheets", l::MAX_SHEETS, Inner::Sheet),
        "textures" => (
            "texture images",
            sawhorse_kernel::looks::MAX_TEXTURES,
            Inner::Skip,
        ),
        "objects" => ("solids", l::MAX_OBJECTS, Inner::Skip),
        "meshes" => ("meshes", l::MAX_OBJECTS, Inner::Skip),
        "nodes" => ("groups and component instances", l::MAX_NODES, Inner::Skip),
        "definitions" => ("component definitions", l::MAX_DEFINITIONS, Inner::Skip),
        _ => return None,
    })
}

/// What one element of a counted list holds that is counted too.
#[derive(Clone, Copy)]
enum Inner {
    Skip,
    /// A scene: the tags it hides.
    Scene,
    /// A sheet: its items.
    Sheet,
}

struct Top<'a> {
    over: &'a Cell<Option<LimitError>>,
}

impl<'de> Visitor<'de> for Top<'_> {
    type Value = Head;
    fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.write_str("a JSON object")
    }
    fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Head, A::Error> {
        let mut head = Head::default();
        while let Some(key) = map.next_key::<Key>()? {
            match key.0.as_str() {
                "format" => head.format = map.next_value::<Option<String>>()?,
                "version" => head.version = map.next_value::<Option<serde_json::Number>>()?,
                k => match limit_of(k) {
                    Some((what, max, inner)) => map.next_value_seed(Count {
                        what,
                        max,
                        inner,
                        over: self.over,
                    })?,
                    None => {
                        map.next_value::<IgnoredAny>()?;
                    }
                },
            }
        }
        Ok(head)
    }
}

/// A key, kept only when it is short (every key the format has is; a long one is skipped).
struct Key(String);

impl<'de> de::Deserialize<'de> for Key {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Key, D::Error> {
        struct V;
        impl Visitor<'_> for V {
            type Value = Key;
            fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
                f.write_str("a key")
            }
            fn visit_str<E: de::Error>(self, v: &str) -> Result<Key, E> {
                Ok(Key(if v.len() <= 64 {
                    v.to_string()
                } else {
                    String::new()
                }))
            }
        }
        d.deserialize_str(V)
    }
}

/// A list (or a map) counted against its limit, each element skipped (or, for scenes and
/// sheets, looked into for the list inside it that is limited too).
struct Count<'a> {
    what: &'static str,
    max: usize,
    inner: Inner,
    over: &'a Cell<Option<LimitError>>,
}

impl Count<'_> {
    fn past<E: de::Error>(&self) -> E {
        self.over.set(Some(LimitError::Declared {
            what: self.what,
            max: self.max,
        }));
        E::custom(format!("more than {} {}", self.max, self.what))
    }
}

impl<'de> DeserializeSeed<'de> for Count<'_> {
    type Value = ();
    fn deserialize<D: Deserializer<'de>>(self, d: D) -> Result<(), D::Error> {
        d.deserialize_any(self)
    }
}

impl<'de> Visitor<'de> for Count<'_> {
    type Value = ();
    fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "a list of {}", self.what)
    }
    fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<(), A::Error> {
        let mut n = 0usize;
        loop {
            let more = match self.inner {
                Inner::Skip => seq.next_element::<IgnoredAny>()?.is_some(),
                Inner::Scene => seq
                    .next_element_seed(Within {
                        key: "hidden_tags",
                        what: "tags hidden by one scene",
                        max: l::MAX_TAGS,
                        over: self.over,
                    })?
                    .is_some(),
                Inner::Sheet => seq
                    .next_element_seed(Within {
                        key: "items",
                        what: "items on one sheet",
                        max: l::MAX_SHEET_ITEMS,
                        over: self.over,
                    })?
                    .is_some(),
            };
            if !more {
                return Ok(());
            }
            n += 1;
            if n > self.max {
                return Err(self.past());
            }
        }
    }
    fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<(), A::Error> {
        let mut n = 0usize;
        while map.next_entry::<IgnoredAny, IgnoredAny>()?.is_some() {
            n += 1;
            if n > self.max {
                return Err(self.past());
            }
        }
        Ok(())
    }
    // Anything else is not a list: the typed parse says what is wrong with it.
    fn visit_unit<E: de::Error>(self) -> Result<(), E> {
        Ok(())
    }
    fn visit_bool<E: de::Error>(self, _: bool) -> Result<(), E> {
        Ok(())
    }
    fn visit_i64<E: de::Error>(self, _: i64) -> Result<(), E> {
        Ok(())
    }
    fn visit_u64<E: de::Error>(self, _: u64) -> Result<(), E> {
        Ok(())
    }
    fn visit_f64<E: de::Error>(self, _: f64) -> Result<(), E> {
        Ok(())
    }
    fn visit_str<E: de::Error>(self, _: &str) -> Result<(), E> {
        Ok(())
    }
}

/// One element (an object) whose list under `key` is counted against `max`.
struct Within<'a> {
    key: &'static str,
    what: &'static str,
    max: usize,
    over: &'a Cell<Option<LimitError>>,
}

impl<'de> DeserializeSeed<'de> for Within<'_> {
    type Value = ();
    fn deserialize<D: Deserializer<'de>>(self, d: D) -> Result<(), D::Error> {
        d.deserialize_any(self)
    }
}

impl<'de> Visitor<'de> for Within<'_> {
    type Value = ();
    fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.write_str("an object")
    }
    fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<(), A::Error> {
        while let Some(k) = map.next_key::<Key>()? {
            if k.0 == self.key {
                map.next_value_seed(Count {
                    what: self.what,
                    max: self.max,
                    inner: Inner::Skip,
                    over: self.over,
                })?;
            } else {
                map.next_value::<IgnoredAny>()?;
            }
        }
        Ok(())
    }
    fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<(), A::Error> {
        while seq.next_element::<IgnoredAny>()?.is_some() {}
        Ok(())
    }
    fn visit_unit<E: de::Error>(self) -> Result<(), E> {
        Ok(())
    }
    fn visit_bool<E: de::Error>(self, _: bool) -> Result<(), E> {
        Ok(())
    }
    fn visit_i64<E: de::Error>(self, _: i64) -> Result<(), E> {
        Ok(())
    }
    fn visit_u64<E: de::Error>(self, _: u64) -> Result<(), E> {
        Ok(())
    }
    fn visit_f64<E: de::Error>(self, _: f64) -> Result<(), E> {
        Ok(())
    }
    fn visit_str<E: de::Error>(self, _: &str) -> Result<(), E> {
        Ok(())
    }
}
