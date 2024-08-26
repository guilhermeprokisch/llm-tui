use crossterm::{
    event::{self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{
    backend::CrosstermBackend,
    layout::{Constraint, Direction, Layout, Margin, Rect},
    style::{Color, Modifier, Style},
    text::Text,
    widgets::{Block, Borders, List, ListItem, ListState, Paragraph, Wrap},
    Frame, Terminal,
};
use serde_json::Value;
use std::io;
use std::process::Command;
use std::time::Duration;

#[derive(Clone)]
struct Conversation {
    id: String,
    name: String,
    messages: Vec<Message>,
}

#[derive(Clone)]
struct Message {
    role: String,
    content: String,
}

enum InputMode {
    Normal,
    Editing,
}

enum FocusedBlock {
    ConversationList,
    ModelSelect,
    Chat,
    Input,
}
struct ModelInfo {
    alias: String,
    full_name: String,
}

struct App {
    input: String,
    input_mode: InputMode,
    focused_block: FocusedBlock,
    conversations: Vec<Conversation>,
    conversation_list_state: ListState,
    current_conversation_index: Option<usize>,
    models: Vec<ModelInfo>,
    model_list_state: ListState,
}

impl App {
    fn new() -> Self {
        let conversations = load_conversations();
        let models = load_models();
        let mut app = App {
            input: String::new(),
            input_mode: InputMode::Normal,
            focused_block: FocusedBlock::ConversationList,
            conversations,
            conversation_list_state: ListState::default(),
            current_conversation_index: None,
            models,
            model_list_state: ListState::default(),
        };
        if !app.conversations.is_empty() {
            app.conversation_list_state.select(Some(0));
            app.current_conversation_index = Some(0);
        }
        if !app.models.is_empty() {
            app.model_list_state.select(Some(0));
        }
        app
    }

    fn next_focus(&mut self) {
        self.focused_block = match self.focused_block {
            FocusedBlock::ConversationList => FocusedBlock::ModelSelect,
            FocusedBlock::ModelSelect => FocusedBlock::Chat,
            FocusedBlock::Chat => FocusedBlock::Input,
            FocusedBlock::Input => FocusedBlock::ConversationList,
        };
    }

    fn next_conversation(&mut self) {
        let i = match self.conversation_list_state.selected() {
            Some(i) => {
                if i >= self.conversations.len() - 1 {
                    0
                } else {
                    i + 1
                }
            }
            None => 0,
        };
        self.conversation_list_state.select(Some(i));
        self.current_conversation_index = Some(i);
    }

    fn previous_conversation(&mut self) {
        let i = match self.conversation_list_state.selected() {
            Some(i) => {
                if i == 0 {
                    self.conversations.len() - 1
                } else {
                    i - 1
                }
            }
            None => 0,
        };
        self.conversation_list_state.select(Some(i));
        self.current_conversation_index = Some(i);
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

    fn send_message(&mut self) {
        if let Some(index) = self.current_conversation_index {
            let conversation = &mut self.conversations[index];
            let prompt = self.input.clone();
            conversation.messages.push(Message {
                role: "user".to_string(),
                content: prompt.clone(),
            });

            let model_alias = &self.models[self.model_list_state.selected().unwrap_or(0)].alias;
            let response = run_llm(&prompt, model_alias);
            conversation.messages.push(Message {
                role: "assistant".to_string(),
                content: response,
            });

            self.input.clear();
        }
    }

    fn start_new_conversation(&mut self) {
        let new_id = self.conversations.len().to_string();
        let new_conversation = Conversation {
            id: new_id.clone(),
            name: format!("New Conversation {}", new_id),
            messages: Vec::new(),
        };
        self.conversations.push(new_conversation);
        self.current_conversation_index = Some(self.conversations.len() - 1);
        self.conversation_list_state
            .select(Some(self.conversations.len() - 1));
    }
}

fn load_models() -> Vec<ModelInfo> {
    let output = Command::new("llm")
        .args(["aliases"])
        .output()
        .expect("Failed to execute llm aliases command");

    String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter_map(|line| {
            let parts: Vec<&str> = line.split(':').collect();
            if parts.len() == 2 {
                Some(ModelInfo {
                    alias: parts[0].trim().to_string(),
                    full_name: parts[1].trim().to_string(),
                })
            } else {
                None
            }
        })
        .collect()
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

        if event::poll(Duration::from_millis(100))? {
            if let Event::Key(key) = event::read()? {
                match app.focused_block {
                    FocusedBlock::ConversationList => match key.code {
                        KeyCode::Down => app.next_conversation(),
                        KeyCode::Up => app.previous_conversation(),
                        KeyCode::Enter => {
                            app.current_conversation_index = app.conversation_list_state.selected();
                            app.focused_block = FocusedBlock::Chat;
                        }
                        KeyCode::Char('n') => {
                            app.start_new_conversation();
                            app.focused_block = FocusedBlock::Input;
                        }
                        KeyCode::Tab => app.next_focus(),
                        KeyCode::Char('q') => break,
                        _ => {}
                    },
                    FocusedBlock::ModelSelect => match key.code {
                        KeyCode::Down => app.next_model(),
                        KeyCode::Up => app.previous_model(),
                        KeyCode::Tab => app.next_focus(),
                        KeyCode::Char('q') => break,
                        _ => {}
                    },
                    FocusedBlock::Chat => match key.code {
                        KeyCode::Tab => app.next_focus(),
                        KeyCode::Char('q') => break,
                        _ => {}
                    },
                    FocusedBlock::Input => match app.input_mode {
                        InputMode::Normal => match key.code {
                            KeyCode::Char('i') => app.input_mode = InputMode::Editing,
                            KeyCode::Tab => app.next_focus(),
                            KeyCode::Char('q') => break,
                            _ => {}
                        },
                        InputMode::Editing => match key.code {
                            KeyCode::Enter => {
                                app.send_message();
                                app.input_mode = InputMode::Normal;
                            }
                            KeyCode::Char(c) => {
                                app.input.push(c);
                            }
                            KeyCode::Backspace => {
                                app.input.pop();
                            }
                            KeyCode::Esc => {
                                app.input_mode = InputMode::Normal;
                            }
                            _ => {}
                        },
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
        .constraints([Constraint::Min(0), Constraint::Length(3)].as_ref())
        .split(f.size());

    let main_chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(30), Constraint::Percentage(70)].as_ref())
        .split(chunks[0]);

    let left_chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(0), Constraint::Length(3)].as_ref())
        .split(main_chunks[0]);

    let right_chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(0), Constraint::Length(3)].as_ref())
        .split(main_chunks[1]);

    render_conversation_list(f, app, left_chunks[0]);
    render_model_select(f, app, left_chunks[1]);
    render_chat(f, app, right_chunks[0]);
    render_input(f, app, right_chunks[1]);
    render_status(f, app, chunks[1]);
}

fn render_status(f: &mut Frame, app: &App, area: Rect) {
    let status = match app.focused_block {
        FocusedBlock::ConversationList => "Conversation List | ↑↓: Navigate | Enter: Select | n: New Conversation | Tab: Next Focus",
        FocusedBlock::ModelSelect => "Model Select | ↑↓: Change Model | Tab: Next Focus",
        FocusedBlock::Chat => "Chat | PgUp/PgDn: Scroll | Tab: Next Focus",
        FocusedBlock::Input => match app.input_mode {
            InputMode::Normal => "Input | i: Start Editing | Tab: Next Focus",
            InputMode::Editing => "Input (Editing) | Enter: Send | Esc: Stop Editing",
        },
    };

    let status_widget = Paragraph::new(status)
        .style(Style::default().fg(Color::Cyan))
        .block(Block::default().borders(Borders::ALL));

    f.render_widget(status_widget, area);
}

fn render_conversation_list(f: &mut Frame, app: &App, area: Rect) {
    let items: Vec<ListItem> = app
        .conversations
        .iter()
        .map(|c| ListItem::new(c.name.clone()))
        .collect();

    let border_style = if matches!(app.focused_block, FocusedBlock::ConversationList) {
        Style::default().fg(Color::Yellow)
    } else {
        Style::default()
    };

    let list = List::new(items)
        .block(
            Block::default()
                .title("Conversations")
                .borders(Borders::ALL)
                .border_style(border_style),
        )
        .highlight_style(Style::default().add_modifier(Modifier::BOLD))
        .highlight_symbol("> ");

    f.render_stateful_widget(list, area, &mut app.conversation_list_state.clone());
}

fn render_model_select(f: &mut Frame, app: &App, area: Rect) {
    let items: Vec<ListItem> = app
        .models
        .iter()
        .map(|m| ListItem::new(format!("{} ({})", m.full_name, m.alias)))
        .collect();

    let border_style = if matches!(app.focused_block, FocusedBlock::ModelSelect) {
        Style::default().fg(Color::Yellow)
    } else {
        Style::default()
    };

    let list = List::new(items)
        .block(
            Block::default()
                .title("Model")
                .borders(Borders::ALL)
                .border_style(border_style),
        )
        .highlight_style(Style::default().add_modifier(Modifier::BOLD))
        .highlight_symbol("> ");

    f.render_stateful_widget(list, area, &mut app.model_list_state.clone());
}

fn render_chat(f: &mut Frame, app: &App, area: Rect) {
    let border_style = if matches!(app.focused_block, FocusedBlock::Chat) {
        Style::default().fg(Color::Yellow)
    } else {
        Style::default()
    };

    let block = Block::default()
        .title("Chat")
        .borders(Borders::ALL)
        .border_style(border_style);
    f.render_widget(block, area);

    if let Some(index) = app.current_conversation_index {
        let conversation = &app.conversations[index];
        let mut text = Text::default();
        for message in &conversation.messages {
            let style = match message.role.as_str() {
                "user" => Style::default().fg(Color::Green),
                "assistant" => Style::default().fg(Color::Blue),
                _ => Style::default(),
            };
            text.extend(Text::styled(
                format!("{}: {}\n\n", message.role, message.content),
                style,
            ));
        }
        let paragraph = Paragraph::new(text).wrap(Wrap { trim: true });
        let inner_area = area.inner(Margin {
            vertical: 1,
            horizontal: 1,
        }); // Removed &
        f.render_widget(paragraph, inner_area);
    }
}

fn render_input(f: &mut Frame, app: &App, area: Rect) {
    let border_style = if matches!(app.focused_block, FocusedBlock::Input) {
        Style::default().fg(Color::Yellow)
    } else {
        Style::default()
    };

    let input = Paragraph::new(app.input.as_str())
        .style(match app.input_mode {
            InputMode::Normal => Style::default(),
            InputMode::Editing => Style::default().fg(Color::Yellow),
        })
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title("Input")
                .border_style(border_style),
        );

    f.render_widget(input, area);

    if let FocusedBlock::Input = app.focused_block {
        if let InputMode::Editing = app.input_mode {
            // Changed set_cursor to set_cursor_position
            f.set_cursor_position(ratatui::layout::Position {
                x: area.x + app.input.len() as u16 + 1,
                y: area.y + 1,
            });
        }
    }
}

fn load_conversations() -> Vec<Conversation> {
    let output = Command::new("llm")
        .args(["logs", "list", "--json"])
        .output()
        .expect("Failed to execute llm logs list command");

    let json: Value = serde_json::from_slice(&output.stdout).expect("Failed to parse JSON output");

    let mut conversations = Vec::new();
    let mut current_conversation: Option<Conversation> = None;

    if let Some(logs) = json.as_array() {
        for log in logs.iter().rev() {
            let conversation_id = log["conversation_id"]
                .as_str()
                .unwrap_or_default()
                .to_string();
            let conversation_name = log["conversation_name"]
                .as_str()
                .unwrap_or_default()
                .to_string();
            let prompt = log["prompt"].as_str().unwrap_or_default().to_string();
            let response = log["response"].as_str().unwrap_or_default().to_string();

            match current_conversation {
                Some(ref mut conv) if conv.id == conversation_id => {
                    conv.messages.insert(
                        0,
                        Message {
                            role: "user".to_string(),
                            content: prompt,
                        },
                    );
                    conv.messages.insert(
                        1,
                        Message {
                            role: "assistant".to_string(),
                            content: response,
                        },
                    );
                }
                _ => {
                    if let Some(conv) = current_conversation.take() {
                        conversations.push(conv);
                    }
                    current_conversation = Some(Conversation {
                        id: conversation_id,
                        name: conversation_name,
                        messages: vec![
                            Message {
                                role: "user".to_string(),
                                content: prompt,
                            },
                            Message {
                                role: "assistant".to_string(),
                                content: response,
                            },
                        ],
                    });
                }
            }
        }
    }

    if let Some(conv) = current_conversation {
        conversations.push(conv);
    }

    conversations
}

fn run_llm(prompt: &str, model_alias: &str) -> String {
    let output = Command::new("llm")
        .args(["-m", model_alias, prompt])
        .output()
        .expect("Failed to execute llm command");

    String::from_utf8_lossy(&output.stdout).to_string()
}
