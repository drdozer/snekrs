# Snekrs

A terminal-based snake game written in Rust using ratatui.

## Playing

Use arrow keys or WASD to control your snake. Collect food (♣♦♥♠★) to grow and score points. Different food items have different values.

- Space: Start game / Pause / Resume
- Q or Esc: Quit, exits game
- Arrow keys or WASD: Change direction

## Building

```bash
cargo build
cargo run
```

## Development

Uses cargo-watch for development:
```bash
cargo install cargo-watch
cargo watch --ignore snekrs.log --ignore .snekrs_high_score.txt -x run
```