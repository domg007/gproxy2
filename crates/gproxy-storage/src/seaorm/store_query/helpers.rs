use sea_orm::{ColumnTrait, Condition, QueryFilter};
use time::OffsetDateTime;

pub(super) fn unix_ms_to_offset_datetime(
    unix_ms: i64,
) -> Result<OffsetDateTime, time::error::ComponentRange> {
    OffsetDateTime::from_unix_timestamp_nanos(i128::from(unix_ms) * 1_000_000)
}

pub(super) fn apply_desc_cursor<S, C1, C2>(
    stmt: S,
    at_column: C1,
    trace_id_column: C2,
    cursor_at_unix_ms: Option<i64>,
    cursor_trace_id: Option<i64>,
) -> S
where
    S: QueryFilter,
    C1: ColumnTrait + Copy,
    C2: ColumnTrait + Copy,
{
    let Some(cursor_at_unix_ms) = cursor_at_unix_ms else {
        return stmt;
    };
    let Some(cursor_trace_id) = cursor_trace_id else {
        return stmt;
    };
    let Ok(cursor_at) = unix_ms_to_offset_datetime(cursor_at_unix_ms) else {
        return stmt;
    };

    stmt.filter(
        Condition::any().add(at_column.lt(cursor_at)).add(
            Condition::all()
                .add(at_column.eq(cursor_at))
                .add(trace_id_column.lt(cursor_trace_id)),
        ),
    )
}
