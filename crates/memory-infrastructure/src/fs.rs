//! Local filesystem adapters rooted at each instance directory.

use std::path::{Component, Path, PathBuf};

use async_trait::async_trait;
use memory_application::ports::memory::SessionObjectStore;
use memory_application::ports::skill::SkillResourceStore;
use memory_domain::error::{DomainError, DomainResult};
use memory_domain::scope::InstanceId;

/// Files under `<data>/instances/<id>/`; independent of the storage backend.
pub struct LocalFileStore {
    data_dir: PathBuf,
}

impl LocalFileStore {
    pub fn new(data_dir: PathBuf) -> Self {
        Self { data_dir }
    }

    fn instance_dir(&self, instance: &InstanceId) -> PathBuf {
        self.data_dir.join("instances").join(instance.as_str())
    }

    /// Resolves `key` under the instance directory, rejecting anything that
    /// could escape it.
    fn resolve(&self, instance: &InstanceId, key: &str) -> DomainResult<PathBuf> {
        let relative = Path::new(key);
        let safe = !key.is_empty()
            && !key.contains('\0')
            && relative
                .components()
                .all(|c| matches!(c, Component::Normal(_)));
        if !safe {
            return Err(DomainError::invalid(format!("invalid storage key: {key}")));
        }
        Ok(self.instance_dir(instance).join(relative))
    }

    fn skill_dir(
        &self,
        instance: &InstanceId,
        skill_id: &str,
        version: u32,
    ) -> DomainResult<PathBuf> {
        self.resolve(instance, &format!("skills/{skill_id}/v{version}/files"))
    }
}

fn io_err(e: std::io::Error) -> DomainError {
    DomainError::unavailable(format!("storage I/O failed: {e}"))
}

async fn write_file(path: &Path, bytes: &[u8]) -> DomainResult<()> {
    if let Some(dir) = path.parent() {
        tokio::fs::create_dir_all(dir).await.map_err(io_err)?;
    }
    // Write-then-rename keeps readers from seeing partial files.
    let tmp = path.with_extension("tmp-write");
    tokio::fs::write(&tmp, bytes).await.map_err(io_err)?;
    tokio::fs::rename(&tmp, path).await.map_err(io_err)
}

async fn remove_tree(path: &Path) -> DomainResult<()> {
    match tokio::fs::remove_dir_all(path).await {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(io_err(e)),
    }
}

fn copy_tree(from: &Path, to: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(to)?;
    for entry in std::fs::read_dir(from)? {
        let entry = entry?;
        let target = to.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            copy_tree(&entry.path(), &target)?;
        } else {
            std::fs::copy(entry.path(), target)?;
        }
    }
    Ok(())
}

#[async_trait]
impl SessionObjectStore for LocalFileStore {
    async fn put(&self, instance: &InstanceId, key: &str, bytes: &[u8]) -> DomainResult<()> {
        write_file(&self.resolve(instance, key)?, bytes).await
    }
}

#[async_trait]
impl SkillResourceStore for LocalFileStore {
    async fn copy_version(
        &self,
        instance: &InstanceId,
        skill_id: &str,
        from: u32,
        to: u32,
    ) -> DomainResult<()> {
        let (src, dst) = (
            self.skill_dir(instance, skill_id, from)?,
            self.skill_dir(instance, skill_id, to)?,
        );
        if !src.exists() {
            return Ok(());
        }
        tokio::task::spawn_blocking(move || copy_tree(&src, &dst))
            .await
            .map_err(|e| DomainError::internal(e.to_string()))?
            .map_err(io_err)
    }

    async fn write(
        &self,
        instance: &InstanceId,
        skill_id: &str,
        version: u32,
        path: &str,
        bytes: &[u8],
    ) -> DomainResult<()> {
        self.resolve(instance, path)?;
        write_file(
            &self.skill_dir(instance, skill_id, version)?.join(path),
            bytes,
        )
        .await
    }

    async fn remove(
        &self,
        instance: &InstanceId,
        skill_id: &str,
        version: u32,
        path: &str,
    ) -> DomainResult<()> {
        self.resolve(instance, path)?;
        match tokio::fs::remove_file(self.skill_dir(instance, skill_id, version)?.join(path)).await
        {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(io_err(e)),
            _ => Ok(()),
        }
    }

    async fn read(
        &self,
        instance: &InstanceId,
        skill_id: &str,
        version: u32,
        path: &str,
    ) -> DomainResult<Option<Vec<u8>>> {
        self.resolve(instance, path)?;
        match tokio::fs::read(self.skill_dir(instance, skill_id, version)?.join(path)).await {
            Ok(bytes) => Ok(Some(bytes)),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(io_err(e)),
        }
    }

    async fn delete_version(
        &self,
        instance: &InstanceId,
        skill_id: &str,
        version: u32,
    ) -> DomainResult<()> {
        remove_tree(&self.resolve(instance, &format!("skills/{skill_id}/v{version}"))?).await
    }

    async fn delete_skill(&self, instance: &InstanceId, skill_id: &str) -> DomainResult<()> {
        remove_tree(&self.resolve(instance, &format!("skills/{skill_id}"))?).await
    }
}
