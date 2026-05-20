use ratatui::{
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{
        Block, Borders, Cell, Clear, List, ListItem, ListState, Paragraph, Row, Table, TableState,
        Wrap,
    },
    Frame,
};

use crate::{
    app::{App, Mode, ACTIONS},
    proxmox::Vm,
};

pub fn draw(frame: &mut Frame, app: &App) {
    let area = frame.area();
    let root = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Min(10),
            Constraint::Length(7),
            Constraint::Length(2),
        ])
        .split(area);

    draw_header(frame, app, root[0]);
    draw_main(frame, app, root[1]);
    draw_log_preview(frame, app, root[2]);
    draw_footer(frame, root[3]);

    match &app.mode {
        Mode::ActionMenu => draw_action_menu(frame, app, area),
        Mode::Confirm { message, .. } => draw_confirm(frame, message, area),
        Mode::Logs => draw_logs(frame, app, area),
        Mode::Help => draw_help(frame, area),
        Mode::Browsing => {}
    }
}

fn draw_header(frame: &mut Frame, app: &App, area: Rect) {
    let title = Line::from(vec![
        Span::styled(
            "PVE VM Launcher",
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw("  "),
        Span::raw(format!("node={}  ", app.proxmox.node())),
        Span::styled(&app.status_line, Style::default().fg(Color::Yellow)),
    ]);
    let block = Block::default().borders(Borders::ALL);
    frame.render_widget(Paragraph::new(title).block(block), area);
}

fn draw_main(frame: &mut Frame, app: &App, area: Rect) {
    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(68), Constraint::Percentage(32)])
        .split(area);

    draw_vm_table(frame, app, chunks[0]);
    draw_details(frame, app, chunks[1]);
}

fn draw_vm_table(frame: &mut Frame, app: &App, area: Rect) {
    let header = Row::new([
        Cell::from("VMID"),
        Cell::from("Name"),
        Cell::from("Status"),
        Cell::from("Memory"),
        Cell::from("Disk"),
        Cell::from("PID"),
    ])
    .style(
        Style::default()
            .fg(Color::Cyan)
            .add_modifier(Modifier::BOLD),
    );

    let rows = app.vms.iter().map(|vm| {
        Row::new([
            Cell::from(vm.vmid.to_string()),
            Cell::from(vm.name.clone()),
            Cell::from(vm.status.clone()),
            Cell::from(format_memory(vm.memory_mb)),
            Cell::from(format_disk(vm.bootdisk_gb)),
            Cell::from(
                vm.pid
                    .map(|pid| pid.to_string())
                    .unwrap_or_else(|| "-".to_string()),
            ),
        ])
        .style(status_style(&vm.status))
    });

    let table = Table::new(
        rows,
        [
            Constraint::Length(7),
            Constraint::Min(18),
            Constraint::Length(10),
            Constraint::Length(10),
            Constraint::Length(10),
            Constraint::Length(8),
        ],
    )
    .header(header)
    .block(
        Block::default()
            .title(format!(" VMs ({}) ", app.vms.len()))
            .borders(Borders::ALL),
    )
    .row_highlight_style(
        Style::default()
            .fg(Color::Black)
            .bg(Color::Cyan)
            .add_modifier(Modifier::BOLD),
    )
    .highlight_symbol(">> ");

    let mut state =
        TableState::default().with_selected((!app.vms.is_empty()).then_some(app.selected));
    frame.render_stateful_widget(table, area, &mut state);
}

fn draw_details(frame: &mut Frame, app: &App, area: Rect) {
    let mut lines = Vec::new();

    if let Some(vm) = app.selected_vm() {
        lines.extend(vm_detail_lines(vm));
    } else {
        lines.push(Line::from("No VM selected"));
    }

    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        "Actions",
        Style::default().add_modifier(Modifier::BOLD),
    )));
    lines.push(Line::from("[Enter] action palette"));
    lines.push(Line::from("[a] auto attach"));
    lines.push(Line::from("[p] SPICE  [v] VNC"));
    lines.push(Line::from("[s] start  [S] shutdown"));
    lines.push(Line::from("[f] stop   [b] reboot"));
    lines.push(Line::from("[x] reset  [r] refresh"));

    let block = Block::default().title(" Details ").borders(Borders::ALL);
    frame.render_widget(
        Paragraph::new(lines).block(block).wrap(Wrap { trim: true }),
        area,
    );
}

fn draw_log_preview(frame: &mut Frame, app: &App, area: Rect) {
    let lines: Vec<Line> = app
        .logs
        .iter()
        .rev()
        .take(5)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .map(|line| Line::from(line.as_str()))
        .collect();

    let block = Block::default().title(" Log ").borders(Borders::ALL);
    frame.render_widget(
        Paragraph::new(lines).block(block).wrap(Wrap { trim: true }),
        area,
    );
}

fn draw_footer(frame: &mut Frame, area: Rect) {
    let line = Line::from(vec![
        Span::styled(
            "q",
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw(" quit  "),
        Span::styled(
            "?",
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw(" help  "),
        Span::styled(
            "l",
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw(" logs  "),
        Span::styled(
            "j/k",
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw(" move  "),
        Span::styled(
            "Enter",
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw(" actions"),
    ]);

    frame.render_widget(Paragraph::new(line).alignment(Alignment::Center), area);
}

fn draw_action_menu(frame: &mut Frame, app: &App, area: Rect) {
    let popup = centered_rect(58, 56, area);
    frame.render_widget(Clear, popup);

    let items: Vec<ListItem> = ACTIONS
        .iter()
        .enumerate()
        .map(|(index, action)| {
            ListItem::new(Line::from(vec![
                Span::styled(
                    format!("{} ", index + 1),
                    Style::default()
                        .fg(Color::Cyan)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled(
                    action.label(),
                    Style::default().add_modifier(Modifier::BOLD),
                ),
                Span::raw("  "),
                Span::styled(action.description(), Style::default().fg(Color::DarkGray)),
            ]))
        })
        .collect();

    let title = app
        .selected_vm()
        .map(|vm| format!(" Actions for VM {} {} ", vm.vmid, vm.name))
        .unwrap_or_else(|| " Actions ".to_string());
    let list = List::new(items)
        .block(Block::default().title(title).borders(Borders::ALL))
        .highlight_symbol(">> ")
        .highlight_style(Style::default().bg(Color::Cyan).fg(Color::Black));

    let mut state = ListState::default().with_selected(Some(app.action_menu_index));
    frame.render_stateful_widget(list, popup, &mut state);
}

fn draw_confirm(frame: &mut Frame, message: &str, area: Rect) {
    let popup = centered_rect(56, 30, area);
    frame.render_widget(Clear, popup);

    let lines = vec![
        Line::from(message.to_string()),
        Line::from(""),
        Line::from(vec![
            Span::styled(
                "y",
                Style::default()
                    .fg(Color::Green)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw(" / "),
            Span::styled(
                "Enter",
                Style::default()
                    .fg(Color::Green)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw(" confirm    "),
            Span::styled(
                "n",
                Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
            ),
            Span::raw(" / "),
            Span::styled(
                "Esc",
                Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
            ),
            Span::raw(" cancel"),
        ]),
    ];

    frame.render_widget(
        Paragraph::new(lines)
            .block(Block::default().title(" Confirm ").borders(Borders::ALL))
            .alignment(Alignment::Center)
            .wrap(Wrap { trim: true }),
        popup,
    );
}

fn draw_logs(frame: &mut Frame, app: &App, area: Rect) {
    let popup = centered_rect(82, 76, area);
    frame.render_widget(Clear, popup);

    let lines: Vec<Line> = app
        .logs
        .iter()
        .map(|line| Line::from(line.as_str()))
        .collect();
    frame.render_widget(
        Paragraph::new(lines)
            .block(
                Block::default()
                    .title(" Logs (Esc to close) ")
                    .borders(Borders::ALL),
            )
            .wrap(Wrap { trim: false }),
        popup,
    );
}

fn draw_help(frame: &mut Frame, area: Rect) {
    let popup = centered_rect(70, 68, area);
    frame.render_widget(Clear, popup);

    let lines = vec![
        Line::from("[j/k] or arrows     Move selection"),
        Line::from("[Enter]            Open action palette"),
        Line::from("[r]                Refresh VM list"),
        Line::from("[a]                Attach automatically"),
        Line::from("[p]                Attach via SPICE"),
        Line::from("[v]                Attach via VNC (experimental)"),
        Line::from("[s]                Start selected VM"),
        Line::from("[S]                Shutdown selected VM"),
        Line::from("[f]                Force stop selected VM"),
        Line::from("[b]                Reboot selected VM"),
        Line::from("[x]                Reset selected VM"),
        Line::from("[l]                Open logs"),
        Line::from("[q]                Quit"),
    ];

    frame.render_widget(
        Paragraph::new(lines)
            .block(Block::default().title(" Help ").borders(Borders::ALL))
            .wrap(Wrap { trim: true }),
        popup,
    );
}

fn vm_detail_lines(vm: &Vm) -> Vec<Line<'static>> {
    vec![
        Line::from(Span::styled(
            format!("{} {}", vm.vmid, vm.name),
            Style::default().add_modifier(Modifier::BOLD),
        )),
        Line::from(vec![
            Span::raw("Status: "),
            Span::styled(vm.status.clone(), status_style(&vm.status)),
        ]),
        Line::from(format!("Node: {}", vm.node)),
        Line::from(format!("Memory: {}", format_memory(vm.memory_mb))),
        Line::from(format!("Bootdisk: {}", format_disk(vm.bootdisk_gb))),
        Line::from(format!(
            "PID: {}",
            vm.pid
                .map(|pid| pid.to_string())
                .unwrap_or_else(|| "-".to_string())
        )),
    ]
}

fn format_memory(value: Option<u64>) -> String {
    value
        .map(|mb| {
            if mb >= 1024 {
                format!("{:.1}G", mb as f64 / 1024.0)
            } else {
                format!("{mb}M")
            }
        })
        .unwrap_or_else(|| "-".to_string())
}

fn format_disk(value: Option<f64>) -> String {
    value
        .map(|gb| format!("{gb:.1}G"))
        .unwrap_or_else(|| "-".to_string())
}

fn status_style(status: &str) -> Style {
    match status {
        "running" => Style::default().fg(Color::Green),
        "stopped" => Style::default().fg(Color::Gray),
        "paused" | "suspended" => Style::default().fg(Color::Yellow),
        _ => Style::default().fg(Color::Red),
    }
}

fn centered_rect(percent_x: u16, percent_y: u16, area: Rect) -> Rect {
    let vertical = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage((100 - percent_y) / 2),
            Constraint::Percentage(percent_y),
            Constraint::Percentage((100 - percent_y) / 2),
        ])
        .split(area);

    Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage((100 - percent_x) / 2),
            Constraint::Percentage(percent_x),
            Constraint::Percentage((100 - percent_x) / 2),
        ])
        .split(vertical[1])[1]
}
