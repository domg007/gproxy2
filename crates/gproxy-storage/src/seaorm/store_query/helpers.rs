use time::OffsetDateTime;

pub(super) fn unix_ms_to_offset_datetime(
    unix_ms: i64,
) -> Result<OffsetDateTime, time::error::ComponentRange> {
    OffsetDateTime::from_unix_timestamp_nanos(i128::from(unix_ms) * 1_000_000)
}
