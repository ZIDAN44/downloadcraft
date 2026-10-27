//! Where a torrent's pieces are written: the files picked from it, in place under their own
//! names, created only once a piece of them arrives; and nothing else. The files picked can change
//! while it runs.
//!
//! A piece can straddle a file that was picked and one that wasn't. librqbit's own storage writes
//! such a piece into both files, so every file of the torrent appears in the folder, and on NTFS
//! writing near the end of a big file left out fills the whole of it with zeros (a 2 KB subtitle
//! next to a 4 GB film took 4 GB). Here the part of a piece that belongs to a file left out stays
//! in memory instead: enough to check the piece and share it, never in the folder. After a restart
//! those few pieces are fetched again.

use std::collections::BTreeMap;
use std::fs::{File, OpenOptions};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use anyhow::{Context, bail};
use librqbit::storage::{StorageFactory, TorrentStorage};
use librqbit::{ManagedTorrentShared, TorrentMetadata};

/// The most bytes kept in memory for the files left out of one torrent.
const MAX_ASIDE: usize = 512 * 1024 * 1024;

/// Makes a [`Picked`] for one torrent: written under `out`, `picked` naming its files to keep
/// (`None` for all). `handle` is filled with the storage's shared state, so the transfer can
/// change the files picked later ([`Shared::pick`]).
pub struct PickedFactory {
    pub out: PathBuf,
    pub picked: Option<Vec<usize>>,
    pub handle: Arc<Mutex<Option<Arc<Shared>>>>,
}

impl StorageFactory for PickedFactory {
    type Storage = Picked;

    fn create(&self, _shared: &ManagedTorrentShared, metadata: &TorrentMetadata) -> anyhow::Result<Picked> {
        let slots = metadata
            .file_infos
            .iter()
            .enumerate()
            .map(|(i, fi)| {
                Mutex::new(Slot {
                    path: (!fi.attrs.padding).then(|| self.out.join(&fi.relative_filename)),
                    kept: self.picked.as_ref().is_none_or(|p| p.contains(&i)),
                    file: None,
                    aside: BTreeMap::new(),
                })
            })
            .collect();
        let shared = Arc::new(Shared { slots, aside: Mutex::new(0) });
        *lock(&self.handle) = Some(shared.clone());
        Ok(Picked { inner: shared })
    }

    fn clone_box(&self) -> librqbit::storage::BoxStorageFactory {
        use librqbit::storage::StorageFactoryExt;
        PickedFactory { out: self.out.clone(), picked: self.picked.clone(), handle: self.handle.clone() }.boxed()
    }
}

/// One file of the torrent.
struct Slot {
    /// Where it's written; `None` for BEP 47 padding, never stored and read as zeros.
    path: Option<PathBuf>,
    /// Picked: what arrives for it is written in place.
    kept: bool,
    /// Open once a piece of it has been written (or it was found on disk).
    file: Option<File>,
    /// A file left out: the bits of its pieces that arrived with a picked file's, by offset.
    aside: BTreeMap<u64, Vec<u8>>,
}

/// The storage's state, shared with the transfer.
pub struct Shared {
    slots: Vec<Mutex<Slot>>,
    /// Bytes kept in memory so far, over every file left out.
    aside: Mutex<usize>,
}

impl Shared {
    /// Change the files picked (`None` for all). A file newly picked gets what was kept aside for
    /// it written in place; one left out keeps what it has on disk.
    pub fn pick(&self, picked: Option<&[usize]>) -> anyhow::Result<()> {
        for (i, cell) in self.slots.iter().enumerate() {
            let mut slot = lock(cell);
            let kept = picked.is_none_or(|p| p.contains(&i));
            if kept && !slot.kept && slot.path.is_some() && !slot.aside.is_empty() {
                open(&mut slot, true)?;
                let chunks = std::mem::take(&mut slot.aside);
                let freed: usize = chunks.values().map(Vec::len).sum();
                if let Some(f) = &slot.file {
                    for (at, bytes) in &chunks {
                        write_at(f, bytes, *at)?;
                    }
                }
                let mut total = lock(&self.aside);
                *total = total.saturating_sub(freed);
            }
            slot.kept = kept;
        }
        Ok(())
    }
}

/// The torrent's storage (see the module's doc).
pub struct Picked {
    inner: Arc<Shared>,
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Open `slot`'s file, made (with its folders) when `create`; an error when it isn't there and
/// may not be made.
fn open(slot: &mut Slot, create: bool) -> anyhow::Result<()> {
    if slot.file.is_some() {
        return Ok(());
    }
    let Some(path) = &slot.path else { bail!("padding") };
    if !create && !path.exists() {
        bail!("not written yet");
    }
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).with_context(|| format!("couldn't make the folder {}", dir.display()))?;
    }
    slot.file = Some(OpenOptions::new().read(true).write(true).create(create).truncate(false).open(path)?);
    Ok(())
}

impl Picked {
    fn slot(&self, id: usize) -> anyhow::Result<MutexGuard<'_, Slot>> {
        self.inner.slots.get(id).map(lock).context("no such file in the torrent")
    }
}

#[cfg(unix)]
fn read_at(f: &File, buf: &mut [u8], at: u64) -> std::io::Result<()> {
    std::os::unix::fs::FileExt::read_exact_at(f, buf, at)
}

#[cfg(unix)]
fn write_at(f: &File, buf: &[u8], at: u64) -> std::io::Result<()> {
    std::os::unix::fs::FileExt::write_all_at(f, buf, at)
}

#[cfg(windows)]
fn read_at(f: &File, mut buf: &mut [u8], mut at: u64) -> std::io::Result<()> {
    while !buf.is_empty() {
        match std::os::windows::fs::FileExt::seek_read(f, buf, at)? {
            0 => return Err(std::io::ErrorKind::UnexpectedEof.into()),
            n => {
                buf = buf.get_mut(n..).unwrap_or_default();
                at = at.saturating_add(n as u64);
            }
        }
    }
    Ok(())
}

#[cfg(windows)]
fn write_at(f: &File, mut buf: &[u8], mut at: u64) -> std::io::Result<()> {
    while !buf.is_empty() {
        match std::os::windows::fs::FileExt::seek_write(f, buf, at)? {
            0 => return Err(std::io::ErrorKind::WriteZero.into()),
            n => {
                buf = buf.get(n..).unwrap_or_default();
                at = at.saturating_add(n as u64);
            }
        }
    }
    Ok(())
}

/// Copy `buf.len()` bytes at `at` out of the pieces kept for a file left out; an error where any
/// of them is missing (so the piece is fetched).
fn read_aside(kept: &BTreeMap<u64, Vec<u8>>, buf: &mut [u8], at: u64) -> anyhow::Result<()> {
    let mut pos = at;
    let mut filled = 0;
    while filled < buf.len() {
        let Some((start, bytes)) = kept.range(..=pos).next_back() else { bail!("not here") };
        let from = usize::try_from(pos - start).unwrap_or(usize::MAX);
        let Some(have) = bytes.get(from..).filter(|h| !h.is_empty()) else { bail!("not here") };
        let n = have.len().min(buf.len() - filled);
        let (Some(dst), Some(src)) = (buf.get_mut(filled..filled + n), have.get(..n)) else { bail!("not here") };
        dst.copy_from_slice(src);
        filled += n;
        pos = pos.saturating_add(n as u64);
    }
    Ok(())
}

impl TorrentStorage for Picked {
    fn init(&mut self, _shared: &ManagedTorrentShared, _metadata: &TorrentMetadata) -> anyhow::Result<()> {
        Ok(())
    }

    /// From what's kept aside when it covers the range, else from the file.
    fn pread_exact(&self, file_id: usize, offset: u64, buf: &mut [u8]) -> anyhow::Result<()> {
        let mut slot = self.slot(file_id)?;
        if slot.path.is_none() {
            buf.fill(0);
            return Ok(());
        }
        if !slot.aside.is_empty() && read_aside(&slot.aside, buf, offset).is_ok() {
            return Ok(());
        }
        open(&mut slot, false)?;
        match &slot.file {
            Some(f) => Ok(read_at(f, buf, offset)?),
            None => bail!("not written yet"),
        }
    }

    /// Into the file when it's picked (or already on disk), else aside in memory.
    fn pwrite_all(&self, file_id: usize, offset: u64, buf: &[u8]) -> anyhow::Result<()> {
        let mut slot = self.slot(file_id)?;
        if slot.path.is_none() {
            return Ok(());
        }
        if slot.kept || slot.file.is_some() {
            open(&mut slot, true)?;
            return match &slot.file {
                Some(f) => Ok(write_at(f, buf, offset)?),
                None => bail!("not open"),
            };
        }
        let mut total = lock(&self.inner.aside);
        let before = slot.aside.get(&offset).map_or(0, Vec::len);
        let after = total.saturating_sub(before).saturating_add(buf.len());
        if after > MAX_ASIDE {
            bail!("too much of the files left out is needed alongside those picked; pick them too");
        }
        slot.aside.insert(offset, buf.to_vec());
        *total = after;
        Ok(())
    }

    fn remove_file(&self, _file_id: usize, filename: &Path) -> anyhow::Result<()> {
        match std::fs::remove_file(filename) {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(e.into()),
            _ => Ok(()),
        }
    }

    fn remove_directory_if_empty(&self, path: &Path) -> anyhow::Result<()> {
        if std::fs::read_dir(path).is_ok_and(|mut d| d.next().is_none()) {
            std::fs::remove_dir(path)?;
        }
        Ok(())
    }

    /// A picked file is given its whole size once the torrent starts (the room is set aside); a
    /// file left out is never made.
    fn ensure_file_length(&self, file_id: usize, length: u64) -> anyhow::Result<()> {
        let mut slot = self.slot(file_id)?;
        if slot.path.is_none() || !slot.kept {
            return Ok(());
        }
        open(&mut slot, true)?;
        if let Some(f) = &slot.file
            && f.metadata()?.len() != length
        {
            f.set_len(length)?;
        }
        Ok(())
    }

    fn take(&self) -> anyhow::Result<Box<dyn TorrentStorage>> {
        Ok(Box::new(Picked { inner: self.inner.clone() }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bytes_left_aside_read_back_only_where_they_arrived() {
        let mut kept = BTreeMap::new();
        kept.insert(100u64, vec![1u8, 2, 3, 4]);
        kept.insert(104u64, vec![5u8, 6]);
        let mut buf = [0u8; 5];
        read_aside(&kept, &mut buf, 101).unwrap();
        assert_eq!(buf, [2, 3, 4, 5, 6], "across two pieces");
        assert!(read_aside(&kept, &mut [0u8; 2], 105).is_err(), "runs past what arrived");
        assert!(read_aside(&kept, &mut [0u8; 1], 99).is_err(), "before it");
        assert!(read_aside(&kept, &mut [0u8; 1], 200).is_err(), "after it");
        read_aside(&kept, &mut [], 7).unwrap();
    }

    #[test]
    fn a_file_picked_later_gets_what_was_kept_aside() {
        let dir = std::env::temp_dir().join(format!("dc-pick-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let slot = |name: &str, kept: bool| Mutex::new(Slot { path: Some(dir.join(name)), kept, file: None, aside: BTreeMap::new() });
        let shared = Arc::new(Shared { slots: vec![slot("a", true), slot("b", false)], aside: Mutex::new(0) });
        let s = Picked { inner: shared.clone() };
        s.pwrite_all(0, 0, b"aaaa").unwrap();
        s.pwrite_all(1, 2, b"bb").unwrap();
        assert!(!dir.join("b").exists(), "a file left out isn't made");
        let mut back = [0u8; 2];
        s.pread_exact(1, 2, &mut back).unwrap();
        assert_eq!(&back, b"bb", "read back from memory, for the piece's check");
        shared.pick(None).unwrap();
        assert_eq!(std::fs::read(dir.join("b")).unwrap(), b"\0\0bb", "written in place once picked");
        assert_eq!(*lock(&shared.aside), 0);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
