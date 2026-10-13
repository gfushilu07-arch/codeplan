//! 对象存储统一访问(Apache OpenDAL 封装)
//!
//! 层级:L2(依赖规则见 ARCHITECTURE.md §1.1,CI 由 xtask lint-deps 强制)。
//!
//! v1 提供 fs 后端;后续(S3/MinIO)换 opendal services 即可,调用方不动。

use std::path::Path;

use opendal::Operator;
use opendal::services::Fs;

pub use opendal::Error as StorageError;

/// 内容寻址对象存储。key 约定由调用方决定(如 `sha256/{oid}`)。
#[derive(Clone)]
pub struct ObjectStore {
    op: Operator,
}

impl ObjectStore {
    /// 文件系统后端(根目录不存在则创建)。
    pub fn fs(root: impl AsRef<Path>) -> Result<Self, StorageError> {
        let root = root.as_ref();
        std::fs::create_dir_all(root).map_err(|e| {
            StorageError::new(opendal::ErrorKind::Unexpected, "create root").set_source(e)
        })?;
        let op = Operator::new(Fs::default().root(&root.to_string_lossy()))?;
        Ok(Self { op })
    }

    /// 写入对象(整体覆盖)。
    pub async fn put(&self, key: &str, data: Vec<u8>) -> Result<(), StorageError> {
        self.op.write(key, data).await?;
        Ok(())
    }

    /// 读取对象;不存在返回 None。
    pub async fn get(&self, key: &str) -> Result<Option<Vec<u8>>, StorageError> {
        if !self.op.exists(key).await? {
            return Ok(None);
        }
        let buf = self.op.read(key).await?;
        Ok(Some(buf.to_vec()))
    }

    /// 对象大小;不存在返回 None。
    pub async fn size(&self, key: &str) -> Result<Option<u64>, StorageError> {
        if !self.op.exists(key).await? {
            return Ok(None);
        }
        let meta = self.op.stat(key).await?;
        Ok(Some(meta.content_length()))
    }

    /// 删除对象(不存在视为成功)。
    pub async fn delete(&self, key: &str) -> Result<(), StorageError> {
        self.op.delete(key).await?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_root() -> String {
        let dir = std::env::temp_dir().join(format!(
            "codeplan-storage-test-{}",
            uuid::Uuid::new_v4().simple()
        ));
        dir.to_string_lossy().into_owned()
    }

    #[tokio::test]
    async fn roundtrip_put_get_size_delete() {
        let store = ObjectStore::fs(temp_root()).expect("fs store");
        assert_eq!(store.get("a/b.txt").await.unwrap(), None);
        assert_eq!(store.size("a/b.txt").await.unwrap(), None);

        store.put("a/b.txt", b"hello".to_vec()).await.unwrap();
        assert_eq!(store.get("a/b.txt").await.unwrap(), Some(b"hello".to_vec()));
        assert_eq!(store.size("a/b.txt").await.unwrap(), Some(5));

        store.put("a/b.txt", b"hello world".to_vec()).await.unwrap();
        assert_eq!(store.size("a/b.txt").await.unwrap(), Some(11));

        store.delete("a/b.txt").await.unwrap();
        assert_eq!(store.get("a/b.txt").await.unwrap(), None);
    }
}
