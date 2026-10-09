//! 纯工具库(L1):统一响应/分页/错误码/强类型 ID/时间。
//!
//! 铁律:不出现任何业务概念,不依赖本项目其他 crate(见 AGENTS.md)。

pub mod error;
pub mod id;
pub mod response;
pub mod time;

#[cfg(feature = "axum")]
pub mod error_http;

#[cfg(test)]
mod tests {
    use super::error::AppError;
    use super::response::{ApiResponse, PageQuery};
    use crate::define_id;

    define_id!(
        /// 测试用 ID。
        TestId
    );

    #[test]
    fn error_accessors() {
        let e = AppError::not_found("GIT", "仓库不存在");
        assert_eq!(e.code(), "GIT-404");
        assert_eq!(e.http_status(), 404);
        assert_eq!(e.message(), "仓库不存在");
    }

    #[test]
    fn response_ok_and_err() {
        let ok: ApiResponse<u32> = ApiResponse::ok(7);
        assert_eq!(ok.code, "OK");
        let e = AppError::forbidden("PM", "无权编辑");
        let err: ApiResponse<()> = ApiResponse::err(&e);
        assert_eq!(err.code, "PM-403");
        assert!(err.data.is_none());
    }

    #[test]
    fn page_query_normalized() {
        let q = PageQuery {
            page: 0,
            size: 500,
            sort: None,
        }
        .normalized();
        assert_eq!((q.page, q.size), (1, 100));
        assert_eq!(q.offset(), 0);
        let q2 = PageQuery {
            page: 3,
            size: 20,
            sort: None,
        }
        .normalized();
        assert_eq!(q2.offset(), 40);
    }

    #[test]
    fn typed_id_roundtrip() {
        let id = TestId::generate();
        let json = serde_json::to_string(&id).expect("序列化失败");
        let back: TestId = serde_json::from_str(&json).expect("反序列化失败");
        assert_eq!(id, back);
        assert_ne!(TestId::generate(), TestId::generate());
    }
}
