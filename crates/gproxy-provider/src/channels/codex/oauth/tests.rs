use super::{parse_id_token_claims, resolve_manual_code_and_state};

#[test]
fn manual_code_parse_prefers_query_code() {
    let (code, state) = resolve_manual_code_and_state(Some(
        "code=direct&state=s1&callback_url=http%3A%2F%2Flocalhost%2Fcb%3Fcode%3Dother%26state%3Ds2",
    ))
    .expect("parse should succeed");
    assert_eq!(code, "direct");
    assert_eq!(state.as_deref(), Some("s1"));
}

#[test]
fn id_token_claim_parse_tolerates_invalid_token() {
    let claims = parse_id_token_claims("invalid");
    assert!(claims.account_id.is_none());
    assert!(claims.email.is_none());
}
