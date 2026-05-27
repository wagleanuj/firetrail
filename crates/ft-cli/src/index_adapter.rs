//! Adapter that lets `ft_storage::EmbeddedStorage` satisfy
//! `ft_index::Storage`.
//!
//! The doctor scaffold (firetrail-s45) has its own copy of this adapter inline
//! in `commands/doctor.rs` — that copy stays where it is until the storage
//! trait merge (firetrail-sm9) lands. The work-graph commands added by
//! firetrail-1xc need the same shim, so it is mirrored here for shared use by
//! `commands::list`, `commands::ready`, `commands::board`, and `commands::graph`.

use std::path::{Path, PathBuf};

use ft_core::{Record, RecordId};
use ft_index::{Storage as IndexStorageTrait, StorageError as IndexStorageError, StorageFilter};
use ft_storage::{EmbeddedStorage, Storage as FsStorage};

/// View of an [`EmbeddedStorage`] as an [`ft_index::Storage`].
pub struct IndexStorage<'a> {
    inner: &'a EmbeddedStorage,
}

impl<'a> IndexStorage<'a> {
    /// Wrap an [`EmbeddedStorage`] reference.
    pub fn new(inner: &'a EmbeddedStorage) -> Self {
        Self { inner }
    }
}

fn map_err(e: ft_storage::StorageError) -> IndexStorageError {
    match e {
        ft_storage::StorageError::NotFound(id) => IndexStorageError::NotFound(id.to_string()),
        other => IndexStorageError::Other(other.to_string()),
    }
}

impl IndexStorageTrait for IndexStorage<'_> {
    fn iter(
        &self,
        filter: StorageFilter,
    ) -> Result<
        Box<dyn Iterator<Item = Result<(Record, PathBuf), IndexStorageError>> + '_>,
        IndexStorageError,
    > {
        let _ = filter;
        let fs_filter = ft_storage::StorageFilter::default();
        let ids = self.inner.list(&fs_filter).map_err(map_err)?;
        let inner = self.inner.clone();
        let iter = ids.into_iter().map(move |id| {
            let path = inner.path_for(&id);
            let record = inner.read(&id).map_err(map_err)?;
            Ok((record, path))
        });
        Ok(Box::new(iter))
    }

    fn read(&self, id: &RecordId) -> Result<(Record, PathBuf), IndexStorageError> {
        let path = self.inner.path_for(id);
        let record = self.inner.read(id).map_err(map_err)?;
        Ok((record, path))
    }

    fn read_path(&self, path: &Path) -> Result<Record, IndexStorageError> {
        // The on-disk filename is `<lower-id>.json`; derive the canonical id
        // from the stem so we can dispatch through the normal validated read.
        let stem = path
            .file_stem()
            .and_then(|s| s.to_str())
            .ok_or_else(|| IndexStorageError::Other(format!("bad path: {}", path.display())))?;
        let upper = stem.to_uppercase();
        let parts: Vec<&str> = upper.splitn(2, '-').collect();
        if parts.len() != 2 {
            return Err(IndexStorageError::Other(format!(
                "cannot parse id from path: {}",
                path.display()
            )));
        }
        // Reconstruct the canonical `<KIND>-<hex>` form.
        let canonical = format!("{}-{}", parts[0], parts[1].to_lowercase());
        let id = RecordId::from_string(canonical)
            .map_err(|e| IndexStorageError::Other(e.to_string()))?;
        self.inner.read(&id).map_err(map_err)
    }
}
