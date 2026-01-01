use std::{env::VarError, str::FromStr};

use tracing::{error, warn};

fn get_prefixed_env_var(key: &str) -> Option<String> {
    let prefixed_key = format!("PGM_{}", key);
    let prefixed = std::env::var(&prefixed_key);
    let fallback = std::env::var(key);
    match (prefixed, fallback) {
        (Ok(val), _) => Some(val),
        (Err(VarError::NotPresent), Ok(val)) => {
            warn!("Environment variable {prefixed_key} not found. Using fallback {key}");
            warn!("This behavior is deprecated and will panic in a future version.");
            Some(val)
        }
        (Err(VarError::NotPresent), _) => {
            error!("Environment variable {prefixed_key} not found");
            None
        }
        (Err(VarError::NotUnicode(_)), _) => {
            error!("Environment variable {prefixed_key} contains non-unicode data");
            None
        }
    }
}

pub(crate) fn env_var<T: FromStr>(key: &str) -> Option<T> {
    get_prefixed_env_var(key).and_then(|v| v.parse().ok())
}
