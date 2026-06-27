# thwack


A small read-only terminal news reader for Hacker News and Lobsters.

![Demo](media/demo.gif)

## Naming

I treat these link aggregators like a daily newspaper. Thwack is the sound a newspaper makes when you hit someone with it.

## Run

```sh
cargo run
```

To install the local binary:

```sh
cargo install --path .
```

## Keys

| Key | Action |
| --- | --- |
| `j` / `↓` | Move down |
| `k` / `↑` | Move up |
| `g` | Jump to top |
| `G` | Jump to bottom |
| `o` | Open selected link |
| `c` | Open selected discussion |
| `r` | Refresh or reload |
| `Tab` | Switch source in posts view |
| `1` / `2` | Select Hacker News / Lobsters |
| `Enter` | Load comments from posts view; toggle collapse in comments view |
| `Space` | Toggle selected comment collapse |
| `Esc` / `b` | Return to posts from comments |
| `h` / `←` | Select previous visible comment |
| `l` / `→` | Select next visible comment |
| `q` / `Ctrl-C` / `Ctrl-D` | Quit |

