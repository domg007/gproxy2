use super::*;

impl AppState {
    pub fn new(init: AppStateInit) -> Self {
        let AppStateInit {
            storage,
            storage_writes,
            http,
            spoof_http,
            global,
            providers,
            tokenizers,
            users,
            keys,
        } = init;
        let snapshot = RuntimeConfigSnapshot { global, providers };
        let credential_states = Arc::new(ChannelCredentialStateStore::from_states(
            snapshot
                .providers
                .providers
                .iter()
                .flat_map(|provider| provider.credentials.channel_states.iter().cloned()),
        ));
        Self {
            infra: AppInfraState {
                storage: Arc::new(ArcSwap::from(storage)),
                storage_writes,
                http_clients: UpstreamHttpClients::new(http, spoof_http),
            },
            runtime: AppRuntimeState {
                config: Arc::new(ArcSwap::from_pointee(snapshot)),
                credential_states,
                tokenizers,
            },
            principals: AppPrincipalState {
                users: Arc::new(ArcSwap::from_pointee(users)),
                keys: Arc::new(ArcSwap::from_pointee(keys)),
            },
        }
    }

    pub fn load_config(&self) -> Arc<RuntimeConfigSnapshot> {
        self.runtime.config.load_full()
    }

    pub fn storage_writes(&self) -> &StorageWriteSender {
        &self.infra.storage_writes
    }

    pub fn credential_states(&self) -> &ChannelCredentialStateStore {
        self.runtime.credential_states.as_ref()
    }

    pub fn load_storage(&self) -> Arc<SeaOrmStorage> {
        self.infra.storage.load_full()
    }

    pub fn replace_storage(&self, storage: Arc<SeaOrmStorage>) {
        self.infra.storage.store(storage);
    }

    pub fn load_http(&self) -> Arc<WreqClient> {
        self.infra.http_clients.load_standard()
    }

    pub fn replace_http(&self, http: Arc<WreqClient>) {
        self.infra.http_clients.replace_standard(http);
    }

    pub fn load_spoof_http(&self) -> Arc<WreqClient> {
        self.infra.http_clients.load_spoof()
    }

    pub fn replace_spoof_http(&self, spoof_http: Arc<WreqClient>) {
        self.infra.http_clients.replace_spoof(spoof_http);
    }

    pub fn replace_http_clients(&self, http: Arc<WreqClient>, spoof_http: Arc<WreqClient>) {
        self.infra.http_clients.replace_all(http, spoof_http);
    }

    pub fn tokenizers(&self) -> Arc<LocalTokenizerStore> {
        self.runtime.tokenizers.clone()
    }

    pub fn upsert_tokenizer_vocab_in_memory(
        &self,
        model: impl Into<String>,
        tokenizer_json: Vec<u8>,
    ) -> Result<(), LocalTokenizerError> {
        self.runtime
            .tokenizers
            .upsert_memory_tokenizer_bytes(model, tokenizer_json)
    }

    pub async fn count_tokens_with_local_tokenizer(
        &self,
        model: &str,
        text: &str,
    ) -> Result<LocalTokenCount, LocalTokenizerError> {
        let http = self.load_http();
        let global = self.load_config().global.clone();
        self.runtime
            .tokenizers
            .count_text_tokens(
                http.as_ref(),
                global.hf_token.as_deref(),
                global.hf_url.as_deref(),
                model,
                text,
            )
            .await
    }

    pub async fn enqueue_storage_write(
        &self,
        event: StorageWriteEvent,
    ) -> Result<(), StorageWriteQueueError> {
        self.infra.storage_writes.enqueue(event).await
    }

    pub fn replace_config(&self, snapshot: RuntimeConfigSnapshot) {
        self.runtime.credential_states.replace_from_states(
            snapshot
                .providers
                .providers
                .iter()
                .flat_map(|provider| provider.credentials.channel_states.iter().cloned()),
        );
        self.runtime.config.store(Arc::new(snapshot));
    }
}
