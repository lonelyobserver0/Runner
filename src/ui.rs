use std::cell::RefCell;
use std::rc::{Rc, Weak};

use gtk::prelude::*;
use gtk::{gdk, gio, glib};
use gtk4_layer_shell::{Edge, KeyboardMode, Layer, LayerShell};

use crate::config::Config;
use crate::elephant::{self, Event, Item, Kind, QueryClient};
use crate::providers::{self, Mode};

const DEFAULT_CSS: &str = include_str!("style.css");

pub struct Options {
    /// Provider forzati da riga di comando (sostituiscono quelli del config).
    pub providers: Option<Vec<String>>,
    pub initial_query: String,
}

/// La query di cui stiamo aspettando i risultati: tutto il resto è superato.
struct Expect {
    /// In modalità singolo provider, gli item devono venire da lì.
    provider: Option<String>,
    query: String,
}

struct State {
    config: Config,
    client: QueryClient,
    /// Modalità attiva (`/bluetooth`): il prompt la mostra, la entry contiene
    /// solo la query. Backspace su entry vuota ne esce.
    active_mode: Option<String>,
    /// Provider installati (arrivano in differita da `elephant listproviders`).
    installed: Vec<String>,
    subscribed_menus: bool,
    /// Risultati della query corrente (o elenco provider in modalità `/`).
    results: Vec<Item>,
    /// Azioni a livello di provider della modalità corrente.
    provider_actions: Vec<Item>,
    /// Provider di cui abbiamo (o abbiamo chiesto) lo stato.
    state_provider: Option<String>,
    /// Righe mostrate: risultati + azioni del provider filtrate.
    rows: Vec<Item>,
    /// qid della risposta in corso: quando cambia, i risultati vanno svuotati.
    qid: i32,
    expect: Option<Expect>,
    /// Query testuale della modalità corrente, per filtrare le azioni del provider.
    mode_query: String,
    providers_override: Option<Vec<String>>,
    rebuild_scheduled: bool,
    /// Colore d'accento, per i caratteri trovati (markup Pango, non CSS).
    accent: String,
    /// L'utente ha spostato la selezione: i rebuild la conservano invece di
    /// tornare alla prima riga. Si azzera quando cambia il testo.
    user_moved: bool,
    query_busy: bool,
    activations_busy: u32,
}

struct Widgets {
    window: gtk::ApplicationWindow,
    entry: gtk::Entry,
    mode: gtk::Label,
    spinner: gtk::Spinner,
    list: gtk::ListBox,
    scroll: gtk::ScrolledWindow,
    footer: gtk::Label,
}

struct Inner {
    state: RefCell<State>,
    w: Widgets,
}

type Shared = Rc<Inner>;

pub fn build(app: &gtk::Application, opts: &Options) {
    let config = Config::load();
    let theme = crate::theme::Theme::load();
    load_css(&theme);

    let (client, events) = match QueryClient::connect(
        opts.providers
            .clone()
            .unwrap_or_else(|| config.providers.clone()),
        config.max_results,
    ) {
        Ok(v) => v,
        Err(e) => {
            eprintln!(
                "runner: cannot connect to elephant ({}): {e}\n\
                 start it with `elephant` or `elephant service enable`",
                elephant::socket_path().display()
            );
            app.quit();
            return;
        }
    };

    let window = gtk::ApplicationWindow::builder()
        .application(app)
        .title("runner")
        .default_width(config.width)
        .css_classes(["runner"])
        .build();

    window.init_layer_shell();
    window.set_namespace(Some("runner"));
    window.set_layer(Layer::Overlay);
    window.set_keyboard_mode(KeyboardMode::Exclusive);
    window.set_anchor(Edge::Top, true);
    window.set_margin(Edge::Top, config.margin_top);

    let mode = gtk::Label::builder()
        .label("❯")
        .css_classes(["runner-mode"])
        .build();

    let entry = gtk::Entry::builder()
        .placeholder_text("Search, or type / for a provider")
        .hexpand(true)
        .css_classes(["runner-entry"])
        .build();

    let spinner = gtk::Spinner::builder()
        .css_classes(["runner-spinner"])
        .build();

    let header = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(8)
        .css_classes(["runner-header"])
        .build();
    header.append(&mode);
    header.append(&entry);
    header.append(&spinner);

    let list = gtk::ListBox::builder()
        .selection_mode(gtk::SelectionMode::Single)
        .activate_on_single_click(true)
        .css_classes(["runner-list"])
        .build();

    let scroll = gtk::ScrolledWindow::builder()
        .hscrollbar_policy(gtk::PolicyType::Never)
        .propagate_natural_height(true)
        .max_content_height(480)
        .child(&list)
        .build();

    let footer = gtk::Label::builder()
        .xalign(0.0)
        .ellipsize(gtk::pango::EllipsizeMode::End)
        .css_classes(["runner-footer"])
        .build();

    let vbox = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .css_classes(["runner-frame"])
        .build();
    vbox.append(&header);
    vbox.append(&scroll);
    vbox.append(&footer);
    window.set_child(Some(&vbox));

    let shared: Shared = Rc::new(Inner {
        state: RefCell::new(State {
            config,
            client,
            active_mode: None,
            installed: Vec::new(),
            subscribed_menus: false,
            results: Vec::new(),
            provider_actions: Vec::new(),
            state_provider: None,
            rows: Vec::new(),
            qid: -1,
            expect: None,
            mode_query: String::new(),
            providers_override: opts.providers.clone(),
            rebuild_scheduled: false,
            accent: theme.accent.clone(),
            user_moved: false,
            query_busy: false,
            activations_busy: 0,
        }),
        w: Widgets {
            window: window.clone(),
            entry: entry.clone(),
            mode,
            spinner,
            list: list.clone(),
            scroll,
            footer,
        },
    });

    // Risposte di elephant → UI.
    glib::spawn_future_local(glib::clone!(
        #[weak(rename_to = shared)]
        shared,
        async move {
            while let Ok(event) = events.recv().await {
                handle_event(&shared, event);
            }
        }
    ));

    // Provider installati: servono solo per `/`, quindi non blocchiamo l'avvio.
    glib::spawn_future_local(glib::clone!(
        #[weak(rename_to = shared)]
        shared,
        async move {
            let installed = gio::spawn_blocking(providers::discover)
                .await
                .unwrap_or_default();
            on_providers_discovered(&shared, installed);
        }
    ));

    entry.connect_changed(glib::clone!(
        #[weak(rename_to = shared)]
        shared,
        move |_| refresh(&shared)
    ));

    // Le azioni del provider sono un gruppo a sé: intestazione sulla prima.
    // Guarda solo i widget (classe e tooltip della riga) perché viene chiamata
    // durante il rebuild, quando lo stato è già in prestito.
    list.set_header_func(|row, before| {
        let is_action = |r: &gtk::ListBoxRow| r.has_css_class("provider-action");
        if !is_action(row) || before.is_some_and(is_action) {
            row.set_header(None::<&gtk::Widget>);
            return;
        }
        let provider = row.tooltip_text().unwrap_or_default();
        let header = gtk::Label::builder()
            .label(format!("{} commands", providers::describe(&provider).0))
            .xalign(0.0)
            .css_classes(["section-header"])
            .build();
        row.set_header(Some(&header));
    });

    list.connect_row_activated(glib::clone!(
        #[weak(rename_to = shared)]
        shared,
        move |_, row| activate(&shared, row.index() as usize, 0)
    ));

    list.connect_row_selected(glib::clone!(
        #[weak(rename_to = shared)]
        shared,
        move |_, _| update_footer(&shared)
    ));

    let keys = gtk::EventControllerKey::new();
    keys.set_propagation_phase(gtk::PropagationPhase::Capture);
    keys.connect_key_pressed(glib::clone!(
        #[weak(rename_to = shared)]
        shared,
        #[upgrade_or]
        glib::Propagation::Proceed,
        move |_, key, _, mods| on_key(&shared, key, mods)
    ));
    window.add_controller(keys);

    if shared.state.borrow().config.close_on_focus_loss {
        window.connect_is_active_notify(|w| {
            if !w.is_active() {
                w.close();
            }
        });
    }

    // Tiene vivo lo stato finché la finestra esiste.
    window.connect_destroy(move |_| {
        let _ = &shared;
    });

    window.present();
    if opts.initial_query.is_empty() {
        // `changed` non scatta su testo vuoto: prima query esplicita.
        entry.emit_by_name::<()>("changed", &[]);
    } else {
        entry.set_text(&opts.initial_query);
        entry.set_position(-1);
    }
}

fn load_css(theme: &crate::theme::Theme) {
    let display = gdk::Display::default().expect("no display");

    // Colori del desktop + stile di default nello stesso provider, così i
    // `@runner_*` si risolvono; lo style.css utente può ridefinirli.
    let base = gtk::CssProvider::new();
    base.load_from_string(&format!(
        "{}\n{DEFAULT_CSS}\n{}",
        theme.colors_css(),
        theme.border_css()
    ));
    gtk::style_context_add_provider_for_display(
        &display,
        &base,
        gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
    );

    let user_css = crate::config::config_dir().join("style.css");
    if user_css.exists() {
        let user = gtk::CssProvider::new();
        user.load_from_path(&user_css);
        gtk::style_context_add_provider_for_display(
            &display,
            &user,
            gtk::STYLE_PROVIDER_PRIORITY_USER,
        );
    }
}

fn on_providers_discovered(shared: &Shared, installed: Vec<String>) {
    let mut st = shared.state.borrow_mut();
    st.installed = installed;
    // I sottomenu si aprono solo via notifica push di elephant.
    if !st.subscribed_menus && st.installed.iter().any(|p| p.starts_with("menus:")) {
        match st.client.subscribe("menus") {
            Ok(()) => st.subscribed_menus = true,
            Err(e) => eprintln!("runner: menu subscription failed: {e}"),
        }
    }
    drop(st);
    // Il testo iniziale (es. `-q /bluetooth`) può cambiare significato ora.
    refresh(shared);
}

/// Interpreta il testo della entry e chiede a elephant i dati corrispondenti.
fn refresh(shared: &Shared) {
    let text = shared.w.entry.text();
    let mut guard = shared.state.borrow_mut();
    let st = &mut *guard;
    let mode = providers::parse_mode(&text, st.active_mode.as_deref(), &st.config, &st.installed);
    st.user_moved = false;

    if let Mode::Enter { provider, query } = mode {
        let query = query.to_owned();
        drop(guard);
        // Non si cambia il testo della entry dentro il suo stesso `changed`.
        let weak = Rc::downgrade(shared);
        glib::idle_add_local_once(move || {
            if let Some(shared) = weak.upgrade() {
                enter_mode(&shared, &provider, &query);
            }
        });
        return;
    }

    let provider = mode.provider().map(str::to_owned);
    if provider.is_none() {
        st.provider_actions.clear();
        st.state_provider = None;
    } else if st.state_provider != provider {
        st.provider_actions.clear();
        st.state_provider = provider.clone();
        if let Err(e) = st
            .client
            .request_state(provider.as_deref().unwrap_or_default())
        {
            eprintln!("runner: provider state request failed: {e}");
        }
    }

    match mode {
        Mode::ProviderList { filter } => {
            st.expect = None;
            st.query_busy = false;
            st.qid = -1;
            st.mode_query = String::new();
            st.results = provider_list(&st.installed, filter);
        }
        Mode::Single {
            ref provider,
            query,
        } => {
            let providers = [provider.clone()];
            send_query(st, Some(&providers), provider.clone().into(), query);
        }
        Mode::Enter { .. } => unreachable!("gestito sopra"),
        Mode::Default { query } => {
            let providers = st.providers_override.clone();
            send_query(st, providers.as_deref(), None, query);
        }
    }

    // Il prompt dice dove si sta cercando, come in una shell.
    let (prompt, placeholder) = match &st.state_provider {
        Some(p) => {
            let name = providers::describe(p).0;
            let placeholder = if st.active_mode.is_some() {
                format!("Search {name}, Backspace to leave")
            } else {
                format!("Search {name}")
            };
            (format!("{} ❯", name.to_lowercase()), placeholder)
        }
        None => (
            "❯".to_owned(),
            "Search, or type / for a provider".to_owned(),
        ),
    };
    drop(guard);
    shared.w.mode.set_text(&prompt);
    shared.w.entry.set_placeholder_text(Some(&placeholder));
    update_spinner(shared);
    schedule_rebuild(shared);
}

fn send_query(st: &mut State, providers: Option<&[String]>, only: Option<String>, query: &str) {
    if let Err(e) = st.client.query(providers, query) {
        eprintln!("runner: query failed: {e}");
    }
    st.expect = Some(Expect {
        provider: only,
        query: query.to_owned(),
    });
    st.mode_query = query.to_owned();
    st.query_busy = true;
}

/// Voci dell'elenco `/`, filtrate per nome tecnico o leggibile.
fn provider_list(installed: &[String], filter: &str) -> Vec<Item> {
    let filter = filter.trim().to_lowercase();
    installed
        .iter()
        .filter_map(|p| {
            let (pretty, icon) = providers::describe(p);
            let matches = filter.is_empty()
                || p.to_lowercase().contains(&filter)
                || pretty.to_lowercase().contains(&filter);
            matches.then(|| Item {
                identifier: p.clone(),
                text: pretty,
                subtext: format!("/{p}"),
                icon: icon.to_owned(),
                provider: p.clone(),
                kind: Kind::Provider,
                ..Default::default()
            })
        })
        .collect()
}

fn handle_event(shared: &Shared, event: Event) {
    match event {
        Event::Item { qid, query, item } => {
            let mut st = shared.state.borrow_mut();
            let Some(expect) = &st.expect else { return };
            if query != expect.query
                || expect
                    .provider
                    .as_ref()
                    .is_some_and(|p| *p != item.provider)
            {
                return;
            }
            if qid != st.qid {
                st.qid = qid;
                st.results.clear();
            }
            st.results.push(item);
        }
        Event::Update { item } => {
            let mut st = shared.state.borrow_mut();
            let found = st
                .results
                .iter_mut()
                .find(|it| it.identifier == item.identifier && it.provider == item.provider);
            let Some(slot) = found else { return };
            *slot = item;
        }
        Event::State { provider, actions } => {
            let mut st = shared.state.borrow_mut();
            if st.state_provider.as_deref() != Some(provider.as_str()) {
                return;
            }
            let icon = providers::describe(&provider).1;
            st.provider_actions = actions
                .into_iter()
                .map(|action| Item {
                    identifier: provider.clone(),
                    text: providers::action_label(&action),
                    icon: icon.to_owned(),
                    provider: provider.clone(),
                    actions: vec![action],
                    kind: Kind::ProviderAction,
                    ..Default::default()
                })
                .collect();
        }
        Event::Subscription(value) => {
            // `menus:<nome>`: elephant chiede di passare a quel (sotto)menu.
            if value.strip_prefix("menus:").is_some_and(|m| !m.is_empty()) {
                enter_mode(shared, &value, "");
            }
            return;
        }
        Event::NoResults => {
            let mut st = shared.state.borrow_mut();
            if st.expect.is_none() {
                return;
            }
            st.results.clear();
            st.qid = -1;
            st.query_busy = false;
            drop(st);
            update_spinner(shared);
        }
        Event::Done => {
            shared.state.borrow_mut().query_busy = false;
            update_spinner(shared);
            return;
        }
        Event::Disconnected(e) => {
            eprintln!("runner: connection to elephant closed: {e}");
            shared.w.window.close();
            return;
        }
    }
    schedule_rebuild(shared);
}

/// Entra nella modalità di un provider; nella entry resta solo `query`.
fn enter_mode(shared: &Shared, provider: &str, query: &str) {
    {
        let mut st = shared.state.borrow_mut();
        if !st.installed.iter().any(|p| p == provider) {
            st.installed.push(provider.to_owned());
        }
        st.active_mode = Some(provider.to_owned());
    }
    set_entry_text(shared, query);
}

fn exit_mode(shared: &Shared) {
    shared.state.borrow_mut().active_mode = None;
    set_entry_text(shared, "");
}

/// Cambia il testo e rilancia la ricerca anche se il testo non cambia
/// (in quel caso `changed` non scatta, ma la modalità sì).
fn set_entry_text(shared: &Shared, text: &str) {
    let entry = &shared.w.entry;
    if entry.text() == text {
        refresh(shared);
    } else {
        entry.set_text(text);
    }
    entry.set_position(-1);
}

/// Gli item arrivano a raffica: si ricostruisce la lista una volta sola, a raffica finita.
fn schedule_rebuild(shared: &Shared) {
    let mut st = shared.state.borrow_mut();
    if st.rebuild_scheduled {
        return;
    }
    st.rebuild_scheduled = true;
    let weak: Weak<Inner> = Rc::downgrade(shared);
    glib::idle_add_local_once(move || {
        if let Some(shared) = weak.upgrade() {
            rebuild(&shared);
        }
    });
}

fn rebuild(shared: &Shared) {
    let w = &shared.w;
    let mut st = shared.state.borrow_mut();
    st.rebuild_scheduled = false;

    let selected_key = w
        .list
        .selected_row()
        .filter(|_| st.user_moved)
        .and_then(|r| st.rows.get(r.index() as usize))
        .map(|it| {
            (
                it.kind,
                it.provider.clone(),
                it.identifier.clone(),
                it.actions.first().cloned(),
            )
        });

    let needle = st.mode_query.trim().to_lowercase();
    // Prima i risultati, poi le azioni del provider: Invio appena entrati in
    // `/bluetooth` deve connettere un dispositivo, non spegnere tutto.
    let mut rows = st.results.clone();
    rows.extend(
        st.provider_actions
            .iter()
            .filter(|a| needle.is_empty() || a.text.to_lowercase().contains(&needle))
            .cloned(),
    );

    // La rimozione della riga selezionata emette `row-selected`: update_footer
    // lo tollera (try_borrow) e viene richiamato da `select` qui sotto.
    while let Some(row) = w.list.row_at_index(0) {
        w.list.remove(&row);
    }
    for item in &rows {
        let row = make_row(item, st.config.icon_size, &st.accent);
        w.list.append(&row);
    }

    let index = selected_key
        .and_then(|key| {
            rows.iter().position(|it| {
                (it.kind, &it.provider, &it.identifier, it.actions.first())
                    == (key.0, &key.1, &key.2, key.3.as_ref())
            })
        })
        .unwrap_or(0);
    st.rows = rows;
    drop(st);
    select(shared, index);
    update_footer(shared);
}

fn make_row(item: &Item, icon_size: i32, accent: &str) -> gtk::ListBoxRow {
    let hbox = gtk::Box::new(gtk::Orientation::Horizontal, 12);

    let icon = if item.icon.starts_with('/') {
        gtk::Image::from_file(&item.icon)
    } else if item.icon.is_empty() {
        gtk::Image::from_icon_name("application-x-executable")
    } else {
        gtk::Image::from_icon_name(&item.icon)
    };
    icon.set_pixel_size(icon_size);
    icon.add_css_class("item-icon");
    hbox.append(&icon);

    let texts = gtk::Box::new(gtk::Orientation::Vertical, 2);
    texts.set_valign(gtk::Align::Center);
    texts.set_hexpand(true);
    texts.append(&text_label(item, "text", &item.text, "item-text", accent));
    if !item.subtext.is_empty() {
        texts.append(&text_label(
            item,
            "subtext",
            &item.subtext,
            "item-subtext",
            accent,
        ));
    }
    hbox.append(&texts);

    let class = match item.kind {
        Kind::Result => "result",
        Kind::ProviderAction => "provider-action",
        Kind::Provider => "provider",
    };
    let row = gtk::ListBoxRow::builder()
        .child(&hbox)
        .css_classes(["item", class])
        .build();
    row.set_tooltip_text(Some(&item.provider));
    row
}

/// Label con i caratteri che hanno fatto match nel colore d'accento.
fn text_label(item: &Item, field: &str, text: &str, class: &str, accent: &str) -> gtk::Label {
    let positions: &[i32] = match &item.fuzzyinfo {
        Some(f) if f.field == field => &f.positions,
        _ => &[],
    };
    let open = format!("<span foreground=\"{accent}\" weight=\"bold\">");
    let mut markup = String::with_capacity(text.len() + positions.len() * (open.len() + 7));
    for (i, ch) in text.chars().enumerate() {
        let escaped = glib::markup_escape_text(ch.encode_utf8(&mut [0; 4]));
        if positions.contains(&(i as i32)) {
            markup.push_str(&open);
            markup.push_str(&escaped);
            markup.push_str("</span>");
        } else {
            markup.push_str(&escaped);
        }
    }
    gtk::Label::builder()
        .label(markup)
        .use_markup(true)
        .xalign(0.0)
        .ellipsize(gtk::pango::EllipsizeMode::End)
        .css_classes([class])
        .build()
}

fn select(shared: &Shared, index: usize) {
    let w = &shared.w;
    let Some(row) = w.list.row_at_index(index as i32) else {
        return;
    };
    w.list.select_row(Some(&row));

    // Scroll manuale: non spostiamo il focus dalla entry.
    let adj = w.scroll.vadjustment();
    if let Some(bounds) = row.compute_bounds(&w.list) {
        let (top, bottom) = (bounds.y() as f64, (bounds.y() + bounds.height()) as f64);
        if top < adj.value() {
            adj.set_value(top);
        } else if bottom > adj.value() + adj.page_size() {
            adj.set_value(bottom - adj.page_size());
        }
    }
}

fn move_selection(shared: &Shared, delta: i32) {
    let len = shared.state.borrow().rows.len() as i32;
    if len == 0 {
        return;
    }
    shared.state.borrow_mut().user_moved = true;
    let current = shared.w.list.selected_row().map_or(-1, |r| r.index());
    select(shared, (current + delta).rem_euclid(len) as usize);
}

/// Azioni dell'item nell'ordine in cui le mappiamo su Invio, Alt+2, Alt+3…
fn item_actions(st: &State, item: &Item) -> Vec<String> {
    match item.kind {
        Kind::Provider => Vec::new(),
        Kind::ProviderAction => item.actions.clone(),
        Kind::Result => providers::order_actions(&item.actions, &st.config.primary_actions),
    }
}

fn update_footer(shared: &Shared) {
    let w = &shared.w;
    // `row-selected` può scattare mentre lo stato è già in prestito (rimozione
    // della riga selezionata): in quel caso aggiorna chi ha il borrow.
    let Ok(st) = shared.state.try_borrow() else {
        return;
    };
    let item = w
        .list
        .selected_row()
        .and_then(|r| st.rows.get(r.index() as usize));
    let key = |k: &str, label: &str| format!("<b>{k}</b>  {}", glib::markup_escape_text(label));
    let hints: Vec<String> = match item {
        Some(item) if item.kind == Kind::Provider => vec![key("Enter", "Open this mode")],
        Some(item) => item_actions(&st, item)
            .iter()
            .take(9)
            .enumerate()
            .map(|(i, a)| {
                let label = providers::action_label(a);
                if i == 0 {
                    key("Enter", &label)
                } else {
                    key(&format!("Alt+{}", i + 1), &label)
                }
            })
            .collect(),
        None => Vec::new(),
    };
    w.footer.set_markup(&hints.join("      "));
    w.footer.set_visible(!hints.is_empty());
}

fn update_spinner(shared: &Shared) {
    let st = shared.state.borrow();
    let busy = st.query_busy || st.activations_busy > 0;
    shared.w.spinner.set_spinning(busy);
}

fn on_key(shared: &Shared, key: gdk::Key, mods: gdk::ModifierType) -> glib::Propagation {
    let ctrl = mods.contains(gdk::ModifierType::CONTROL_MASK);
    let alt = mods.contains(gdk::ModifierType::ALT_MASK);

    match key {
        gdk::Key::Escape => shared.w.window.close(),
        gdk::Key::BackSpace
            if shared.w.entry.text().is_empty() && shared.state.borrow().active_mode.is_some() =>
        {
            exit_mode(shared)
        }
        gdk::Key::Tab if selected_kind(shared) == Some(Kind::Provider) => {
            activate_selected(shared, 0)
        }
        gdk::Key::Down | gdk::Key::Tab => move_selection(shared, 1),
        gdk::Key::Up | gdk::Key::ISO_Left_Tab => move_selection(shared, -1),
        gdk::Key::j | gdk::Key::n if ctrl => move_selection(shared, 1),
        gdk::Key::k | gdk::Key::p if ctrl => move_selection(shared, -1),
        gdk::Key::Return | gdk::Key::KP_Enter => activate_selected(shared, 0),
        _ if alt => match key.to_unicode().and_then(|c| c.to_digit(10)) {
            Some(n @ 1..=9) => activate_selected(shared, n as usize - 1),
            _ => return glib::Propagation::Proceed,
        },
        _ => return glib::Propagation::Proceed,
    }
    glib::Propagation::Stop
}

fn selected_kind(shared: &Shared) -> Option<Kind> {
    let row = shared.w.list.selected_row()?;
    shared
        .state
        .borrow()
        .rows
        .get(row.index() as usize)
        .map(|it| it.kind)
}

fn activate_selected(shared: &Shared, action: usize) {
    if let Some(row) = shared.w.list.selected_row() {
        activate(shared, row.index() as usize, action);
    }
}

fn activate(shared: &Shared, index: usize, action_index: usize) {
    let st = shared.state.borrow();
    let Some(item) = st.rows.get(index).cloned() else {
        return;
    };

    if item.kind == Kind::Provider {
        drop(st);
        return enter_mode(shared, &item.identifier, "");
    }

    let actions = item_actions(&st, &item);
    let action = match actions.get(action_index) {
        Some(a) => a.clone(),
        // Un provider senza azioni dichiarate usa quella di default ("").
        None if action_index == 0 => String::new(),
        None => return,
    };

    // Sottomenu: elephant si limiterebbe a notificarci di aprirlo, lo facciamo noi.
    if action == "menus:open"
        && let Some(sub) = item
            .identifier
            .strip_prefix("menus:")
            .and_then(|r| r.split(':').next())
    {
        let sub = format!("menus:{sub}");
        drop(st);
        return enter_mode(shared, &sub, "");
    }

    let query = st.mode_query.clone();
    let single = st.state_provider.is_some();
    let base_provider = item.provider.split(':').next().unwrap_or_default();
    let keep_open =
        item.kind == Kind::ProviderAction || st.config.keep_open.iter().any(|p| p == base_provider);
    drop(st);

    let window = shared.w.window.clone();
    // L'hold tiene vivo il processo finché elephant non conferma, anche se la
    // finestra si chiude nel frattempo.
    let hold = window.application().map(|app| app.hold());

    if !keep_open {
        window.set_visible(false);
        glib::spawn_future_local(async move {
            let result =
                gio::spawn_blocking(move || elephant::activate(&item, &action, &query, single))
                    .await;
            if let Ok(Err(e)) = result {
                eprintln!("runner: activation failed: {e}");
            }
            window.close();
            drop(hold);
        });
        return;
    }

    // Provider interattivo: si resta aperti e si ricarica a lavoro finito.
    shared.state.borrow_mut().activations_busy += 1;
    update_spinner(shared);
    let weak = Rc::downgrade(shared);
    glib::spawn_future_local(async move {
        let result =
            gio::spawn_blocking(move || elephant::activate(&item, &action, &query, single)).await;
        if let Ok(Err(e)) = result {
            eprintln!("runner: activation failed: {e}");
        }
        if let Some(shared) = weak.upgrade() {
            {
                let mut st = shared.state.borrow_mut();
                st.activations_busy -= 1;
                // Forza una nuova richiesta di stato (es. acceso ↔ spento).
                st.state_provider = None;
            }
            refresh(&shared);
        }
        drop(hold);
    });
}
