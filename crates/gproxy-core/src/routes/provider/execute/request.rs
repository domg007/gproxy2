use super::*;

pub(in crate::routes::provider) async fn execute_transform_request(
    state: Arc<AppState>,
    channel: ChannelId,
    provider: ProviderDefinition,
    auth: RequestAuthContext,
    request: TransformRequest,
) -> Result<Response, UpstreamError> {
    let context = ExecuteRequestContext {
        state,
        channel,
        provider,
        auth,
    };
    let dispatch = prepare_execute_dispatch(&context, request).await?;
    let executed = execute_upstream_dispatch(&context, &dispatch).await;

    let upstream = match executed.result {
        Ok(upstream) => upstream,
        Err(err) => {
            record_execute_failure(
                &context,
                &dispatch,
                executed.tracked_http_events.as_slice(),
                &err,
            )
            .await;
            return Err(err);
        }
    };

    flush_tracked_http_events(
        &context,
        &dispatch,
        upstream.credential_id,
        executed.tracked_http_events.as_slice(),
        upstream.request_meta.as_ref(),
    )
    .await;

    if let Some(update) = upstream.credential_update.clone() {
        apply_credential_update_and_persist(
            context.state.clone(),
            context.channel.clone(),
            context.provider.clone(),
            update,
        )
        .await;
    }

    handle_upstream_success(&context, &dispatch, upstream).await
}
