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

    // RAPL package power (CPU total power draw)
    let rapl_pkgs: Vec<&(SensorId, SensorReading)> = snapshot
        .iter()
        .filter(|(id, _)| {
            id.source == "cpu" && id.chip == "rapl" && id.sensor.starts_with("package")
        })
        .collect();
    let multi_pkg = rapl_pkgs.len() > 1;
    for (id, reading) in &rapl_pkgs {
        let key = format!("{}/{}/{}", id.source, id.chip, id.sensor);
        let spark = history
            .data
            .get(&key)
            .map(|buf| sparkline_str(buf, spark_width))
            .unwrap_or_default();
        let prec = format_precision(&reading.unit);
        // On multi-socket systems, include the package index to disambiguate
        let label = if multi_pkg {
            format!("Pkg {}: ", id.sensor.trim_start_matches("package-"))
        } else {
            "Power: ".into()
        };
        lines.push(Line::from(vec![
            Span::styled(label, Style::default().fg(Color::White)),
            Span::styled(
                format!("{:>6.*}{}", prec, reading.current, reading.unit),
                Style::default().fg(Color::Magenta),
            ),
            Span::raw("  "),
            Span::styled(spark, Style::default().fg(Color::DarkGray)),
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
// Memory Panel (RAPL sub-domains + HSMP DDR metrics)
// ---------------------------------------------------------------------------

/// HSMP sensor names that belong in the Memory panel rather than Platform.
const HSMP_MEMORY_SENSORS: &[&str] = &["ddr_bw_max", "ddr_bw_used", "ddr_bw_util", "mclk"];

fn is_hsmp_memory_sensor(id: &SensorId) -> bool {
    id.source == "hsmp" && HSMP_MEMORY_SENSORS.contains(&id.sensor.as_str())
}

fn build_memory_panel<'a>(snapshot: &'a [(SensorId, SensorReading)]) -> Option<Panel<'a>> {
    let mut lines: Vec<Line<'_>> = Vec::new();

    // HSMP DDR bandwidth and memory clock
    for (_, r) in snapshot.iter().filter(|(id, _)| is_hsmp_memory_sensor(id)) {
        let prec = format_precision(&r.unit);
        let unit_str = format!("{}", r.unit);
        lines.push(Line::from(vec![
            Span::styled(
                format!("{:<20} ", truncate_label(&r.label, 20)),
                Style::default().fg(Color::White),
            ),
            Span::styled(
                format!("{:>7.*}{}", prec, r.current, unit_str),
                Style::default().fg(Color::Cyan),
            ),
        ]));
    }

    // RAPL sub-domains (core, uncore, dram — package is in the CPU panel)
    for (_, r) in snapshot.iter().filter(|(id, _)| {
        id.source == "cpu" && id.chip == "rapl" && !id.sensor.starts_with("package")
    }) {
        let prec = format_precision(&r.unit);
        lines.push(Line::from(vec![
            Span::styled(
                format!("{:<20} ", truncate_label(&r.label, 20)),
                Style::default().fg(Color::White),
            ),
            Span::styled(
                format!("{:>7.*}W", prec, r.current),
                Style::default().fg(Color::Magenta),
            ),
        ]));
    }

    if lines.is_empty() {
        return None;
    }

    Some(Panel {
        title: "Memory",
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
        if id.sensor == "read_mbps" {
            entry.0 = Some(r.current);
        } else if id.sensor == "write_mbps" {
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

struct NetIfaceData<'a> {
    name: &'a str,
    rx: f64,
    tx: f64,
    link_speed_mb: Option<f64>,
}

fn build_network_panel<'a>(snapshot: &'a [(SensorId, SensorReading)]) -> Option<Panel<'a>> {
    let net_sensors: Vec<&(SensorId, SensorReading)> = snapshot
        .iter()
        .filter(|(id, _)| id.source == "net")
        .collect();

    if net_sensors.is_empty() {
        return None;
    }

    // Group by chip (interface name)
    let mut ifaces: HashMap<&str, NetIfaceData<'_>> = HashMap::new();
    for (id, r) in &net_sensors {
        let entry = ifaces.entry(id.chip.as_str()).or_insert(NetIfaceData {
            name: id.chip.as_str(),
            rx: 0.0,
            tx: 0.0,
            link_speed_mb: None,
        });
        match id.sensor.as_str() {
            "rx_mbps" => entry.rx = r.current,
            "tx_mbps" => entry.tx = r.current,
            "link_speed" => entry.link_speed_mb = Some(r.current),
            _ => {}
        }
    }

    let mut iface_list: Vec<NetIfaceData<'_>> = ifaces.into_values().collect();
    iface_list.sort_by(|a, b| a.name.cmp(b.name));
    iface_list.truncate(MAX_PANEL_ENTRIES);

    const BAR_WIDTH: usize = 6;
    let lines: Vec<Line<'_>> = iface_list
        .iter()
        .map(|d| {
            let iface = truncate_label(d.name, 10);
            let rx_bar = net_bar(d.rx, d.link_speed_mb, BAR_WIDTH);
            let tx_bar = net_bar(d.tx, d.link_speed_mb, BAR_WIDTH);
            Line::from(vec![
                Span::styled(format!("{iface:<10}"), Style::default().fg(Color::White)),
                Span::styled(" \u{2193}", Style::default().fg(Color::Green)),
                Span::styled(format!("{:>7.1}", d.rx), Style::default().fg(Color::Green)),
                Span::raw(" "),
                Span::styled(rx_bar, Style::default().fg(Color::Green)),
                Span::styled(" \u{2191}", Style::default().fg(Color::Cyan)),
                Span::styled(format!("{:>7.1}", d.tx), Style::default().fg(Color::Cyan)),
                Span::raw(" "),
                Span::styled(tx_bar, Style::default().fg(Color::Cyan)),
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
    // DDR bandwidth and memory clock are shown in the Memory panel.
    let hsmp: Vec<&(SensorId, SensorReading)> = snapshot
        .iter()
        .filter(|(id, _)| id.source == "hsmp" && !is_hsmp_memory_sensor(id))
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

/// Network activity bar. Uses link-speed utilization when available,
/// falls back to log-scale (0.01–1000+ MiB/s) otherwise.
/// Both `mibs` and `link_speed_mibs` are in MiB/s (binary megabytes/sec).
fn net_bar(mibs: f64, link_speed_mibs: Option<f64>, width: usize) -> String {
    let frac = if let Some(speed) = link_speed_mibs {
        if speed > 0.0 {
            (mibs / speed).clamp(0.0, 1.0)
        } else {
            0.0
        }
    } else if mibs <= 0.001 {
        0.0
    } else {
        // Log scale: 0.01 MiB/s → 0.0, 1000 MiB/s → 1.0
        ((mibs.log10() + 2.0) / 5.0).clamp(0.0, 1.0)
    };
    let filled = (frac * width as f64).ceil() as usize;
    (0..width)
        .map(|i| if i < filled { '\u{2588}' } else { '\u{2591}' })
        .collect()
}

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_net_bar_zero_traffic() {
        let bar = net_bar(0.0, None, 6);
        assert_eq!(bar, "\u{2591}\u{2591}\u{2591}\u{2591}\u{2591}\u{2591}");
    }

    #[test]
    fn test_net_bar_full_link_speed() {
        let bar = net_bar(125.0, Some(125.0), 6);
        assert_eq!(bar, "\u{2588}\u{2588}\u{2588}\u{2588}\u{2588}\u{2588}");
    }

    #[test]
    fn test_net_bar_half_link_speed() {
        let bar = net_bar(62.5, Some(125.0), 6);
        // 50% → ceil(3.0) = 3 filled
        assert_eq!(bar, "\u{2588}\u{2588}\u{2588}\u{2591}\u{2591}\u{2591}");
    }

    #[test]
    fn test_net_bar_log_scale_high() {
        // 1000 MB/s → log10(1000)+2 / 5 = 5/5 = 1.0 → all filled
        let bar = net_bar(1000.0, None, 6);
        assert_eq!(bar, "\u{2588}\u{2588}\u{2588}\u{2588}\u{2588}\u{2588}");
    }

    #[test]
    fn test_net_bar_log_scale_low() {
        // 0.01 MB/s → log10(0.01)+2 / 5 = 0/5 = 0.0 → none filled
        let bar = net_bar(0.01, None, 6);
        assert_eq!(bar, "\u{2591}\u{2591}\u{2591}\u{2591}\u{2591}\u{2591}");
    }

    #[test]
    fn test_net_bar_exceeds_link_speed() {
        // Clamped to 1.0
        let bar = net_bar(200.0, Some(125.0), 6);
        assert_eq!(bar, "\u{2588}\u{2588}\u{2588}\u{2588}\u{2588}\u{2588}");
    }
}
