use std::collections::HashMap;
use std::io::{self, Stdout};

use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, LineGauge, Paragraph};

use crate::model::sensor::{SensorCategory, SensorId, SensorReading};

use super::{SensorHistory, SystemSummary, format_precision, sparkline_spans, theme::TuiTheme};

struct LayoutParams {
    num_columns: u8,
    max_entries: usize,
    spark_width: usize,
    available_rows: u16,
}

/// Generous max entries based on available rows. Panels that don't need this
/// many will show all their data and become fixed-size (`truncated: false`).
/// Panels that get truncated will expand into remaining space via `Fill(1)`.
fn max_entries_for_column(available_rows: u16) -> usize {
    (available_rows.saturating_sub(2) as usize).clamp(2, 100)
}

fn compute_layout(width: u16, height: u16, panel_count: usize) -> LayoutParams {
    let num_columns: u8 = if width >= 200 {
        3
    } else if width >= 120 {
        2
    } else {
        1
    };

    let spark_width = if width < 80 {
        0
    } else if width < 120 {
        10
    } else if width < 200 {
        15
    } else {
        20
    };

    let available_rows = height.saturating_sub(4); // header(3) + status(1)

    let panels_per_col = panel_count.max(1).div_ceil(num_columns as usize) as u16;
    let rows_per_panel = available_rows / panels_per_col;

    let max_entries = (rows_per_panel.saturating_sub(2) as usize).clamp(2, 50);

    LayoutParams {
        num_columns,
        max_entries,
        spark_width,
        available_rows,
    }
}

fn panel_priority(title: &str) -> u8 {
    match title {
        "Errors" => 0,
        "Platform" => 1,
        "Memory" => 2,
        "Voltage" => 3,
        "Fans" => 4,
        "CPU Freq" => 4,
        "Power" => 5,
        "Storage" => 6,
        "Network" => 6,
        "GPU" => 7,
        "Thermal" => 8,
        "CPU" => 9,
        _ => 5,
    }
}

#[allow(clippy::too_many_arguments)]
pub fn render(
    terminal: &mut Terminal<CrosstermBackend<Stdout>>,
    snapshot: &[(SensorId, SensorReading)],
    history: &SensorHistory,
    elapsed_str: &str,
    sensor_count: usize,
    theme: &TuiTheme,
    dashboard_config: &crate::config::DashboardConfig,
    sys: &SystemSummary,
) -> io::Result<()> {
    terminal.draw(|frame| {
        let size = frame.area();
        let estimated_panels = if dashboard_config.panels.is_empty() {
            12
        } else {
            dashboard_config.panels.len()
        };
        let layout = compute_layout(size.width, size.height, estimated_panels);

        // Outer layout: header + main + status
        let outer = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(3),
                Constraint::Min(1),
                Constraint::Length(1),
            ])
            .split(size);

        // Header with system summary
        let header_text = if sys.cpu_model.is_empty() {
            format!(
                " {} | {} | {sensor_count} sensors | {elapsed_str}",
                sys.hostname, sys.kernel,
            )
        } else {
            format!(
                " {} | {} | {} | {sensor_count} sensors | {elapsed_str}",
                sys.hostname, sys.cpu_model, sys.kernel,
            )
        };
        let header = Paragraph::new(header_text)
            .style(theme.accent_style())
            .block(
                Block::default()
                    .borders(Borders::BOTTOM)
                    .border_style(theme.border_style()),
            );
        frame.render_widget(header, outer[0]);

        // Status bar
        let status = Paragraph::new(format!(
            " q: quit | d: tree view | t: theme | /: search | {sensor_count} sensors | {elapsed_str}"
        ))
        .style(theme.status_style());
        frame.render_widget(status, outer[2]);

        // Build panel data
        let panels = if dashboard_config.panels.is_empty() {
            build_panels(snapshot, history, &layout, theme)
        } else {
            build_custom_panels(snapshot, history, &dashboard_config.panels, &layout, theme)
        };

        if panels.is_empty() {
            return;
        }

        // Separate errors panel (full-width) from normal panels
        let (mut normal, errors): (Vec<_>, Vec<_>) =
            panels.into_iter().partition(|p| p.title != "Errors");

        // Drop lowest-priority panels if space is too tight
        if !normal.is_empty() {
            let num_cols = layout.num_columns as u16;
            loop {
                let panels_per_col = ((normal.len() as f32) / (num_cols as f32)).ceil() as u16;
                if panels_per_col == 0
                    || layout.available_rows / panels_per_col >= 4
                    || normal.len() <= 1
                {
                    break;
                }
                // Remove the panel with the lowest priority value (least important)
                if let Some(idx) = normal
                    .iter()
                    .enumerate()
                    .min_by_key(|(_, p)| panel_priority(&p.title))
                    .map(|(i, _)| i)
                {
                    normal.remove(idx);
                }
            }
        }

        match layout.num_columns {
            3 => render_three_col(frame, outer[1], &normal, &errors, theme),
            2 => render_wide(frame, outer[1], &normal, &errors, theme),
            _ => render_narrow(frame, outer[1], &normal, &errors, theme),
        }
    })?;
    Ok(())
}

struct Panel<'a> {
    title: String,
    /// Optional headline value shown after the title (e.g., "54.0°C").
    headline: Option<String>,
    content: PanelContent<'a>,
    column: Column,
    /// True if the panel had more data than it could show (was truncated).
    /// Truncated panels expand to fill remaining space; others get tight sizing.
    truncated: bool,
}

enum PanelContent<'a> {
    /// Standard text lines (current behavior for most panels).
    Lines(Vec<Line<'a>>),
    /// Mixed content: text lines interleaved with gauge widgets.
    Mixed(Vec<PanelRow<'a>>),
    /// Multi-column layout: rows distributed across N columns.
    MultiCol {
        rows: Vec<PanelRow<'a>>,
        columns: u8,
    },
}

enum PanelRow<'a> {
    Text(Line<'a>),
    Gauge {
        label: String,
        label_style: Style,
        ratio: f64,
        filled_style: Style,
        unfilled_style: Style,
    },
}

impl<'a> PanelContent<'a> {
    fn height(&self) -> u16 {
        match self {
            PanelContent::Lines(lines) => lines.len() as u16,
            PanelContent::Mixed(rows) => rows.len() as u16,
            PanelContent::MultiCol { rows, columns } => {
                let cols = (*columns).max(1) as usize;
                rows.len().div_ceil(cols) as u16
            }
        }
    }

    #[cfg(test)]
    fn lines(&self) -> &[Line<'a>] {
        match self {
            PanelContent::Lines(lines) => lines,
            PanelContent::Mixed(_) | PanelContent::MultiCol { .. } => &[],
        }
    }
}

#[derive(Clone, Copy, PartialEq)]
enum Column {
    Left,
    Center,
    Right,
}

fn render_wide(
    frame: &mut ratatui::Frame,
    area: Rect,
    normal: &[Panel<'_>],
    errors: &[Panel<'_>],
    theme: &TuiTheme,
) {
    let errors_height = if errors.is_empty() {
        0
    } else {
        errors.iter().map(|p| p.content.height() + 2).sum::<u16>()
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
        .filter(|p| matches!(p.column, Column::Left | Column::Center))
        .collect();
    let right: Vec<&Panel<'_>> = normal
        .iter()
        .filter(|p| matches!(p.column, Column::Right))
        .collect();

    render_column(frame, cols[0], &left, theme);
    render_column(frame, cols[1], &right, theme);

    // Errors full width
    if !errors.is_empty() {
        render_column(
            frame,
            main_split[1],
            &errors.iter().collect::<Vec<_>>(),
            theme,
        );
    }
}

fn render_three_col(
    frame: &mut ratatui::Frame,
    area: Rect,
    normal: &[Panel<'_>],
    errors: &[Panel<'_>],
    theme: &TuiTheme,
) {
    let errors_height = if errors.is_empty() {
        0
    } else {
        errors.iter().map(|p| p.content.height() + 2).sum::<u16>()
    };

    let main_split = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(1), Constraint::Length(errors_height)])
        .split(area);

    // Three columns: 33% / 34% / 33%
    let cols = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage(33),
            Constraint::Percentage(34),
            Constraint::Percentage(33),
        ])
        .split(main_split[0]);

    let left: Vec<&Panel<'_>> = normal.iter().filter(|p| p.column == Column::Left).collect();
    let center: Vec<&Panel<'_>> = normal
        .iter()
        .filter(|p| p.column == Column::Center)
        .collect();
    let right: Vec<&Panel<'_>> = normal
        .iter()
        .filter(|p| p.column == Column::Right)
        .collect();

    render_column(frame, cols[0], &left, theme);
    render_column(frame, cols[1], &center, theme);
    render_column(frame, cols[2], &right, theme);

    // Errors full width
    if !errors.is_empty() {
        render_column(
            frame,
            main_split[1],
            &errors.iter().collect::<Vec<_>>(),
            theme,
        );
    }
}

fn render_narrow(
    frame: &mut ratatui::Frame,
    area: Rect,
    normal: &[Panel<'_>],
    errors: &[Panel<'_>],
    theme: &TuiTheme,
) {
    let all: Vec<&Panel<'_>> = normal.iter().chain(errors.iter()).collect();
    render_column(frame, area, &all, theme);
}

fn render_column(frame: &mut ratatui::Frame, area: Rect, panels: &[&Panel<'_>], theme: &TuiTheme) {
    if panels.is_empty() {
        return;
    }

    // Truncated panels (have more data to show) expand to fill remaining space.
    // Non-truncated panels get tight sizing. This ensures panels with more
    // data (e.g., many thermal sensors) grow into space freed by smaller panels.
    let constraints: Vec<Constraint> = panels
        .iter()
        .map(|p| {
            if p.truncated {
                Constraint::Fill(1)
            } else {
                Constraint::Length(p.content.height() + 2)
            }
        })
        .collect();

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints(constraints)
        .split(area);

    for (i, panel) in panels.iter().enumerate() {
        let accent = theme.panel_accent(&panel.title);
        let title_text = match &panel.headline {
            Some(h) => format!(" {} {} ", panel.title, h),
            None => format!(" {} ", panel.title),
        };
        let block = Block::default()
            .title(title_text)
            .title_style(Style::default().fg(accent).add_modifier(Modifier::BOLD))
            .borders(Borders::ALL)
            .border_style(Style::default().fg(accent));
        match &panel.content {
            PanelContent::Lines(lines) => {
                let paragraph = Paragraph::new(lines.clone()).block(block);
                frame.render_widget(paragraph, chunks[i]);
            }
            PanelContent::Mixed(rows) => {
                let inner = block.inner(chunks[i]);
                frame.render_widget(block, chunks[i]);
                render_rows(frame, inner, rows);
            }
            PanelContent::MultiCol { rows, columns } => {
                let inner = block.inner(chunks[i]);
                frame.render_widget(block, chunks[i]);
                let ncols = (*columns).max(1) as usize;
                let rows_per_col = rows.len().div_ceil(ncols);
                let col_constraints: Vec<Constraint> = (0..ncols)
                    .map(|_| Constraint::Ratio(1, ncols as u32))
                    .collect();
                let col_areas = Layout::default()
                    .direction(Direction::Horizontal)
                    .constraints(col_constraints)
                    .split(inner);
                for (c, col_area) in col_areas.iter().enumerate() {
                    let start = c * rows_per_col;
                    let end = rows.len().min(start + rows_per_col);
                    if start < end {
                        render_rows(frame, *col_area, &rows[start..end]);
                    }
                }
            }
        }
    }
}

fn render_rows(frame: &mut ratatui::Frame, area: Rect, rows: &[PanelRow<'_>]) {
    let row_constraints: Vec<Constraint> = rows.iter().map(|_| Constraint::Length(1)).collect();
    let row_areas = Layout::default()
        .direction(Direction::Vertical)
        .constraints(row_constraints)
        .split(area);
    for (j, row) in rows.iter().enumerate() {
        if j >= row_areas.len() {
            break;
        }
        match row {
            PanelRow::Text(line) => {
                let p = Paragraph::new(line.clone());
                frame.render_widget(p, row_areas[j]);
            }
            PanelRow::Gauge {
                label,
                label_style,
                ratio,
                filled_style,
                unfilled_style,
            } => {
                let safe_ratio = if ratio.is_finite() {
                    ratio.clamp(0.0, 1.0)
                } else {
                    0.0
                };
                let gauge = LineGauge::default()
                    .ratio(safe_ratio)
                    .label(label.as_str())
                    .style(*label_style)
                    .filled_style(*filled_style)
                    .unfilled_style(*unfilled_style);
                frame.render_widget(gauge, row_areas[j]);
            }
        }
    }
}

fn build_panels<'a>(
    snapshot: &'a [(SensorId, SensorReading)],
    history: &'a SensorHistory,
    layout: &LayoutParams,
    theme: &TuiTheme,
) -> Vec<Panel<'a>> {
    let spark_width = layout.spark_width;
    let three_col = layout.num_columns >= 3;

    // Generous limit — panels that show all their data become fixed-size,
    // truncated panels expand via Fill(1). Paragraph clips any overflow.
    let max_entries = max_entries_for_column(layout.available_rows);

    let mut panels = Vec::new();

    if let Some(p) = build_cpu_panel(snapshot, history, spark_width, theme) {
        panels.push(p);
    }
    if let Some(p) = build_thermal_panel(snapshot, history, spark_width, max_entries, theme) {
        panels.push(p);
    }
    if let Some(p) = build_memory_panel(snapshot, theme) {
        panels.push(p);
    }
    if let Some(p) = build_power_panel(snapshot, history, spark_width, max_entries, theme) {
        panels.push(p);
    }
    if let Some(p) = build_storage_panel(snapshot, max_entries, theme) {
        panels.push(p);
    }
    if let Some(p) = build_network_panel(snapshot, max_entries, theme) {
        panels.push(p);
    }
    if let Some(p) = build_fans_panel(snapshot, max_entries, theme) {
        panels.push(p);
    }
    if let Some(p) = build_platform_panel(snapshot, max_entries, theme) {
        panels.push(p);
    }
    // Per-core panels only in 3-col — too many rows for narrow layouts
    if three_col {
        if let Some(p) = build_cpu_freq_panel(snapshot, history, spark_width, max_entries, theme) {
            panels.push(p);
        }
    }
    // Voltage and GPU in all layout modes
    if let Some(p) = build_voltage_panel(snapshot, history, spark_width, max_entries, theme) {
        panels.push(p);
    }
    if let Some(p) = build_gpu_panel(snapshot, history, spark_width, max_entries, theme) {
        panels.push(p);
    }
    if let Some(p) = build_errors_panel(snapshot, theme) {
        panels.push(p);
    }

    // Assign columns based on layout mode
    if three_col {
        // Left: CPU, CPU Freq
        // Center: Memory, Storage, Network, Voltage, GPU
        // Right: Thermal, Power, Fans, Platform
        for panel in &mut panels {
            panel.column = match panel.title.as_str() {
                "CPU" | "CPU Freq" => Column::Left,
                "Memory" | "Storage" | "Network" | "Voltage" | "GPU" => Column::Center,
                "Thermal" | "Power" | "Fans" | "Platform" => Column::Right,
                _ => Column::Left, // Errors, etc.
            };
        }
    }
    // In 2-col mode, keep the assignments from the individual builders

    panels
}

// ---------------------------------------------------------------------------
// CPU Panel
// ---------------------------------------------------------------------------

fn build_cpu_panel<'a>(
    snapshot: &'a [(SensorId, SensorReading)],
    history: &'a SensorHistory,
    spark_width: usize,
    theme: &TuiTheme,
) -> Option<Panel<'a>> {
    let util_sensors: Vec<&(SensorId, SensorReading)> = snapshot
        .iter()
        .filter(|(id, _)| id.source == "cpu" && id.chip == "utilization")
        .collect();

    if util_sensors.is_empty() {
        return None;
    }

    let mut rows: Vec<PanelRow<'_>> = Vec::new();
    let mut headline: Option<String> = None;

    // Total CPU utilization gauge
    if let Some((_, reading)) = util_sensors.iter().find(|(id, _)| id.sensor == "total") {
        headline = Some(format!("{:.1}%", reading.current));
        let accent = theme.panel_cpu;
        let filled_style = if reading.current > 90.0 {
            Style::default().fg(theme.crit)
        } else if reading.current > 70.0 {
            Style::default().fg(theme.warn)
        } else {
            Style::default().fg(accent)
        };
        rows.push(PanelRow::Gauge {
            label_style: theme.label_style(),
            label: String::new(),
            ratio: reading.current / 100.0,
            filled_style,
            unfilled_style: Style::default().fg(theme.muted),
        });
    }

    // Per-core dense bar
    let mut cores: Vec<(&SensorId, &SensorReading)> = util_sensors
        .iter()
        .filter(|(id, _)| id.sensor.starts_with("cpu") && id.sensor != "total")
        .map(|(id, r)| (id, r))
        .collect();
    cores.sort_by(|(a, _), (b, _)| a.natural_cmp(b));

    if !cores.is_empty() {
        // Per-core heatmap grid: each core is a colored ██ block.
        // Fixed at 24 cores per row (fits 3-col layout with 2-char-wide blocks).
        let cols_per_row = 24usize;
        for chunk in cores.chunks(cols_per_row) {
            let spans: Vec<Span<'_>> = chunk
                .iter()
                .map(|(_, r)| {
                    let color =
                        theme.sparkline_color(SensorCategory::Utilization, r.current / 100.0);
                    Span::styled("\u{2588}\u{2588}", Style::default().fg(color))
                })
                .collect();
            rows.push(PanelRow::Text(Line::from(spans)));
        }
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
        let spark_spans = history
            .data
            .get(&key)
            .map(|buf| sparkline_spans(buf, spark_width, reading.category, theme))
            .unwrap_or_default();
        let prec = format_precision(&reading.unit);
        // On multi-socket systems, include the package index to disambiguate
        let label = if multi_pkg {
            format!("Pkg {}: ", id.sensor.trim_start_matches("package-"))
        } else {
            "Power: ".into()
        };
        let mut spans = vec![
            Span::styled(label, theme.label_style()),
            Span::styled(
                format!("{:>6.*}{}", prec, reading.current, reading.unit),
                theme.power_style(),
            ),
            Span::raw("  "),
        ];
        spans.extend(spark_spans);
        rows.push(PanelRow::Text(Line::from(spans)));
    }

    Some(Panel {
        title: "CPU".into(),
        headline,
        content: PanelContent::Mixed(rows),
        column: Column::Left,
        truncated: false,
    })
}

// ---------------------------------------------------------------------------
// Thermal Panel
// ---------------------------------------------------------------------------

fn build_thermal_panel<'a>(
    snapshot: &'a [(SensorId, SensorReading)],
    history: &'a SensorHistory,
    spark_width: usize,
    max_entries: usize,
    theme: &TuiTheme,
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
    let total = temps.len();
    temps.truncate(max_entries);

    let lines: Vec<Line<'_>> = temps
        .iter()
        .map(|(id, r)| {
            let label = truncate_label(&r.label, 20);
            let key = format!("{}/{}/{}", id.source, id.chip, id.sensor);
            let spark_spans = history
                .data
                .get(&key)
                .map(|buf| sparkline_spans(buf, spark_width, r.category, theme))
                .unwrap_or_default();
            let prec = format_precision(&r.unit);
            let mut spans = vec![
                Span::styled(format!("{label:<20} "), theme.label_style()),
                Span::styled(
                    format!("{:>6.*}{}", prec, r.current, r.unit),
                    theme.value_style(r),
                ),
                Span::raw(" "),
            ];
            spans.extend(spark_spans);
            Line::from(spans)
        })
        .collect();

    let headline = temps
        .first()
        .map(|(_, r)| format!("{:.1}\u{00b0}C", r.current));

    Some(Panel {
        title: "Thermal".into(),
        headline,
        content: PanelContent::Lines(lines),
        column: Column::Right,
        truncated: total > max_entries,
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

fn build_memory_panel<'a>(
    snapshot: &'a [(SensorId, SensorReading)],
    theme: &TuiTheme,
) -> Option<Panel<'a>> {
    let mut rows: Vec<PanelRow<'a>> = Vec::new();

    // RAM usage gauge
    let ram_util = snapshot
        .iter()
        .find(|(id, _)| id.source == "memory" && id.sensor == "ram_util");
    let ram_used = snapshot
        .iter()
        .find(|(id, _)| id.source == "memory" && id.sensor == "ram_used");
    let ram_total = snapshot
        .iter()
        .find(|(id, _)| id.source == "memory" && id.sensor == "ram_total");

    if let (Some((_, util)), Some((_, used)), Some((_, total))) = (ram_util, ram_used, ram_total) {
        let used_gb = used.current / 1024.0;
        let total_gb = total.current / 1024.0;
        let label = format!(
            "RAM  {:.0}/{:.0} GB ({:.0}%)",
            used_gb, total_gb, util.current
        );
        let accent = theme.panel_memory;
        rows.push(PanelRow::Gauge {
            label,
            label_style: theme.label_style(),
            ratio: util.current / 100.0,
            filled_style: Style::default().fg(accent),
            unfilled_style: Style::default().fg(theme.muted),
        });
    }

    // Swap usage gauge (only if swap exists)
    let swap_util = snapshot
        .iter()
        .find(|(id, _)| id.source == "memory" && id.sensor == "swap_util");
    let swap_used = snapshot
        .iter()
        .find(|(id, _)| id.source == "memory" && id.sensor == "swap_used");
    let swap_total = snapshot
        .iter()
        .find(|(id, _)| id.source == "memory" && id.sensor == "swap_total");

    if let (Some((_, util)), Some((_, used)), Some((_, total))) = (swap_util, swap_used, swap_total)
    {
        let used_gb = used.current / 1024.0;
        let total_gb = total.current / 1024.0;
        let label = format!(
            "Swap {:.1}/{:.0} GB ({:.0}%)",
            used_gb, total_gb, util.current
        );
        let accent = theme.panel_memory;
        rows.push(PanelRow::Gauge {
            label,
            label_style: theme.label_style(),
            ratio: util.current / 100.0,
            filled_style: Style::default().fg(accent),
            unfilled_style: Style::default().fg(theme.muted),
        });
    }

    // Cached + Buffers as text
    if let Some((_, r)) = snapshot
        .iter()
        .find(|(id, _)| id.source == "memory" && id.sensor == "cached")
    {
        let cached_gb = r.current / 1024.0;
        rows.push(PanelRow::Text(Line::from(vec![
            Span::styled("Cached + Buffers     ", theme.label_style()),
            Span::styled(format!("{:>7.1} GB", cached_gb), theme.info_style()),
        ])));
    }

    // HSMP DDR bandwidth and memory clock
    for (_, r) in snapshot.iter().filter(|(id, _)| is_hsmp_memory_sensor(id)) {
        let prec = format_precision(&r.unit);
        let unit_str = r.unit.to_string();
        rows.push(PanelRow::Text(Line::from(vec![
            Span::styled(
                format!("{:<20} ", truncate_label(&r.label, 20)),
                theme.label_style(),
            ),
            Span::styled(
                format!("{:>7.*}{}", prec, r.current, unit_str),
                theme.info_style(),
            ),
        ])));
    }

    // RAPL sub-domains (core, uncore, dram — package is in the CPU panel)
    for (_, r) in snapshot.iter().filter(|(id, _)| {
        id.source == "cpu" && id.chip == "rapl" && !id.sensor.starts_with("package")
    }) {
        let prec = format_precision(&r.unit);
        rows.push(PanelRow::Text(Line::from(vec![
            Span::styled(
                format!("{:<20} ", truncate_label(&r.label, 20)),
                theme.label_style(),
            ),
            Span::styled(format!("{:>7.*}W", prec, r.current), theme.power_style()),
        ])));
    }

    if rows.is_empty() {
        return None;
    }

    Some(Panel {
        title: "Memory".into(),
        headline: None,
        content: PanelContent::Mixed(rows),
        column: Column::Left,
        truncated: false,
    })
}

// ---------------------------------------------------------------------------
// Power Panel
// ---------------------------------------------------------------------------

fn build_power_panel<'a>(
    snapshot: &'a [(SensorId, SensorReading)],
    history: &'a SensorHistory,
    spark_width: usize,
    max_entries: usize,
    theme: &TuiTheme,
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
    let total = power.len();
    power.truncate(max_entries);

    let lines: Vec<Line<'_>> = power
        .iter()
        .map(|(id, r)| {
            let label = truncate_label(&r.label, 20);
            let key = format!("{}/{}/{}", id.source, id.chip, id.sensor);
            let spark_spans = history
                .data
                .get(&key)
                .map(|buf| sparkline_spans(buf, spark_width, r.category, theme))
                .unwrap_or_default();
            let prec = format_precision(&r.unit);
            let mut spans = vec![
                Span::styled(format!("{label:<20} "), theme.label_style()),
                Span::styled(
                    format!("{:>7.*}{}", prec, r.current, r.unit),
                    theme.power_style(),
                ),
                Span::raw(" "),
            ];
            spans.extend(spark_spans);
            Line::from(spans)
        })
        .collect();

    Some(Panel {
        title: "Power".into(),
        headline: None,
        content: PanelContent::Lines(lines),
        column: Column::Right,
        truncated: total > max_entries,
    })
}

// ---------------------------------------------------------------------------
// Storage Panel
// ---------------------------------------------------------------------------

fn build_storage_panel<'a>(
    snapshot: &'a [(SensorId, SensorReading)],
    max_entries: usize,
    theme: &TuiTheme,
) -> Option<Panel<'a>> {
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
    let total_devs = dev_list.len();
    dev_list.truncate(max_entries);

    let lines: Vec<Line<'_>> = dev_list
        .into_iter()
        .map(|(name, read, write)| {
            let dev = truncate_label(name, 10);
            Line::from(vec![
                Span::styled(format!("{dev:<10}"), theme.label_style()),
                Span::styled(" R ", theme.good_style()),
                Span::styled(format!("{read:>8.1}"), theme.good_style()),
                Span::styled("  W ", theme.crit_style()),
                Span::styled(format!("{write:>8.1}"), theme.crit_style()),
                Span::styled(" MB/s", theme.muted_style()),
            ])
        })
        .collect();

    Some(Panel {
        title: "Storage".into(),
        headline: None,
        content: PanelContent::Lines(lines),
        column: Column::Left,
        truncated: total_devs > max_entries,
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

fn build_network_panel<'a>(
    snapshot: &'a [(SensorId, SensorReading)],
    max_entries: usize,
    theme: &TuiTheme,
) -> Option<Panel<'a>> {
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
    let total_ifaces = iface_list.len();
    iface_list.truncate(max_entries);

    const BAR_WIDTH: usize = 6;
    let lines: Vec<Line<'_>> = iface_list
        .iter()
        .map(|d| {
            let iface = truncate_label(d.name, 10);
            let rx_bar = net_bar(d.rx, d.link_speed_mb, BAR_WIDTH);
            let tx_bar = net_bar(d.tx, d.link_speed_mb, BAR_WIDTH);
            // link_speed_mb is in MiB/s; convert back to Mbps for display
            let link = match d.link_speed_mb {
                Some(mibs) => {
                    let mbps = mibs * 8.388_608;
                    if mbps >= 1000.0 {
                        let gbps = mbps / 1000.0;
                        // Show decimal for fractional speeds (2.5G, 5G, etc.)
                        if (gbps - gbps.round()).abs() < 0.1 {
                            format!(" {:.0}G", gbps.round())
                        } else {
                            format!(" {:.1}G", gbps)
                        }
                    } else {
                        format!(" {:.0}M", mbps.round())
                    }
                }
                None => String::new(),
            };
            Line::from(vec![
                Span::styled(format!("{iface:<10}"), theme.label_style()),
                Span::styled(" \u{2193}", theme.good_style()),
                Span::styled(format!("{:>7.1}", d.rx), theme.good_style()),
                Span::raw(" "),
                Span::styled(rx_bar, theme.good_style()),
                Span::styled(" \u{2191}", theme.info_style()),
                Span::styled(format!("{:>7.1}", d.tx), theme.info_style()),
                Span::raw(" "),
                Span::styled(tx_bar, theme.info_style()),
                Span::styled(link, theme.muted_style()),
            ])
        })
        .collect();

    Some(Panel {
        title: "Network".into(),
        headline: None,
        content: PanelContent::Lines(lines),
        column: Column::Right,
        truncated: total_ifaces > max_entries,
    })
}

// ---------------------------------------------------------------------------
// Fans Panel
// ---------------------------------------------------------------------------

fn build_fans_panel<'a>(
    snapshot: &'a [(SensorId, SensorReading)],
    max_entries: usize,
    theme: &TuiTheme,
) -> Option<Panel<'a>> {
    let mut fans: Vec<&(SensorId, SensorReading)> = snapshot
        .iter()
        .filter(|(_, r)| r.category == SensorCategory::Fan)
        .collect();

    if fans.is_empty() {
        return None;
    }

    fans.sort_by(|(a, _), (b, _)| a.natural_cmp(b));
    let total_fans = fans.len();
    let use_two_col = total_fans > 6;
    let effective_max = if use_two_col {
        max_entries * 2
    } else {
        max_entries
    };
    fans.truncate(effective_max);

    let rows: Vec<PanelRow<'_>> = fans
        .iter()
        .map(|(_, r)| {
            let label = truncate_label(&r.label, 16);
            PanelRow::Text(Line::from(vec![
                Span::styled(format!("{label:<16} "), theme.label_style()),
                Span::styled(format!("{:>5.0} RPM", r.current), theme.value_style(r)),
            ]))
        })
        .collect();

    let content = if use_two_col {
        PanelContent::MultiCol { rows, columns: 2 }
    } else {
        PanelContent::Mixed(rows)
    };

    Some(Panel {
        title: "Fans".into(),
        headline: None,
        content,
        column: Column::Left,
        truncated: total_fans > effective_max,
    })
}

// ---------------------------------------------------------------------------
// Platform (HSMP) Panel
// ---------------------------------------------------------------------------

fn build_platform_panel<'a>(
    snapshot: &'a [(SensorId, SensorReading)],
    max_entries: usize,
    theme: &TuiTheme,
) -> Option<Panel<'a>> {
    // DDR bandwidth and memory clock are shown in the Memory panel.
    let hsmp: Vec<&(SensorId, SensorReading)> = snapshot
        .iter()
        .filter(|(id, _)| id.source == "hsmp" && !is_hsmp_memory_sensor(id))
        .collect();

    if hsmp.is_empty() {
        return None;
    }

    let total_hsmp = hsmp.len();
    let lines: Vec<Line<'_>> = hsmp
        .iter()
        .take(max_entries)
        .map(|(_, r)| {
            let prec = format_precision(&r.unit);
            let unit_str = r.unit.to_string();
            Line::from(vec![
                Span::styled(
                    format!("{:<20} ", truncate_label(&r.label, 20)),
                    theme.label_style(),
                ),
                Span::styled(
                    format!("{:>7.*}{}", prec, r.current, unit_str),
                    theme.info_style(),
                ),
            ])
        })
        .collect();

    Some(Panel {
        title: "Platform".into(),
        headline: None,
        content: PanelContent::Lines(lines),
        column: Column::Right,
        truncated: total_hsmp > max_entries,
    })
}

// ---------------------------------------------------------------------------
// CPU Freq Panel (3-col only — per-core frequency)
// ---------------------------------------------------------------------------

fn build_cpu_freq_panel<'a>(
    snapshot: &'a [(SensorId, SensorReading)],
    _history: &'a SensorHistory,
    _spark_width: usize,
    max_entries: usize,
    theme: &TuiTheme,
) -> Option<Panel<'a>> {
    let mut freqs: Vec<&(SensorId, SensorReading)> = snapshot
        .iter()
        .filter(|(id, _)| id.source == "cpu" && id.chip == "cpufreq")
        .collect();

    if freqs.is_empty() {
        return None;
    }

    freqs.sort_by(|(a, _), (b, _)| a.natural_cmp(b));
    let total = freqs.len();
    // Only use 2-column layout when entries exceed single-column capacity
    let use_two_col = total > max_entries;
    let effective_max = if use_two_col {
        max_entries * 2
    } else {
        max_entries
    };
    freqs.truncate(effective_max);

    // Use the global max observed frequency as the gauge ceiling
    let max_freq = freqs
        .iter()
        .map(|(_, r)| r.max)
        .fold(0.0f64, f64::max)
        .max(1.0);

    let rows: Vec<PanelRow<'_>> = freqs
        .iter()
        .map(|(_, r)| {
            let ratio = if max_freq > 0.0 {
                r.current / max_freq
            } else {
                0.0
            };
            let label = truncate_label(&r.label, 14);
            PanelRow::Gauge {
                label: format!("{label:<14} {:>5.0}{}", r.current, r.unit),
                label_style: theme.label_style(),
                ratio,
                filled_style: Style::default().fg(theme.panel_frequency),
                unfilled_style: Style::default().fg(theme.muted),
            }
        })
        .collect();

    let content = if use_two_col {
        PanelContent::MultiCol { rows, columns: 2 }
    } else {
        PanelContent::Mixed(rows)
    };

    Some(Panel {
        title: "CPU Freq".into(),
        headline: None,
        content,
        column: Column::Center,
        truncated: total > effective_max,
    })
}

// ---------------------------------------------------------------------------
// Voltage Panel (3-col only)
// ---------------------------------------------------------------------------

fn build_voltage_panel<'a>(
    snapshot: &'a [(SensorId, SensorReading)],
    history: &'a SensorHistory,
    spark_width: usize,
    max_entries: usize,
    theme: &TuiTheme,
) -> Option<Panel<'a>> {
    let mut volts: Vec<&(SensorId, SensorReading)> = snapshot
        .iter()
        .filter(|(_, r)| r.category == SensorCategory::Voltage)
        .collect();

    if volts.is_empty() {
        return None;
    }

    volts.sort_by(|(a, _), (b, _)| a.natural_cmp(b));
    let total = volts.len();
    volts.truncate(max_entries);

    let lines: Vec<Line<'_>> = volts
        .iter()
        .map(|(id, r)| {
            let label = truncate_label(&r.label, 20);
            let key = format!("{}/{}/{}", id.source, id.chip, id.sensor);
            let spark_spans = history
                .data
                .get(&key)
                .map(|buf| sparkline_spans(buf, spark_width, r.category, theme))
                .unwrap_or_default();
            let prec = format_precision(&r.unit);
            let mut spans = vec![
                Span::styled(format!("{label:<20} "), theme.label_style()),
                Span::styled(
                    format!("{:>7.*}{}", prec, r.current, r.unit),
                    theme.voltage_style(),
                ),
                Span::raw(" "),
            ];
            spans.extend(spark_spans);
            Line::from(spans)
        })
        .collect();

    Some(Panel {
        title: "Voltage".into(),
        headline: None,
        content: PanelContent::Lines(lines),
        column: Column::Right, // 2-col: right; 3-col: remapped to center
        truncated: total > max_entries,
    })
}

// ---------------------------------------------------------------------------
// GPU Panel (3-col only — groups NVML + amdgpu sensors per GPU)
// ---------------------------------------------------------------------------

fn build_gpu_panel<'a>(
    snapshot: &'a [(SensorId, SensorReading)],
    history: &'a SensorHistory,
    spark_width: usize,
    max_entries: usize,
    theme: &TuiTheme,
) -> Option<Panel<'a>> {
    let gpu_sensors: Vec<&(SensorId, SensorReading)> = snapshot
        .iter()
        .filter(|(id, _)| id.source == "nvml" || id.source == "amdgpu")
        .collect();

    if gpu_sensors.is_empty() {
        return None;
    }

    let total = gpu_sensors.len();
    let lines: Vec<Line<'_>> = gpu_sensors
        .iter()
        .take(max_entries)
        .map(|(id, r)| {
            // Build a compact label: "GPU0 Temp", "GPU1 Power", etc.
            let gpu_idx = id.chip.trim_start_matches(|c: char| !c.is_ascii_digit());
            let sensor_name = match id.sensor.as_str() {
                "temperature" => "Temp",
                "fan_speed" | "fan" => "Fan",
                "power" => "Power",
                "core_clock" => "Core Clk",
                "mem_clock" => "Mem Clk",
                "gpu_util" => "GPU Util",
                "mem_util" => "Mem Util",
                "vram_used" => "VRAM Used",
                other => other,
            };
            let label = format!("GPU{gpu_idx} {sensor_name}");
            let key = format!("{}/{}/{}", id.source, id.chip, id.sensor);
            // Use uniform color for all GPU sparklines to avoid rainbow effect
            let spark_spans = history
                .data
                .get(&key)
                .map(|buf| sparkline_spans(buf, spark_width, SensorCategory::Other, theme))
                .unwrap_or_default();
            let prec = format_precision(&r.unit);
            let unit_str = r.unit.to_string();
            // Pad unit to 3 display columns (°C is 2 chars but 3 bytes)
            let unit_display_width = unit_str.chars().count();
            let unit_padded = format!(
                "{unit_str}{}",
                " ".repeat(3usize.saturating_sub(unit_display_width))
            );
            let mut spans = vec![
                Span::styled(format!("{label:<20} "), theme.label_style()),
                Span::styled(format!("{:>7.*}", prec, r.current), theme.value_style(r)),
                Span::styled(unit_padded, theme.muted_style()),
                Span::raw(" "),
            ];
            spans.extend(spark_spans);
            Line::from(spans)
        })
        .collect();

    Some(Panel {
        title: "GPU".into(),
        headline: None,
        content: PanelContent::Lines(lines),
        column: Column::Left, // 2-col: left; 3-col: remapped to center
        truncated: total > max_entries,
    })
}

// ---------------------------------------------------------------------------
// Errors Panel (EDAC / AER / MCE)
// ---------------------------------------------------------------------------

fn build_errors_panel<'a>(
    snapshot: &'a [(SensorId, SensorReading)],
    theme: &TuiTheme,
) -> Option<Panel<'a>> {
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
            theme.warn_style().add_modifier(Modifier::BOLD),
        ),
        Span::styled(format!("  ({detail})"), theme.warn_style()),
    ])];

    Some(Panel {
        title: "Errors".into(),
        headline: None,
        content: PanelContent::Lines(lines),
        column: Column::Left, // doesn't matter, errors span full width
        truncated: false,
    })
}

// ---------------------------------------------------------------------------
// Custom Panels
// ---------------------------------------------------------------------------

fn build_custom_panels<'a>(
    snapshot: &'a [(SensorId, SensorReading)],
    history: &'a SensorHistory,
    configs: &[crate::config::PanelConfig],
    layout: &LayoutParams,
    theme: &TuiTheme,
) -> Vec<Panel<'a>> {
    let columns = match layout.num_columns {
        3 => &[Column::Left, Column::Center, Column::Right][..],
        2 => &[Column::Left, Column::Right][..],
        _ => &[Column::Left][..],
    };

    let mut panels = Vec::new();
    for (i, config) in configs.iter().enumerate() {
        if let Some(mut panel) = build_custom_panel(snapshot, history, config, layout, theme) {
            panel.column = columns[i % columns.len()];
            panels.push(panel);
        }
    }
    panels
}

fn build_custom_panel<'a>(
    snapshot: &'a [(SensorId, SensorReading)],
    history: &'a SensorHistory,
    config: &crate::config::PanelConfig,
    layout: &LayoutParams,
    theme: &TuiTheme,
) -> Option<Panel<'a>> {
    let pattern = config.filter.as_ref().map(|f| {
        glob::Pattern::new(f).unwrap_or_else(|e| {
            log::warn!("Invalid dashboard panel glob '{}': {e}", f);
            glob::Pattern::new("__invalid__").unwrap() // matches nothing
        })
    });

    let category = config.category.as_ref().and_then(|c| {
        let parsed = crate::config::parse_category(c);
        if parsed.is_none() {
            log::warn!("Unknown dashboard panel category '{c}'");
        }
        parsed
    });
    // If category was specified but invalid, show nothing for this panel
    if config.category.is_some() && category.is_none() {
        return None;
    }

    let match_opts = glob::MatchOptions {
        require_literal_separator: false,
        ..Default::default()
    };

    let mut matched: Vec<&(SensorId, SensorReading)> = snapshot
        .iter()
        .filter(|(id, r)| {
            let key = format!("{}/{}/{}", id.source, id.chip, id.sensor);
            let glob_ok = pattern
                .as_ref()
                .is_none_or(|p| p.matches_with(&key, match_opts));
            let cat_ok = category.is_none_or(|c| r.category == c);
            glob_ok && cat_ok
        })
        .collect();

    if matched.is_empty() {
        return None;
    }

    // Sort
    let sort_order = config.sort.as_deref().unwrap_or("desc");
    match sort_order {
        "asc" => matched.sort_by(|(_, a), (_, b)| {
            a.current
                .partial_cmp(&b.current)
                .unwrap_or(std::cmp::Ordering::Equal)
        }),
        "name" => matched.sort_by(|(_, a), (_, b)| a.label.cmp(&b.label)),
        _ => matched.sort_by(|(_, a), (_, b)| {
            b.current
                .partial_cmp(&a.current)
                .unwrap_or(std::cmp::Ordering::Equal)
        }),
    }

    let max = config
        .max_entries
        .unwrap_or(layout.max_entries)
        .min(layout.max_entries)
        .max(1);
    let total_matched = matched.len();
    matched.truncate(max);

    let spark_width = if config.sparklines {
        layout.spark_width
    } else {
        0
    };

    let lines: Vec<Line<'_>> = matched
        .iter()
        .map(|(id, r)| {
            let label = truncate_label(&r.label, 20);
            let prec = format_precision(&r.unit);
            let mut spans = vec![
                Span::styled(format!("{label:<20} "), theme.label_style()),
                Span::styled(
                    format!("{:>7.*}{}", prec, r.current, r.unit),
                    theme.value_style(r),
                ),
            ];
            if spark_width > 0 {
                let key = format!("{}/{}/{}", id.source, id.chip, id.sensor);
                let spark_spans = history
                    .data
                    .get(&key)
                    .map(|buf| sparkline_spans(buf, spark_width, r.category, theme))
                    .unwrap_or_default();
                spans.push(Span::raw(" "));
                spans.extend(spark_spans);
            }
            Line::from(spans)
        })
        .collect();

    Some(Panel {
        title: config.title.clone(),
        headline: None,
        content: PanelContent::Lines(lines),
        column: Column::Left, // caller will reassign
        truncated: total_matched > max,
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

    #[test]
    fn test_compute_layout_small() {
        let l = compute_layout(80, 24, 9);
        assert_eq!(l.num_columns, 1);
        assert_eq!(l.spark_width, 10);
        assert!(l.max_entries >= 2);
    }

    #[test]
    fn test_compute_layout_standard() {
        let l = compute_layout(160, 50, 9);
        assert_eq!(l.num_columns, 2);
        assert_eq!(l.spark_width, 15);
        assert!(l.max_entries > 6);
    }

    #[test]
    fn test_compute_layout_ultrawide() {
        let l = compute_layout(250, 60, 9);
        assert_eq!(l.num_columns, 3);
        assert_eq!(l.spark_width, 20);
    }

    #[test]
    fn test_compute_layout_tiny() {
        let l = compute_layout(60, 10, 9);
        assert_eq!(l.num_columns, 1);
        assert_eq!(l.spark_width, 0);
        assert_eq!(l.max_entries, 2); // clamped to minimum
    }

    #[test]
    fn test_panel_priority_ordering() {
        assert!(panel_priority("CPU") > panel_priority("Thermal"));
        assert!(panel_priority("Thermal") > panel_priority("Errors"));
        assert!(panel_priority("Errors") < panel_priority("Storage"));
        // New panels have explicit priorities
        assert!(panel_priority("CPU Cores") < panel_priority("GPU"));
        assert!(panel_priority("CPU Freq") < panel_priority("GPU"));
        assert!(panel_priority("Voltage") < panel_priority("Power"));
        assert!(panel_priority("GPU") == panel_priority("GPU"));
    }

    // -- Custom panel tests --------------------------------------------------

    use crate::model::sensor::{SensorReading, SensorUnit};

    fn make_sensor(
        source: &str,
        chip: &str,
        sensor: &str,
        label: &str,
        value: f64,
        unit: SensorUnit,
        category: SensorCategory,
    ) -> (SensorId, SensorReading) {
        (
            SensorId {
                source: source.into(),
                chip: chip.into(),
                sensor: sensor.into(),
            },
            SensorReading::new(label.to_string(), value, unit, category),
        )
    }

    fn test_layout() -> LayoutParams {
        compute_layout(200, 50, 9)
    }

    #[test]
    fn test_custom_panel_glob_filter() {
        let snapshot = vec![
            make_sensor(
                "hwmon",
                "nct6798",
                "temp1",
                "CPU",
                50.0,
                SensorUnit::Celsius,
                SensorCategory::Temperature,
            ),
            make_sensor(
                "hwmon",
                "nct6798",
                "in0",
                "Vcore",
                1.2,
                SensorUnit::Volts,
                SensorCategory::Voltage,
            ),
            make_sensor(
                "gpu",
                "gpu0",
                "temp",
                "GPU Temp",
                60.0,
                SensorUnit::Celsius,
                SensorCategory::Temperature,
            ),
        ];
        let history = SensorHistory::new();
        let layout = test_layout();
        let theme = super::super::theme::TuiTheme::default();

        let config = crate::config::PanelConfig {
            title: "Test".into(),
            filter: Some("hwmon/*".into()),
            category: None,
            max_entries: None,
            sparklines: true,
            sort: None,
        };
        let panel = build_custom_panel(&snapshot, &history, &config, &layout, &theme);
        assert!(panel.is_some());
        assert_eq!(panel.unwrap().content.lines().len(), 2); // matches hwmon sensors only
    }

    #[test]
    fn test_custom_panel_category_filter() {
        let snapshot = vec![
            make_sensor(
                "hwmon",
                "nct6798",
                "temp1",
                "CPU",
                50.0,
                SensorUnit::Celsius,
                SensorCategory::Temperature,
            ),
            make_sensor(
                "hwmon",
                "nct6798",
                "in0",
                "Vcore",
                1.2,
                SensorUnit::Volts,
                SensorCategory::Voltage,
            ),
        ];
        let history = SensorHistory::new();
        let layout = test_layout();
        let theme = super::super::theme::TuiTheme::default();

        let config = crate::config::PanelConfig {
            title: "Temps".into(),
            filter: None,
            category: Some("temperature".into()),
            max_entries: None,
            sparklines: true,
            sort: None,
        };
        let panel = build_custom_panel(&snapshot, &history, &config, &layout, &theme).unwrap();
        assert_eq!(panel.content.lines().len(), 1); // only the temp sensor
    }

    #[test]
    fn test_custom_panel_invalid_category_returns_none() {
        let snapshot = vec![make_sensor(
            "hwmon",
            "nct6798",
            "temp1",
            "CPU",
            50.0,
            SensorUnit::Celsius,
            SensorCategory::Temperature,
        )];
        let history = SensorHistory::new();
        let layout = test_layout();
        let theme = super::super::theme::TuiTheme::default();

        let config = crate::config::PanelConfig {
            title: "Bad".into(),
            filter: None,
            category: Some("temprature".into()), // typo
            max_entries: None,
            sparklines: true,
            sort: None,
        };
        assert!(build_custom_panel(&snapshot, &history, &config, &layout, &theme).is_none());
    }

    #[test]
    fn test_custom_panel_sort_desc() {
        let snapshot = vec![
            make_sensor(
                "hwmon",
                "nct6798",
                "temp1",
                "Low",
                30.0,
                SensorUnit::Celsius,
                SensorCategory::Temperature,
            ),
            make_sensor(
                "hwmon",
                "nct6798",
                "temp2",
                "High",
                80.0,
                SensorUnit::Celsius,
                SensorCategory::Temperature,
            ),
            make_sensor(
                "hwmon",
                "nct6798",
                "temp3",
                "Mid",
                55.0,
                SensorUnit::Celsius,
                SensorCategory::Temperature,
            ),
        ];
        let history = SensorHistory::new();
        let layout = test_layout();
        let theme = super::super::theme::TuiTheme::default();

        let config = crate::config::PanelConfig {
            title: "Sorted".into(),
            filter: None,
            category: Some("temperature".into()),
            max_entries: None,
            sparklines: false,
            sort: Some("desc".into()),
        };
        let panel = build_custom_panel(&snapshot, &history, &config, &layout, &theme).unwrap();
        assert_eq!(panel.content.lines().len(), 3);
        // First line should contain "High" (80°C), not "Low" (30°C)
        let first_line = format!("{}", panel.content.lines()[0]);
        assert!(
            first_line.contains("High"),
            "Expected 'High' first, got: {first_line}"
        );
    }

    #[test]
    fn test_custom_panel_empty_snapshot() {
        let snapshot: Vec<(SensorId, SensorReading)> = vec![];
        let history = SensorHistory::new();
        let layout = test_layout();
        let theme = super::super::theme::TuiTheme::default();

        let config = crate::config::PanelConfig {
            title: "Empty".into(),
            filter: None,
            category: None,
            max_entries: None,
            sparklines: true,
            sort: None,
        };
        assert!(build_custom_panel(&snapshot, &history, &config, &layout, &theme).is_none());
    }

    #[test]
    fn test_custom_panel_max_entries_zero_clamped() {
        let snapshot = vec![
            make_sensor(
                "hwmon",
                "nct6798",
                "temp1",
                "CPU",
                50.0,
                SensorUnit::Celsius,
                SensorCategory::Temperature,
            ),
            make_sensor(
                "hwmon",
                "nct6798",
                "temp2",
                "GPU",
                60.0,
                SensorUnit::Celsius,
                SensorCategory::Temperature,
            ),
        ];
        let history = SensorHistory::new();
        let layout = test_layout();
        let theme = super::super::theme::TuiTheme::default();

        let config = crate::config::PanelConfig {
            title: "Clamped".into(),
            filter: None,
            category: None,
            max_entries: Some(0),
            sparklines: true,
            sort: None,
        };
        let panel = build_custom_panel(&snapshot, &history, &config, &layout, &theme).unwrap();
        assert_eq!(panel.content.lines().len(), 1); // clamped to 1
    }
}
