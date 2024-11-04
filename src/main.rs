use crossterm::{
    event::{self, DisableMouseCapture, EnableMouseCapture, Event},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use log::{error, info, warn};
use rand::Rng;
use ratatui::{
    prelude::*,
    style::{Style, Stylize},
    widgets::*,
};
use simplelog::{Config, LevelFilter, WriteLogger};
use std::collections::VecDeque;
use std::fs::{self, File};
use std::io::{self, Read, Write};
use std::time::{Duration, Instant};

const MORSEL_SYMBOLS: [&str; 5] = ["♣", "♦", "♥", "♠", "★"];
const HIGH_SCORE_FILE: &str = ".snekrs_high_score.txt";

fn main() -> Result<(), io::Error> {
    // Set up logging before anything else
    WriteLogger::init(
        LevelFilter::Info,
        Config::default(),
        File::create("snekrs.log")?,
    )
    .expect("Failed to initialize logger");

    info!("Starting Snekrs");

    // Setup terminal
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    // Create app state
    let mut game = Game::new();

    // Run game loop
    let tick_rate = Duration::from_millis(150);
    let mut last_tick = Instant::now();

    let mut ignore_input = false;
    loop {
        terminal.draw(|f| game.render(f))?;

        // Handle input
        if !ignore_input && event::poll(tick_rate)? {
            if let Event::Key(key) = event::read()? {
                game.handle_input(key);
                ignore_input = true;
            }
        }

        if last_tick.elapsed() >= tick_rate {
            game.update();
            last_tick = Instant::now();
            ignore_input = false;
        }

        match game.state {
            GameState::Exit => break,
            _ => {}
        }
    }

    // Cleanup terminal
    disable_raw_mode()?;
    execute!(
        terminal.backend_mut(),
        LeaveAlternateScreen,
        DisableMouseCapture
    )?;
    terminal.show_cursor()?;

    Ok(())
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
struct Size {
    width: u16,
    height: u16,
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Direction {
    North,
    East,
    South,
    West,
}

impl Direction {
    fn opposite(&self) -> Direction {
        match self {
            Direction::North => Direction::South,
            Direction::South => Direction::North,
            Direction::East => Direction::West,
            Direction::West => Direction::East,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct Pos {
    x: u16,
    y: u16,
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct PosDelta {
    x: i32,
    y: i32,
}

impl From<Direction> for PosDelta {
    fn from(dir: Direction) -> Self {
        match dir {
            Direction::North => PosDelta { x: 0, y: -1 },
            Direction::South => PosDelta { x: 0, y: 1 },
            Direction::East => PosDelta { x: 1, y: 0 },
            Direction::West => PosDelta { x: -1, y: 0 },
        }
    }
}

impl Pos {
    fn wrapped_add(&self, delta: PosDelta, size: Size) -> Pos {
        let new_x = (self.x as i32 + delta.x).rem_euclid(size.width as i32) as u16;
        let new_y = (self.y as i32 + delta.y).rem_euclid(size.height as i32) as u16;
        Pos { x: new_x, y: new_y }
    }
}

#[derive(Debug)]
struct Snek {
    head: Pos,
    body: VecDeque<Pos>,
    direction: Direction,
    pending_growth: u16,
}

impl Default for Snek {
    fn default() -> Self {
        Self::new(Size::default(), 0)
    }
}

impl Snek {
    fn new(size: Size, initial_length: u16) -> Self {
        let mid_x = size.width / 2;
        let mid_y = size.height / 2;
        let half_length = initial_length / 2;
        let length_rounding = initial_length % 2;

        let mut body = VecDeque::new();
        for i in 0..(initial_length) {
            body.push_back(Pos {
                x: mid_x - half_length - length_rounding + i,
                y: mid_y,
            });
        }
        let head = Pos {
            x: mid_x + half_length,
            y: mid_y,
        };

        Snek {
            head,
            body,
            direction: Direction::East,
            pending_growth: 0,
        }
    }

    fn change_direction(&mut self, new_direction: Direction) {
        if new_direction != self.direction && new_direction.opposite() != self.direction {
            self.direction = new_direction;
        }
    }

    fn slither(&mut self, arena_size: Size) {
        // Calculate new head position using wrapped_add
        let new_head = self.head.wrapped_add(self.direction.into(), arena_size);

        // Add old head to body
        self.body.push_back(self.head);

        // Update head
        self.head = new_head;

        // Remove tail unless growing
        if self.pending_growth > 0 {
            self.pending_growth -= 1;
        } else {
            self.body.pop_front();
        }
    }

    fn would_collide_with_body(&self, pos: impl Into<Pos>) -> bool {
        self.body.contains(&pos.into())
    }

    fn would_collide_with_head(&self, pos: impl Into<Pos>) -> bool {
        self.head == pos.into()
    }

    fn snack(&mut self, morsel: Morsel) {
        self.pending_growth += morsel.growth_value;
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct Morsel {
    pos: Pos,
    growth_value: u16,
}

impl From<Morsel> for Pos {
    fn from(morsel: Morsel) -> Self {
        morsel.pos
    }
}

#[derive(Debug)]
enum StepResult {
    Ongoing,     // Normal movement, no special events
    Nommed(u16), // Ate food, with score increment
    Collision,   // Hit self, game over
}

#[derive(Debug, Default)]
struct SnekHaus {
    size: Size,
    snek: Snek,
    moresels: Vec<Morsel>,
    score: u16,
}

impl SnekHaus {
    fn new(size: Size, initial_length: u16) -> Self {
        SnekHaus {
            size,
            snek: Snek::new(size, initial_length),
            moresels: Vec::new(),
            score: 0,
        }
    }

    fn check_nomming(&mut self) -> Option<u16> {
        // Find any morsel at the head position and remove it
        if let Some(index) = self.moresels.iter().position(|m| m.pos == self.snek.head) {
            let morsel = self.moresels.swap_remove(index);
            let score_increase = morsel.growth_value;
            self.score += score_increase; // assuming score increases by growth value
            self.snek.snack(morsel);
            Some(score_increase)
        } else {
            None
        }
    }

    fn check_snek_hit_itself(&self) -> bool {
        self.snek.would_collide_with_body(self.snek.head)
    }

    fn move_snek(&mut self) {
        self.snek.slither(self.size);
    }

    fn slither_on(&mut self) -> StepResult {
        self.move_snek();

        if self.check_snek_hit_itself() {
            return StepResult::Collision;
        }

        if let Some(score_increase) = self.check_nomming() {
            return StepResult::Nommed(score_increase);
        }

        StepResult::Ongoing
    }

    fn change_direction(&mut self, new_direction: Direction) {
        self.snek.change_direction(new_direction);
    }

    fn place_morsel(&mut self, morsel: Morsel) {
        assert!(
            !self.snek.would_collide_with_body(morsel)
                && !self.snek.would_collide_with_head(morsel),
            "Attempted to place morsel at invalid position"
        );
        self.moresels.push(morsel);
    }

    fn new_morsel(&self, rng: &mut impl Rng) -> Morsel {
        loop {
            let pos = Pos {
                x: rng.gen_range(0..self.size.width),
                y: rng.gen_range(0..self.size.height),
            };

            if !self.snek.would_collide_with_body(pos) && pos != self.snek.head {
                return Morsel {
                    pos,
                    growth_value: rng.gen_range(1..=5),
                };
            }
        }
    }
}

#[derive(Debug)]
enum GameState {
    ReadyToStart,
    Playing(SnekHaus),
    Paused(SnekHaus),
    GameOver { haus: SnekHaus, final_score: u16 },
    Exit,
}

struct Game {
    state: GameState,
    high_score: u16,
    arena_size: Option<Size>,
}

impl Game {
    fn new() -> Self {
        Game {
            state: GameState::ReadyToStart,
            high_score: Self::load_high_score(),
            arena_size: None,
        }
    }

    fn load_high_score() -> u16 {
        match fs::read_to_string(HIGH_SCORE_FILE).map(|s| s.trim().parse().unwrap_or(0)) {
            Ok(score) => score,
            Err(e) => {
                error!("Error loading high score: {}", e);
                0
            }
        }
    }

    fn save_high_score(&self) {
        if let Err(e) = fs::write(HIGH_SCORE_FILE, self.high_score.to_string()) {
            error!("Error saving high score: {}", e);
        }
    }

    fn update_high_score(&mut self, score: u16) {
        if score > self.high_score {
            self.high_score = score;
            self.save_high_score();
        }
    }

    fn render(&mut self, frame: &mut Frame) {
        let score_text = match &self.state {
            GameState::Playing(haus) | GameState::Paused(haus) => {
                format!(
                    "SNEK    High Score: {}    Score: {}",
                    self.high_score, haus.score
                )
            }
            _ => {
                format!("SNEK    High Score: {}", self.high_score)
            }
        };

        let size = frame.area();
        let layout = Layout::default()
            .direction(layout::Direction::Vertical)
            .constraints([
                Constraint::Length(3), // Title + High Score
                Constraint::Min(0),    // Game area
            ])
            .split(size);

        // Render title area with high score
        frame.render_widget(
            Paragraph::new(score_text)
                .alignment(Alignment::Left)
                .block(Block::default().borders(Borders::ALL)),
            layout[0],
        );

        // Game area - different for each state
        match &self.state {
            GameState::ReadyToStart => {
                let block = Block::default().borders(Borders::ALL);
                let inner_area = block.inner(layout[1]);
                self.arena_size = Some(Size {
                    width: inner_area.width,
                    height: inner_area.height,
                });
                frame.render_widget(
                    Paragraph::new("Press SPACE to start")
                        .alignment(Alignment::Center)
                        .block(block),
                    layout[1],
                );
            }
            GameState::Playing(haus) => {
                let block = Block::default().title("Playing").borders(Borders::ALL);
                let inner_area = block.inner(layout[1]);

                frame.render_widget(block, layout[1]);
                frame.render_widget(haus, inner_area);
            }
            GameState::Paused(haus) => {
                let block = Block::default()
                    .title("Paused. Press SPACE to continue")
                    .borders(Borders::ALL);
                let inner_area = block.inner(layout[1]);

                frame.render_widget(block, layout[1]);
                frame.render_widget(haus, inner_area);
            }
            GameState::GameOver { final_score, haus } => {
                let block = Block::default().borders(Borders::ALL);
                let inner_area = block.inner(layout[1]);

                frame.render_widget(block, layout[1]);
                frame.render_widget(haus, inner_area);
                frame.render_widget(
                    Paragraph::new(format!(
                        "GAME OVER\nFinal Score: {}\nPress SPACE to play again",
                        final_score
                    ))
                    .alignment(Alignment::Center),
                    inner_area,
                );
            }
            GameState::Exit => {}
        }
    }

    fn handle_input(&mut self, key: event::KeyEvent) {
        use event::KeyCode;

        let new_state = match &mut self.state {
            GameState::ReadyToStart => match key.code {
                KeyCode::Char('q') | KeyCode::Esc => Some(GameState::Exit),
                KeyCode::Char(' ') => {
                    let size = self.arena_size.expect("Arena size not initialized");
                    let mut haus = SnekHaus::new(size, 3);

                    let mut rng = rand::thread_rng();
                    let morsel = haus.new_morsel(&mut rng);
                    haus.place_morsel(morsel);

                    Some(GameState::Playing(haus))
                }
                _ => None,
            },
            GameState::Playing(haus) => match key.code {
                KeyCode::Char('q') => {
                    let final_score = haus.score;
                    Some(GameState::GameOver {
                        haus: std::mem::take(haus),
                        final_score,
                    })
                }
                KeyCode::Esc => Some(GameState::Exit),
                KeyCode::Char(' ') => Some(GameState::Paused(std::mem::take(haus))),
                KeyCode::Up | KeyCode::Char('w') => {
                    haus.change_direction(Direction::North);
                    None
                }
                KeyCode::Down | KeyCode::Char('s') => {
                    haus.change_direction(Direction::South);
                    None
                }
                KeyCode::Left | KeyCode::Char('a') => {
                    haus.change_direction(Direction::West);
                    None
                }
                KeyCode::Right | KeyCode::Char('d') => {
                    haus.change_direction(Direction::East);
                    None
                }
                _ => None,
            },
            GameState::Paused(haus) => match key.code {
                KeyCode::Char('q') => {
                    let final_score = haus.score;
                    Some(GameState::GameOver {
                        haus: std::mem::take(haus),
                        final_score,
                    })
                }
                KeyCode::Esc => Some(GameState::Exit),
                KeyCode::Char(' ') => Some(GameState::Playing(std::mem::take(haus))),
                _ => None,
            },
            GameState::GameOver { .. } => match key.code {
                KeyCode::Esc => Some(GameState::Exit),
                KeyCode::Char(' ') | KeyCode::Char('q') => Some(GameState::ReadyToStart),
                _ => None,
            },
            _ => None,
        };

        if let Some(new_state) = new_state {
            self.state = new_state;
        }
    }

    fn update(&mut self) {
        match &mut self.state {
            GameState::Playing(haus) => {
                match haus.slither_on() {
                    StepResult::Collision => {
                        // Game over - save the haus and score
                        let final_score = haus.score;
                        let haus = std::mem::take(haus);
                        self.update_high_score(final_score);
                        self.state = GameState::GameOver { haus, final_score };
                    }
                    StepResult::Nommed(_score) => {
                        let mut rng = rand::thread_rng();
                        let morsel = haus.new_morsel(&mut rng);
                        haus.place_morsel(morsel);
                    }
                    StepResult::Ongoing => {
                        // Normal movement, nothing special to do
                    }
                }
            }
            _ => {}
        }
    }
}

impl Widget for &SnekHaus {
    fn render(self, area: Rect, buf: &mut Buffer) {
        for pos in &self.snek.body {
            buf[(pos.x + area.x, pos.y + area.y)]
                .set_symbol(" ")
                .set_bg(Color::Green);
        }

        // Add snake head (different symbol/color)
        buf[(self.snek.head.x + area.x, self.snek.head.y + area.y)]
            .set_symbol("😀")
            .set_fg(Color::Yellow);

        // Add morsels
        for morsel in &self.moresels {
            buf[(morsel.pos.x + area.x, morsel.pos.y + area.y)]
                .set_symbol(MORSEL_SYMBOLS[morsel.growth_value as usize - 1])
                .set_fg(Color::LightRed);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_opposite_directions() {
        // Test direct opposites
        assert_eq!(Direction::North.opposite(), Direction::South);
        assert_eq!(Direction::South.opposite(), Direction::North);
        assert_eq!(Direction::East.opposite(), Direction::West);
        assert_eq!(Direction::West.opposite(), Direction::East);

        // Test double opposite returns to original for all directions
        assert_eq!(Direction::North.opposite().opposite(), Direction::North);
        assert_eq!(Direction::South.opposite().opposite(), Direction::South);
        assert_eq!(Direction::East.opposite().opposite(), Direction::East);
        assert_eq!(Direction::West.opposite().opposite(), Direction::West);
    }

    #[test]
    fn test_zero_delta() {
        let arena_size = Size {
            width: 10,
            height: 10,
        };

        let pos = Pos { x: 5, y: 5 };
        let delta = PosDelta { x: 0, y: 0 };
        let new_pos = pos.wrapped_add(delta, arena_size);
        assert_eq!(new_pos, pos);

        // Test at boundaries too
        let pos = Pos { x: 0, y: 0 };
        let new_pos = pos.wrapped_add(delta, arena_size);
        assert_eq!(new_pos, pos);

        let pos = Pos { x: 9, y: 9 };
        let new_pos = pos.wrapped_add(delta, arena_size);
        assert_eq!(new_pos, pos);
    }

    #[test]
    fn test_arena_size_deltas() {
        let arena_size = Size {
            width: 10,
            height: 10,
        };
        let non_square_arena_size = Size {
            width: 8,
            height: 6,
        };
        let pos = Pos { x: 5, y: 5 };

        // Moving exactly one arena width/height should return to the same position
        let delta = PosDelta { x: 10, y: 10 };
        let new_pos = pos.wrapped_add(delta, arena_size);
        assert_eq!(new_pos, pos);

        // Moving exactly negative arena width/height should also return to same position
        let delta = PosDelta { x: -10, y: -10 };
        let new_pos = pos.wrapped_add(delta, arena_size);
        assert_eq!(new_pos, pos);

        // Test with different x and y arena dimensions
        let pos = Pos { x: 3, y: 3 };
        let delta = PosDelta { x: 8, y: 6 };
        let new_pos = pos.wrapped_add(delta, non_square_arena_size); // Non-square arena
        assert_eq!(new_pos, pos);
    }

    #[test]
    fn test_large_deltas() {
        let arena_size = Size {
            width: 10,
            height: 10,
        };

        let pos = Pos { x: 5, y: 5 };
        let delta = PosDelta { x: 3, y: -3 };
        let new_pos = pos.wrapped_add(delta, arena_size);
        assert_eq!(new_pos, Pos { x: 8, y: 2 });

        // Test wrapping with large positive delta
        let delta = PosDelta { x: 8, y: 12 };
        let new_pos = pos.wrapped_add(delta, arena_size);
        assert_eq!(new_pos, Pos { x: 3, y: 7 }); // (5+8)%10=3, (5+12)%10=7

        // Test wrapping with large negative delta
        let delta = PosDelta { x: -12, y: -8 };
        let new_pos = pos.wrapped_add(delta, arena_size);
        assert_eq!(new_pos, Pos { x: 3, y: 7 }); // (5-12)%10=3, (5-8)%10=7
    }

    #[test]
    fn test_very_large_deltas() {
        let arena_size = Size {
            width: 10,
            height: 10,
        };

        let pos = Pos { x: 5, y: 5 };
        // Test with deltas larger than arena size
        let delta = PosDelta { x: 25, y: -15 }; // Multiple wraps
        let new_pos = pos.wrapped_add(delta, arena_size);
        assert_eq!(new_pos, Pos { x: 0, y: 0 }); // (5+25)%10=0, (5-15)%10=0
    }

    #[test]
    fn test_basic_movement() {
        let arena_size = Size {
            width: 10,
            height: 10,
        };

        let pos = Pos { x: 5, y: 5 };
        let delta: PosDelta = Direction::North.into();
        let new_pos = pos.wrapped_add(delta, arena_size);
        assert_eq!(new_pos, Pos { x: 5, y: 4 });
    }

    #[test]
    fn test_wrap_underflow() {
        let arena_size = Size {
            width: 10,
            height: 10,
        };

        let pos = Pos { x: 0, y: 0 };
        let delta: PosDelta = Direction::North.into();
        let new_pos = pos.wrapped_add(delta, arena_size);
        assert_eq!(new_pos, Pos { x: 0, y: 9 });

        let pos = Pos { x: 0, y: 5 };
        let delta: PosDelta = Direction::West.into();
        let new_pos = pos.wrapped_add(delta, arena_size);
        assert_eq!(new_pos, Pos { x: 9, y: 5 });
    }

    #[test]
    fn test_wrap_overflow() {
        let arena_size = Size {
            width: 10,
            height: 10,
        };

        let pos = Pos { x: 9, y: 9 };
        let delta: PosDelta = Direction::South.into();
        let new_pos = pos.wrapped_add(delta, arena_size);
        assert_eq!(new_pos, Pos { x: 9, y: 0 });

        let pos = Pos { x: 9, y: 5 };
        let delta: PosDelta = Direction::East.into();
        let new_pos = pos.wrapped_add(delta, arena_size);
        assert_eq!(new_pos, Pos { x: 0, y: 5 });
    }

    #[test]
    fn test_all_directions() {
        let arena_size = Size {
            width: 10,
            height: 10,
        };

        let pos = Pos { x: 5, y: 5 };

        assert_eq!(
            pos.wrapped_add(Direction::North.into(), arena_size),
            Pos { x: 5, y: 4 }
        );
        assert_eq!(
            pos.wrapped_add(Direction::South.into(), arena_size),
            Pos { x: 5, y: 6 }
        );
        assert_eq!(
            pos.wrapped_add(Direction::East.into(), arena_size),
            Pos { x: 6, y: 5 }
        );
        assert_eq!(
            pos.wrapped_add(Direction::West.into(), arena_size),
            Pos { x: 4, y: 5 }
        );
    }

    #[test]
    fn test_collision_detection() {
        let snek = Snek {
            head: Pos { x: 5, y: 5 },
            body: VecDeque::from([Pos { x: 5, y: 6 }, Pos { x: 5, y: 7 }, Pos { x: 6, y: 7 }]),
            direction: Direction::North,
            pending_growth: 0,
        };

        assert!(snek.would_collide_with_body(Pos { x: 5, y: 6 })); // First segment
        assert!(snek.would_collide_with_body(Pos { x: 6, y: 7 })); // Last segment
        assert!(!snek.would_collide_with_body(Pos { x: 5, y: 5 })); // Head position
        assert!(!snek.would_collide_with_body(Pos { x: 4, y: 6 })); // Adjacent but not colliding
    }

    #[test]
    fn test_snacking() {
        let mut snek = Snek {
            head: Pos { x: 5, y: 5 },
            body: VecDeque::new(),
            direction: Direction::North,
            pending_growth: 0,
        };

        let morsel = Morsel {
            pos: Pos { x: 5, y: 4 }, // position doesn't matter for snacking
            growth_value: 3,
        };

        snek.snack(morsel);
        assert_eq!(snek.pending_growth, 3);

        // Test multiple snacks stack
        snek.snack(Morsel {
            pos: Pos { x: 0, y: 0 },
            growth_value: 2,
        });
        assert_eq!(snek.pending_growth, 5);
    }

    #[test]
    fn test_new_odd_snek() {
        let size = Size {
            width: 10,
            height: 10,
        };

        // Test with odd length
        let snek = Snek::new(size, 3);
        println!("Odd length snek:");
        println!("  head: {:?}", snek.head);
        println!("  body: {:?}", snek.body);

        // Convert body to Vec for easier inspection
        let body: Vec<_> = snek.body.iter().collect();
        println!("  body segments:");
        for (i, pos) in body.iter().enumerate() {
            println!("    segment {}: {:?}", i, pos);
        }

        assert_eq!(snek.head, Pos { x: 6, y: 5 });
        assert_eq!(snek.body.len(), 3);
        assert_eq!(snek.body[0], Pos { x: 3, y: 5 }); // Leftmost segment
        assert_eq!(snek.body[1], Pos { x: 4, y: 5 }); // Middle segment
        assert_eq!(snek.body[2], Pos { x: 5, y: 5 }); // Rightmost segment
    }

    #[test]
    fn test_new_snek_even() {
        let size = Size {
            width: 10,
            height: 10,
        };
        let snek = Snek::new(size, 4);

        // Debug output
        println!("Even length snek:");
        println!("  head: {:?}", snek.head);
        println!("  body: {:?}", snek.body);
        println!("  body segments:");
        for (i, pos) in snek.body.iter().enumerate() {
            println!("    segment {}: {:?}", i, pos);
        }

        assert_eq!(snek.head, Pos { x: 7, y: 5 }); // mid_x(5) + half_length(2)
        assert_eq!(snek.body.len(), 4);
        assert_eq!(snek.body[0], Pos { x: 3, y: 5 }); // leftmost
        assert_eq!(snek.body[1], Pos { x: 4, y: 5 });
        assert_eq!(snek.body[2], Pos { x: 5, y: 5 });
        assert_eq!(snek.body[3], Pos { x: 6, y: 5 }); // rightmost
        assert_eq!(snek.direction, Direction::East);
        assert_eq!(snek.pending_growth, 0);
    }

    #[test]
    fn test_snek_movement() {
        let size = Size {
            width: 10,
            height: 10,
        };
        let mut snek = Snek::new(size, 3);
        println!("Starting snek: {:?}", snek);

        // Record initial positions
        let initial_head = snek.head;
        let initial_body: Vec<Pos> = snek.body.iter().cloned().collect();

        // Move once
        snek.slither(size);

        println!("Moved snek: {:?}", snek);

        // Verify:
        // 1. New head is one step east of old head
        assert_eq!(
            snek.head,
            Pos {
                x: initial_head.x + 1,
                y: initial_head.y
            },
            "Head position should be one step east of old head"
        );

        // 2. Old head is now last body segment
        assert_eq!(
            snek.body.back(),
            Some(&initial_head),
            "Old head is now the highest body segment"
        );

        // 3. Middle segments moved up
        assert_eq!(
            initial_body[2], snek.body[1],
            "Middle segments moved up 2 to 1"
        );
        assert_eq!(
            initial_body[1], snek.body[0],
            "Middle segments moved up 1 to 0"
        );

        // 4. Last body segment (tail) was removed
        assert_eq!(snek.body.len(), 3, "Length remained the same");
        assert!(!snek.body.contains(&initial_body[0]), "Tail was removed");
    }

    #[test]
    fn test_snek_self_collision() {
        let size = Size {
            width: 10,
            height: 10,
        };
        let mut haus = SnekHaus::new(size, 3);

        // Initial state should not be self-colliding
        assert!(!haus.check_snek_hit_itself());

        // Create a situation where snake hits itself
        // We'll need to manually create a snake in a self-colliding position
        haus.snek.body.push_back(haus.snek.head);
        assert!(haus.check_snek_hit_itself());
    }
    #[test]
    fn test_nomming() {
        let size = Size {
            width: 10,
            height: 10,
        };
        let mut haus = SnekHaus::new(size, 3);

        // Add a morsel where the snake head is
        let morsel = Morsel {
            pos: haus.snek.head,
            growth_value: 2,
        };
        haus.moresels.push(morsel);

        // Should return the growth value when nomming occurs
        assert_eq!(haus.check_nomming(), Some(2));

        // Verify effects of nomming
        assert!(haus.moresels.is_empty());
        assert_eq!(haus.score, 2);
        assert_eq!(haus.snek.pending_growth, 2);

        // Should return None when no morsel present
        assert_eq!(haus.check_nomming(), None);
    }

    #[test]
    fn test_nomming_with_multiple_morsels() {
        let size = Size {
            width: 10,
            height: 10,
        };
        let mut haus = SnekHaus::new(size, 3);

        // Add multiple morsels, only one at head position
        let head_morsel = Morsel {
            pos: haus.snek.head,
            growth_value: 2,
        };
        let other_morsel = Morsel {
            pos: Pos { x: 0, y: 0 },
            growth_value: 3,
        };

        haus.moresels.push(head_morsel);
        haus.moresels.push(other_morsel);

        assert_eq!(haus.check_nomming(), Some(2));
        assert_eq!(haus.moresels.len(), 1); // Other morsel should remain
        assert_eq!(haus.moresels[0].growth_value, 3); // Verify correct morsel remained
    }

    #[test]
    fn test_change_direction() {
        let size = Size {
            width: 10,
            height: 10,
        };
        let mut haus = SnekHaus::new(size, 3);

        // Initial direction is North
        haus.change_direction(Direction::East);
        assert_eq!(haus.snek.direction, Direction::East);

        // Can't reverse direction
        haus.change_direction(Direction::West);
        assert_eq!(haus.snek.direction, Direction::East);
    }

    #[test]
    fn test_place_morsel() {
        let size = Size {
            width: 10,
            height: 10,
        };
        let mut haus = SnekHaus::new(size, 3);

        // Valid placement
        let valid_morsel = Morsel {
            pos: Pos { x: 0, y: 0 },
            growth_value: 1,
        };
        haus.place_morsel(valid_morsel);
        assert_eq!(haus.moresels.len(), 1);

        // Invalid placement should panic
        let invalid_morsel = Morsel {
            pos: haus.snek.head,
            growth_value: 1,
        };
        let result = std::panic::catch_unwind(move || {
            haus.place_morsel(invalid_morsel);
        });
        assert!(result.is_err());
    }
}
