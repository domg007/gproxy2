use std::sync::Arc;

use arc_swap::ArcSwap;
use wreq::Client as WreqClient;

/// Container for upstream HTTP clients.
///
/// This keeps client ownership out of `AppState` fields so we can later
/// centralize client middleware wiring (e.g. logging/tracing) in one place.
#[derive(Clone)]
pub struct UpstreamHttpClients {
    standard: Arc<ArcSwap<WreqClient>>,
    spoof: Arc<ArcSwap<WreqClient>>,
}

impl UpstreamHttpClients {
    pub fn new(standard: Arc<WreqClient>, spoof: Arc<WreqClient>) -> Self {
        Self {
            standard: Arc::new(ArcSwap::from(standard)),
            spoof: Arc::new(ArcSwap::from(spoof)),
        }
    }

    pub fn load_standard(&self) -> Arc<WreqClient> {
        self.standard.load_full()
    }

    pub fn replace_standard(&self, client: Arc<WreqClient>) {
        self.standard.store(client);
    }

    pub fn load_spoof(&self) -> Arc<WreqClient> {
        self.spoof.load_full()
    }

    pub fn replace_spoof(&self, client: Arc<WreqClient>) {
        self.spoof.store(client);
    }

    pub fn replace_all(&self, standard: Arc<WreqClient>, spoof: Arc<WreqClient>) {
        self.standard.store(standard);
        self.spoof.store(spoof);
    }
}
