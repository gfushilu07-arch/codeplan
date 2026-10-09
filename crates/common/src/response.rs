//! 统一响应包装与分页参数。

use serde::{Deserialize, Serialize};

use crate::error::AppError;

/// 所有 API 的统一响应包装:`{ code, message, data }`。成功时 `code = "OK"`。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApiResponse<T> {
    /// `"OK"` 或平台错误码(`<域>-<3位>`)。
    pub code: String,
    /// 面向用户的消息。
    pub message: String,
    /// 业务数据(错误时缺省)。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<T>,
}

impl<T> ApiResponse<T> {
    /// 成功响应。
    #[must_use]
    pub fn ok(data: T) -> Self {
        Self {
            code: "OK".to_owned(),
            message: "ok".to_owned(),
            data: Some(data),
        }
    }

    /// 错误响应(由 [`AppError`] 构造)。
    #[must_use]
    pub fn err(error: &AppError) -> Self {
        Self {
            code: error.code().to_owned(),
            message: error.message().to_owned(),
            data: None,
        }
    }
}

/// 分页结果。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Page<T> {
    /// 当前页数据。
    pub items: Vec<T>,
    /// 总条数。
    pub total: i64,
    /// 页码(1 起)。
    pub page: u32,
    /// 每页条数。
    pub size: u32,
}

/// 分页查询参数(query)。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PageQuery {
    /// 页码(1 起,默认 1)。
    #[serde(default = "default_page")]
    pub page: u32,
    /// 每页条数(默认 20,上限 100)。
    #[serde(default = "default_size")]
    pub size: u32,
    /// 排序(字段名,方向由前缀 `-` 表示降序,如 `-created_at`)。
    pub sort: Option<String>,
}

fn default_page() -> u32 {
    1
}

fn default_size() -> u32 {
    20
}

impl Default for PageQuery {
    fn default() -> Self {
        Self {
            page: default_page(),
            size: default_size(),
            sort: None,
        }
    }
}

impl PageQuery {
    /// 归一化:page 至少为 1,size 截断到 1..=100。
    #[must_use]
    pub fn normalized(self) -> Self {
        Self {
            page: self.page.max(1),
            size: self.size.clamp(1, 100),
            sort: self.sort.clone(),
        }
    }

    /// SQL OFFSET。
    #[must_use]
    pub fn offset(&self) -> i64 {
        i64::from(self.page.saturating_sub(1)) * i64::from(self.size)
    }
}
