use anyhow::Result;
use axum::http::StatusCode;
use axum::routing::get;

mod admin_ui;

#[tokio::main]
async fn main() -> Result<()> {
    let boot = gproxy_core::bootstrap::bootstrap_from_env().await?;
    let global = boot.state.global.load();
    let state_for_proxy = boot.state.clone();

    let upstream_cfg = gproxy_core::upstream_client::UpstreamClientConfig::from_global(&global);
    let upstream_client: std::sync::Arc<dyn gproxy_core::upstream_client::UpstreamClient> =
        std::sync::Arc::new(
            gproxy_core::upstream_client::WreqUpstreamClient::new_with_proxy_resolver(
                upstream_cfg,
                move || state_for_proxy.global.load().proxy.clone(),
            )?,
        );
    let engine = std::sync::Arc::new(gproxy_core::proxy_engine::ProxyEngine::new(
        boot.state.clone(),
        boot.registry.clone(),
        upstream_client,
        boot.storage.clone(),
    ));

    let app = axum::Router::new()
        .merge(gproxy_router::proxy_router(engine))
        .nest(
            "/admin",
            gproxy_router::admin_router(boot.state.clone(), boot.storage.clone()),
        )
        .route("/favicon.ico", get(|| async { StatusCode::NO_CONTENT }))
        .route("/", get(admin_ui::index))
        .route("/assets/{*path}", get(admin_ui::asset));

    let bind = format!("{}:{}", global.host, global.port);
    let listener = tokio::net::TcpListener::bind(&bind).await?;
    println!("listening on {bind}");
    axum::serve(listener, app).await?;
    Ok(())
}
