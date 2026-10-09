//! 强类型 ID:防止不同实体的 UUID 串用(如把 RepoId 当 UserId 传入)。

/// 定义一个基于 UUID 的强类型 ID newtype。
///
/// - 自动实现 Debug/Clone/Copy/eq/hash/ord/Serialize/Deserialize(transparent);
/// - `XxxId::generate()` 生成新 ID;`Display` 输出 UUID 字符串。
#[macro_export]
macro_rules! define_id {
    ($(#[$meta:meta])* $name:ident) => {
        $(#[$meta])*
        #[derive(
            Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord,
            serde::Serialize, serde::Deserialize,
        )]
        #[serde(transparent)]
        pub struct $name(uuid::Uuid);

        impl $name {
            /// 生成新 ID(v4)。
            #[must_use]
            pub fn generate() -> Self {
                Self(uuid::Uuid::new_v4())
            }

            /// 由 UUID 构造。
            #[must_use]
            pub fn from_uuid(value: uuid::Uuid) -> Self {
                Self(value)
            }

            /// 底层 UUID。
            #[must_use]
            pub fn as_uuid(self) -> uuid::Uuid {
                self.0
            }
        }

        impl std::fmt::Display for $name {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                write!(f, "{}", self.0)
            }
        }

        impl TryFrom<&str> for $name {
            type Error = uuid::Error;

            fn try_from(value: &str) -> Result<Self, Self::Error> {
                Ok(Self(uuid::Uuid::parse_str(value)?))
            }
        }
    };
}
