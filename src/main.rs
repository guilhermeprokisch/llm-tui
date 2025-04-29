use copypasta::{ClipboardContext, ClipboardProvider};
use crossbeam_channel::{unbounded, Receiver as CrossbeamReceiver, Sender as CrossbeamSender};
use crossterm::{
    event::{self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::widgets::Gauge;
use ratatui::{
    backend::CrosstermBackend,
    layout::{Constraint, Direction, Layout, Position, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, ListState, Paragraph},
    Frame, Terminal,
};
use serde_json::Value;
use std::io;
use std::io::{BufRead, BufReader, Write};
use std::net::{TcpListener, TcpStream};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};
use unicode_width::UnicodeWidthStr;

#[derive(Clone, PartialEq)]
enum AppState {
    Normal,
    Thinking,
}

enum FeedbackType {
    Positive,
    Negative,
}

struct FeedbackMessage {
    message: String,
    feedback_type: FeedbackType,
    expires_at: Instant,
}

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

#[derive(Copy, Clone, PartialEq)]
enum InputMode {
    Normal,
    Editing,
}

#[derive(Clone, PartialEq)]
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
    show_conversation_list: bool,
    show_debug_log: bool,
    chat_state: ChatState,
    feedback: Option<FeedbackMessage>,
    tx: Sender<String>,
    rx: Receiver<String>,
    remote_command_rx: CrossbeamReceiver<String>,
    remote_command_tx: CrossbeamSender<String>,
    log_tx: Sender<String>,
    log_rx: Receiver<String>,
    debug_log: Vec<String>,
    state: AppState,
    server_running: Arc<AtomicBool>,
    remote_message_received: bool,
}

struct ChatState {
    list_state: ListState,
}

impl ChatState {
    fn new() -> Self {
        Self {
            list_state: ListState::default(),
        }
    }
}

impl App {
    fn new() -> Self {
        let (tx, rx) = channel();
        let (remote_command_tx, remote_command_rx) = unbounded();
        let (log_tx, log_rx) = channel();
        let server_running = Arc::new(AtomicBool::new(false));

        let conversations = load_conversations();
        let models = load_models(log_tx.clone());
        let app = App {
            input: String::new(),
            input_mode: InputMode::Normal,
            focused_block: FocusedBlock::ConversationList,
            conversations,
            conversation_list_state: ListState::default(),
            current_conversation_index: None,
            models,
            model_list_state: ListState::default(),
            show_conversation_list: true,
            show_debug_log: false,
            chat_state: ChatState::new(),
            feedback: None,
            state: AppState::Normal,
            server_running,
            remote_message_received: false,
            tx,
            rx,
            remote_command_rx,
            remote_command_tx,
            log_tx,
            log_rx,
            debug_log: Vec::new(),
        };
        app
    }
    fn exit_edit_mode(&mut self) {
        if self.input_mode == InputMode::Editing {
            self.input_mode = InputMode::Normal;
        }
    }

    fn next_focus(&mut self) {
        self.exit_edit_mode();
        self.focused_block = match self.focused_block {
            FocusedBlock::ConversationList => {
                if self.show_conversation_list {
                    FocusedBlock::ModelSelect
                } else {
                    FocusedBlock::Chat
                }
            }
            FocusedBlock::ModelSelect => FocusedBlock::Chat,
            FocusedBlock::Chat => FocusedBlock::Input,
            FocusedBlock::Input => {
                if self.show_conversation_list {
                    FocusedBlock::ConversationList
                } else {
                    FocusedBlock::Chat
                }
            }
        };
    }

    fn next_conversation(&mut self) {
        if self.conversations.is_empty() {
            return;
        }
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
    }

    fn previous_conversation(&mut self) {
        if self.conversations.is_empty() {
            return;
        }
        let i = match self.conversation_list_state.selected() {
            Some(i) => {
                if i == 0 {
                    self.conversations.len() - 1
                } else {
                    i - 1
                }
            }
            None => self.conversations.len().saturating_sub(1),
        };
        self.conversation_list_state.select(Some(i));
    }

    fn next_model(&mut self) {
        if self.models.is_empty() {
            return;
        }
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
        if self.models.is_empty() {
            return;
        }
        let i = match self.model_list_state.selected() {
            Some(i) => {
                if i == 0 {
                    self.models.len() - 1
                } else {
                    i - 1
                }
            }
            None => self.models.len().saturating_sub(1),
        };
        self.model_list_state.select(Some(i));
    }

    fn send_message(&mut self) {
        if let Some(index) = self.current_conversation_index {
            if self.models.is_empty() {
                self.set_feedback(
                    "No models loaded. Cannot send message.".to_string(),
                    FeedbackType::Negative,
                );
                return;
            }
            let conversation = &mut self.conversations[index];
            let prompt = self.input.trim().to_string();
            if prompt.is_empty() {
                return;
            }
            conversation.messages.push(Message {
                role: "user".to_string(),
                content: prompt.clone(),
            });
            self.input.clear();
            self.state = AppState::Thinking;
            self.scroll_to_bottom();

            let tx_clone = self.tx.clone();
            let log_tx_clone = self.log_tx.clone();
            let selected_model_index = self.model_list_state.selected().unwrap_or(0);
            let model_alias = self.models[selected_model_index].alias.clone();

            let prompt_clone = prompt.clone();
            thread::spawn(move || {
                let response = run_llm(&prompt_clone, &model_alias, log_tx_clone);
                let _ = tx_clone.send(response);
            });
        } else {
            self.set_feedback(
                "Select a conversation first.".to_string(),
                FeedbackType::Negative,
            );
        }
    }

    fn check_for_response(&mut self) {
        if let Ok(response) = self.rx.try_recv() {
            if let Some(index) = self.current_conversation_index {
                let conversation = &mut self.conversations[index];
                conversation.messages.push(Message {
                    role: "assistant".to_string(),
                    content: response,
                });
                self.state = AppState::Normal;
                self.scroll_to_bottom();
            }
        }
    }

    fn check_for_logs(&mut self) {
        while let Ok(log_message) = self.log_rx.try_recv() {
            self.debug_log.push(log_message);
            const MAX_LOG_LINES: usize = 100;
            let log_len = self.debug_log.len();
            if log_len > MAX_LOG_LINES {
                self.debug_log.drain(0..(log_len - MAX_LOG_LINES));
            }
        }
    }

    fn scroll_to_bottom(&mut self) {
        if let Some(index) = self.current_conversation_index {
            let message_count = self.conversations[index].messages.len();
            if message_count > 0 {
                self.chat_state.list_state.select(Some(message_count - 1));
            } else {
                self.chat_state.list_state.select(None);
            }
        } else {
            self.chat_state.list_state.select(None);
        }
    }

    fn start_new_conversation(&mut self) {
        let new_id = format!("conv_{}", self.conversations.len());
        let new_conversation = Conversation {
            id: new_id.clone(),
            name: format!("Conversation {}", self.conversations.len() + 1),
            messages: Vec::new(),
        };
        self.conversations.push(new_conversation);
        let new_index = self.conversations.len() - 1;
        self.current_conversation_index = Some(new_index);
        self.conversation_list_state.select(Some(new_index));
        self.scroll_to_bottom();
    }

    fn toggle_conversation_list(&mut self) {
        self.show_conversation_list = !self.show_conversation_list;
        if !self.show_conversation_list
            && (self.focused_block == FocusedBlock::ConversationList
                || self.focused_block == FocusedBlock::ModelSelect)
        {
            self.focused_block = FocusedBlock::Chat;
        } else if self.show_conversation_list
            && !(self.focused_block == FocusedBlock::ConversationList
                || self.focused_block == FocusedBlock::ModelSelect)
        {
            self.focused_block = FocusedBlock::ConversationList;
        }
    }

    fn toggle_debug_log(&mut self) {
        self.show_debug_log = !self.show_debug_log;
    }

    fn selected_message_index(&self) -> Option<usize> {
        self.chat_state.list_state.selected()
    }

    fn next_message(&mut self) {
        if let Some(index) = self.current_conversation_index {
            let messages = &self.conversations[index].messages;
            if messages.is_empty() {
                return;
            }
            let i = match self.chat_state.list_state.selected() {
                Some(i) => {
                    if i >= messages.len() - 1 {
                        messages.len() - 1
                    } else {
                        i + 1
                    }
                }
                None => 0,
            };
            self.chat_state.list_state.select(Some(i));
        }
    }

    fn previous_message(&mut self) {
        if let Some(index) = self.current_conversation_index {
            let messages = &self.conversations[index].messages;
            if messages.is_empty() {
                return;
            }
            let i = match self.chat_state.list_state.selected() {
                Some(i) => {
                    if i == 0 {
                        0
                    } else {
                        i - 1
                    }
                }
                None => messages.len().saturating_sub(1),
            };
            self.chat_state.list_state.select(Some(i));
        }
    }

    fn copy_selected_message_to_clipboard(&mut self) -> io::Result<()> {
        if let Some(conversation_index) = self.current_conversation_index {
            if let Some(message_index) = self.chat_state.list_state.selected() {
                let conversation = &self.conversations[conversation_index];
                if let Some(message) = conversation.messages.get(message_index) {
                    return match ClipboardContext::new() {
                        Ok(mut ctx) => match ctx.set_contents(message.content.clone()) {
                            Ok(_) => Ok(()),
                            Err(e) => Err(io::Error::new(
                                io::ErrorKind::Other,
                                format!("Clipboard set error: {}", e),
                            )),
                        },
                        Err(e) => Err(io::Error::new(
                            io::ErrorKind::Other,
                            format!("Clipboard context error: {}", e),
                        )),
                    };
                }
            }
        }
        Err(io::Error::new(
            io::ErrorKind::NotFound,
            "No message selected or conversation active",
        ))
    }

    fn handle_remote_command(&mut self) {
        if let Ok(input) = self.remote_command_rx.try_recv() {
            if self.current_conversation_index.is_none() {
                self.start_new_conversation();
            }
            self.input = input;
            self.send_message();
            self.set_feedback(
                "Remote message received and sent!".to_string(),
                FeedbackType::Positive,
            );
        }
    }

    fn set_feedback(&mut self, message: String, feedback_type: FeedbackType) {
        self.feedback = Some(FeedbackMessage {
            message,
            feedback_type,
            expires_at: Instant::now() + Duration::from_secs(3),
        });
    }

    fn update_feedback(&mut self) {
        if let Some(feedback) = &self.feedback {
            if Instant::now() > feedback.expires_at {
                self.feedback = None;
            }
        }
    }
}

fn load_models(log_tx: Sender<String>) -> Vec<ModelInfo> {
    let _ = log_tx.send("[DEBUG] load_models: Executing 'llm models'".to_string());
    let output = match Command::new("llm").args(["models"]).output() {
        Ok(output) => output,
        Err(e) => {
            let _ = log_tx.send(format!(
                "[ERROR] load_models: Failed to execute 'llm models': {}",
                e
            ));
            return vec![];
        }
    };

    let _ = log_tx.send("[DEBUG] load_models: Parsing output".to_string());
    let stdout_str = String::from_utf8_lossy(&output.stdout);
    let stderr_str = String::from_utf8_lossy(&output.stderr);

    if !output.status.success() {
        let _ = log_tx.send(format!(
            "[ERROR] load_models: llm models command failed with status {}: {}",
            output.status, stderr_str
        ));
    }

    stdout_str
        .lines()
        .filter_map(|line| {
            let parts: Vec<&str> = line.splitn(2, ':').collect();
            if parts.len() == 2 {
                let model_part = parts[1].trim();
                let model_name = model_part
                    .split('(')
                    .next()
                    .unwrap_or("")
                    .trim()
                    .to_string();
                if !model_name.is_empty() {
                    let _ =
                        log_tx.send(format!("[DEBUG] load_models: Found model '{}'", model_name));
                    Some(ModelInfo {
                        alias: model_name.clone(),
                        full_name: model_name,
                    })
                } else {
                    let _ = log_tx.send(format!(
                        "[DEBUG] load_models: Skipping line (empty model name): '{}'",
                        line
                    ));
                    None
                }
            } else {
                let _ = log_tx.send(format!(
                    "[DEBUG] load_models: Skipping line (no colon): '{}'",
                    line
                ));
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

    let app = Arc::new(Mutex::new(App::new()));

    let app_clone_server = Arc::clone(&app);
    let server_running_flag = Arc::clone(&app.lock().unwrap().server_running);
    thread::spawn(move || match TcpListener::bind("127.0.0.1:8080") {
        Ok(listener) => {
            server_running_flag.store(true, Ordering::SeqCst);
            for stream in listener.incoming() {
                match stream {
                    Ok(stream) => {
                        let tx = match app_clone_server.lock() {
                            Ok(guard) => guard.remote_command_tx.clone(),
                            Err(_) => {
                                eprintln!("Failed to lock app for remote command tx");
                                continue;
                            }
                        };
                        thread::spawn(move || {
                            handle_client(stream, tx);
                        });
                    }
                    Err(e) => eprintln!("TCP stream error: {}", e),
                }
            }
        }
        Err(e) => {
            server_running_flag.store(false, Ordering::SeqCst);
            eprintln!("Failed to bind TCP server: {}", e);
            let app_guard = app_clone_server.lock();
            if let Ok(app_instance) = app_guard {
                let _ = app_instance
                    .log_tx
                    .send(format!("[ERROR] Failed to bind TCP server: {}", e));
            }
        }
    });

    loop {
        {
            let mut app_guard = app.lock().unwrap();
            app_guard.update_feedback();
            app_guard.check_for_response();
            app_guard.handle_remote_command();
            app_guard.check_for_logs();

            if let Err(e) = terminal.draw(|f| ui(f, &mut *app_guard)) {
                eprintln!("Terminal draw error: {}", e);
                break;
            }
        }

        if event::poll(Duration::from_millis(50))? {
            if let Event::Key(key) = event::read()? {
                let mut app_guard = app.lock().unwrap();

                // Handle Input Editing Mode FIRST - it overrides everything else
                if app_guard.input_mode == InputMode::Editing {
                    match key.code {
                        KeyCode::Enter => {
                            app_guard.send_message();
                            app_guard.input_mode = InputMode::Normal;
                        }
                        KeyCode::Char(c) => {
                            app_guard.input.push(c);
                        }
                        KeyCode::Backspace => {
                            app_guard.input.pop();
                        }
                        KeyCode::Esc => {
                            app_guard.input_mode = InputMode::Normal;
                        }
                        // Ignore other keys (like 'h', 'd', 'q', 'Tab') while editing
                        _ => {}
                    }
                    continue; // Skip further processing if in editing mode
                }

                // Handle Global Keybindings (only when NOT in Input Editing Mode)
                match key.code {
                    KeyCode::Char('q') => break,
                    KeyCode::Char('d') => {
                        app_guard.toggle_debug_log();
                        continue;
                    }
                    KeyCode::Char('h') => {
                        app_guard.toggle_conversation_list();
                        continue;
                    }
                    KeyCode::Tab => {
                        app_guard.next_focus();
                        continue;
                    }
                    _ => {} // Not a global key, proceed to block-specific
                }

                // Handle Block-Specific Keybindings (only when NOT in Input Editing Mode)
                let focused_block = app_guard.focused_block.clone();
                match focused_block {
                    FocusedBlock::ConversationList => match key.code {
                        KeyCode::Char('j') | KeyCode::Down => app_guard.next_conversation(),
                        KeyCode::Char('k') | KeyCode::Up => app_guard.previous_conversation(),
                        KeyCode::Enter => {
                            app_guard.current_conversation_index =
                                app_guard.conversation_list_state.selected();
                            if app_guard.current_conversation_index.is_some() {
                                app_guard.scroll_to_bottom();
                                app_guard.focused_block = FocusedBlock::Chat;
                            }
                        }
                        KeyCode::Char('n') => {
                            app_guard.start_new_conversation();
                            app_guard.focused_block = FocusedBlock::Input;
                            app_guard.input_mode = InputMode::Editing; // Go directly to editing
                        }
                        KeyCode::Char('i') => {
                            // Allow 'i' to jump to input editing
                            app_guard.focused_block = FocusedBlock::Input;
                            app_guard.input_mode = InputMode::Editing;
                        }
                        _ => {}
                    },
                    FocusedBlock::ModelSelect => match key.code {
                        KeyCode::Char('j') | KeyCode::Down => app_guard.next_model(),
                        KeyCode::Char('k') | KeyCode::Up => app_guard.previous_model(),
                        KeyCode::Char('i') => {
                            // Allow 'i' to jump to input editing
                            app_guard.focused_block = FocusedBlock::Input;
                            app_guard.input_mode = InputMode::Editing;
                        }
                        _ => {}
                    },
                    FocusedBlock::Chat => match key.code {
                        KeyCode::Char('j') | KeyCode::Down => app_guard.next_message(),
                        KeyCode::Char('k') | KeyCode::Up => app_guard.previous_message(),
                        KeyCode::PageDown => {
                            for _ in 0..10 {
                                app_guard.next_message();
                            }
                        }
                        KeyCode::PageUp => {
                            for _ in 0..10 {
                                app_guard.previous_message();
                            }
                        }
                        KeyCode::Char('i') => {
                            // Allow 'i' to jump to input editing
                            app_guard.focused_block = FocusedBlock::Input;
                            app_guard.input_mode = InputMode::Editing;
                        }
                        KeyCode::Char('y') => {
                            match app_guard.copy_selected_message_to_clipboard() {
                                Ok(_) => app_guard.set_feedback(
                                    "Message copied!".to_string(),
                                    FeedbackType::Positive,
                                ),
                                Err(e) => app_guard.set_feedback(
                                    format!("Copy failed: {}", e),
                                    FeedbackType::Negative,
                                ),
                            }
                        }
                        _ => {}
                    },
                    FocusedBlock::Input => {
                        // Only handle 'i' here when in Normal mode
                        match key.code {
                            KeyCode::Char('i') => app_guard.input_mode = InputMode::Editing,
                            _ => {}
                        }
                    }
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
    let constraints = if app.show_debug_log {
        vec![
            Constraint::Min(0),
            Constraint::Percentage(20),
            Constraint::Length(1),
        ]
    } else {
        vec![Constraint::Min(0), Constraint::Length(1)]
    };

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints(constraints.as_slice())
        .split(f.area());

    let main_content_area = chunks[0];
    let status_area = chunks[chunks.len() - 1];

    let main_chunks = if app.show_conversation_list {
        Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(30), Constraint::Percentage(70)].as_ref())
            .split(main_content_area)
    } else {
        Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(100)].as_ref())
            .split(main_content_area)
    };

    if app.show_conversation_list {
        let left_chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Min(0), Constraint::Length(3)].as_ref())
            .split(main_chunks[0]);
        render_conversation_list(f, app, left_chunks[0]);
        render_model_select(f, app, left_chunks[1]);
    }

    let right_area = if app.show_conversation_list {
        main_chunks[1]
    } else {
        main_chunks[0]
    };
    let right_chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(0), Constraint::Length(3)].as_ref())
        .split(right_area);
    render_chat(f, app, right_chunks[0]);
    render_input(f, app, right_chunks[1]);

    if app.show_debug_log {
        let debug_log_area = chunks[1];
        render_debug_log(f, app, debug_log_area);
    }

    render_status(f, app, status_area);
}

fn render_status(f: &mut Frame, app: &App, area: Rect) {
    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(80), Constraint::Percentage(20)].as_ref())
        .split(area);

    let status_text_area = chunks[0];
    let server_gauge_area = chunks[1];

    let status_span = if let Some(feedback) = &app.feedback {
        let feedback_color = match feedback.feedback_type {
            FeedbackType::Positive => Color::Green,
            FeedbackType::Negative => Color::Red,
        };
        Span::styled(&feedback.message, Style::default().fg(feedback_color))
    } else if matches!(app.state, AppState::Thinking) {
        Span::styled(
            "⏳ Processing request...",
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        )
    } else {
        // Display appropriate help text based on mode and focus
        let help_text = if app.input_mode == InputMode::Editing {
            "Input (Editing) | Ent:Send | Esc:Stop Edit | q:Quit" // Editing mode has limited actions
        } else {
            match app.focused_block { // Normal mode help text varies by focus
                  FocusedBlock::ConversationList => "Conv List | j/k:Nav | Ent:Sel | n:New | i:Edit | Tab:Next | h:Toggle Conv | d:Toggle Debug | q:Quit",
                  FocusedBlock::ModelSelect => "Model Select | j/k:Change | i:Edit | Tab:Next | h:Toggle Conv | d:Toggle Debug | q:Quit",
                  FocusedBlock::Chat => "Chat | j/k/PgUp/PgDn:Scroll | y:Copy | i:Edit | Tab:Next | h:Toggle Conv | d:Toggle Debug | q:Quit",
                  FocusedBlock::Input => "Input | i:Edit | Tab:Next | h:Toggle Conv | d:Toggle Debug | q:Quit",
             }
        };
        Span::styled(help_text, Style::default().fg(Color::DarkGray))
    };

    let status_widget = Paragraph::new(status_span).style(Style::default());
    f.render_widget(status_widget, status_text_area);

    let server_status = if app.server_running.load(Ordering::SeqCst) {
        "SRV: On"
    } else {
        "SRV: Off"
    };
    let server_color = if app.server_running.load(Ordering::SeqCst) {
        Color::Green
    } else {
        Color::Red
    };
    let gauge = Gauge::default()
        .gauge_style(Style::default().fg(server_color))
        .ratio(if app.server_running.load(Ordering::SeqCst) {
            1.0
        } else {
            0.0
        })
        .label(server_status);

    f.render_widget(gauge, server_gauge_area);
}

fn render_conversation_list(f: &mut Frame, app: &mut App, area: Rect) {
    let items: Vec<ListItem> = app
        .conversations
        .iter()
        .map(|c| ListItem::new(c.name.clone()))
        .collect();

    let border_style = if app.focused_block == FocusedBlock::ConversationList {
        Style::default().fg(Color::Yellow)
    } else {
        Style::default().fg(Color::DarkGray)
    };
    let list = List::new(items)
        .block(
            Block::default()
                .title("Conversations (n:New)")
                .borders(Borders::ALL)
                .border_style(border_style),
        )
        .highlight_style(
            Style::default()
                .add_modifier(Modifier::BOLD)
                .bg(Color::DarkGray),
        )
        .highlight_symbol("> ");
    f.render_stateful_widget(list, area, &mut app.conversation_list_state);
}

fn render_model_select(f: &mut Frame, app: &mut App, area: Rect) {
    let items: Vec<ListItem> = app
        .models
        .iter()
        .map(|m| ListItem::new(m.alias.clone()))
        .collect();
    let border_style = if app.focused_block == FocusedBlock::ModelSelect {
        Style::default().fg(Color::Yellow)
    } else {
        Style::default().fg(Color::DarkGray)
    };
    let list = List::new(items)
        .block(
            Block::default()
                .title("Model (j/k)")
                .borders(Borders::ALL)
                .border_style(border_style),
        )
        .highlight_style(
            Style::default()
                .add_modifier(Modifier::BOLD)
                .bg(Color::DarkGray),
        )
        .highlight_symbol("> ");
    f.render_stateful_widget(list, area, &mut app.model_list_state);
}

fn render_chat(f: &mut Frame, app: &mut App, area: Rect) {
    let border_style = if app.focused_block == FocusedBlock::Chat {
        Style::default().fg(Color::Yellow)
    } else {
        Style::default().fg(Color::DarkGray)
    };
    let block = Block::default()
        .title("Chat (j/k/PgUp/PgDn, y:Copy)")
        .borders(Borders::ALL)
        .border_style(border_style);
    let inner_area = block.inner(area);
    f.render_widget(block, area);

    let messages: Vec<ListItem> = if let Some(index) = app.current_conversation_index {
        app.conversations[index]
            .messages
            .iter()
            .map(|msg| {
                let (style, prefix) = match msg.role.as_str() {
                    "user" => (Style::default().fg(Color::Green), "You: "),
                    "assistant" => (Style::default().fg(Color::Cyan), "AI:  "),
                    _ => (Style::default().fg(Color::DarkGray), "???: "),
                };
                let content = format!("{}{}", prefix, msg.content);
                let wrapped_content =
                    textwrap::wrap(&content, inner_area.width.saturating_sub(2) as usize);
                let lines: Vec<Line> = wrapped_content
                    .into_iter()
                    .map(|line| Line::from(vec![Span::styled(line.to_string(), style)]))
                    .collect();
                ListItem::new(lines).style(style)
            })
            .collect()
    } else {
        vec![ListItem::new("Select or start a conversation (n)")]
    };

    let list_highlight_style = Style::default().add_modifier(Modifier::REVERSED);

    let messages_list = List::new(messages)
        .highlight_style(list_highlight_style)
        .highlight_symbol(if app.focused_block == FocusedBlock::Chat {
            "> "
        } else {
            "  "
        });

    f.render_stateful_widget(messages_list, inner_area, &mut app.chat_state.list_state);
}

fn render_input(f: &mut Frame, app: &mut App, area: Rect) {
    let border_style = if app.focused_block == FocusedBlock::Input {
        Style::default().fg(Color::Yellow)
    } else {
        Style::default().fg(Color::DarkGray)
    };
    let input = Paragraph::new(app.input.as_str())
        .style(match app.input_mode {
            InputMode::Normal => Style::default().fg(Color::DarkGray),
            InputMode::Editing => Style::default().fg(Color::White),
        })
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title("Input (i:Edit, Esc:Stop, Enter:Send)")
                .border_style(border_style),
        );
    f.render_widget(input, area);

    if app.focused_block == FocusedBlock::Input && app.input_mode == InputMode::Editing {
        let input_width = UnicodeWidthStr::width(app.input.as_str());
        let cursor_x = (area.x + 1 + input_width as u16).min(area.right() - 1);
        f.set_cursor_position(Position {
            x: cursor_x,
            y: area.y + 1,
        });
    }
}

fn render_debug_log(f: &mut Frame, app: &App, area: Rect) {
    let reversed_logs: Vec<ListItem> = app
        .debug_log
        .iter()
        .rev()
        .map(|msg| ListItem::new(msg.as_str()))
        .collect();

    let log_list = List::new(reversed_logs)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title("Debug Log (d:Toggle)"),
        )
        .style(Style::default().fg(Color::DarkGray));

    f.render_widget(log_list, area);
}

fn load_conversations() -> Vec<Conversation> {
    let output = match Command::new("llm")
        .args(["logs", "list", "--json"])
        .output()
    {
        Ok(out) => out,
        Err(e) => {
            eprintln!("Failed to execute llm logs list command: {}", e);
            return vec![];
        }
    };

    let json: Value = match serde_json::from_slice(&output.stdout) {
        Ok(val) => val,
        Err(e) => {
            eprintln!("Failed to parse llm logs JSON output: {}", e);
            eprintln!("Raw output: {}", String::from_utf8_lossy(&output.stdout));
            return vec![];
        }
    };

    let mut conversations_map: std::collections::HashMap<String, Conversation> =
        std::collections::HashMap::new();
    let mut conversation_order: Vec<String> = Vec::new();

    if let Some(logs) = json.as_array() {
        for log in logs.iter() {
            let conversation_id = log["conversation_id"].as_str().unwrap_or("").to_string();
            let conversation_name = log["conversation_name"]
                .as_str()
                .unwrap_or("Unnamed Conversation")
                .to_string();
            let prompt = log["prompt"].as_str().unwrap_or("").to_string();
            let response = log["response"].as_str().unwrap_or("").to_string();
            let _timestamp = log["timestamp"].as_str().unwrap_or("").to_string();

            if conversation_id.is_empty() {
                continue;
            }

            let conversation = conversations_map
                .entry(conversation_id.clone())
                .or_insert_with(|| {
                    conversation_order.push(conversation_id.clone());
                    Conversation {
                        id: conversation_id.clone(),
                        name: conversation_name,
                        messages: Vec::new(),
                    }
                });

            if !prompt.is_empty() {
                conversation.messages.push(Message {
                    role: "user".to_string(),
                    content: prompt,
                });
            }
            if !response.is_empty() {
                conversation.messages.push(Message {
                    role: "assistant".to_string(),
                    content: response,
                });
            }
        }
    }

    conversation_order
        .into_iter()
        .filter_map(|id| conversations_map.remove(&id))
        .collect()
}

fn run_llm(prompt: &str, model_alias: &str, log_tx: Sender<String>) -> String {
    let _ = log_tx.send(format!(
        "[DEBUG] run_llm: Executing with model: '{}'",
        model_alias
    ));
    let mut command = Command::new("llm");
    command.args(["-m", model_alias, prompt]);
    command.stdout(Stdio::piped());
    command.stderr(Stdio::piped());

    let _ = log_tx.send("[DEBUG] run_llm: Spawning command...".to_string());
    let child = match command.spawn() {
        Ok(child) => child,
        Err(e) => {
            let err_msg = format!("Error: Failed to spawn llm command: {}", e);
            let _ = log_tx.send(format!("[ERROR] run_llm: {}", err_msg));
            return err_msg;
        }
    };
    let _ = log_tx.send("[DEBUG] run_llm: Command spawned.".to_string());

    let _ = log_tx.send("[DEBUG] run_llm: Waiting for command output...".to_string());
    match child.wait_with_output() {
        Ok(output) => {
            let stdout_str = String::from_utf8_lossy(&output.stdout).to_string();
            let stderr_str = String::from_utf8_lossy(&output.stderr).to_string();
            let _ = log_tx.send(format!(
                "[DEBUG] run_llm: Command finished with status: {}",
                output.status
            ));

            if !output.status.success() {
                let err_msg = format!("Error executing llm ({}): {}", output.status, stderr_str);
                let _ = log_tx.send(format!("[ERROR] run_llm: {}", err_msg));
                if stderr_str.is_empty() {
                    format!(
                        "{}\nError executing llm ({}): No stderr output",
                        stdout_str, output.status
                    )
                } else {
                    format!(
                        "{}\nError executing llm ({}): {}",
                        stdout_str, output.status, stderr_str
                    )
                }
            } else {
                let _ = log_tx.send("[DEBUG] run_llm: Command succeeded.".to_string());
                if !stderr_str.is_empty() {
                    let _ =
                        log_tx.send(format!("[WARN] run_llm: Stderr on success: {}", stderr_str));
                }
                stdout_str
            }
        }
        Err(e) => {
            let err_msg = format!(
                "Error: Failed to wait for or read output from llm command: {}",
                e
            );
            let _ = log_tx.send(format!("[ERROR] run_llm: {}", err_msg));
            err_msg
        }
    }
}

fn handle_client(mut stream: TcpStream, tx: CrossbeamSender<String>) {
    let mut command_output = String::new();
    let mut reader = BufReader::new(&stream);

    match reader.read_line(&mut command_output) {
        Ok(0) => { /* Connection closed */ }
        Ok(_) => {
            if let Err(e) = tx.send(command_output.trim().to_string()) {
                eprintln!("Failed to send remote command to main thread: {}", e);
            }
            if let Err(e) = stream.write_all(b"Command received.\n") {
                eprintln!("Failed to write confirmation to client: {}", e);
            }
        }
        Err(e) => {
            eprintln!("Failed to read from TCP stream: {}", e);
        }
    }
}
