//! 时间工具:全平台统一 UTC。

use chrono::{DateTime, Utc};

/// 当前 UTC 时间。
#[must_use]
pub fn now() -> DateTime<Utc> {
    Utc::now()
}

/// RFC 3339 字符串(带时区偏移 `Z`)。
#[must_use]
pub fn to_rfc3339(t: DateTime<Utc>) -> String {
    t.to_rfc3339_opts(chrono::SecondsFormat::Secs, true)
}
