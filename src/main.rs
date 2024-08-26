use crossterm::{
    event::{self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{
    backend::CrosstermBackend,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, ListState, Paragraph},
    Frame, Terminal,
};
use std::io;
use std::process::Command;
use std::time::{Duration, Instant};

enum InputMode {
    Normal,
    Editing,
}

struct ModelInfo {
    full_name: String,
    aliases: Vec<String>,
    preferred_alias: String,
}

enum FocusedBlock {
    ModelSelect,
    Input,
    Output,
}

struct App {
    input: String,
    input_cursor: usize,
    output: String,
    model_list_state: ListState,
    input_mode: InputMode,
    focused_block: FocusedBlock,
    models: Vec<ModelInfo>,
    is_thinking: bool,
    spinner_state: usize,
    last_update: Instant,
    spinner: Spinner,
}

impl App {
    fn new() -> Self {
        let models = get_available_models();
        let default_model = get_default_model(&models);
        let default_index = models
            .iter()
            .position(|m| m.preferred_alias == default_model)
            .unwrap_or(0);
        let mut model_list_state = ListState::default();
        model_list_state.select(Some(default_index));

        App {
            input: String::new(),
            input_cursor: 0,
            output: String::new(),
            models,
            model_list_state,
            input_mode: InputMode::Normal,
            focused_block: FocusedBlock::Input,
            spinner_state: 0,
            last_update: Instant::now(),
            spinner: Spinner::new(),
            is_thinking: false,
        }
    }

    fn next_model(&mut self) {
        let i = match self.model_list_state.selected() {
            Some(i) => {
                if i >= self.models.len() - 1 {
                    0
                } else {
                    i + 1
                }
            }
            None => 0,
        };
        self.model_list_state.select(Some(i));
    }

    fn previous_model(&mut self) {
        let i = match self.model_list_state.selected() {
            Some(i) => {
                if i == 0 {
                    self.models.len() - 1
                } else {
                    i - 1
                }
            }
            None => 0,
        };
        self.model_list_state.select(Some(i));
    }

    fn selected_model(&self) -> &str {
        &self.models[self.model_list_state.selected().unwrap_or(0)].preferred_alias
    }

    fn next_focus(&mut self) {
        self.focused_block = match self.focused_block {
            FocusedBlock::ModelSelect => FocusedBlock::Input,
            FocusedBlock::Input => FocusedBlock::Output,
            FocusedBlock::Output => FocusedBlock::ModelSelect,
        };
    }
    fn update_spinner(&mut self) {
        if self.is_thinking {
            self.spinner.update();
        }
    }
}

struct Spinner {
    frames: Vec<char>,
    current_frame: usize,
    last_update: Instant,
}

impl Spinner {
    fn new() -> Self {
        Spinner {
            frames: vec!['|', '/', '-', '\\'],
            current_frame: 0,
            last_update: Instant::now(),
        }
    }

    fn update(&mut self) {
        let now = Instant::now();
        if now.duration_since(self.last_update) >= Duration::from_millis(100) {
            self.current_frame = (self.current_frame + 1) % self.frames.len();
            self.last_update = now;
        }
    }

    fn current_frame(&self) -> char {
        self.frames[self.current_frame]
    }
}

fn main() -> Result<(), io::Error> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let mut app = App::new();

    loop {
        terminal.draw(|f| ui(f, &mut app))?;
        app.update_spinner();

        if event::poll(Duration::from_millis(50))? {
            if let Event::Key(key) = event::read()? {
                match app.focused_block {
                    FocusedBlock::ModelSelect => match key.code {
                        KeyCode::Up => app.previous_model(),
                        KeyCode::Down => app.next_model(),
                        KeyCode::Tab => app.next_focus(),
                        KeyCode::Char('q') => break,
                        _ => {}
                    },
                    FocusedBlock::Input => match app.input_mode {
                        InputMode::Normal => match key.code {
                            KeyCode::Char('i') => app.input_mode = InputMode::Editing,
                            KeyCode::Enter => {
                                let output = run_llm(&app.input, app.selected_model());
                                app.output = output;
                                app.input.clear();
                                app.input_cursor = 0;
                            }
                            KeyCode::Tab => app.next_focus(),
                            KeyCode::Char('q') => break,
                            _ => {}
                        },
                        InputMode::Editing => match key.code {
                            KeyCode::Enter => {
                                app.is_thinking = true;
                                terminal.draw(|f| ui(f, &mut app))?; // Redraw immediately to show spinner
                                let output = run_llm(&app.input, app.selected_model());
                                app.output = output;
                                app.input.clear();
                                app.input_cursor = 0;
                                app.input_mode = InputMode::Normal;
                                app.is_thinking = false;
                            }
                            KeyCode::Char(c) => {
                                app.input.insert(app.input_cursor, c);
                                app.input_cursor += 1;
                            }
                            KeyCode::Backspace => {
                                if app.input_cursor > 0 {
                                    app.input.remove(app.input_cursor - 1);
                                    app.input_cursor -= 1;
                                }
                            }
                            KeyCode::Left => {
                                if app.input_cursor > 0 {
                                    app.input_cursor -= 1;
                                }
                            }
                            KeyCode::Right => {
                                if app.input_cursor < app.input.len() {
                                    app.input_cursor += 1;
                                }
                            }
                            KeyCode::Esc => {
                                app.input_mode = InputMode::Normal;
                            }
                            _ => {}
                        },
                    },
                    FocusedBlock::Output => match key.code {
                        KeyCode::Tab => app.next_focus(),
                        KeyCode::Char('q') => break,
                        _ => {}
                    },
                }
            }
        }
    }

    disable_raw_mode()?;
    execute!(
        terminal.backend_mut(),
        LeaveAlternateScreen,
        DisableMouseCapture
    )?;
    terminal.show_cursor()?;

    Ok(())
}

fn ui(f: &mut Frame, app: &mut App) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .margin(1)
        .constraints(
            [
                Constraint::Length(3),
                Constraint::Min(3),
                Constraint::Percentage(70),
                Constraint::Length(3),
            ]
            .as_ref(),
        )
        .split(f.area()); // Changed from f.size() to f.area()

    let (model_chunk, input_chunk, output_chunk, status_chunk) =
        (chunks[0], chunks[1], chunks[2], chunks[3]);

    render_model_select(f, app, model_chunk);
    render_input(f, app, input_chunk);
    render_output(f, app, output_chunk);
    render_status(f, app, status_chunk);

    if app.is_thinking {
        render_spinner(f, app);
    }
}

fn render_spinner(f: &mut Frame, app: &App) {
    let spinner_char = app.spinner.current_frame();
    let spinner_text = format!(" {} Thinking...", spinner_char);
    let spinner_widget = Paragraph::new(spinner_text)
        .style(Style::default().fg(Color::Yellow))
        .block(Block::default().borders(Borders::ALL));

    let area = centered_rect(30, 3, f.size());
    f.render_widget(ratatui::widgets::Clear, area); // Clear the area first
    f.render_widget(spinner_widget, area);
}

fn centered_rect(percent_x: u16, percent_y: u16, r: Rect) -> Rect {
    let popup_layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage((100 - percent_y) / 2),
            Constraint::Percentage(percent_y),
            Constraint::Percentage((100 - percent_y) / 2),
        ])
        .split(r);

    Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage((100 - percent_x) / 2),
            Constraint::Percentage(percent_x),
            Constraint::Percentage((100 - percent_x) / 2),
        ])
        .split(popup_layout[1])[1]
}

fn render_model_select(f: &mut Frame, app: &mut App, area: Rect) {
    let model_block = Block::default()
        .title("Model")
        .borders(Borders::ALL)
        .border_style(
            Style::default().fg(if let FocusedBlock::ModelSelect = app.focused_block {
                Color::Yellow
            } else {
                Color::White
            }),
        );

    let models: Vec<ListItem> = app
        .models
        .iter()
        .map(|m| {
            let content = if m.aliases.is_empty() {
                m.full_name.clone()
            } else {
                format!("{} ({})", m.full_name, m.aliases.join(", "))
            };
            ListItem::new(Line::from(vec![Span::styled(content, Style::default())]))
        })
        .collect();

    let models_list = List::new(models).block(model_block).highlight_style(
        Style::default()
            .add_modifier(Modifier::BOLD)
            .fg(Color::Yellow),
    );

    f.render_stateful_widget(models_list, area, &mut app.model_list_state);
}

fn render_input(f: &mut Frame, app: &App, area: Rect) {
    let input_block = Block::default()
        .title("Input")
        .borders(Borders::ALL)
        .border_style(
            Style::default().fg(if let FocusedBlock::Input = app.focused_block {
                Color::Yellow
            } else {
                Color::White
            }),
        );

    let input = Paragraph::new(app.input.as_str())
        .block(input_block)
        .style(match app.input_mode {
            InputMode::Normal => Style::default(),
            InputMode::Editing => Style::default().fg(Color::Yellow),
        });

    f.render_widget(input, area);

    // Draw the cursor
    if let FocusedBlock::Input = app.focused_block {
        f.set_cursor(area.x + app.input_cursor as u16 + 1, area.y + 1);
    }
}

fn render_output(f: &mut Frame, app: &App, area: Rect) {
    let output_block = Block::default()
        .title("Output")
        .borders(Borders::ALL)
        .border_style(
            Style::default().fg(if let FocusedBlock::Output = app.focused_block {
                Color::Yellow
            } else {
                Color::White
            }),
        );

    let output = Paragraph::new(app.output.as_str()).block(output_block);
    f.render_widget(output, area);
}

fn render_status(f: &mut Frame, app: &App, area: Rect) {
    let status = match app.focused_block {
        FocusedBlock::ModelSelect => "Use ↑↓ to select model, Tab to switch focus",
        FocusedBlock::Input => match app.input_mode {
            InputMode::Normal => "Press 'i' to insert, Enter to send, Tab to switch focus",
            InputMode::Editing => "Editing: Enter to send, Esc to cancel",
        },
        FocusedBlock::Output => "Tab to switch focus",
    };

    let status_widget = Paragraph::new(status)
        .block(Block::default().borders(Borders::ALL))
        .style(Style::default().fg(Color::Cyan));

    f.render_widget(status_widget, area);
}

fn run_llm(prompt: &str, model: &str) -> String {
    let output = Command::new("llm")
        .args(["-m", model, prompt])
        .output()
        .expect("Failed to execute command");

    if !output.status.success() {
        return format!("Error: {}", String::from_utf8_lossy(&output.stderr));
    }

    String::from_utf8_lossy(&output.stdout).to_string()
}

fn get_available_models() -> Vec<ModelInfo> {
    let output = Command::new("llm")
        .arg("models")
        .output()
        .expect("Failed to execute command");

    let mut models = Vec::new();

    if output.status.success() {
        for line in String::from_utf8_lossy(&output.stdout).lines() {
            if let Some((full_name, aliases_str)) = line.split_once(" (aliases: ") {
                let full_name = full_name.trim().to_string();
                let aliases: Vec<String> = aliases_str
                    .trim_end_matches(')')
                    .split(", ")
                    .map(|s| s.to_string())
                    .collect();

                let preferred_alias = if !aliases.is_empty() {
                    aliases[0].clone()
                } else {
                    full_name.clone()
                };

                models.push(ModelInfo {
                    full_name,
                    aliases,
                    preferred_alias,
                });
            } else if !line.contains("(aliases:") {
                // Handle models without aliases
                let full_name = line.trim().to_string();
                models.push(ModelInfo {
                    full_name: full_name.clone(),
                    aliases: vec![],
                    preferred_alias: full_name,
                });
            }
        }
    }

    models
}

fn get_default_model(models: &[ModelInfo]) -> String {
    models
        .iter()
        .find(|m| m.full_name.contains("claude-3-5-sonnet"))
        .map(|m| m.preferred_alias.clone())
        .unwrap_or_else(|| {
            models
                .first()
                .map(|m| m.preferred_alias.clone())
                .unwrap_or_else(|| "Unknown".to_string())
        })
}
