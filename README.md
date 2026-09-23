# runner

Launcher per Wayland (layer-shell, GTK4) che usa [elephant](https://github.com/abenz1267/elephant)
come backend: runner è solo la UI, ricerca e attivazione le fa elephant.

## Requisiti

- `elephant` in esecuzione (`elephant service enable` per averlo come servizio utente)
- `gtk4`, `gtk4-layer-shell`
- un compositor con `wlr-layer-shell` (Hyprland, Sway, niri, …)

## Build

```sh
cargo build --release
install -Dm755 target/release/runner ~/.local/bin/runner
```

## Uso

```sh
runner                     # provider del config
runner -q /bluetooth       # apre direttamente in modalità bluetooth
runner -p bluetooth        # solo un provider
runner -q fire             # testo iniziale
```

### Modalità

Ogni provider di elephant installato è una modalità:

| Scrivi | Succede |
|---|---|
| `/` | Elenco dei provider installati (`elephant listproviders`); `Invio`/`Tab` entra |
| `/blu` | Elenco filtrato |
| `/bluetooth` · `/bluetooth cuffie` | Entra nella modalità: il prompt diventa `bluetooth ❯` e nella barra resta solo la query |
| `/menus:<nome>` | Un menu di elephant; i sottomenu si aprono con `Invio` |
| prefisso del config (es. `=2+2`) | Scorciatoia verso un provider |

In una modalità compaiono anche le **azioni del provider**, segnate come `azione`
(per il bluetooth: Accendi/Spegni, Cerca dispositivi; per i sottomenu: Menu superiore).
Stanno dopo i risultati, così `Invio` appena entrati agisce su un elemento.

I provider in `keep_open` (default: bluetooth, wireplumber, playerctl, todo) non
chiudono il launcher dopo un'azione: aspettano che elephant finisca (lo spinner
gira, es. connessione o scansione di 5 s) e ricaricano la lista.

Lanciarlo mentre è già aperto lo chiude, quindi basta un solo keybind. Hyprland:

```
bind = SUPER, SPACE, exec, runner
```

| Tasto | Azione |
|---|---|
| `↑` `↓` · `Tab` · `Ctrl+J/K` · `Ctrl+N/P` | Muove la selezione |
| `Invio` / click | Azione principale dell'elemento (es. Avvia, Connetti) |
| `Alt+2`…`Alt+9` | Altre azioni, elencate nel footer |
| `Backspace` a barra vuota | Esce dalla modalità |
| `Esc` | Chiude |

## Configurazione

- `~/.config/runner/config.toml`: vedi [`config.example.toml`](config.example.toml)
- `~/.config/runner/style.css`: caricato sopra lo stile di default
  ([`src/style.css`](src/style.css)).

### Aspetto

Runner prende l'aspetto dal desktop invece di averne uno suo:

- **Colori da pywal** (`~/.cache/wal/colors.json`): `background`, `foreground` e
  `color4` come accento, lo stesso del tema GTK. Superfici e linee sono miscele
  di sfondo e testo (`mix()`), quindi funzionano con qualsiasi wallpaper.
  Senza pywal usa una palette di riserva.
- **Bordo da Hyprland:** stesso gradiente di `general:col.active_border`, angoli
  vivi. Il launcher sembra una finestra col focus.
- Per cambiare i colori basta ridefinire `runner_bg`, `runner_fg` e
  `runner_accent` con `@define-color` nello `style.css` utente.

## Note

- **Ordine delle azioni:** elephant non indica un'azione di default, e il
  bluetooth manda `remove` per prima. Runner ordina: prima `primary_actions` del
  config, poi le altre, in fondo quelle distruttive (`remove`, `erase_history`,
  `delete`…), senza doppioni.

- **Protocollo:** runner parla direttamente col socket di elephant
  (`$XDG_RUNTIME_DIR/elephant/elephant.sock`) in formato JSON: niente protobuf.
  Frame richiesta `[tipo u8][formato u8][len u32 BE][payload]`, risposta
  `[tipo u8][len u32 BE][payload]`.
- **Renderer:** runner imposta `GSK_RENDERER=gl` se non è già definito. Con il
  renderer Vulkan di default GTK enumera tutte le GPU e sui portatili ibridi sveglia
  la dGPU sospesa: l'avvio passava da ~0,2 s a ~2 s.
