//! Colori presi dal desktop: palette di pywal e bordo attivo di Hyprland.
//! Il launcher deve sembrare una finestra del sistema, non un'app a parte.

use std::process::Command;

use serde::Deserialize;

pub struct Theme {
    pub background: String,
    pub foreground: String,
    pub accent: String,
    /// Colori e angolo del bordo attivo di Hyprland, se disponibile.
    pub border: Option<(Vec<String>, u32)>,
}

/// Palette di riserva quando pywal non c'è.
const FALLBACK: (&str, &str, &str) = ("#1d1a24", "#d4cfd8", "#e29b69");

#[derive(Deserialize)]
struct Wal {
    special: WalSpecial,
    colors: std::collections::HashMap<String, String>,
}

#[derive(Deserialize)]
struct WalSpecial {
    background: String,
    foreground: String,
}

#[derive(Deserialize)]
struct HyprOption {
    #[serde(default)]
    gradient: String,
}

impl Theme {
    pub fn load() -> Self {
        let (background, foreground, accent) =
            load_wal().unwrap_or_else(|| (FALLBACK.0.into(), FALLBACK.1.into(), FALLBACK.2.into()));
        Self {
            background,
            foreground,
            accent,
            border: load_hypr_border(),
        }
    }

    /// Colori base: lo stile di default li mescola per ottenere il resto.
    pub fn colors_css(&self) -> String {
        format!(
            "@define-color runner_bg {};\n@define-color runner_fg {};\n@define-color runner_accent {};\n",
            self.background, self.foreground, self.accent
        )
    }

    /// Bordo sfumato come quello delle finestre attive di Hyprland. Va caricato
    /// dopo lo stile di default: la sua shorthand `border` azzera `border-image`.
    pub fn border_css(&self) -> String {
        let Some((colors, angle)) = &self.border else {
            return String::new();
        };
        let stops = if colors.len() == 1 {
            format!("{0}, {0}", colors[0])
        } else {
            colors.join(", ")
        };
        format!(
            "window.runner > .runner-frame {{ border-image: linear-gradient({angle}deg, {stops}) 1; }}\n"
        )
    }
}

/// `~/.cache/wal/colors.json`. L'accento è `color4`, come nel tema GTK3:
/// pywal non garantisce il significato dei colori, ma così almeno il
/// launcher e le app GTK usano lo stesso.
fn load_wal() -> Option<(String, String, String)> {
    let home = std::env::var_os("HOME")?;
    let path = std::path::Path::new(&home).join(".cache/wal/colors.json");
    let wal: Wal = serde_json::from_str(&std::fs::read_to_string(path).ok()?).ok()?;
    let accent = wal
        .colors
        .get("color4")
        .cloned()
        .unwrap_or_else(|| FALLBACK.2.into());
    Some((wal.special.background, wal.special.foreground, accent))
}

/// `hyprctl -j getoption general:col.active_border` → `"ee33ccff ee00ff99 45deg"`,
/// colori in AARRGGBB.
fn load_hypr_border() -> Option<(Vec<String>, u32)> {
    std::env::var_os("HYPRLAND_INSTANCE_SIGNATURE")?;
    let out = Command::new("hyprctl")
        .args(["-j", "getoption", "general:col.active_border"])
        .output()
        .ok()?;
    let opt: HyprOption = serde_json::from_slice(&out.stdout).ok()?;
    parse_gradient(&opt.gradient)
}

fn parse_gradient(spec: &str) -> Option<(Vec<String>, u32)> {
    let mut colors = Vec::new();
    let mut angle = 0;
    for token in spec.split_whitespace() {
        if let Some(deg) = token.strip_suffix("deg") {
            angle = deg.parse().ok()?;
        } else if token.len() == 8 && token.chars().all(|c| c.is_ascii_hexdigit()) {
            let a = u8::from_str_radix(&token[0..2], 16).ok()?;
            colors.push(format!("alpha(#{}, {:.2})", &token[2..], a as f32 / 255.0));
        }
    }
    (!colors.is_empty()).then_some((colors, angle))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hypr_gradient() {
        let (colors, angle) = parse_gradient("ee33ccff ee00ff99 45deg").unwrap();
        assert_eq!(angle, 45);
        assert_eq!(colors, ["alpha(#33ccff, 0.93)", "alpha(#00ff99, 0.93)"]);
        assert!(parse_gradient("").is_none());
    }
}
