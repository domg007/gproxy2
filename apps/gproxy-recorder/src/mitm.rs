use std::sync::Arc;

use dashmap::DashMap;
use rustls::ServerConfig;
use tokio::net::TcpStream;
use tokio_rustls::TlsAcceptor;

use crate::ca::CaAuthority;

/// MITM TLS acceptor that dynamically generates certificates per domain.
pub struct MitmAcceptor {
    ca: Arc<CaAuthority>,
    cache: DashMap<String, Arc<ServerConfig>>,
}

impl MitmAcceptor {
    pub fn new(ca: Arc<CaAuthority>) -> Self {
        Self {
            ca,
            cache: DashMap::new(),
        }
    }

    /// Perform a TLS handshake with the client, presenting a dynamically
    /// generated certificate for the given domain.
    pub async fn accept(
        &self,
        stream: TcpStream,
        domain: &str,
    ) -> Result<tokio_rustls::server::TlsStream<TcpStream>, Box<dyn std::error::Error + Send + Sync>>
    {
        let server_config = self.get_or_create_config(domain)?;
        let acceptor = TlsAcceptor::from(server_config);
        let tls_stream = acceptor.accept(stream).await?;
        Ok(tls_stream)
    }

    fn get_or_create_config(
        &self,
        domain: &str,
    ) -> Result<Arc<ServerConfig>, Box<dyn std::error::Error + Send + Sync>> {
        if let Some(config) = self.cache.get(domain) {
            return Ok(Arc::clone(config.value()));
        }

        let (cert_pem, key_pem) = crate::ca::issue_cert(&self.ca, domain)?;

        let certs =
            rustls_pemfile::certs(&mut cert_pem.as_bytes()).collect::<Result<Vec<_>, _>>()?;
        let key = rustls_pemfile::private_key(&mut key_pem.as_bytes())?
            .ok_or("Failed to parse leaf private key")?;

        let config = ServerConfig::builder()
            .with_no_client_auth()
            .with_single_cert(certs, key)?;

        let config = Arc::new(config);
        self.cache.insert(domain.to_string(), Arc::clone(&config));
        Ok(config)
    }
}
