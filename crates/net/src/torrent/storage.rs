//! Where a torrent's pieces are written: the files picked from it, in place under their own
//! names, created only once a piece of them arrives; and nothing else.
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
use std::sync::{Arc, Mutex, PoisonError};

use anyhow::{Context, bail};
use librqbit::storage::{StorageFactory, TorrentStorage};
use librqbit::{ManagedTorrentShared, TorrentMetadata};

/// The most bytes kept in memory for the files left out of one torrent.
const MAX_ASIDE: usize = 512 * 1024 * 1024;

/// Makes a [`Picked`] for one torrent: written under `out`, `picked` naming its files to keep
/// (`None` for all).
pub struct PickedFactory {
    pub out: PathBuf,
    pub picked: Option<Vec<usize>>,
}

impl StorageFactory for PickedFactory {
    type Storage = Picked;

    fn create(&self, _shared: &ManagedTorrentShared, metadata: &TorrentMetadata) -> anyhow::Result<Picked> {
        let slots = metadata
            .file_infos
            .iter()
            .enumerate()
            .map(|(i, fi)| {
                if fi.attrs.padding {
                    Slot::Padding
                } else if self.picked.as_ref().is_none_or(|p| p.contains(&i)) {
                    Slot::Kept { path: self.out.join(&fi.relative_filename), file: Mutex::new(None) }
                } else {
                    Slot::Aside(Mutex::new(BTreeMap::new()))
                }
            })
            .collect();
        Ok(Picked { inner: Arc::new(Inner { slots, aside: Mutex::new(0) }) })
    }

    fn clone_box(&self) -> librqbit::storage::BoxStorageFactory {
        use librqbit::storage::StorageFactoryExt;
        PickedFactory { out: self.out.clone(), picked: self.picked.clone() }.boxed()
    }
}

enum Slot {
    /// BEP 47 padding: never stored, read as zeros by librqbit.
    Padding,
    /// A file picked: written in place, opened once a piece of it arrives.
    Kept { path: PathBuf, file: Mutex<Option<File>> },
    /// A file left out: the bits of its pieces that arrived with a picked file's, by offset.
    Aside(Mutex<BTreeMap<u64, Vec<u8>>>),
}

struct Inner {
    slots: Vec<Slot>,
    /// Bytes kept in memory so far, over every file left out.
    aside: Mutex<usize>,
}

/// The torrent's storage (see the module's doc).
pub struct Picked {
    inner: Arc<Inner>,
}

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(PoisonError::into_inner)
}

impl Picked {
    fn slot(&self, id: usize) -> anyhow::Result<&Slot> {
        self.inner.slots.get(id).context("no such file in the torrent")
    }

    /// The open file of a picked one, created (with its folders) when `create`.
    fn with_file<R>(path: &Path, cell: &Mutex<Option<File>>, create: bool, f: impl FnOnce(&File) -> anyhow::Result<R>) -> anyhow::Result<R> {
        let mut held = lock(cell);
        if held.is_none() {
            if !create && !path.exists() {
                bail!("not written yet");
            }
            if let Some(dir) = path.parent() {
                std::fs::create_dir_all(dir).with_context(|| format!("couldn't make the folder {}", dir.display()))?;
            }
            let opened = OpenOptions::new().read(true).write(true).create(create).truncate(false).open(path)?;
            *held = Some(opened);
        }
        match held.as_ref() {
            Some(file) => f(file),
            None => bail!("not open"),
        }
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

    fn pread_exact(&self, file_id: usize, offset: u64, buf: &mut [u8]) -> anyhow::Result<()> {
        match self.slot(file_id)? {
            Slot::Padding => {
                buf.fill(0);
                Ok(())
            }
            Slot::Kept { path, file } => Self::with_file(path, file, false, |f| Ok(read_at(f, buf, offset)?)),
            Slot::Aside(kept) => read_aside(&lock(kept), buf, offset),
        }
    }

    fn pwrite_all(&self, file_id: usize, offset: u64, buf: &[u8]) -> anyhow::Result<()> {
        match self.slot(file_id)? {
            Slot::Padding => Ok(()),
            Slot::Kept { path, file } => Self::with_file(path, file, true, |f| Ok(write_at(f, buf, offset)?)),
            Slot::Aside(kept) => {
                let mut total = lock(&self.inner.aside);
                let mut kept = lock(kept);
                let before = kept.get(&offset).map_or(0, Vec::len);
                let after = total.saturating_sub(before).saturating_add(buf.len());
                if after > MAX_ASIDE {
                    bail!("too much of the files left out is needed alongside those picked; pick them too");
                }
                kept.insert(offset, buf.to_vec());
                *total = after;
                Ok(())
            }
        }
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
        match self.slot(file_id)? {
            Slot::Kept { path, file } => Self::with_file(path, file, true, |f| {
                if f.metadata()?.len() != length {
                    f.set_len(length)?;
                }
                Ok(())
            }),
            _ => Ok(()),
        }
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
}
