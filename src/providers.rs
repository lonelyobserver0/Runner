//! Conoscenza dei provider di elephant: quali sono installati, come si
//! chiamano, come ordinare le loro azioni e come interpretare il testo digitato.

use std::process::Command;

use crate::config::Config;

/// Provider installati, come li elenca `elephant listproviders`
/// (i menu compaiono come `menus:<nome>`).
pub fn discover() -> Vec<String> {
    match Command::new("elephant").arg("listproviders").output() {
        Ok(out) if out.status.success() => String::from_utf8_lossy(&out.stdout)
            .lines()
            .map(str::trim)
            .filter(|l| !l.is_empty())
            .map(String::from)
            .collect(),
        Ok(out) => {
            eprintln!(
                "runner: elephant listproviders: {}",
                String::from_utf8_lossy(&out.stderr)
            );
            Vec::new()
        }
        Err(e) => {
            eprintln!("runner: elephant listproviders: {e}");
            Vec::new()
        }
    }
}

/// Nome leggibile e icona di un provider.
pub fn describe(provider: &str) -> (String, &'static str) {
    if let Some(menu) = provider.strip_prefix("menus:") {
        return (menu.to_owned(), "view-list-symbolic");
    }
    let (name, icon) = match provider {
        "desktopapplications" => ("Applicazioni", "applications-other"),
        "bluetooth" => ("Bluetooth", "bluetooth-symbolic"),
        "calc" => ("Calcolatrice", "accessories-calculator"),
        "clipboard" => ("Appunti", "edit-paste"),
        "files" => ("File", "folder"),
        "runner" => ("Comandi", "utilities-terminal"),
        "symbols" => ("Simboli ed emoji", "face-smile"),
        "unicode" => ("Unicode", "accessories-character-map"),
        "websearch" => ("Ricerca web", "web-browser"),
        "windows" => ("Finestre", "preferences-system-windows"),
        "wireplumber" => ("Audio", "audio-volume-high"),
        "playerctl" => ("Media", "multimedia-player"),
        "todo" => ("Todo", "checkbox-checked-symbolic"),
        "bookmarks" => ("Segnalibri", "user-bookmarks"),
        "snippets" => ("Snippet", "text-x-generic"),
        "archlinuxpkgs" => ("Pacchetti Arch", "package-x-generic"),
        "providerlist" => ("Provider", "applications-other"),
        "1password" | "bitwarden" | "protonpass" => (provider, "dialog-password"),
        "niriactions" => ("Azioni niri", "preferences-system"),
        "nirisessions" => ("Sessioni niri", "preferences-system"),
        _ => (provider, "application-x-executable"),
    };
    (name.to_owned(), icon)
}

/// Etichetta leggibile di un'azione.
pub fn action_label(action: &str) -> String {
    let known = match action {
        "start" => "Avvia",
        "connect" => "Connetti",
        "disconnect" => "Disconnetti",
        "pair" => "Associa",
        "trust" => "Considera attendibile",
        "untrust" => "Revoca attendibilità",
        "remove" => "Rimuovi",
        "power_on" => "Accendi",
        "power_off" => "Spegni",
        "find" => "Cerca dispositivi",
        "pin" => "Fissa",
        "unpin" => "Togli dai fissati",
        "erase_history" => "Cancella dalla cronologia",
        "activate" => "Apri",
        "open" => "Apri",
        "copy" => "Copia",
        "delete" => "Elimina",
        "menus:open" => "Apri",
        "menus:parent" => "Menu superiore",
        "menus:default" => "Esegui",
        _ => "",
    };
    if !known.is_empty() {
        return known.to_owned();
    }
    let words = action
        .trim_start_matches("menus:")
        .replace(['_', '-', ':'], " ");
    let mut chars = words.chars();
    chars
        .next()
        .map_or_else(String::new, |c| c.to_uppercase().chain(chars).collect())
}

/// Azioni che non devono mai finire sotto Invio.
const DESTRUCTIVE: &[&str] = &["remove", "erase_history", "delete", "delete_all", "clear"];

/// Ordina le azioni di un item: prima quelle preferite (nell'ordine del config),
/// poi le altre nell'ordine di elephant, in fondo le distruttive. Toglie i doppioni
/// (il provider bluetooth, ad es., manda `remove` due volte e per primo).
pub fn order_actions(actions: &[String], preferred: &[String]) -> Vec<String> {
    let mut out: Vec<String> = Vec::with_capacity(actions.len());
    for a in actions {
        if !out.contains(a) {
            out.push(a.clone());
        }
    }
    let rank = |a: &String| {
        if let Some(i) = preferred.iter().position(|p| p == a) {
            i
        } else if DESTRUCTIVE.contains(&a.as_str()) {
            usize::MAX
        } else {
            preferred.len()
        }
    };
    out.sort_by_key(rank); // stabile: a parità conserva l'ordine di elephant
    out
}

/// Cosa significa il testo nella barra di ricerca.
#[derive(Debug, PartialEq, Eq)]
pub enum Mode<'a> {
    /// Ricerca normale sui provider di default.
    Default { query: &'a str },
    /// Un solo provider, scelto con un prefisso del config o con `/nome`.
    Single { provider: String, query: &'a str },
    /// `/filtro`: elenco dei provider installati.
    ProviderList { filter: &'a str },
}

impl Mode<'_> {
    pub fn provider(&self) -> Option<&str> {
        match self {
            Mode::Single { provider, .. } => Some(provider),
            _ => None,
        }
    }
}

/// `/bluetooth` o `/bluetooth testo` → modalità bluetooth; `/blu` → elenco
/// provider filtrato. Il nome deve coincidere esattamente con un provider
/// installato: così `/b` mostra l'elenco invece di saltare al primo che combacia.
pub fn parse_mode<'a>(text: &'a str, config: &Config, installed: &[String]) -> Mode<'a> {
    if !config.provider_prefix.is_empty()
        && let Some(rest) = text.strip_prefix(config.provider_prefix.as_str())
    {
        let (name, query) = rest.split_once(' ').unwrap_or((rest, ""));
        if installed.iter().any(|p| p == name) {
            return Mode::Single {
                provider: name.to_owned(),
                query,
            };
        }
        return Mode::ProviderList { filter: rest };
    }
    match config.split_prefix(text) {
        (Some(provider), query) => Mode::Single { provider, query },
        (None, query) => Mode::Default { query },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn installed() -> Vec<String> {
        vec![
            "desktopapplications".into(),
            "bluetooth".into(),
            "menus:power".into(),
        ]
    }

    #[test]
    fn slash_mode() {
        let c = Config::default();
        let p = installed();
        assert_eq!(parse_mode("/", &c, &p), Mode::ProviderList { filter: "" });
        assert_eq!(
            parse_mode("/blue", &c, &p),
            Mode::ProviderList { filter: "blue" }
        );
        assert_eq!(
            parse_mode("/bluetooth", &c, &p),
            Mode::Single {
                provider: "bluetooth".into(),
                query: ""
            }
        );
        assert_eq!(
            parse_mode("/bluetooth cuf", &c, &p),
            Mode::Single {
                provider: "bluetooth".into(),
                query: "cuf"
            }
        );
        assert_eq!(
            parse_mode("/menus:power ", &c, &p),
            Mode::Single {
                provider: "menus:power".into(),
                query: ""
            }
        );
        assert_eq!(
            parse_mode("/nope x", &c, &p),
            Mode::ProviderList { filter: "nope x" }
        );
        assert_eq!(parse_mode("fire", &c, &p), Mode::Default { query: "fire" });
    }

    #[test]
    fn config_prefix() {
        let mut c = Config::default();
        c.prefixes.insert("=".into(), "calc".into());
        assert_eq!(
            parse_mode("=2+2", &c, &[]),
            Mode::Single {
                provider: "calc".into(),
                query: "2+2"
            }
        );
    }

    #[test]
    fn bluetooth_actions_are_safe() {
        let preferred = Config::default().primary_actions;
        let a = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert_eq!(
            order_actions(&a(&["remove", "trust", "remove", "connect"]), &preferred),
            a(&["connect", "trust", "remove"])
        );
        assert_eq!(
            order_actions(&a(&["start", "pin", "erase_history"]), &preferred),
            a(&["start", "pin", "erase_history"])
        );
    }
}
