mod config;
mod elephant;
mod providers;
mod ui;

use gtk::prelude::*;
use gtk::{gio, glib};

const APP_ID: &str = "dev.loneobs.Runner";

const HELP: &str = "\
runner — launcher Wayland basato su elephant

USO: runner [-p PROVIDER]... [-q QUERY]

Nella barra: `/` elenca i provider installati, `/bluetooth` entra in quella
modalità (idem `/menus:<nome>`). Esempio: runner -q /bluetooth

  -p, --provider NOME  interroga solo questo provider (ripetibile)
  -q, --query TESTO    testo iniziale nella barra di ricerca
  -h, --help           mostra questo aiuto

Se runner è già aperto, lanciarlo di nuovo lo chiude (comodo da bindare).
Config: ~/.config/runner/config.toml, stile: ~/.config/runner/style.css";

fn parse_args() -> Result<ui::Options, String> {
    let mut providers = Vec::new();
    let mut initial_query = String::new();
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "-p" | "--provider" => providers.push(args.next().ok_or("manca il nome del provider")?),
            "-q" | "--query" => initial_query = args.next().ok_or("manca il testo della query")?,
            "-h" | "--help" => {
                println!("{HELP}");
                std::process::exit(0);
            }
            other => return Err(format!("argomento sconosciuto: {other}")),
        }
    }
    Ok(ui::Options {
        providers: (!providers.is_empty()).then_some(providers),
        initial_query,
    })
}

fn main() -> glib::ExitCode {
    // Il renderer Vulkan di default enumera tutte le GPU: sui portatili ibridi
    // sveglia la dGPU sospesa e l'avvio passa da ~0.2s a ~2s. GL usa solo
    // quella del display. Resta sovrascrivibile dall'ambiente.
    if std::env::var_os("GSK_RENDERER").is_none() {
        // SAFETY: siamo ancora single-thread, prima di inizializzare GTK.
        unsafe { std::env::set_var("GSK_RENDERER", "gl") };
    }

    let opts = match parse_args() {
        Ok(o) => o,
        Err(e) => {
            eprintln!("runner: {e}\n\n{HELP}");
            return glib::ExitCode::FAILURE;
        }
    };

    // Istanza unica: una seconda invocazione arriva qui come `activate`
    // sull'istanza già attiva, che si chiude (toggle).
    let app = gtk::Application::new(Some(APP_ID), gio::ApplicationFlags::empty());
    app.connect_activate(move |app| match app.active_window() {
        // Una finestra nascosta sta solo finendo un'attivazione: non conta.
        Some(window) if window.is_visible() => window.close(),
        _ => ui::build(app, &opts),
    });

    // Gli argomenti li abbiamo già letti noi.
    app.run_with_args::<&str>(&[])
}
