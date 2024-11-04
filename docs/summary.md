# Snekrs - A Terminal Snake Game

A snake game implementation using Rust and the ratatui terminal UI framework.

## Components

### Core Types
- `Direction` - Cardinal directions (North, South, East, West)
- `Pos` - Position in 2D space with wrapping behavior
- `Size` - Dimensions of play area
- `Morsel` - Food items with different growth values
- `Snek` - The snake with head, body segments and movement logic
- `SnekHaus` - The game arena containing snake and food

### Game States
- ReadyToStart - Initial state waiting for player
- Playing - Active gameplay
- Paused - Game temporarily suspended
- GameOver - Game ended with final score
- Exit - Clean shutdown

### Features
- Snake grows when eating food
- Food items with different values (represented by card suits)
- Wrapping arena boundaries
- High score persistence
- Input handling (Arrow keys and WASD)
- Game state transitions
- Logging system

### Technical Details
- Built with ratatui for terminal UI
- Uses crossterm for terminal control
- File-based high score persistence
- Logging to external file