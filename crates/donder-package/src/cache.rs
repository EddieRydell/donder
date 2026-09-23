use std::collections::BTreeSet;
use std::fs::{self, File, OpenOptions};
use std::io::{Cursor, Write};

use camino::{Utf8Path, Utf8PathBuf};
use fs4::FileExt;
use zip::ZipArchive;

use crate::{
    PackageError, PackageManifest, ReleaseReceipt, collect_files, inspect_archive, sha256_hex,
    validate_archive_path,
};

const RELEASE_RECEIPT_FILE: &str = "donder-release.json";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CacheStatus {
    Missing,
    Ready,
}

#[derive(Clone, Debug)]
pub struct CacheStore {
    root: Utf8PathBuf,
}

impl CacheStore {
    pub fn new(root: Utf8PathBuf) -> Self {
        Self { root }
    }

    pub fn root(&self) -> &Utf8Path {
        &self.root
    }

    pub fn entry_path(&self, hash: &str) -> Utf8PathBuf {
        self.root.join("entries").join(hash)
    }

    pub fn archive_path(&self, hash: &str) -> Utf8PathBuf {
        self.entry_path(hash).join("archive.zip")
    }

    pub fn package_path(&self, hash: &str) -> Utf8PathBuf {
        self.entry_path(hash).join("package")
    }

    pub fn status(&self, hash: &str) -> Result<CacheStatus, PackageError> {
        validate_hash_key(hash)?;
        if !self.entry_path(hash).exists() {
            return Ok(CacheStatus::Missing);
        }
        let lock = self.lock(hash, false)?;
        self.validate_entry(hash)?;
        drop(lock);
        Ok(CacheStatus::Ready)
    }

    pub fn install(
        &self,
        hash: &str,
        bytes: &[u8],
        validate: impl FnOnce(&Utf8Path) -> Result<(), PackageError>,
    ) -> Result<Utf8PathBuf, PackageError> {
        validate_hash_key(hash)?;
        if bytes.len() > crate::MAX_ARCHIVE_BYTES {
            return Err(PackageError::Archive(format!(
                "archive exceeds {} bytes",
                crate::MAX_ARCHIVE_BYTES
            )));
        }
        if sha256_hex(bytes) != hash {
            return Err(PackageError::Archive(
                "cache content hash mismatch".to_string(),
            ));
        }
        let _ = inspect_archive(bytes)?;
        fs::create_dir_all(self.root.join("entries"))?;
        let lock = self.lock(hash, true)?;
        if self.entry_path(hash).exists() {
            self.validate_entry(hash)?;
            validate(&self.package_path(hash))?;
            drop(lock);
            return Ok(self.archive_path(hash));
        }

        let temporary = tempfile::Builder::new()
            .prefix(".donder-cache-")
            .tempdir_in(self.root.join("entries"))?;
        let temporary_root = Utf8Path::from_path(temporary.path()).ok_or_else(|| {
            PackageError::Invalid("cache temporary path is not UTF-8".to_string())
        })?;
        let package_root = temporary_root.join("package");
        fs::create_dir(&package_root)?;
        extract_archive(bytes, &package_root)?;
        write_synced(&temporary_root.join("archive.zip"), bytes)?;
        validate_cache_entry(temporary_root, hash)?;
        validate(&package_root)?;
        let kept = temporary.keep();
        let kept = Utf8PathBuf::from_path_buf(kept)
            .map_err(|_| PackageError::Invalid("cache temporary path is not UTF-8".to_string()))?;
        fs::rename(&kept, self.entry_path(hash))?;
        drop(lock);
        Ok(self.archive_path(hash))
    }

    pub fn read(&self, hash: &str) -> Result<Vec<u8>, PackageError> {
        validate_hash_key(hash)?;
        let lock = self.lock(hash, false)?;
        self.validate_entry(hash)?;
        let bytes = fs::read(self.archive_path(hash))?;
        drop(lock);
        Ok(bytes)
    }

    pub fn package_root(&self, hash: &str) -> Result<Utf8PathBuf, PackageError> {
        validate_hash_key(hash)?;
        let lock = self.lock(hash, false)?;
        self.validate_entry(hash)?;
        let root = self.package_path(hash);
        drop(lock);
        Ok(root)
    }

    fn validate_entry(&self, hash: &str) -> Result<ReleaseReceipt, PackageError> {
        validate_cache_entry(&self.entry_path(hash), hash)
    }

    fn lock(&self, hash: &str, exclusive: bool) -> Result<File, PackageError> {
        let directory = self.root.join("locks");
        fs::create_dir_all(&directory)?;
        let file = OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(directory.join(format!("{hash}.lock")))?;
        if exclusive {
            FileExt::lock(&file)?;
        } else {
            FileExt::lock_shared(&file)?;
        }
        Ok(file)
    }
}

fn extract_archive(bytes: &[u8], root: &Utf8Path) -> Result<(), PackageError> {
    let mut archive = ZipArchive::new(Cursor::new(bytes))?;
    for index in 0..archive.len() {
        let mut entry = archive.by_index(index)?;
        let name = entry.name().to_string();
        validate_archive_path(&name)?;
        let output = root.join(&name);
        if entry.is_dir() {
            fs::create_dir_all(&output)?;
            continue;
        }
        if let Some(parent) = output.parent() {
            fs::create_dir_all(parent)?;
        }
        let mut file = OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&output)?;
        std::io::copy(&mut entry, &mut file)?;
        file.sync_all()?;
    }
    Ok(())
}

fn write_synced(path: &Utf8Path, bytes: &[u8]) -> Result<(), PackageError> {
    let mut file = OpenOptions::new().create_new(true).write(true).open(path)?;
    file.write_all(bytes)?;
    file.sync_all()?;
    Ok(())
}

fn validate_cache_entry(entry_root: &Utf8Path, hash: &str) -> Result<ReleaseReceipt, PackageError> {
    let archive_path = entry_root.join("archive.zip");
    let package_root = entry_root.join("package");
    if !archive_path.is_file() || !package_root.is_dir() {
        return Err(PackageError::Archive(
            "cached package entry is incomplete".to_string(),
        ));
    }
    let bytes = fs::read(&archive_path)?;
    if sha256_hex(&bytes) != hash {
        return Err(PackageError::Archive(
            "cached archive is corrupt".to_string(),
        ));
    }
    let receipt = inspect_archive(&bytes)?;
    let extracted_receipt = fs::read(package_root.join(RELEASE_RECEIPT_FILE))?;
    let extracted_receipt: ReleaseReceipt = serde_json::from_slice(&extracted_receipt)?;
    if extracted_receipt != receipt {
        return Err(PackageError::Archive(
            "cached release receipt is corrupt".to_string(),
        ));
    }
    let mut paths = Vec::new();
    collect_files(&package_root, &package_root, &mut paths)?;
    let actual = paths.into_iter().collect::<BTreeSet<_>>();
    let mut expected = receipt.files.keys().cloned().collect::<BTreeSet<_>>();
    expected.insert(RELEASE_RECEIPT_FILE.to_string());
    if actual != expected {
        return Err(PackageError::Archive(
            "cached package file set is corrupt".to_string(),
        ));
    }
    for (path, expected_file) in &receipt.files {
        let content = fs::read(package_root.join(path))?;
        if content.len() as u64 != expected_file.size
            || sha256_hex(&content) != expected_file.sha256
        {
            return Err(PackageError::Archive(format!(
                "cached package file is corrupt: `{path}`"
            )));
        }
    }
    let manifest = PackageManifest::read(&package_root)?;
    if manifest.module_id != receipt.module_id {
        return Err(PackageError::Archive(
            "cached package manifest identity is corrupt".to_string(),
        ));
    }
    Ok(receipt)
}

fn validate_hash_key(hash: &str) -> Result<(), PackageError> {
    if hash.len() != 64
        || !hash
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        return Err(PackageError::Invalid(
            "cache key must be a lowercase SHA-256 digest".to_string(),
        ));
    }
    Ok(())
}
