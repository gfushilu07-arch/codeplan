//! [`AppError`] → HTTP 响应适配(feature = "axum")。

use axum::Json;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};

use crate::error::AppError;
use crate::response::ApiResponse;

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        let status =
            StatusCode::from_u16(self.http_status()).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
        (status, Json(ApiResponse::<()>::err(&self))).into_response()
    }
}
