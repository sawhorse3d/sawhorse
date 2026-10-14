//! Zip containers read with a budget (lane 16): `.3mf` imports and the `.shz` reader
//! (`crates/shz`) open archives through [`Archive`], which checks the archive as a whole
//! (entry count, a name listed twice, entries sharing compressed bytes) before inflating
//! anything, and reads each entry through its own cap: refused by its declared size and
//! declared ratio first, inflated at most one byte past what it declares, and required to be
//! exactly that size. All entries read together stay under the archive's total.

use std::io::{Cursor, Read};

/// The most an entry over 1 MiB may claim to inflate per compressed byte: Deflate cannot do
/// better than about 1032 to 1, so a claim above this is a lie or a bomb.
pub const MAX_RATIO: u64 = 1100;

/// Why an archive or an entry was refused.
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum ArchiveError {
    #[error("not a zip ({0})")]
    NotZip(String),
    #[error("{0}")]
    Damaged(String),
}

/// Central directory records from `start`: how many there are, so a name listed twice
/// (which the zip reader folds into one entry) is seen. `None`: the records could not be
/// walked (ZIP64 or an unusual layout).
fn central_records(bytes: &[u8], start: u64, limit: usize) -> Option<usize> {
    let mut at = usize::try_from(start).ok()?;
    let mut n = 0;
    let u16_at = |o: usize| -> Option<usize> {
        Some(usize::from(u16::from_le_bytes(
            bytes.get(o..o + 2)?.try_into().ok()?,
        )))
    };
    while bytes.get(at..at + 4) == Some(&[0x50, 0x4b, 0x01, 0x02][..]) {
        n += 1;
        if n > limit {
            return Some(n);
        }
        at += 46 + u16_at(at + 28)? + u16_at(at + 30)? + u16_at(at + 32)?;
    }
    Some(n)
}

/// An archive and what has been read from it so far.
pub struct Archive<'a> {
    pub zip: zip::ZipArchive<Cursor<&'a [u8]>>,
    read: u64,
    total: u64,
}

impl<'a> Archive<'a> {
    /// Open `bytes` as a zip of at most `max_entries` entries, of which at most `total`
    /// bytes will be read, checking it as a whole before any entry is inflated.
    pub fn open(
        bytes: &'a [u8],
        max_entries: usize,
        total: u64,
    ) -> Result<Archive<'a>, ArchiveError> {
        let mut zip = zip::ZipArchive::new(Cursor::new(bytes))
            .map_err(|e| ArchiveError::NotZip(e.to_string()))?;
        if zip.len() > max_entries {
            return Err(ArchiveError::Damaged(format!(
                "the archive holds {} entries (at most {max_entries})",
                zip.len()
            )));
        }
        match central_records(bytes, zip.central_directory_start(), max_entries) {
            Some(n) if n == zip.len() => {}
            Some(n) if n > max_entries => {
                return Err(ArchiveError::Damaged(format!(
                    "the archive holds more than {max_entries} entries"
                )));
            }
            None => {}
            Some(_) => {
                return Err(ArchiveError::Damaged(
                    "an entry name is listed twice in the archive".into(),
                ));
            }
        }
        if zip
            .has_overlapping_files()
            .map_err(|e| ArchiveError::Damaged(e.to_string()))?
        {
            return Err(ArchiveError::Damaged(
                "two entries share the same compressed bytes".into(),
            ));
        }
        Ok(Archive {
            zip,
            read: 0,
            total,
        })
    }

    /// Every entry's name and declared size, in archive order (nothing inflated).
    pub fn listing(&mut self) -> Result<Vec<(String, u64)>, ArchiveError> {
        let mut out = Vec::with_capacity(self.zip.len());
        for i in 0..self.zip.len() {
            let f = self
                .zip
                .by_index_raw(i)
                .map_err(|e| ArchiveError::Damaged(e.to_string()))?;
            out.push((f.name().to_string(), f.size()));
        }
        Ok(out)
    }

    /// Entry `name`, at most `cap` bytes (see the module docs). `None` when it is absent.
    pub fn entry(&mut self, name: &str, cap: u64) -> Result<Option<Vec<u8>>, ArchiveError> {
        match self.entry_inspected(name, cap, |_| Ok::<(), std::convert::Infallible>(()))? {
            None => Ok(None),
            Some(Ok((v, ()))) => Ok(Some(v)),
            Some(Err(never)) => match never {},
        }
    }

    /// Entry `name` as [`Archive::entry`] reads it, shown to `inspect` while it inflates
    /// (lane 16b): `inspect` reads from the start of the entry as far as it likes, and what it
    /// reads is kept, so the entry is inflated once. When `inspect` returns `Err`, reading
    /// stops there (the rest is never inflated) and that error is returned inside `Some`.
    /// `None` when the entry is absent.
    #[allow(clippy::type_complexity)]
    pub fn entry_inspected<T, E>(
        &mut self,
        name: &str,
        cap: u64,
        inspect: impl FnOnce(&mut dyn Read) -> Result<T, E>,
    ) -> Result<Option<Result<(Vec<u8>, T), E>>, ArchiveError> {
        let damaged = |m: String| ArchiveError::Damaged(format!("{name}: {m}"));
        let mut f = match self.zip.by_name(name) {
            Ok(f) => f,
            Err(zip::result::ZipError::FileNotFound) => return Ok(None),
            Err(e) => return Err(damaged(e.to_string())),
        };
        let declared = f.size();
        if declared > cap {
            return Err(damaged(format!(
                "{declared} bytes inflated, more than the {cap} this entry may hold"
            )));
        }
        if declared > 1024 * 1024 && declared / f.compressed_size().max(1) > MAX_RATIO {
            return Err(damaged(
                "it claims to inflate more than Deflate can (a zip bomb?)".into(),
            ));
        }
        if self.read.saturating_add(declared) > self.total {
            return Err(ArchiveError::Damaged(format!(
                "the archive inflates to more than {} MB in all",
                self.total / 1024 / 1024
            )));
        }
        // `declared` is at most `cap`: the allocation is bounded, and so is the inflating
        // (one byte past the declared size is enough to know the header lied). Lane 16b: the
        // copy grows with what is read until RESERVE_AFTER bytes, then takes the declared
        // size at once; an entry the inspector stops early never allocates its claim.
        let mut v = Vec::with_capacity((declared as usize).min(RESERVE_AFTER / 8));
        let mut limited = (&mut f).take(declared + 1);
        let seen = {
            let mut tee = Tee {
                inner: &mut limited,
                out: &mut v,
                failed: None,
                declared: declared as usize,
            };
            let r = inspect(&mut tee);
            if let Some(e) = tee.failed.take() {
                return Err(damaged(e));
            }
            r
        };
        let seen = match seen {
            Ok(t) => t,
            Err(e) => return Ok(Some(Err(e))),
        };
        v.reserve_exact((declared as usize + 1).saturating_sub(v.len()));
        limited
            .read_to_end(&mut v)
            .map_err(|e| damaged(e.to_string()))?;
        if v.len() as u64 != declared {
            return Err(damaged(format!(
                "it holds {}{} bytes, not the {declared} its header says",
                v.len(),
                if v.len() as u64 > declared {
                    " or more"
                } else {
                    ""
                }
            )));
        }
        self.read += declared;
        Ok(Some(Ok((v, seen))))
    }
}

/// How much of an inspected entry is read before its copy takes the whole declared size.
const RESERVE_AFTER: usize = 8 << 20;

/// A reader that keeps a copy of everything read through it (and the first read error, so
/// a damaged entry is reported as damaged whatever the inspector made of it).
struct Tee<'a, R: Read> {
    inner: R,
    out: &'a mut Vec<u8>,
    failed: Option<String>,
    declared: usize,
}

impl<R: Read> Read for Tee<'_, R> {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        match self.inner.read(buf) {
            Ok(n) => {
                let len = self.out.len() + n;
                if len > self.out.capacity() && len >= RESERVE_AFTER {
                    // Past the first few megabytes: the rest at once, as declared.
                    self.out
                        .reserve_exact((self.declared + 1).max(len) - self.out.len());
                }
                self.out.extend_from_slice(&buf[..n]);
                Ok(n)
            }
            Err(e) => {
                self.failed.get_or_insert_with(|| e.to_string());
                Err(e)
            }
        }
    }
}
