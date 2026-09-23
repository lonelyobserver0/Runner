use std::collections::BTreeMap;
use std::path::PathBuf;

use serde::Deserialize;

#[derive(Deserialize, Debug)]
#[serde(default, deny_unknown_fields)]
pub struct Config {
    /// Provider interrogati quando la query non ha un prefisso.
    pub providers: Vec<String>,
    /// Prefisso per scegliere un provider per nome: `/bluetooth`, `/` da solo
    /// elenca quelli installati. Stringa vuota per disattivarlo.
    pub provider_prefix: String,
    /// Prefisso → provider, es. `"=" = "calc"`: `=2+2` interroga solo `calc` con `2+2`.
    pub prefixes: BTreeMap<String, String>,
    pub max_results: i32,
    pub width: i32,
    /// Distanza dal bordo superiore dello schermo, in pixel.
    pub margin_top: i32,
    pub icon_size: i32,
    /// Chiude il launcher quando perde il focus della tastiera.
    pub close_on_focus_loss: bool,
    /// Azioni da preferire per Invio, in ordine di priorità.
    pub primary_actions: Vec<String>,
    /// Provider "interattivi": dopo un'azione il launcher resta aperto e ricarica.
    pub keep_open: Vec<String>,
    /// Anteprima della finestra selezionata (provider windows).
    pub window_previews: bool,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            providers: vec!["desktopapplications".into()],
            prefixes: BTreeMap::new(),
            max_results: 50,
            width: 680,
            margin_top: 180,
            icon_size: 32,
            close_on_focus_loss: true,
            provider_prefix: "/".into(),
            primary_actions: [
                "start",
                "run",
                "focus",
                "focus_workspace",
                "connect",
                "disconnect",
                "pair",
                "menus:open",
                "menus:default",
                "activate",
                "open",
                "copy",
            ]
            .map(String::from)
            .to_vec(),
            window_previews: true,
            keep_open: ["bluetooth", "wireplumber", "playerctl", "todo"]
                .map(String::from)
                .to_vec(),
        }
    }
}

pub fn config_dir() -> PathBuf {
    std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))
        .unwrap_or_default()
        .join("runner")
}

impl Config {
    pub fn load() -> Self {
        let path = config_dir().join("config.toml");
        match std::fs::read_to_string(&path) {
            Ok(text) => toml::from_str(&text).unwrap_or_else(|e| {
                eprintln!("runner: {}: {e}", path.display());
                Self::default()
            }),
            Err(_) => Self::default(),
        }
    }

    /// Risolve un eventuale prefisso: restituisce il provider da usare
    /// (None = quelli di default) e la query ripulita dal prefisso.
    /// Vince il prefisso più lungo, così `>>` può convivere con `>`.
    pub fn split_prefix<'a>(&self, text: &'a str) -> (Option<String>, &'a str) {
        self.prefixes
            .iter()
            .filter(|(prefix, _)| text.starts_with(prefix.as_str()))
            .max_by_key(|(prefix, _)| prefix.len())
            .map(|(prefix, provider)| (Some(provider.clone()), &text[prefix.len()..]))
            .unwrap_or((None, text))
    }
}
