use sea_orm::{ColumnTrait, QueryFilter, QueryOrder, Select};
use time::OffsetDateTime;

/// Convert unix milliseconds to OffsetDateTime.
pub(crate) fn unix_ms_to_offset_datetime(ms: i64) -> OffsetDateTime {
    OffsetDateTime::from_unix_timestamp_nanos(ms as i128 * 1_000_000)
        .unwrap_or(OffsetDateTime::UNIX_EPOCH)
}

/// Apply descending cursor-based pagination.
/// Filters: (at < cursor_at) OR (at == cursor_at AND trace_id < cursor_trace_id).
pub(crate) fn apply_desc_cursor<E, AtCol, IdCol>(
    mut select: Select<E>,
    cursor_at_unix_ms: Option<i64>,
    cursor_trace_id: Option<i64>,
    at_column: AtCol,
    id_column: IdCol,
) -> Select<E>
where
    E: sea_orm::EntityTrait,
    AtCol: ColumnTrait,
    IdCol: ColumnTrait,
{
    if let Some(cursor_at_ms) = cursor_at_unix_ms {
        let cursor_at = unix_ms_to_offset_datetime(cursor_at_ms);
        if let Some(cursor_id) = cursor_trace_id {
            select = select.filter(
                sea_orm::Condition::any()
                    .add(at_column.lt(cursor_at))
                    .add(
                        sea_orm::Condition::all()
                            .add(at_column.eq(cursor_at))
                            .add(id_column.lt(cursor_id)),
                    ),
            );
        } else {
            select = select.filter(at_column.lt(cursor_at));
        }
    }
    select
}
