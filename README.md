# wl-actions

A CLI tool that counts keyboard presses, mouse clicks, scroll events, and touch interactions to measure "actions" required for a task on Wayland.

## Usage

```bash
wl-actions firefox
wl-actions -- alacritty -e vim
wl-actions -q ghostty  # quiet mode, only show summary
```

Press **Ctrl+C** to stop and see the summary.

## What it counts

| Event Type | Wayland Event | What Counts |
|------------|---------------|-------------|
| Key press | `wl_keyboard::key` | Only `PRESSED` state (ignores release and repeat) |
| Mouse click | `wl_pointer::button` | Only `PRESSED` state |
| Scroll | `wl_pointer::axis_discrete` / `axis_value120` | Each discrete scroll step |
| Touch | `wl_touch::down` | Each touch start |

## Output

Live display (updated every 100ms):
```
Keys: 42 | Clicks: 15 | Scrolls: 8 | Touch: 3 | Total: 68
```

Summary on exit:
```
=== Action Summary ===
Duration: 2m 34s
Key presses: 42
Button clicks: 15
Scroll steps: 8
Touch taps: 3
Total actions: 68
Actions per minute: 26.5
```

## Building

### Prerequisites

- Rust 1.85+ (uses edition 2024)
- Linux with Wayland

### From source

```bash
# Clone the repository
git clone https://github.com/kaihendry/wl-actions.git
cd wl-actions

# Build release binary
make

# Or using cargo directly
cargo build --release

# Install to ~/.cargo/bin
make install
```

### Using cargo install

```bash
cargo install --git https://github.com/kaihendry/wl-actions
```

## Options

```
wl-actions [OPTIONS] <PROGRAM>...

Arguments:
  <PROGRAM>...  The program to run (and its arguments)

Options:
  -q, --quiet                        Suppress live output, only show summary
      --generate-completion <SHELL>  Generate shell completions [bash, elvish, fish, powershell, zsh]
  -h, --help                         Print help
```

## Shell completions

```bash
# Bash
wl-actions --generate-completion bash > ~/.local/share/bash-completion/completions/wl-actions

# Zsh
wl-actions --generate-completion zsh > ~/.zfunc/_wl-actions

# Fish
wl-actions --generate-completion fish > ~/.config/fish/completions/wl-actions.fish
```

## How it works

wl-actions wraps a Wayland application by creating a proxy between the app and the compositor using [wl-proxy](https://github.com/mahkoh/wl-proxy). It intercepts input events, counts them, and forwards them to the application unchanged.

Note: You need to close any existing instance of an application before wrapping it (e.g., Chrome uses a single-process model).

## License

MIT
