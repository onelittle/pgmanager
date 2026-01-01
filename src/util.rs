use std::str::FromStr;

use tracing::warn;

fn get_prefixed_env_var(key: &str) -> Option<String> {
    std::env::var(format!("PGM_{}", key))
        .map_err(|e| match e {
            std::env::VarError::NotPresent => {
                warn!("Environment variable PMG_{} not found.", key);
                warn!("  Falling back to {}.", key);
            }
            _ => {
                warn!("{}", e);
            }
        })
        .or_else(|_| std::env::var(key))
        .map_err(|e| match e {
            std::env::VarError::NotPresent => {
                warn!("Environment variable {} not found", key);
            }
            _ => {
                warn!("{}", e);
            }
        })
        .ok()
}

pub(crate) fn env_var<T: FromStr>(key: &str) -> Option<T> {
    get_prefixed_env_var(key).and_then(|v| v.parse().ok())
}
