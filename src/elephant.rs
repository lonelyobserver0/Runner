//! Client minimale per il socket di elephant.
//!
//! Wire format (vedi `internal/comm/comm.go` in elephant):
//! - richiesta: `[tipo u8][formato u8][lunghezza u32 BE][payload]`
//! - risposta:  `[tipo u8][lunghezza u32 BE][payload]`
//!
//! Usiamo il formato JSON (1) così non serve generare codice protobuf.

use std::io::{self, Read, Write};
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::thread;
use std::time::Duration;

use serde::{Deserialize, Serialize};

const REQ_QUERY: u8 = 0;
const REQ_ACTIVATE: u8 = 1;
const REQ_SUBSCRIBE: u8 = 2;
const REQ_STATE: u8 = 4;
const FORMAT_JSON: u8 = 1;

const RESP_ITEM: u8 = 0;
const RESP_ASYNC_ITEM: u8 = 1;
const RESP_ACTIVATION_FINISHED: u8 = 2;
const RESP_PROVIDER_STATE: u8 = 3;
/// Sulla connessione di sottoscrizione il tipo 0 è "dati cambiati".
const RESP_SUBSCRIPTION_CHANGED: u8 = 0;
const RESP_NO_RESULTS: u8 = 254;
const RESP_QUERY_DONE: u8 = 255;

#[derive(Serialize)]
struct QueryRequest<'a> {
    providers: &'a [String],
    query: &'a str,
    maxresults: i32,
    exactsearch: bool,
}

#[derive(Serialize)]
struct ActivateRequest<'a> {
    provider: &'a str,
    identifier: &'a str,
    action: &'a str,
    query: &'a str,
    arguments: &'a str,
    single: bool,
}

#[derive(Serialize)]
struct StateRequest<'a> {
    provider: &'a str,
}

#[derive(Serialize)]
struct SubscribeRequest<'a> {
    interval: i32,
    provider: &'a str,
    query: &'a str,
}

#[derive(Deserialize)]
struct StateResponse {
    #[serde(default)]
    provider: String,
    #[serde(default)]
    actions: Vec<String>,
}

#[derive(Deserialize)]
struct SubscribeResponse {
    #[serde(default)]
    value: String,
}

#[derive(Deserialize)]
struct QueryResponse {
    #[serde(default)]
    query: String,
    item: Option<Item>,
    #[serde(default)]
    qid: i32,
}

#[derive(Deserialize, Default, Clone, Debug)]
pub struct FuzzyInfo {
    #[serde(default)]
    pub field: String,
    #[serde(default)]
    pub positions: Vec<i32>,
}

/// Cosa rappresenta una riga della lista: solo `Result` arriva da elephant,
/// le altre le costruisce runner.
#[derive(Default, Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    #[default]
    Result,
    /// Azione a livello di provider (es. accendi/spegni bluetooth).
    ProviderAction,
    /// Voce dell'elenco provider mostrato da `/`.
    Provider,
}

#[derive(Deserialize, Default, Clone, Debug)]
pub struct Item {
    #[serde(default)]
    pub identifier: String,
    #[serde(default)]
    pub text: String,
    #[serde(default)]
    pub subtext: String,
    #[serde(default)]
    pub icon: String,
    #[serde(default)]
    pub provider: String,
    pub fuzzyinfo: Option<FuzzyInfo>,
    #[serde(default)]
    pub actions: Vec<String>,
    #[serde(skip)]
    pub kind: Kind,
}

#[derive(Debug)]
pub enum Event {
    /// Un risultato della query `query` (`qid` identifica la richiesta lato server).
    Item {
        qid: i32,
        query: String,
        item: Item,
    },
    /// Aggiornamento asincrono di un item già mostrato.
    Update {
        item: Item,
    },
    /// Azioni a livello di provider.
    State {
        provider: String,
        actions: Vec<String>,
    },
    /// Notifica da una sottoscrizione, es. `menus:<menu da aprire>`.
    Subscription(String),
    NoResults,
    Done,
    Disconnected(String),
}

pub fn socket_path() -> PathBuf {
    match std::env::var_os("XDG_RUNTIME_DIR") {
        Some(dir) => PathBuf::from(dir).join("elephant/elephant.sock"),
        None => std::env::temp_dir().join("elephant/elephant.sock"),
    }
}

fn send(stream: &mut UnixStream, kind: u8, payload: &[u8]) -> io::Result<()> {
    let mut buf = Vec::with_capacity(6 + payload.len());
    buf.push(kind);
    buf.push(FORMAT_JSON);
    buf.extend_from_slice(&(payload.len() as u32).to_be_bytes());
    buf.extend_from_slice(payload);
    stream.write_all(&buf)
}

fn recv(stream: &mut UnixStream) -> io::Result<(u8, Vec<u8>)> {
    let mut header = [0u8; 5];
    stream.read_exact(&mut header)?;
    let len = u32::from_be_bytes(header[1..5].try_into().unwrap()) as usize;
    let mut payload = vec![0u8; len];
    stream.read_exact(&mut payload)?;
    Ok((header[0], payload))
}

/// Connessione persistente per le query. Elephant annulla da sé la query
/// precedente sulla stessa connessione, quindi basta inviare ad ogni tasto.
pub struct QueryClient {
    stream: UnixStream,
    providers: Vec<String>,
    max_results: i32,
    events: async_channel::Sender<Event>,
}

impl QueryClient {
    pub fn connect(
        providers: Vec<String>,
        max_results: i32,
    ) -> io::Result<(Self, async_channel::Receiver<Event>)> {
        let stream = UnixStream::connect(socket_path())?;
        let mut reader = stream.try_clone()?;
        let (tx, rx) = async_channel::unbounded();
        let events = tx.clone();

        thread::spawn(move || {
            loop {
                let event = match recv(&mut reader) {
                    Ok((kind, payload)) => match kind {
                        RESP_ITEM | RESP_ASYNC_ITEM => {
                            match serde_json::from_slice::<QueryResponse>(&payload) {
                                Ok(QueryResponse {
                                    query,
                                    item: Some(item),
                                    qid,
                                }) => {
                                    if kind == RESP_ITEM {
                                        Event::Item { qid, query, item }
                                    } else {
                                        Event::Update { item }
                                    }
                                }
                                Ok(_) => continue,
                                Err(e) => {
                                    eprintln!("runner: invalid response: {e}");
                                    continue;
                                }
                            }
                        }
                        RESP_PROVIDER_STATE => {
                            match serde_json::from_slice::<StateResponse>(&payload) {
                                Ok(r) => Event::State {
                                    provider: r.provider,
                                    actions: r.actions,
                                },
                                Err(e) => {
                                    eprintln!("runner: invalid provider state: {e}");
                                    continue;
                                }
                            }
                        }
                        RESP_NO_RESULTS => Event::NoResults,
                        RESP_QUERY_DONE => Event::Done,
                        _ => continue,
                    },
                    Err(e) => {
                        let _ = tx.send_blocking(Event::Disconnected(e.to_string()));
                        break;
                    }
                };
                if tx.send_blocking(event).is_err() {
                    break;
                }
            }
        });

        Ok((
            Self {
                stream,
                providers,
                max_results,
                events,
            },
            rx,
        ))
    }

    /// Chiede le azioni a livello di provider; la risposta arriva come `Event::State`.
    pub fn request_state(&mut self, provider: &str) -> io::Result<()> {
        let payload =
            serde_json::to_vec(&StateRequest { provider }).expect("serialize state request");
        send(&mut self.stream, REQ_STATE, &payload)
    }

    /// Sottoscrive le notifiche push di un provider su una connessione dedicata
    /// (elephant le usa ad es. per dire a quale menu passare).
    pub fn subscribe(&self, provider: &str) -> io::Result<()> {
        let mut stream = UnixStream::connect(socket_path())?;
        let req = SubscribeRequest {
            interval: 0,
            provider,
            query: "",
        };
        let payload = serde_json::to_vec(&req).expect("serialize subscribe request");
        send(&mut stream, REQ_SUBSCRIBE, &payload)?;

        let tx = self.events.clone();
        thread::spawn(move || {
            while let Ok((kind, payload)) = recv(&mut stream) {
                if kind != RESP_SUBSCRIPTION_CHANGED {
                    continue;
                }
                let Ok(r) = serde_json::from_slice::<SubscribeResponse>(&payload) else {
                    continue;
                };
                if tx.send_blocking(Event::Subscription(r.value)).is_err() {
                    break;
                }
            }
        });
        Ok(())
    }

    pub fn query(&mut self, providers: Option<&[String]>, query: &str) -> io::Result<()> {
        let req = QueryRequest {
            providers: providers.unwrap_or(&self.providers),
            query,
            maxresults: self.max_results,
            exactsearch: false,
        };
        let payload = serde_json::to_vec(&req).expect("serialize query");
        send(&mut self.stream, REQ_QUERY, &payload)
    }
}

/// Attiva un item su una connessione dedicata e attende la conferma.
/// Alcune azioni (es. connessione bluetooth) rispondono solo a lavoro finito,
/// quindi il timeout è largo; scaduto quello, si va avanti comunque.
pub fn activate(item: &Item, action: &str, query: &str, single: bool) -> io::Result<()> {
    let mut stream = UnixStream::connect(socket_path())?;
    stream.set_read_timeout(Some(Duration::from_secs(30)))?;
    let req = ActivateRequest {
        provider: &item.provider,
        identifier: &item.identifier,
        action,
        query,
        arguments: "",
        single,
    };
    let payload = serde_json::to_vec(&req).expect("serialize activate request");
    send(&mut stream, REQ_ACTIVATE, &payload)?;
    loop {
        let (kind, _) = recv(&mut stream)?;
        if kind == RESP_ACTIVATION_FINISHED {
            return Ok(());
        }
    }
}
