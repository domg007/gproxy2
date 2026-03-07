use super::*;

impl AppState {
    pub fn load_users(&self) -> Arc<Vec<MemoryUser>> {
        self.principals.users.load_full()
    }

    pub fn replace_users(&self, users: Vec<MemoryUser>) {
        self.principals.users.store(Arc::new(users));
    }

    pub fn load_keys(&self) -> Arc<HashMap<String, MemoryUserKey>> {
        self.principals.keys.load_full()
    }

    pub fn replace_keys(&self, keys: HashMap<String, MemoryUserKey>) {
        self.principals.keys.store(Arc::new(keys));
    }

    pub fn query_users_in_memory(&self, query: &UserQuery) -> Vec<MemoryUser> {
        let mut rows: Vec<_> = self.principals.users.load().iter().cloned().collect();
        if let Scope::Eq(id) = query.id {
            rows.retain(|row| row.id == id);
        }
        if let Scope::Eq(name) = &query.name {
            rows.retain(|row| &row.name == name);
        }
        rows
    }

    pub fn query_user_keys_in_memory(&self, query: &UserKeyQuery) -> Vec<MemoryUserKey> {
        let mut rows: Vec<_> = self.principals.keys.load().values().cloned().collect();
        if let Scope::Eq(id) = query.id {
            rows.retain(|row| row.id == id);
        }
        if let Scope::Eq(user_id) = query.user_id {
            rows.retain(|row| row.user_id == user_id);
        }
        if let Scope::Eq(api_key) = &query.api_key {
            rows.retain(|row| &row.api_key == api_key);
        }
        rows
    }

    pub fn authenticate_api_key_in_memory(&self, api_key: &str) -> Option<MemoryUserKey> {
        self.principals.keys.load().get(api_key).cloned()
    }

    pub fn upsert_user_in_memory(&self, payload: UserWrite) {
        self.principals.users.rcu(|users| {
            let mut next = users.as_ref().clone();
            if let Some(existing) = next.iter_mut().find(|row| row.id == payload.id) {
                existing.name = payload.name.clone();
                existing.password = payload.password.clone();
                existing.enabled = payload.enabled;
            } else {
                next.push(MemoryUser {
                    id: payload.id,
                    name: payload.name.clone(),
                    password: payload.password.clone(),
                    enabled: payload.enabled,
                });
            }
            next.sort_by_key(|row| row.id);
            Arc::new(next)
        });
    }

    pub fn delete_user_in_memory(&self, id: i64) {
        self.principals.users.rcu(|users| {
            let mut next = users.as_ref().clone();
            next.retain(|row| row.id != id);
            Arc::new(next)
        });
        self.principals.keys.rcu(|keys| {
            let filtered = keys
                .iter()
                .filter(|(_, row)| row.user_id != id)
                .map(|(api_key, row)| (api_key.clone(), row.clone()))
                .collect::<HashMap<_, _>>();
            Arc::new(filtered)
        });
    }

    pub fn upsert_user_key_in_memory(&self, payload: UserKeyWrite) {
        self.principals.keys.rcu(|keys| {
            let mut next = keys.as_ref().clone();
            next.retain(|_, row| row.id != payload.id && row.api_key != payload.api_key);
            next.insert(
                payload.api_key.clone(),
                MemoryUserKey {
                    id: payload.id,
                    user_id: payload.user_id,
                    api_key: payload.api_key.clone(),
                    enabled: payload.enabled,
                },
            );
            Arc::new(next)
        });
    }

    pub fn delete_user_key_in_memory(&self, id: i64) {
        self.principals.keys.rcu(|keys| {
            let mut next = keys.as_ref().clone();
            next.retain(|_, row| row.id != id);
            Arc::new(next)
        });
    }
}
