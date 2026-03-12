use std::collections::HashMap;
use std::io::{self, Stdout};

use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph};

use crate::model::sensor::{SensorCategory, SensorId, SensorReading};

use super::{SensorHistory, format_precision, sparkline_str, value_style};

/// Maximum sensors shown per panel.
const MAX_PANEL_ENTRIES: usize = 6;

pub fn render(
    terminal: &mut Terminal<CrosstermBackend<Stdout>>,
    snapshot: &[(SensorId, SensorReading)],
    history: &SensorHistory,
    elapsed_str: &str,
    sensor_count: usize,
) -> io::Result<()> {
    terminal.draw(|frame| {
        let size = frame.area();
        let wide = size.width >= 120;

        // Outer layout: header + main + status
        let outer = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(3),
                Constraint::Min(1),
                Constraint::Length(1),
            ])
            .split(size);

        // Header
        let header = Paragraph::new(format!(
            " sio dashboard | {sensor_count} sensors | {elapsed_str}"
        ))
        .style(
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        )
        .block(
            Block::default()
                .borders(Borders::BOTTOM)
                .border_style(Style::default().fg(Color::DarkGray)),
        );
        frame.render_widget(header, outer[0]);

        // Status bar
        let status = Paragraph::new(format!(
            " d: tree view | /: search | {sensor_count} sensors | {elapsed_str}"
        ))
        .style(Style::default().fg(Color::DarkGray).bg(Color::Black));
        frame.render_widget(status, outer[2]);

        // Build panel data
        let panels = build_panels(snapshot, history, size.width);

        if panels.is_empty() {
            return;
        }

        // Separate errors panel (full-width) from normal panels
        let (normal, errors): (Vec<_>, Vec<_>) =
            panels.into_iter().partition(|p| p.title != "Errors");

        if wide {
            render_wide(frame, outer[1], &normal, &errors);
        } else {
            render_narrow(frame, outer[1], &normal, &errors);
        }
    })?;
    Ok(())
}

struct Panel<'a> {
    title: &'a str,
    lines: Vec<Line<'a>>,
    column: Column,
}

#[derive(Clone, Copy)]
enum Column {
    Left,
    Right,
}

fn render_wide(frame: &mut ratatui::Frame, area: Rect, normal: &[Panel<'_>], errors: &[Panel<'_>]) {
    let errors_height = if errors.is_empty() {
        0
    } else {
        errors.iter().map(|p| p.lines.len() as u16 + 2).sum::<u16>()
    };

    let main_split = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(1), Constraint::Length(errors_height)])
        .split(area);

    // Two columns
    let cols = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
        .split(main_split[0]);

    let left: Vec<&Panel<'_>> = normal
        .iter()
        .filter(|p| matches!(p.column, Column::Left))
        .collect();
    let right: Vec<&Panel<'_>> = normal
        .iter()
        .filter(|p| matches!(p.column, Column::Right))
        .collect();

    render_column(frame, cols[0], &left);
    render_column(frame, cols[1], &right);

    // Errors full width
    if !errors.is_empty() {
        render_column(frame, main_split[1], &errors.iter().collect::<Vec<_>>());
    }
}

fn render_narrow(
    frame: &mut ratatui::Frame,
    area: Rect,
    normal: &[Panel<'_>],
    errors: &[Panel<'_>],
) {
    let all: Vec<&Panel<'_>> = normal.iter().chain(errors.iter()).collect();
    render_column(frame, area, &all);
}

fn render_column(frame: &mut ratatui::Frame, area: Rect, panels: &[&Panel<'_>]) {
    if panels.is_empty() {
        return;
    }

    let constraints: Vec<Constraint> = panels
        .iter()
        .map(|p| Constraint::Length(p.lines.len() as u16 + 2)) // +2 for block borders
        .chain(std::iter::once(Constraint::Min(0))) // absorb remaining space
        .collect();

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints(constraints)
        .split(area);

    for (i, panel) in panels.iter().enumerate() {
        let block = Block::default()
            .title(format!(" {} ", panel.title))
            .title_style(
                Style::default()
                    .fg(Color::White)
                    .add_modifier(Modifier::BOLD),
            )
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::DarkGray));
        let paragraph = Paragraph::new(panel.lines.clone()).block(block);
        frame.render_widget(paragraph, chunks[i]);
    }
}

fn build_panels<'a>(
    snapshot: &'a [(SensorId, SensorReading)],
    history: &'a SensorHistory,
    term_width: u16,
) -> Vec<Panel<'a>> {
    let spark_width = if term_width >= 120 { 15 } else { 10 };
    let mut panels = Vec::new();

    if let Some(p) = build_cpu_panel(snapshot, history, spark_width) {
        panels.push(p);
    }
    if let Some(p) = build_thermal_panel(snapshot, history, spark_width) {
        panels.push(p);
    }
    if let Some(p) = build_memory_panel(snapshot) {
        panels.push(p);
    }
    if let Some(p) = build_power_panel(snapshot, history, spark_width) {
        panels.push(p);
    }
    if let Some(p) = build_storage_panel(snapshot) {
        panels.push(p);
    }
    if let Some(p) = build_network_panel(snapshot) {
        panels.push(p);
    }
    if let Some(p) = build_fans_panel(snapshot) {
        panels.push(p);
    }
    if let Some(p) = build_platform_panel(snapshot) {
        panels.push(p);
    }
    if let Some(p) = build_errors_panel(snapshot) {
        panels.push(p);
    }

    panels
}

// ---------------------------------------------------------------------------
// CPU Panel
// ---------------------------------------------------------------------------

fn build_cpu_panel<'a>(
    snapshot: &'a [(SensorId, SensorReading)],
    history: &'a SensorHistory,
    spark_width: usize,
) -> Option<Panel<'a>> {
    let util_sensors: Vec<&(SensorId, SensorReading)> = snapshot
        .iter()
        .filter(|(id, _)| id.source == "cpu" && id.chip == "utilization")
        .collect();

    if util_sensors.is_empty() {
        return None;
    }

    let mut lines: Vec<Line<'_>> = Vec::new();

    // Total CPU line
    if let Some((id, reading)) = util_sensors.iter().find(|(id, _)| id.sensor == "total") {
        let key = format!("{}/{}/{}", id.source, id.chip, id.sensor);
        let spark = history
            .data
            .get(&key)
            .map(|buf| sparkline_str(buf, spark_width))
            .unwrap_or_default();
        lines.push(Line::from(vec![
            Span::styled("Total: ", Style::default().fg(Color::White)),
            Span::styled(format!("{:5.1}%", reading.current), value_style(reading)),
            Span::raw("  "),
            Span::styled(spark, Style::default().fg(Color::DarkGray)),
        ]));
    }

    // Per-core dense bar
    let mut cores: Vec<(&SensorId, &SensorReading)> = util_sensors
        .iter()
        .filter(|(id, _)| id.sensor.starts_with("cpu") && id.sensor != "total")
        .map(|(id, r)| (id, r))
        .collect();
    cores.sort_by(|(a, _), (b, _)| a.natural_cmp(b));

    if !cores.is_empty() {
        let bar: String = cores
            .iter()
            .map(|(_, r)| core_block_char(r.current))
            .collect();
        // Color the bar by overall utilization
        let avg_util: f64 = cores.iter().map(|(_, r)| r.current).sum::<f64>() / cores.len() as f64;
        let bar_color = if avg_util > 80.0 {
            Color::Red
        } else if avg_util > 50.0 {
            Color::Yellow
        } else {
            Color::Green
        };
        lines.push(Line::from(vec![
            Span::styled("Cores: ", Style::default().fg(Color::White)),
            Span::styled(bar, Style::default().fg(bar_color)),
        ]));
    }

    Some(Panel {
        title: "CPU",
        lines,
        column: Column::Left,
    })
}

fn core_block_char(pct: f64) -> char {
    if pct >= 87.5 {
        '\u{2588}' // █
    } else if pct >= 62.5 {
        '\u{2593}' // ▓
    } else if pct >= 37.5 {
        '\u{2592}' // ▒
    } else if pct >= 12.5 {
        '\u{2591}' // ░
    } else {
        '\u{00b7}' // ·
    }
}

// ---------------------------------------------------------------------------
// Thermal Panel
// ---------------------------------------------------------------------------

fn build_thermal_panel<'a>(
    snapshot: &'a [(SensorId, SensorReading)],
    history: &'a SensorHistory,
    spark_width: usize,
) -> Option<Panel<'a>> {
    let mut temps: Vec<&(SensorId, SensorReading)> = snapshot
        .iter()
        .filter(|(_, r)| r.category == SensorCategory::Temperature)
        .collect();

    if temps.is_empty() {
        return None;
    }

    temps.sort_by(|(_, a), (_, b)| {
        b.current
            .partial_cmp(&a.current)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    temps.truncate(MAX_PANEL_ENTRIES);

    let lines: Vec<Line<'_>> = temps
        .iter()
        .map(|(id, r)| {
            let label = truncate_label(&r.label, 20);
            let key = format!("{}/{}/{}", id.source, id.chip, id.sensor);
            let spark = history
                .data
                .get(&key)
                .map(|buf| sparkline_str(buf, spark_width))
                .unwrap_or_default();
            let prec = format_precision(&r.unit);
            Line::from(vec![
                Span::styled(format!("{label:<20} "), Style::default().fg(Color::White)),
                Span::styled(
                    format!("{:>6.*}{}", prec, r.current, r.unit),
                    value_style(r),
                ),
                Span::raw(" "),
                Span::styled(spark, Style::default().fg(Color::DarkGray)),
            ])
        })
        .collect();

    Some(Panel {
        title: "Thermal",
        lines,
        column: Column::Right,
    })
}

// ---------------------------------------------------------------------------
// Memory / RAPL Panel
// ---------------------------------------------------------------------------

fn build_memory_panel<'a>(snapshot: &'a [(SensorId, SensorReading)]) -> Option<Panel<'a>> {
    let rapl: Vec<&(SensorId, SensorReading)> = snapshot
        .iter()
        .filter(|(id, _)| id.source == "cpu" && id.chip == "rapl")
        .collect();

    if rapl.is_empty() {
        return None;
    }

    let lines: Vec<Line<'_>> = rapl
        .iter()
        .map(|(_, r)| {
            let prec = format_precision(&r.unit);
            Line::from(vec![
                Span::styled("RAPL: ", Style::default().fg(Color::White)),
                Span::styled(
                    format!("{:<14}", truncate_label(&r.label, 14)),
                    Style::default().fg(Color::White),
                ),
                Span::styled(
                    format!("{:>7.*}W", prec, r.current),
                    Style::default().fg(Color::Magenta),
                ),
            ])
        })
        .collect();

    Some(Panel {
        title: "Memory / RAPL",
        lines,
        column: Column::Left,
    })
}

// ---------------------------------------------------------------------------
// Power Panel
// ---------------------------------------------------------------------------

fn build_power_panel<'a>(
    snapshot: &'a [(SensorId, SensorReading)],
    history: &'a SensorHistory,
    spark_width: usize,
) -> Option<Panel<'a>> {
    let mut power: Vec<&(SensorId, SensorReading)> = snapshot
        .iter()
        .filter(|(id, r)| {
            r.category == SensorCategory::Power && !(id.source == "cpu" && id.chip == "rapl")
        })
        .collect();

    if power.is_empty() {
        return None;
    }

    power.sort_by(|(_, a), (_, b)| {
        b.current
            .partial_cmp(&a.current)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    power.truncate(MAX_PANEL_ENTRIES);

    let lines: Vec<Line<'_>> = power
        .iter()
        .map(|(id, r)| {
            let label = truncate_label(&r.label, 20);
            let key = format!("{}/{}/{}", id.source, id.chip, id.sensor);
            let spark = history
                .data
                .get(&key)
                .map(|buf| sparkline_str(buf, spark_width))
                .unwrap_or_default();
            let prec = format_precision(&r.unit);
            Line::from(vec![
                Span::styled(format!("{label:<20} "), Style::default().fg(Color::White)),
                Span::styled(
                    format!("{:>7.*}{}", prec, r.current, r.unit),
                    Style::default().fg(Color::Magenta),
                ),
                Span::raw(" "),
                Span::styled(spark, Style::default().fg(Color::DarkGray)),
            ])
        })
        .collect();

    Some(Panel {
        title: "Power",
        lines,
        column: Column::Right,
    })
}

// ---------------------------------------------------------------------------
// Storage Panel
// ---------------------------------------------------------------------------

fn build_storage_panel<'a>(snapshot: &'a [(SensorId, SensorReading)]) -> Option<Panel<'a>> {
    let disk_sensors: Vec<&(SensorId, SensorReading)> = snapshot
        .iter()
        .filter(|(id, _)| id.source == "disk")
        .collect();

    if disk_sensors.is_empty() {
        return None;
    }

    // Group by chip (device name), find read/write per device
    let mut devices: HashMap<&str, (Option<f64>, Option<f64>)> = HashMap::new();
    for (id, r) in &disk_sensors {
        let entry = devices.entry(id.chip.as_str()).or_insert((None, None));
        let sensor_lc = id.sensor.to_ascii_lowercase();
        if sensor_lc.contains("read") {
            entry.0 = Some(r.current);
        } else if sensor_lc.contains("write") {
            entry.1 = Some(r.current);
        }
    }

    let mut dev_list: Vec<(&str, f64, f64)> = devices
        .into_iter()
        .map(|(name, (r, w))| (name, r.unwrap_or(0.0), w.unwrap_or(0.0)))
        .collect();
    dev_list.sort_by(|a, b| a.0.cmp(b.0));
    dev_list.truncate(MAX_PANEL_ENTRIES);

    let lines: Vec<Line<'_>> = dev_list
        .into_iter()
        .map(|(name, read, write)| {
            let dev = truncate_label(name, 10);
            Line::from(vec![
                Span::styled(format!("{dev:<10}"), Style::default().fg(Color::White)),
                Span::styled(" R ", Style::default().fg(Color::Green)),
                Span::styled(format!("{read:>8.1}"), Style::default().fg(Color::Green)),
                Span::styled("  W ", Style::default().fg(Color::Red)),
                Span::styled(format!("{write:>8.1}"), Style::default().fg(Color::Red)),
                Span::styled(" MB/s", Style::default().fg(Color::DarkGray)),
            ])
        })
        .collect();

    Some(Panel {
        title: "Storage",
        lines,
        column: Column::Left,
    })
}

// ---------------------------------------------------------------------------
// Network Panel
// ---------------------------------------------------------------------------

fn build_network_panel<'a>(snapshot: &'a [(SensorId, SensorReading)]) -> Option<Panel<'a>> {
    let net_sensors: Vec<&(SensorId, SensorReading)> = snapshot
        .iter()
        .filter(|(id, _)| id.source == "net")
        .collect();

    if net_sensors.is_empty() {
        return None;
    }

    // Group by chip (interface name), find rx/tx per interface
    let mut ifaces: HashMap<&str, (Option<f64>, Option<f64>)> = HashMap::new();
    for (id, r) in &net_sensors {
        let entry = ifaces.entry(id.chip.as_str()).or_insert((None, None));
        let sensor_lc = id.sensor.to_ascii_lowercase();
        if sensor_lc.contains("rx") || sensor_lc.contains("read") || sensor_lc.contains("down") {
            entry.0 = Some(r.current);
        } else if sensor_lc.contains("tx")
            || sensor_lc.contains("write")
            || sensor_lc.contains("up")
        {
            entry.1 = Some(r.current);
        }
    }

    let mut iface_list: Vec<(&str, f64, f64)> = ifaces
        .into_iter()
        .map(|(name, (rx, tx))| (name, rx.unwrap_or(0.0), tx.unwrap_or(0.0)))
        .collect();
    iface_list.sort_by(|a, b| a.0.cmp(b.0));
    iface_list.truncate(MAX_PANEL_ENTRIES);

    let lines: Vec<Line<'_>> = iface_list
        .into_iter()
        .map(|(name, rx, tx)| {
            let iface = truncate_label(name, 10);
            Line::from(vec![
                Span::styled(format!("{iface:<10}"), Style::default().fg(Color::White)),
                Span::styled(" \u{2193} ", Style::default().fg(Color::Green)),
                Span::styled(format!("{rx:>8.1}"), Style::default().fg(Color::Green)),
                Span::styled("  \u{2191} ", Style::default().fg(Color::Cyan)),
                Span::styled(format!("{tx:>8.1}"), Style::default().fg(Color::Cyan)),
                Span::styled(" MB/s", Style::default().fg(Color::DarkGray)),
            ])
        })
        .collect();

    Some(Panel {
        title: "Network",
        lines,
        column: Column::Right,
    })
}

// ---------------------------------------------------------------------------
// Fans Panel
// ---------------------------------------------------------------------------

fn build_fans_panel<'a>(snapshot: &'a [(SensorId, SensorReading)]) -> Option<Panel<'a>> {
    let mut fans: Vec<&(SensorId, SensorReading)> = snapshot
        .iter()
        .filter(|(_, r)| r.category == SensorCategory::Fan)
        .collect();

    if fans.is_empty() {
        return None;
    }

    fans.sort_by(|(a, _), (b, _)| a.natural_cmp(b));
    fans.truncate(MAX_PANEL_ENTRIES);

    let lines: Vec<Line<'_>> = fans
        .iter()
        .map(|(_, r)| {
            let label = truncate_label(&r.label, 20);
            Line::from(vec![
                Span::styled(format!("{label:<20} "), Style::default().fg(Color::White)),
                Span::styled(format!("{:>5.0} RPM", r.current), value_style(r)),
            ])
        })
        .collect();

    Some(Panel {
        title: "Fans",
        lines,
        column: Column::Left,
    })
}

// ---------------------------------------------------------------------------
// Platform (HSMP) Panel
// ---------------------------------------------------------------------------

fn build_platform_panel<'a>(snapshot: &'a [(SensorId, SensorReading)]) -> Option<Panel<'a>> {
    let hsmp: Vec<&(SensorId, SensorReading)> = snapshot
        .iter()
        .filter(|(id, _)| id.source == "hsmp")
        .collect();

    if hsmp.is_empty() {
        return None;
    }

    let lines: Vec<Line<'_>> = hsmp
        .iter()
        .take(MAX_PANEL_ENTRIES)
        .map(|(_, r)| {
            let prec = format_precision(&r.unit);
            let unit_str = format!("{}", r.unit);
            Line::from(vec![
                Span::styled(
                    format!("{:<20} ", truncate_label(&r.label, 20)),
                    Style::default().fg(Color::White),
                ),
                Span::styled(
                    format!("{:>7.*}{}", prec, r.current, unit_str),
                    Style::default().fg(Color::Cyan),
                ),
            ])
        })
        .collect();

    Some(Panel {
        title: "Platform",
        lines,
        column: Column::Right,
    })
}

// ---------------------------------------------------------------------------
// Errors Panel (EDAC / AER / MCE)
// ---------------------------------------------------------------------------

fn build_errors_panel<'a>(snapshot: &'a [(SensorId, SensorReading)]) -> Option<Panel<'a>> {
    let errors: Vec<&(SensorId, SensorReading)> = snapshot
        .iter()
        .filter(|(id, r)| {
            (id.source == "edac" || id.source == "aer" || id.source == "mce") && r.current > 0.0
        })
        .collect();

    if errors.is_empty() {
        return None;
    }

    let total: f64 = errors.iter().map(|(_, r)| r.current).sum();
    let sources: Vec<String> = errors
        .iter()
        .map(|(id, r)| format!("{}/{}: {:.0}", id.source, id.sensor, r.current))
        .collect();
    let detail = if sources.len() <= 3 {
        sources.join(", ")
    } else {
        format!("{} counters active", sources.len())
    };

    let lines = vec![Line::from(vec![
        Span::styled(
            format!("\u{26a0} {total:.0} total errors"),
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(format!("  ({detail})"), Style::default().fg(Color::Yellow)),
    ])];

    Some(Panel {
        title: "Errors",
        lines,
        column: Column::Left, // doesn't matter, errors span full width
    })
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn truncate_label(label: &str, max: usize) -> String {
    if label.chars().count() <= max {
        label.to_string()
    } else {
        let end = label
            .char_indices()
            .nth(max.saturating_sub(1))
            .map_or(label.len(), |(i, _)| i);
        format!("{}\u{2026}", &label[..end])
    }
}
