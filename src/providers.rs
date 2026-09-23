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
        "desktopapplications" => ("Applications", "applications-other"),
        "bluetooth" => ("Bluetooth", "bluetooth-symbolic"),
        "calc" => ("Calculator", "accessories-calculator"),
        "clipboard" => ("Clipboard", "edit-paste"),
        "files" => ("Files", "folder"),
        "runner" => ("Commands", "utilities-terminal"),
        "symbols" => ("Symbols and emoji", "face-smile"),
        "unicode" => ("Unicode", "accessories-character-map"),
        "websearch" => ("Web search", "web-browser"),
        "windows" => ("Windows", "preferences-system-windows"),
        "wireplumber" => ("Audio", "audio-volume-high"),
        "playerctl" => ("Media", "multimedia-player"),
        "todo" => ("Todo", "checkbox-checked-symbolic"),
        "bookmarks" => ("Bookmarks", "user-bookmarks"),
        "snippets" => ("Snippet", "text-x-generic"),
        "archlinuxpkgs" => ("Arch packages", "package-x-generic"),
        "providerlist" => ("Provider", "applications-other"),
        "1password" | "bitwarden" | "protonpass" => (provider, "dialog-password"),
        "niriactions" => ("Niri actions", "preferences-system"),
        "nirisessions" => ("Niri sessions", "preferences-system"),
        _ => (provider, "application-x-executable"),
    };
    (name.to_owned(), icon)
}

/// Etichetta leggibile di un'azione.
pub fn action_label(action: &str) -> String {
    let known = match action {
        "start" => "Start",
        "connect" => "Connect",
        "disconnect" => "Disconnect",
        "pair" => "Pair",
        "trust" => "Trust",
        "untrust" => "Untrust",
        "remove" => "Remove",
        "power_on" => "Turn on",
        "power_off" => "Turn off",
        "find" => "Scan for devices",
        "pin" => "Pin",
        "unpin" => "Unpin",
        "erase_history" => "Remove from history",
        "activate" => "Open",
        "open" => "Open",
        "copy" => "Copy",
        "delete" => "Delete",
        "menus:open" => "Open",
        "menus:parent" => "Back to parent menu",
        "menus:default" => "Run",
        "focus" => "Switch to",
        "focus_workspace" => "Go to workspace",
        "run" => "Run",
        "runterminal" => "Run in terminal",
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
    /// Un solo provider: prefisso del config (resta nel testo) o modalità attiva.
    Single { provider: String, query: &'a str },
    /// `/nome` completo: si entra nella modalità e nel testo resta solo la query.
    Enter { provider: String, query: &'a str },
    /// `/filtro`: elenco dei provider installati.
    ProviderList { filter: &'a str },
}

impl Mode<'_> {
    pub fn provider(&self) -> Option<&str> {
        match self {
            Mode::Single { provider, .. } | Mode::Enter { provider, .. } => Some(provider),
            _ => None,
        }
    }
}

/// `/bluetooth` o `/bluetooth testo` → si entra nella modalità bluetooth;
/// `/blu` → elenco provider filtrato. Il nome deve coincidere esattamente con un
/// provider installato: così `/b` mostra l'elenco invece di saltare al primo
/// che combacia. Con una modalità già attiva il testo è tutto query.
pub fn parse_mode<'a>(
    text: &'a str,
    active: Option<&str>,
    config: &Config,
    installed: &[String],
) -> Mode<'a> {
    if let Some(provider) = active {
        return Mode::Single {
            provider: provider.to_owned(),
            query: text,
        };
    }
    if !config.provider_prefix.is_empty()
        && let Some(rest) = text.strip_prefix(config.provider_prefix.as_str())
    {
        let (name, query) = rest.split_once(' ').unwrap_or((rest, ""));
        if installed.iter().any(|p| p == name) {
            return Mode::Enter {
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

    fn enter<'a>(provider: &str, query: &'a str) -> Mode<'a> {
        Mode::Enter {
            provider: provider.into(),
            query,
        }
    }

    #[test]
    fn slash_mode() {
        let c = Config::default();
        let p = installed();
        let parse = |t| parse_mode(t, None, &c, &p);
        assert_eq!(parse("/"), Mode::ProviderList { filter: "" });
        assert_eq!(parse("/blue"), Mode::ProviderList { filter: "blue" });
        assert_eq!(parse("/bluetooth"), enter("bluetooth", ""));
        assert_eq!(parse("/bluetooth cuf"), enter("bluetooth", "cuf"));
        assert_eq!(parse("/menus:power "), enter("menus:power", ""));
        assert_eq!(parse("/nope x"), Mode::ProviderList { filter: "nope x" });
        assert_eq!(parse("fire"), Mode::Default { query: "fire" });
        // Dentro una modalità anche "/" è testo normale.
        assert_eq!(
            parse_mode("/x", Some("bluetooth"), &c, &p),
            Mode::Single {
                provider: "bluetooth".into(),
                query: "/x"
            }
        );
    }

    #[test]
    fn config_prefix() {
        let mut c = Config::default();
        c.prefixes.insert("=".into(), "calc".into());
        assert_eq!(
            parse_mode("=2+2", None, &c, &[]),
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
        // Provider runner: Invio esegue, Alt+2 apre nel terminale.
        assert_eq!(
            order_actions(&a(&["run", "runterminal", "erase_history"]), &preferred),
            a(&["run", "runterminal", "erase_history"])
        );
    }
}
