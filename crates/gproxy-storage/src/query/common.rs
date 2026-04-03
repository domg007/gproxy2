use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum Scope<T> {
    #[default]
    All,
    Eq(T),
    In(Vec<T>),
}

impl<T> Scope<T> {
    pub fn as_eq(&self) -> Option<&T> {
        match self {
            Self::All => None,
            Self::Eq(value) => Some(value),
            Self::In(_) => None,
        }
    }
}
