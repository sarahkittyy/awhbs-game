//! Tiny key/value persistence: `localStorage` in the browser, `settings.txt` (key=value lines)
//! next to the executable on desktop.

#[cfg(target_arch = "wasm32")]
mod imp {
    fn storage() -> Option<web_sys::Storage> {
        web_sys::window()?.local_storage().ok()?
    }

    pub fn get(key: &str) -> Option<String> {
        storage()?.get_item(&format!("awhbs.{key}")).ok()?
    }

    pub fn set(key: &str, value: &str) {
        if let Some(s) = storage() {
            let _ = s.set_item(&format!("awhbs.{key}"), value);
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
mod imp {
    use std::collections::BTreeMap;
    use std::path::PathBuf;

    fn path() -> PathBuf {
        std::env::current_exe()
            .ok()
            .and_then(|p| p.parent().map(|d| d.join("settings.txt")))
            .unwrap_or_else(|| "settings.txt".into())
    }

    fn read() -> BTreeMap<String, String> {
        std::fs::read_to_string(path())
            .unwrap_or_default()
            .lines()
            .filter_map(|l| l.split_once('='))
            .map(|(k, v)| (k.trim().to_string(), v.trim().to_string()))
            .collect()
    }

    pub fn get(key: &str) -> Option<String> {
        read().remove(key)
    }

    pub fn set(key: &str, value: &str) {
        let mut all = read();
        all.insert(key.to_string(), value.to_string());
        let text: String = all.iter().map(|(k, v)| format!("{k}={v}\n")).collect();
        let _ = std::fs::write(path(), text);
    }
}

pub use imp::{get, set};

pub fn get_f32(key: &str) -> Option<f32> {
    get(key)?.parse().ok().filter(|v: &f32| v.is_finite())
}
