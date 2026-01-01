use std::{env::VarError, str::FromStr};

use tracing::{error, warn};

fn get_prefixed_env_var(key: &str) -> Option<String> {
    let prefixed_key = format!("PGM_{}", key);
    env_var_with_fallback(&prefixed_key, key)
}

pub(crate) fn env_var_with_fallback(key: &str, fallback_key: &str) -> Option<String> {
    let prefixed = std::env::var(key);
    let fallback = std::env::var(fallback_key);
    match (prefixed, fallback) {
        (Ok(val), _) => Some(val),
        (Err(VarError::NotPresent), Ok(val)) => {
            warn!("Environment variable {key} not found. Using fallback {key}");
            warn!("This behavior is deprecated and will panic in a future version.");
            Some(val)
        }
        (Err(VarError::NotPresent), _) => {
            error!("Environment variable {key} not found");
            None
        }
        (Err(VarError::NotUnicode(_)), _) => {
            error!("Environment variable {key} contains non-unicode data");
            None
        }
    }
}

pub(crate) fn env_var<T: FromStr>(key: &str) -> Option<T> {
    get_prefixed_env_var(key).and_then(|v| v.parse().ok())
}
