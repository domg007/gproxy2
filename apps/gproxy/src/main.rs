use anyhow::Result;
use axum::Router;
use axum::extract::DefaultBodyLimit;
use axum::middleware::from_fn_with_state;
use axum::routing::get;
use gproxy_core::management_router;
use tokio::net::TcpListener;
mod admin_ui;
mod bootstrap;
mod middleware;

const MAX_AXUM_BODY_BYTES: usize = 50 * 1024 * 1024;

fn parse_author_and_email(authors: &str) -> (String, String) {
    let first = authors
        .split(':')
        .map(str::trim)
        .find(|value| !value.is_empty())
        .unwrap_or_default();
    if first.is_empty() {
        return ("unknown".to_string(), "unknown".to_string());
    }

    if let Some((name, rest)) = first.split_once('<') {
        let email = rest.split_once('>').map(|(value, _)| value).unwrap_or(rest);
        let name = name.trim();
        let email = email.trim();
        return (
            if name.is_empty() {
                "unknown".to_string()
            } else {
                name.to_string()
            },
            if email.is_empty() {
                "unknown".to_string()
            } else {
                email.to_string()
            },
        );
    }

    (first.to_string(), "unknown".to_string())
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "info,sqlx=warn,sqlx::query=warn,sea_orm=warn".into()),
        )
        .with_target(false)
        .compact()
        .init();

    let boot = bootstrap::bootstrap_from_env().await?;
    let config = boot.state.config.load();
    let host = config.global.host.clone();
    let port = config.global.port;
    let username = boot
        .state
        .load_users()
        .first()
        .map(|user| user.name.clone())
        .unwrap_or_else(|| "admin".to_string());
    let password = config.global.admin_key.clone();
    let bind_addr = format!("{host}:{port}");
    let (author, email) = parse_author_and_email(env!("CARGO_PKG_AUTHORS"));

    println!("========================================");
    println!(
        "gproxy | author: {} | email: {} | version: {}",
        author,
        email,
        env!("CARGO_PKG_VERSION")
    );
    println!("listen: http://{bind_addr}");
    println!("username: {username}");
    println!("password: {password}");
    println!("========================================");

    let _ = (&boot.config_path, &boot.config, &boot.storage_write_worker);
    let _storage = boot.storage.connection();

    let app = Router::new()
        .route("/favicon.ico", get(admin_ui::favicon))
        .route("/", get(admin_ui::index))
        .route("/assets/{*path}", get(admin_ui::asset))
        .merge(management_router(boot.state.clone()))
        .layer(from_fn_with_state(
            boot.state.clone(),
            middleware::downstream_event::middleware,
        ))
        .layer(DefaultBodyLimit::max(MAX_AXUM_BODY_BYTES));
    let listener = TcpListener::bind(&bind_addr).await?;
    axum::serve(listener, app).await?;

    Ok(())
}
