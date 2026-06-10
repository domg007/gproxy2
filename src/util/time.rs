//! Dual-target wall clock: `SystemTime` on native, `js_sys::Date` on wasm
//! (where `SystemTime::now` panics on wasm32-unknown-unknown).

/// Current unix time in seconds.
#[cfg(not(target_arch = "wasm32"))]
pub fn unix_now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// Current unix time in seconds (JS host clock).
#[cfg(target_arch = "wasm32")]
pub fn unix_now() -> i64 {
    (js_sys::Date::now() / 1000.0) as i64
}
