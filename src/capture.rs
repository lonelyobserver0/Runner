//! Anteprime delle finestre.
//!
//! Elephant dà solo titolo, app_id e un id interno. Per catturare una finestra
//! serve il suo identificativo `ext-foreign-toplevel-list`: lo leggiamo con una
//! connessione Wayland nostra, poi la cattura la fa `grim -T <identificativo>`
//! (protocolli `ext-foreign-toplevel-image-capture-source` + `ext-image-copy-capture`).

use std::error::Error;
use std::process::Command;

use wayland_client::backend::ObjectId;
use wayland_client::globals::{GlobalListContents, registry_queue_init};
use wayland_client::protocol::wl_registry;
use wayland_client::{Connection, Dispatch, Proxy, QueueHandle, event_created_child};
use wayland_protocols::ext::foreign_toplevel_list::v1::client::{
    ext_foreign_toplevel_handle_v1::{self, ExtForeignToplevelHandleV1},
    ext_foreign_toplevel_list_v1::{self, ExtForeignToplevelListV1},
};

#[derive(Default, Clone, Debug)]
pub struct Toplevel {
    pub identifier: String,
    pub title: String,
    pub app_id: String,
}

/// In ordine di apertura: serve a distinguere finestre con titolo uguale.
#[derive(Default)]
struct ListState {
    toplevels: Vec<(ObjectId, Toplevel)>,
}

impl Dispatch<wl_registry::WlRegistry, GlobalListContents> for ListState {
    fn event(
        _: &mut Self,
        _: &wl_registry::WlRegistry,
        _: wl_registry::Event,
        _: &GlobalListContents,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<ExtForeignToplevelListV1, ()> for ListState {
    fn event(
        state: &mut Self,
        _: &ExtForeignToplevelListV1,
        event: ext_foreign_toplevel_list_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if let ext_foreign_toplevel_list_v1::Event::Toplevel { toplevel } = event {
            state.toplevels.push((toplevel.id(), Toplevel::default()));
        }
    }

    event_created_child!(ListState, ExtForeignToplevelListV1, [
        ext_foreign_toplevel_list_v1::EVT_TOPLEVEL_OPCODE => (ExtForeignToplevelHandleV1, ()),
    ]);
}

impl Dispatch<ExtForeignToplevelHandleV1, ()> for ListState {
    fn event(
        state: &mut Self,
        handle: &ExtForeignToplevelHandleV1,
        event: ext_foreign_toplevel_handle_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        let id = handle.id();
        let Some((_, t)) = state.toplevels.iter_mut().find(|(h, _)| *h == id) else {
            return;
        };
        match event {
            ext_foreign_toplevel_handle_v1::Event::Title { title } => t.title = title,
            ext_foreign_toplevel_handle_v1::Event::AppId { app_id } => t.app_id = app_id,
            ext_foreign_toplevel_handle_v1::Event::Identifier { identifier } => {
                t.identifier = identifier
            }
            _ => {}
        }
    }
}

/// Le finestre aperte con il loro identificativo, in un colpo solo.
pub fn list_toplevels() -> Result<Vec<Toplevel>, Box<dyn Error>> {
    let conn = Connection::connect_to_env()?;
    let (globals, mut queue) = registry_queue_init::<ListState>(&conn)?;
    let qh = queue.handle();
    let list: ExtForeignToplevelListV1 = globals.bind(&qh, 1..=1, ())?;
    let mut state = ListState::default();
    // Primo giro: arrivano gli handle; secondo: i loro dettagli.
    queue.roundtrip(&mut state)?;
    queue.roundtrip(&mut state)?;
    list.stop();
    Ok(state
        .toplevels
        .into_iter()
        .map(|(_, t)| t)
        .filter(|t| !t.identifier.is_empty())
        .collect())
}

/// L'identificativo della finestra corrispondente a un item del provider
/// windows (titolo nel `text`, app_id nel `subtext`). Con più finestre uguali,
/// la n-esima occorrenza tra gli item va alla n-esima tra le finestre.
pub fn match_window<'a>(
    toplevels: &'a [Toplevel],
    title: &str,
    app_id: &str,
    occurrence: usize,
) -> Option<&'a str> {
    toplevels
        .iter()
        .filter(|t| t.title == title && t.app_id == app_id)
        .nth(occurrence)
        .map(|t| t.identifier.as_str())
}

/// Cattura una finestra ridotta di `scale` e restituisce un JPEG.
pub fn capture(identifier: &str, scale: f64) -> Result<Vec<u8>, Box<dyn Error>> {
    let out = Command::new("grim")
        .args([
            "-T",
            identifier,
            "-s",
            &scale.to_string(),
            "-t",
            "jpeg",
            "-q",
            "80",
            "-",
        ])
        .output()?;
    if !out.status.success() || out.stdout.is_empty() {
        return Err(format!("grim: {}", String::from_utf8_lossy(&out.stderr).trim()).into());
    }
    Ok(out.stdout)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn duplicate_titles_match_in_order() {
        let t = |id: &str, title: &str| Toplevel {
            identifier: id.into(),
            title: title.into(),
            app_id: "kitty".into(),
        };
        let list = [t("a", "fish"), t("b", "vim"), t("c", "fish")];
        assert_eq!(match_window(&list, "fish", "kitty", 0), Some("a"));
        assert_eq!(match_window(&list, "fish", "kitty", 1), Some("c"));
        assert_eq!(match_window(&list, "fish", "firefox", 0), None);
    }

    /// Richiede una sessione Wayland: `cargo test -- --ignored --nocapture`.
    #[test]
    #[ignore]
    fn lists_and_captures_live_windows() {
        let toplevels = list_toplevels().expect("ext-foreign-toplevel-list");
        assert!(!toplevels.is_empty());
        let dir = std::env::var("RUNNER_CAPTURE_DIR").ok();
        for t in &toplevels {
            let jpeg = capture(&t.identifier, 0.25).expect("grim -T");
            println!("{}\t{}\t{} byte", t.identifier, t.app_id, jpeg.len());
            if let Some(dir) = &dir {
                std::fs::write(format!("{dir}/{}.jpg", t.app_id), &jpeg).unwrap();
            }
        }
    }
}
