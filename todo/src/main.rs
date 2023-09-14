use chrono::DateTime;
use chrono::Utc;
use crossterm::event;
use crossterm::event::Event as CEvent;
use crossterm::event::KeyCode;
use crossterm::terminal::disable_raw_mode;
use crossterm::terminal::enable_raw_mode;
use serde::{Deserialize, Serialize};
use serde_json;
use std::fs;
use std::sync::mpsc;
use std::time::{Duration, Instant};
use std::vec;
use std::{io, thread};
use thiserror::Error;
use tui::style::Color;
use tui::style::Modifier;
use tui::text::{Span, Spans};
use tui::widgets::{
    Block, BorderType, Borders, Cell, Clear, List, ListItem, ListState, Paragraph, Row, Table, Tabs,
};
use tui::{
    backend::CrosstermBackend,
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::Style,
    Terminal,
};

const DB_PATH: &str = "./data/db.json";

#[derive(Serialize, Deserialize, Clone)]
struct Task {
    id: usize,
    name: String,
    created_at: DateTime<Utc>,
    completed_at: Option<DateTime<Utc>>,
}

#[derive(Error, Debug)]
pub enum Error {
    #[error("Error reading the DB file {0}")]
    ReadDBError(#[from] io::Error),
    #[error("Error parsing the DB file {0}")]
    ParseDBError(#[from] serde_json::Error),
}

enum Event<I> {
    Input(I),
    Tick,
}

#[derive(Copy, Clone, Debug)]
enum MenuItem {
    Home,
    Tasks,
}

impl From<MenuItem> for usize {
    fn from(input: MenuItem) -> usize {
        match input {
            MenuItem::Home => 0,
            MenuItem::Tasks => 1,
        }
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    enable_raw_mode()?;
    let (tx, rx) = mpsc::channel();
    let tick_rate = Duration::from_millis(200);

    thread::spawn(move || {
        let mut last_tick = Instant::now();

        loop {
            let timeout = tick_rate
                .checked_sub(last_tick.elapsed())
                .unwrap_or_else(|| Duration::from_secs(0));

            if event::poll(timeout).expect("Poll works") {
                if let CEvent::Key(key) = event::read().expect("Can read events") {
                    tx.send(Event::Input(key)).expect("Can send events");
                }
            }

            if last_tick.elapsed() >= tick_rate {
                if let Ok(_) = tx.send(Event::Tick) {
                    last_tick = Instant::now();
                }
            }
        }
    });

    let stdout = io::stdout();
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;
    terminal.clear()?;

    let menu_titles = vec!["Home", "Tasks"];
    let mut show_pop_up = false;
    let mut active_menu_item = MenuItem::Home;
    let mut task_list_state = ListState::default();
    task_list_state.select(Some(0));

    loop {
        terminal.draw(|rect| {
            let size = rect.size();
            let chunks = Layout::default()
                .direction(Direction::Vertical)
                .margin(2)
                .constraints(
                    [
                        Constraint::Length(3),
                        Constraint::Min(2),
                        Constraint::Length(3),
                    ]
                    .as_ref(),
                )
                .split(size);

            let copyright = Paragraph::new("todo-CLI 2023 - all rights reserved")
                .style(Style::default().fg(Color::LightCyan))
                .alignment(Alignment::Center)
                .block(
                    Block::default()
                        .borders(Borders::ALL)
                        .style(Style::default().fg(Color::White))
                        .title("Copyright")
                        .border_type(BorderType::Plain),
                );

            rect.render_widget(copyright, chunks[2]);

            let menu = menu_titles
                .iter()
                .map(|t| {
                    let (first, rest) = t.split_at(1);
                    Spans::from(vec![
                        Span::styled(
                            first,
                            Style::default()
                                .fg(Color::Yellow)
                                .add_modifier(Modifier::UNDERLINED),
                        ),
                        Span::styled(rest, Style::default().fg(Color::White)),
                    ])
                })
                .collect();

            let tabs = Tabs::new(menu)
                .select(active_menu_item.into())
                .block(Block::default().title("Menu").borders(Borders::ALL))
                .style(Style::default().fg(Color::White))
                .highlight_style(Style::default().fg(Color::Yellow))
                .divider(Span::raw("|"));

            rect.render_widget(tabs, chunks[0]);

            if show_pop_up {
                let (block, area) = render_popup(size);
                rect.render_widget(Clear, area);
                rect.render_widget(block, area);
            }

            match active_menu_item {
                MenuItem::Home => rect.render_widget(render_home(), chunks[1]),
                MenuItem::Tasks => {
                    let todo_chunks = Layout::default()
                        .direction(Direction::Horizontal)
                        .constraints(
                            [Constraint::Percentage(20), Constraint::Percentage(80)].as_ref(),
                        )
                        .split(chunks[1]);
                    let (left, right) = render_todo(&task_list_state);
                    rect.render_stateful_widget(left, todo_chunks[0], &mut task_list_state);
                    rect.render_widget(right, todo_chunks[1]);
                }
            }
        })?;

        match rx.recv()? {
            Event::Input(event) => match event.code {
                KeyCode::Char('q') => {
                    disable_raw_mode()?;
                    terminal.show_cursor()?;
                    break;
                }
                KeyCode::Char('h') => active_menu_item = MenuItem::Home,
                KeyCode::Char('t') => active_menu_item = MenuItem::Tasks,
                KeyCode::Char('a') => show_pop_up = true,
                KeyCode::Enter => show_pop_up = false,
                KeyCode::Char('d') => {
                    remove_task_at_index(&mut task_list_state).unwrap_or_else(|_| ());
                }
                KeyCode::Down => {
                    if let Some(selected) = task_list_state.selected() {
                        let amount_tasks = read_db().expect("Can read db.").len();
                        if selected >= amount_tasks - 1 {
                            task_list_state.select(Some(0));
                        } else {
                            task_list_state.select(Some(selected + 1));
                        }
                    }
                }
                KeyCode::Up => {
                    if let Some(selected) = task_list_state.selected() {
                        let amount_tasks = read_db().expect("Can read db.").len();
                        if selected > 0 {
                            task_list_state.select(Some(selected - 1));
                        } else {
                            task_list_state.select(Some(amount_tasks - 1));
                        }
                    }
                }
                _ => {}
            },
            Event::Tick => {}
        }
    }

    Ok(())
}

fn render_home<'a>() -> Paragraph<'a> {
    let home = Paragraph::new(vec![
        Spans::from(vec![Span::raw("")]),
        Spans::from(vec![Span::raw("Welcome")]),
        Spans::from(vec![Span::raw("")]),
        Spans::from(vec![Span::raw("to")]),
        Spans::from(vec![Span::styled(
            "todo-CLI",
            Style::default().fg(Color::LightBlue),
        )]),
        Spans::from(vec![Span::raw("")]),
        Spans::from(vec![Span::raw("Press 't' top access the todo list")]),
    ])
    .alignment(Alignment::Center)
    .block(
        Block::default()
            .borders(Borders::ALL)
            .style(Style::default().fg(Color::White))
            .title("Home")
            .border_type(BorderType::Plain),
    );
    home
}

fn read_db() -> Result<Vec<Task>, Error> {
    let db_content = fs::read_to_string(DB_PATH)?;
    let parsed: Vec<Task> = serde_json::from_str(&db_content)?;
    Ok(parsed)
}

fn write_db(tasks: &Vec<Task>) -> Result<(), Error> {
    fs::write(DB_PATH, &serde_json::to_vec(tasks)?)?;
    Ok(())
}

fn render_todo<'a>(task_list_state: &ListState) -> (List<'a>, Table<'a>) {
    let tasks = Block::default()
        .borders(Borders::ALL)
        .style(Style::default().fg(Color::White))
        .title("Todo list")
        .border_type(BorderType::Plain);

    let task_list = read_db().expect("Can fetch task list");
    let items: Vec<_> = task_list
        .iter()
        .map(|task| {
            ListItem::new(Spans::from(vec![Span::styled(
                task.name.clone(),
                Style::default(),
            )]))
        })
        .collect();

    let selected_task = task_list
        .get(
            task_list_state
                .selected()
                .expect("There is always a selected task."),
        )
        .expect("Exists")
        .clone();

    let list = List::new(items).block(tasks).highlight_style(
        Style::default()
            .bg(Color::Yellow)
            .fg(Color::Black)
            .add_modifier(Modifier::BOLD),
    );

    let task_detail = Table::new(vec![Row::new(vec![
        Cell::from(Span::raw(selected_task.id.to_string())),
        Cell::from(Span::raw(selected_task.name)),
        Cell::from(Span::raw(selected_task.created_at.to_string())),
        Cell::from(Span::raw(match selected_task.completed_at {
            Some(completed_at) => completed_at.to_string(),
            None => "".to_string(),
        })),
    ])])
    .header(Row::new(vec![
        Cell::from(Span::styled(
            "ID",
            Style::default().add_modifier(Modifier::BOLD),
        )),
        Cell::from(Span::styled(
            "Name",
            Style::default().add_modifier(Modifier::BOLD),
        )),
        Cell::from(Span::styled(
            "Created At",
            Style::default().add_modifier(Modifier::BOLD),
        )),
        Cell::from(Span::styled(
            "Completed At",
            Style::default().add_modifier(Modifier::BOLD),
        )),
    ]))
    .block(
        Block::default()
            .borders(Borders::ALL)
            .style(Style::default().fg(Color::White))
            .title("Detail")
            .border_type(BorderType::Plain),
    )
    .widths(&[
        Constraint::Percentage(8),
        Constraint::Percentage(23),
        Constraint::Percentage(23),
        Constraint::Percentage(23),
    ]);
    (list, task_detail)
}

fn add_task_to_db(task_name: &str) -> Result<Vec<Task>, Error> {
    let mut parsed = read_db()?;

    let new_id = match parsed.last() {
        Some(task) => task.id + 1,
        None => 0,
    };

    parsed.push(Task {
        id: new_id,
        name: task_name.to_string(),
        created_at: Utc::now(),
        completed_at: None,
    });
    write_db(&parsed)?;
    Ok(parsed)
}

fn remove_task_at_index(task_list_state: &mut ListState) -> Result<(), Error> {
    if let Some(selected) = task_list_state.selected() {
        let mut parsed = read_db()?;
        parsed.remove(selected);
        write_db(&parsed)?;
        task_list_state.select(Some(selected - 1));
    }
    Ok(())
}

fn render_popup<'a>(size: Rect) -> (Block<'a>, Rect) {
    let block = Block::default().title("Add task").borders(Borders::ALL);
    let area = centered_rect(60, 20, size);
    (block, area)
}

fn centered_rect(percent_x: u16, percent_y: u16, rect: Rect) -> Rect {
    let popup_layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints(
            [
                Constraint::Percentage((100 - percent_y) / 2),
                Constraint::Percentage(percent_y),
                Constraint::Percentage((100 - percent_y) / 2),
            ]
            .as_ref(),
        )
        .split(rect);

    Layout::default()
        .direction(Direction::Horizontal)
        .constraints(
            [
                Constraint::Percentage((100 - percent_x) / 2),
                Constraint::Percentage(percent_x),
                Constraint::Percentage((100 - percent_x) / 2),
            ]
            .as_ref(),
        )
        .split(popup_layout[1])[1]
}
