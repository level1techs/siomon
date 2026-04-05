use std::collections::VecDeque;
use std::io::{self, Stdout};

use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;
use ratatui::layout::{Constraint, Direction, Flex, Layout, Rect};
use ratatui::style::Color;
use ratatui::style::{Modifier, Style};
use ratatui::symbols::Marker;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Axis, Block, Borders, Chart, Dataset, GraphType, LineGauge, Paragraph};

use crate::model::sensor::{SensorCategory, SensorId, SensorReading};

use super::{SensorHistory, SystemSummary, format_precision, sparkline_spans, theme::TuiTheme};

struct LayoutParams {
    num_columns: u8,
    max_entries: usize,
    spark_width: usize,
    available_rows: u16,
    chart_max_points: usize,
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

    let col_width = width / (num_columns as u16).max(1);

    // Sparkline width fills remaining column width after label + value overhead.
    // borders(2) + label(20) + space(1) + value(8) + space(1) + trailing(3) = 35 chars
    let spark_width = col_width.saturating_sub(35) as usize;

    let available_rows = height.saturating_sub(4); // header(3) + status(1)

    let panels_per_col = panel_count.max(1).div_ceil(num_columns as usize) as u16;
    let rows_per_panel = available_rows / panels_per_col.max(1);

    let max_entries = (rows_per_panel.saturating_sub(2) as usize).clamp(2, 50);

    LayoutParams {
        num_columns,
        max_entries,
        spark_width,
        available_rows,
        chart_max_points: 0, // set by caller
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
        "CPU" | "CPU Analyzer" | "CCD Analyzer" => 9,
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
    poll_interval_ms: u64,
    sys: &SystemSummary,
) -> io::Result<()> {
    terminal.draw(|frame| {
        let size = frame.area();
        let estimated_panels = if dashboard_config.panels.is_empty() {
            12
        } else {
            dashboard_config.panels.len()
        };
        let mut layout = compute_layout(size.width, size.height, estimated_panels);
        layout.chart_max_points = if poll_interval_ms > 0 {
            ((dashboard_config.chart_history_secs * 1000) / poll_interval_ms) as usize
        } else {
            300
        };

        // Outer layout: header + main + status
        let outer = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(3),
                Constraint::Fill(1),
                Constraint::Length(1),
            ])
            .flex(Flex::Start)
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
            " q: quit | d: tree view | m: DIMM | t: theme | /: search | {sensor_count} sensors | {elapsed_str}"
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

/// A single time-series dataset for a Chart panel.
struct ChartSeries {
    name: String,
    color: Color,
    data: Vec<(f64, f64)>,
}

/// Colors cycled through for chart datasets to differentiate lines.
/// Chart line colors ordered for maximum contrast between consecutive entries:
/// alternates hue and lightness so adjacent datasets are always visually distinct.
const CHART_COLORS: &[Color] = &[
    Color::Cyan,
    Color::LightRed,
    Color::Green,
    Color::Magenta,
    Color::Yellow,
    Color::Blue,
    Color::LightGreen,
    Color::Red,
    Color::LightCyan,
    Color::LightMagenta,
    Color::LightYellow,
    Color::LightBlue,
];

// ---------------------------------------------------------------------------
// CPU Analyzer — histogram heatmap data structures
// ---------------------------------------------------------------------------

/// Bucket sensor history values into a time-distribution histogram.
///
/// Returns `num_buckets` normalized fractions (0.0–1.0) representing the
/// fraction of samples that fell into each bucket. Bucket boundaries span
/// `[min, max]` evenly.
fn compute_histogram(samples: &[f64], num_buckets: usize, min: f64, max: f64) -> Vec<f64> {
    if num_buckets == 0 {
        return Vec::new();
    }
    let mut bins = vec![0u32; num_buckets];
    let range = (max - min).max(f64::EPSILON);
    for &v in samples {
        if !v.is_finite() {
            continue;
        }
        let idx = (((v - min) / range) * (num_buckets - 1) as f64).round() as usize;
        bins[idx.min(num_buckets - 1)] += 1;
    }
    let total = samples.len().max(1) as f64;
    bins.iter().map(|&c| c as f64 / total).collect()
}

/// Map a time-distribution fraction to a TrueColor gradient.
///
/// Adapted from CPU-Heatmap's `heatColor()`:
/// - 0.0 → near-black (no time spent)
/// - 0.0–0.5 → green → yellow
/// - 0.5–1.0 → yellow → orange-red
fn heat_color(fraction: f64) -> Color {
    if fraction < 0.001 {
        return Color::Rgb(20, 20, 30);
    }
    let t = fraction.clamp(0.0, 1.0);
    let (r, g, b) = if t < 0.5 {
        let u = t / 0.5;
        ((u * 255.0) as u8, (180.0 - u * 15.0) as u8, 0u8)
    } else {
        let u = (t - 0.5) / 0.5;
        ((255.0 - u * 35.0) as u8, (165.0 - u * 145.0) as u8, 0u8)
    };
    Color::Rgb(r, g, b)
}

/// Blue gradient for frequency histogram bins.
/// Near-black → deep blue → bright cyan-blue.
fn freq_heat_color(fraction: f64) -> Color {
    if fraction < 0.001 {
        return Color::Rgb(10, 12, 30);
    }
    let t = fraction.clamp(0.0, 1.0);
    let r = (20.0 + t * 60.0) as u8;
    let g = (30.0 + t * 150.0) as u8;
    let b = (80.0 + t * 175.0) as u8;
    Color::Rgb(r, g, b)
}

/// Extract the last `max_points` finite samples from a sensor history buffer.
fn history_samples(buf: &VecDeque<f64>, max_points: usize) -> Vec<f64> {
    let start = buf.len().saturating_sub(max_points);
    buf.iter()
        .skip(start)
        .copied()
        .filter(|v| v.is_finite())
        .collect()
}

/// A single histogram group (e.g. all core frequencies, or all core loads).
struct HistogramData {
    /// (row_label, bins) — bins are normalized fractions 0.0–1.0.
    rows: Vec<(String, Vec<f64>)>,
    /// Global max bin value across all rows (for color normalization).
    global_max: f64,
    /// Axis labels for bucket boundaries.
    axis_labels: Vec<String>,
    /// Background color for zero-valued cells in this section.
    bg_color: Color,
}

/// A single thread's load histogram within a core group.
struct ThreadRow {
    label: String,  // "Th1", "Th2", or "" for CCD mode
    bins: Vec<f64>, // normalized histogram bins
    avg_load: f64,  // weighted average load 0–100
}

/// A physical core group: 1 freq row + N thread load rows.
struct CoreGroup {
    label: String,           // "C0", "C1", "CCD0", etc.
    freq_bins: Vec<f64>,     // frequency histogram bins
    threads: Vec<ThreadRow>, // 1 per SMT thread (or 1 for CCD)
}

impl CoreGroup {
    /// Total display rows: 1 freq + N threads.
    fn row_count(&self) -> usize {
        1 + self.threads.len()
    }
}

/// Pre-computed data for the full CPU Analyzer composite panel.
struct CpuAnalyzerData {
    /// Physical core groups in display order.
    groups: Vec<CoreGroup>,
    /// Frequency histogram global max (for color normalization).
    freq_global_max: f64,
    /// Load histogram global max.
    load_global_max: f64,
    /// Frequency axis labels.
    freq_axis_labels: Vec<String>,
    /// Load axis labels.
    load_axis_labels: Vec<String>,
    /// Frequency histogram background color.
    freq_bg: Color,
    /// Load histogram background color.
    load_bg: Color,
    /// Package power histogram (Watts bins) — None if no RAPL/HSMP.
    power_histogram: Option<HistogramData>,
    /// L3 cache hit rate histogram — None if no perf sensor.
    l3_histogram: Option<HistogramData>,
    /// DDR bandwidth histogram (Gbps bins) — None if no HSMP/resctrl.
    ddr_histogram: Option<HistogramData>,
}

/// Generate evenly-spaced axis labels for a range.
fn axis_labels(min: f64, max: f64, count: usize, unit: &str, precision: usize) -> Vec<String> {
    if count == 0 {
        return Vec::new();
    }
    let step = (max - min) / (count.max(1) - 1) as f64;
    (0..count)
        .map(|i| {
            let v = min + step * i as f64;
            if precision == 0 {
                format!("{:.0}{}", v, unit)
            } else {
                format!("{:.prec$}{}", v, unit, prec = precision)
            }
        })
        .collect()
}

enum PanelContent<'a> {
    /// Standard text lines (current behavior for most panels).
    Lines(Vec<Line<'a>>),
    /// Mixed content: text lines interleaved with gauge widgets.
    Mixed(Vec<PanelRow<'a>>),
    /// Time-series chart with multiple colored line datasets.
    TimeChart {
        series: Vec<ChartSeries>,
        y_unit: String,
    },
    /// Full CPU Analyzer composite panel with histogram heatmaps.
    CpuAnalyzer(Box<CpuAnalyzerData>),
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
            PanelContent::TimeChart { .. } => {
                // Charts grow to fill; minimum 5 rows for legibility
                5
            }
            PanelContent::CpuAnalyzer(data) => {
                // Sum of all core group rows + 2 axis rows + 1 legend
                let core_rows: u16 = data.groups.iter().map(|g| g.row_count() as u16).sum();
                core_rows + 3
            }
        }
    }

    /// Whether this panel benefits from extra vertical space.
    /// TimeCharts and standalone Heatmaps grow; CpuAnalyzer and text do not.
    fn is_growable(&self) -> bool {
        matches!(self, PanelContent::TimeChart { .. })
    }

    #[cfg(test)]
    fn lines(&self) -> &[Line<'a>] {
        match self {
            PanelContent::Lines(lines) => lines,
            _ => &[],
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

    // Two columns — Fill(1) + SpaceBetween for true equal-width distribution
    let cols = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Fill(1), Constraint::Fill(1)])
        .flex(Flex::SpaceBetween)
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

    // Three columns — Fill(1) + SpaceBetween for true equal-width distribution
    let cols = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Fill(1),
            Constraint::Fill(1),
            Constraint::Fill(1),
        ])
        .flex(Flex::SpaceBetween)
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

    // Sizing strategy:
    // - Truncated panels get Fill(1) — expand to fill remaining space.
    // - Growable panels (TimeCharts, Heatmaps) get Fill(h) — grow proportionally.
    // - Fixed panels (CpuAnalyzer, Lines, Mixed) get Length(h) — exact size, no stretch.
    // This ensures the CPU Analyzer never steals space from other panels.
    let constraints: Vec<Constraint> = panels
        .iter()
        .map(|p| {
            let h = p.content.height() + 2;
            if p.truncated {
                Constraint::Fill(1)
            } else if p.content.is_growable() {
                Constraint::Fill(h)
            } else {
                Constraint::Length(h)
            }
        })
        .collect();

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints(constraints)
        .flex(Flex::Start)
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
            PanelContent::TimeChart { series, y_unit } => {
                render_time_chart(frame, chunks[i], block, series, y_unit, theme);
            }
            PanelContent::CpuAnalyzer(data) => {
                let inner = block.inner(chunks[i]);
                frame.render_widget(block, chunks[i]);
                render_cpu_analyzer(frame, inner, data, theme);
            }
        }
    }
}

// ---------------------------------------------------------------------------
// CPU Analyzer — composite panel rendering
// ---------------------------------------------------------------------------

/// Render the full CPU Analyzer panel. Side columns (Power, L3, DDR) are
/// always shown — greyed out when data is unavailable.
fn render_cpu_analyzer(
    frame: &mut ratatui::Frame,
    area: Rect,
    data: &CpuAnalyzerData,
    _theme: &TuiTheme,
) {
    if area.width < 10 || area.height < 5 {
        return;
    }

    // Vertical: freq axis (1) + core rows + load axis (1) + legend (1)
    let vert = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1), // freq axis
            Constraint::Fill(1),   // core rows
            Constraint::Length(1), // load axis
            Constraint::Length(1), // legend
        ])
        .flex(Flex::Start)
        .split(area);
    let freq_axis_area = vert[0];
    let core_area = vert[1];
    let load_axis_area = vert[2];
    let legend_area = vert[3];

    // Horizontal columns — applied to the CORE ROWS area.
    // Side bars span the full content height (freq_axis through load_axis).
    let side_area = Rect {
        y: freq_axis_area.y,
        height: freq_axis_area
            .height
            .saturating_add(core_area.height)
            .saturating_add(load_axis_area.height),
        ..area
    };
    let cols = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Length(6), // Power scale
            Constraint::Length(3), // Power bar
            Constraint::Length(6), // Core labels
            Constraint::Length(5), // Load summary
            Constraint::Fill(1),   // Histogram body
            Constraint::Length(2), // L3 HR label
            Constraint::Length(3), // L3 bar
            Constraint::Length(2), // DRAM BW label
            Constraint::Length(3), // DRAM bar
        ])
        .flex(Flex::Start)
        .split(side_area);

    // Side bars span full height (axes + core rows)
    render_vertical_bar_or_grey(
        frame,
        cols[0],
        cols[1],
        data.power_histogram.as_ref(),
        Color::Rgb(245, 160, 160),
        Color::Rgb(95, 33, 33),
    );
    render_vertical_label(frame, cols[5], "L3 HR", Color::Rgb(196, 168, 224));
    render_vertical_bar_single(
        frame,
        cols[6],
        data.l3_histogram.as_ref(),
        Color::Rgb(40, 15, 60),
    );
    render_vertical_label(frame, cols[7], "DRAM BW", Color::Rgb(192, 160, 224));
    render_vertical_bar_single(
        frame,
        cols[8],
        data.ddr_histogram.as_ref(),
        Color::Rgb(70, 50, 100),
    );

    // Core labels + load summary use only the core_area Y range (aligned with histogram rows)
    let core_labels_area = Rect {
        y: core_area.y,
        height: core_area.height,
        ..cols[2]
    };
    let load_summary_area = Rect {
        y: core_area.y,
        height: core_area.height,
        ..cols[3]
    };
    render_core_labels_grouped(frame, core_labels_area, &data.groups);
    render_load_summary_grouped(frame, load_summary_area, &data.groups);

    // Histogram body: axes + core rows in the central column
    let hist_col = cols[4];
    let hist_freq_axis = Rect {
        y: freq_axis_area.y,
        height: 1,
        ..hist_col
    };
    let hist_core = Rect {
        y: core_area.y,
        height: core_area.height,
        ..hist_col
    };
    let hist_load_axis = Rect {
        y: load_axis_area.y,
        height: 1,
        ..hist_col
    };
    render_axis_line(
        frame,
        hist_freq_axis,
        &data.freq_axis_labels,
        Color::Rgb(122, 184, 245),
    );
    render_axis_line(
        frame,
        hist_load_axis,
        &data.load_axis_labels,
        Color::Rgb(143, 212, 143),
    );
    render_histogram_core_rows(frame, hist_core, data);

    render_heat_legend(frame, legend_area, data);
}

/// Render a vertical bar column with scale labels. Greyed out if no data.
fn render_vertical_bar_or_grey(
    frame: &mut ratatui::Frame,
    scale_area: Rect,
    bar_area: Rect,
    histogram: Option<&HistogramData>,
    label_color: Color,
    grey_bg: Color,
) {
    if let Some(h) = histogram {
        // Scale labels — reversed so highest value is at the top
        let mut labels_top_down: Vec<String> = h.axis_labels.clone();
        labels_top_down.reverse();
        render_vertical_scale(frame, scale_area, &labels_top_down, label_color);
        // Histogram bar
        render_vertical_bar_single(frame, bar_area, Some(h), h.bg_color);
    } else {
        // Greyed-out: show "N/A" centered in scale area, dim bars
        let na_y = scale_area.y + scale_area.height / 2;
        if na_y < scale_area.y + scale_area.height {
            frame.render_widget(
                Paragraph::new(Line::styled(
                    format!("{:^w$}", "N/A", w = scale_area.width as usize),
                    Style::default().fg(Color::DarkGray),
                )),
                Rect {
                    y: na_y,
                    height: 1,
                    ..scale_area
                },
            );
        }
        for y in 0..bar_area.height {
            let r = Rect {
                y: bar_area.y + y,
                height: 1,
                ..bar_area
            };
            let fill: String = "\u{2591}".repeat(bar_area.width as usize);
            frame.render_widget(
                Paragraph::new(Line::styled(fill, Style::default().fg(grey_bg))),
                r,
            );
        }
    }
}

/// Render a single vertical histogram bar (or greyed-out placeholder).
fn render_vertical_bar_single(
    frame: &mut ratatui::Frame,
    area: Rect,
    histogram: Option<&HistogramData>,
    grey_bg: Color,
) {
    if area.height == 0 || area.width == 0 {
        return;
    }
    if let Some(h) = histogram {
        if let Some((_, bins)) = h.rows.first() {
            if !bins.is_empty() {
                let gmax = h.global_max.max(f64::EPSILON);
                for row in 0..area.height {
                    // Bottom-up: row 0 = top of area, map to high bin indices
                    let bin_idx =
                        ((area.height - 1 - row) as usize * bins.len()) / area.height as usize;
                    let bin_idx = bin_idx.min(bins.len() - 1);
                    let fraction = bins[bin_idx] / gmax;
                    let color = heat_color(fraction);
                    let r = Rect {
                        y: area.y + row,
                        height: 1,
                        ..area
                    };
                    let fill: String = "\u{2588}".repeat(area.width as usize);
                    frame.render_widget(
                        Paragraph::new(Line::styled(
                            fill,
                            Style::default().fg(color).bg(h.bg_color),
                        )),
                        r,
                    );
                }
                return;
            }
        }
    }
    // Greyed-out placeholder with "N/A" centered
    let mid = area.height / 2;
    for row in 0..area.height {
        let r = Rect {
            y: area.y + row,
            height: 1,
            ..area
        };
        if row == mid {
            // Show "N/A" (or "N" if width < 3) over the grey bar
            let text = if area.width >= 3 {
                "N/A"
            } else {
                "\u{00d7}" // × symbol
            };
            frame.render_widget(
                Paragraph::new(Line::styled(
                    format!("{:^w$}", text, w = area.width as usize),
                    Style::default().fg(Color::DarkGray),
                )),
                r,
            );
        } else {
            let fill: String = "\u{2591}".repeat(area.width as usize);
            frame.render_widget(
                Paragraph::new(Line::styled(fill, Style::default().fg(grey_bg))),
                r,
            );
        }
    }
}

/// Render vertical scale labels evenly distributed.
fn render_vertical_scale(frame: &mut ratatui::Frame, area: Rect, labels: &[String], color: Color) {
    if area.height == 0 || labels.is_empty() {
        return;
    }
    let step = if labels.len() > 1 {
        (area.height as usize).saturating_sub(1) / (labels.len() - 1).max(1)
    } else {
        0
    };
    for (i, label) in labels.iter().enumerate() {
        let y = area.y + (i * step).min(area.height.saturating_sub(1) as usize) as u16;
        let text: String = label.chars().take(area.width as usize).collect();
        frame.render_widget(
            Paragraph::new(Line::styled(text, Style::default().fg(color))),
            Rect {
                x: area.x,
                y,
                width: area.width,
                height: 1,
            },
        );
    }
}

/// Render a vertical text label (e.g., "L3", "DDR"), centered vertically.
fn render_vertical_label(frame: &mut ratatui::Frame, area: Rect, text: &str, color: Color) {
    let chars: Vec<char> = text.chars().collect();
    let start_y = area.y + area.height.saturating_sub(chars.len() as u16) / 2;
    for (i, &ch) in chars.iter().enumerate() {
        let y = start_y + i as u16;
        if y >= area.y + area.height {
            break;
        }
        frame.render_widget(
            Paragraph::new(Line::styled(
                format!("{:^w$}", ch, w = area.width as usize),
                Style::default().fg(color).add_modifier(Modifier::BOLD),
            )),
            Rect {
                x: area.x,
                y,
                width: area.width,
                height: 1,
            },
        );
    }
}

/// Render core labels: physical core bold, thread labels grey.
fn render_core_labels_grouped(frame: &mut ratatui::Frame, area: Rect, groups: &[CoreGroup]) {
    let w = area.width as usize;
    let mut y = area.y;
    for group in groups {
        if y >= area.y + area.height {
            break;
        }
        // Freq row: bold core label
        let text: String = group.label.chars().take(w).collect();
        frame.render_widget(
            Paragraph::new(Line::styled(
                format!("{:>w$}", text),
                Style::default()
                    .fg(Color::White)
                    .add_modifier(Modifier::BOLD),
            )),
            Rect {
                x: area.x,
                y,
                width: area.width,
                height: 1,
            },
        );
        y += 1;
        // Thread rows: grey labels
        for thread in &group.threads {
            if y >= area.y + area.height {
                break;
            }
            let tl: String = thread.label.chars().take(w).collect();
            frame.render_widget(
                Paragraph::new(Line::styled(
                    format!("{:>w$}", tl),
                    Style::default().fg(Color::DarkGray),
                )),
                Rect {
                    x: area.x,
                    y,
                    width: area.width,
                    height: 1,
                },
            );
            y += 1;
        }
    }
}

/// Render load summary: one colored avg-load square per thread.
fn render_load_summary_grouped(frame: &mut ratatui::Frame, area: Rect, groups: &[CoreGroup]) {
    let w = area.width as usize;
    let mut y = area.y;
    for group in groups {
        if y >= area.y + area.height {
            break;
        }
        y += 1; // skip freq row (no load summary for it)
        for thread in &group.threads {
            if y >= area.y + area.height {
                break;
            }
            let fraction = (thread.avg_load / 100.0).clamp(0.0, 1.0);
            let color = heat_color(fraction);
            let text = format!("{:>3.0}%", thread.avg_load);
            frame.render_widget(
                Paragraph::new(Line::styled(
                    format!("{:>w$}", text),
                    Style::default().fg(Color::White).bg(color),
                )),
                Rect {
                    x: area.x,
                    y,
                    width: area.width,
                    height: 1,
                },
            );
            y += 1;
        }
    }
}

/// Render grouped core histogram rows (no axes — caller handles those).
fn render_histogram_core_rows(frame: &mut ratatui::Frame, area: Rect, data: &CpuAnalyzerData) {
    if area.width == 0 || data.groups.is_empty() {
        return;
    }
    let w = area.width as usize;
    let freq_gmax = data.freq_global_max.max(f64::EPSILON);
    let load_gmax = data.load_global_max.max(f64::EPSILON);
    let mut y = area.y;

    for group in &data.groups {
        if y >= area.y + area.height {
            break;
        }
        render_histogram_row(
            frame,
            Rect {
                x: area.x,
                y,
                width: area.width,
                height: 1,
            },
            &group.freq_bins,
            freq_gmax,
            w,
            data.freq_bg,
            freq_heat_color,
        );
        y += 1;
        for thread in &group.threads {
            if y >= area.y + area.height {
                break;
            }
            render_histogram_row(
                frame,
                Rect {
                    x: area.x,
                    y,
                    width: area.width,
                    height: 1,
                },
                &thread.bins,
                load_gmax,
                w,
                data.load_bg,
                heat_color,
            );
            y += 1;
        }
    }
}

/// Render a single histogram row as colored `█` characters.
/// `color_fn` maps a normalized fraction (0.0–1.0) to a display color.
fn render_histogram_row(
    frame: &mut ratatui::Frame,
    area: Rect,
    bins: &[f64],
    global_max: f64,
    target_width: usize,
    bg_color: Color,
    color_fn: fn(f64) -> Color,
) {
    if area.width == 0 || bins.is_empty() {
        return;
    }
    let w = target_width.min(area.width as usize);
    let spans: Vec<Span<'_>> = (0..w)
        .map(|col| {
            let bin_idx = (col * bins.len()) / w;
            let bin_idx = bin_idx.min(bins.len() - 1);
            let fraction = bins[bin_idx] / global_max;
            Span::styled(
                "\u{2588}",
                Style::default().fg(color_fn(fraction)).bg(bg_color),
            )
        })
        .collect();
    frame.render_widget(Paragraph::new(Line::from(spans)), area);
}

/// Render axis labels evenly distributed across the width.
fn render_axis_line(frame: &mut ratatui::Frame, area: Rect, labels: &[String], color: Color) {
    if area.width == 0 || labels.is_empty() {
        return;
    }
    let w = area.width as usize;
    let mut buf = vec![' '; w];
    let total_labels = labels.len();
    let mut last_end = 0usize;
    for (i, label) in labels.iter().enumerate() {
        let pos = if total_labels <= 1 {
            0
        } else {
            (i * w.saturating_sub(1)) / (total_labels - 1)
        };
        let start = pos.saturating_sub(label.len() / 2).max(last_end);
        if start + label.len() > w {
            continue;
        }
        for (j, ch) in label.chars().enumerate() {
            if start + j < w {
                buf[start + j] = ch;
            }
        }
        last_end = start + label.len() + 1;
    }
    let text: String = buf.into_iter().collect();
    frame.render_widget(
        Paragraph::new(Line::styled(text, Style::default().fg(color))),
        area,
    );
}

/// Render the legend bar at the bottom of the CPU Analyzer panel.
fn render_heat_legend(frame: &mut ratatui::Frame, area: Rect, _data: &CpuAnalyzerData) {
    if area.width == 0 {
        return;
    }
    let mut spans: Vec<Span<'_>> = Vec::new();

    // Freq gradient (blue): min→max
    spans.push(Span::raw("Freq "));
    for i in 0..6 {
        let frac = i as f64 / 5.0;
        spans.push(Span::styled(
            "\u{2588}",
            Style::default().fg(freq_heat_color(frac)),
        ));
    }
    // Load gradient (heat): min→max
    spans.push(Span::raw("  Load "));
    for i in 0..6 {
        let frac = i as f64 / 5.0;
        spans.push(Span::styled(
            "\u{2588}",
            Style::default().fg(heat_color(frac)),
        ));
    }
    spans.push(Span::raw("  "));
    spans.push(Span::styled(
        "\u{2588}\u{2588}",
        Style::default().fg(Color::Rgb(190, 65, 65)),
    ));
    spans.push(Span::raw(" Power "));
    spans.push(Span::styled(
        "\u{2588}\u{2588}",
        Style::default().fg(Color::Rgb(80, 30, 120)),
    ));
    spans.push(Span::raw(" L3 HR "));
    spans.push(Span::styled(
        "\u{2588}\u{2588}",
        Style::default().fg(Color::Rgb(140, 100, 200)),
    ));
    spans.push(Span::raw(" DRAM BW"));

    frame.render_widget(Paragraph::new(Line::from(spans)), area);
}

/// Build a ChartSeries from sensor history, assigning a color from the palette.
/// `max_points` limits the chart to the most recent N data points.
fn make_chart_series(
    id: &SensorId,
    reading: &SensorReading,
    history: &SensorHistory,
    color_idx: usize,
    max_points: usize,
) -> ChartSeries {
    let key = format!("{}/{}/{}", id.source, id.chip, id.sensor);
    let data: Vec<(f64, f64)> = history
        .data
        .get(&key)
        .map(|buf| {
            let start = buf.len().saturating_sub(max_points);
            buf.iter()
                .skip(start)
                .enumerate()
                .map(|(i, &v)| (i as f64, if v.is_finite() { v } else { 0.0 }))
                .collect()
        })
        .unwrap_or_default();
    let color = CHART_COLORS[color_idx % CHART_COLORS.len()];
    ChartSeries {
        name: truncate_label(&reading.label, 16),
        color,
        data,
    }
}

fn render_time_chart(
    frame: &mut ratatui::Frame,
    area: Rect,
    panel_block: Block<'_>,
    series: &[ChartSeries],
    y_unit: &str,
    theme: &TuiTheme,
) {
    if series.is_empty() {
        frame.render_widget(panel_block, area);
        return;
    }

    // Find global Y bounds across all datasets
    let mut y_min = f64::INFINITY;
    let mut y_max = f64::NEG_INFINITY;
    let mut x_max: f64 = 0.0;
    for s in series {
        for &(x, y) in &s.data {
            if y.is_finite() {
                if y < y_min {
                    y_min = y;
                }
                if y > y_max {
                    y_max = y;
                }
            }
            if x > x_max {
                x_max = x;
            }
        }
    }
    if !y_min.is_finite() || !y_max.is_finite() {
        frame.render_widget(panel_block, area);
        return;
    }
    // Add small padding to Y bounds
    let y_range = (y_max - y_min).max(0.1);
    y_min = (y_min - y_range * 0.05).max(0.0);
    y_max += y_range * 0.05;

    let datasets: Vec<Dataset<'_>> = series
        .iter()
        .map(|s| {
            Dataset::default()
                .name(s.name.as_str())
                .marker(Marker::Braille)
                .graph_type(GraphType::Line)
                .style(Style::default().fg(s.color))
                .data(&s.data)
        })
        .collect();

    let y_mid = (y_min + y_max) / 2.0;
    let x_axis = Axis::default()
        .bounds([0.0, x_max.max(1.0)])
        .style(Style::default().fg(theme.muted));

    let y_axis = Axis::default()
        .bounds([y_min, y_max])
        .labels([
            format!("{:.0}{y_unit}", y_min),
            format!("{:.0}{y_unit}", y_mid),
            format!("{:.0}{y_unit}", y_max),
        ])
        .style(Style::default().fg(theme.muted));

    let chart = Chart::new(datasets)
        .block(panel_block)
        .x_axis(x_axis)
        .y_axis(y_axis)
        .legend_position(Some(ratatui::widgets::LegendPosition::TopRight))
        .hidden_legend_constraints((Constraint::Min(0), Constraint::Min(0)));

    frame.render_widget(chart, area);
}

fn render_rows(frame: &mut ratatui::Frame, area: Rect, rows: &[PanelRow<'_>]) {
    let row_constraints: Vec<Constraint> = rows.iter().map(|_| Constraint::Length(1)).collect();
    let row_areas = Layout::default()
        .direction(Direction::Vertical)
        .constraints(row_constraints)
        .flex(Flex::Start)
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

    let chart_max = layout.chart_max_points;

    let mut panels = Vec::new();

    if let Some(p) = build_cpu_panel(snapshot, history, layout, theme) {
        panels.push(p);
    }
    if let Some(p) = build_thermal_panel(snapshot, history, spark_width, theme) {
        panels.push(p);
    }
    if let Some(p) = build_memory_panel(snapshot, history, chart_max, theme) {
        panels.push(p);
    }
    if let Some(p) = build_power_panel(snapshot, history, chart_max, theme) {
        panels.push(p);
    }
    if let Some(p) = build_storage_panel(snapshot, history, chart_max, theme) {
        panels.push(p);
    }
    if let Some(p) = build_network_panel(snapshot, history, chart_max, theme) {
        panels.push(p);
    }
    if let Some(p) = build_fans_panel(snapshot, history, chart_max, theme) {
        panels.push(p);
    }
    if let Some(p) = build_platform_panel(snapshot, max_entries, theme) {
        panels.push(p);
    }
    if three_col {
        if let Some(p) = build_cpu_freq_panel(snapshot, history, chart_max, theme) {
            panels.push(p);
        }
    }
    if let Some(p) = build_voltage_panel(snapshot, history, chart_max, theme) {
        panels.push(p);
    }
    if let Some(p) = build_gpu_panel(snapshot, history, chart_max, theme) {
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
                "CPU" | "CPU Analyzer" | "CCD Analyzer" | "CPU Freq" => Column::Left,
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
    layout: &LayoutParams,
    _theme: &TuiTheme,
) -> Option<Panel<'a>> {
    let chart_max = layout.chart_max_points;

    let util_sensors: Vec<&(SensorId, SensorReading)> = snapshot
        .iter()
        .filter(|(id, _)| id.source == "cpu" && id.chip == "utilization")
        .collect();

    if util_sensors.is_empty() {
        return None;
    }

    // Per-core dense bar
    let mut cores: Vec<(&SensorId, &SensorReading)> = util_sensors
        .iter()
        .filter(|(id, _)| id.sensor.starts_with("cpu") && id.sensor != "total")
        .map(|(id, r)| (id, r))
        .collect();
    cores.sort_by(|(a, _), (b, _)| a.natural_cmp(b));

    let numa_nodes = read_numa_nodes();
    let multi_numa = numa_nodes.len() > 1;

    // Total utilization headline
    let headline = util_sensors
        .iter()
        .find(|(id, _)| id.sensor == "total")
        .map(|(_, r)| format!("{:.1}%", r.current));

    // Number of histogram buckets = estimate based on terminal width
    // Subtract fixed columns (~25 chars) from available column width
    let col_width = layout.spark_width + 35; // reverse the subtraction in compute_layout
    let num_buckets = col_width.saturating_sub(25).clamp(10, 80);

    if cores.is_empty() {
        return None;
    }

    // Always use CpuAnalyzer — works even with 0 history samples (shows empty histogram)
    let analyzer = build_cpu_analyzer_data(
        snapshot,
        history,
        &cores,
        &numa_nodes,
        multi_numa,
        chart_max,
        num_buckets,
    );

    Some(Panel {
        title: if multi_numa {
            "CCD Analyzer".into()
        } else {
            "CPU Analyzer".into()
        },
        headline,
        content: PanelContent::CpuAnalyzer(Box::new(analyzer)),

        column: Column::Left,
        truncated: false,
    })
}

/// Build the CpuAnalyzerData from sensor history.
///
/// For single NUMA: groups logical CPUs by physical core (via
/// `/sys/devices/system/cpu/cpu*/topology/core_id`). Each physical core
/// gets 1 freq row + N thread load rows (like CPU-Heatmap: C0, Th1, Th2).
/// For multi-NUMA: each NUMA node becomes one group with one aggregated row.
fn build_cpu_analyzer_data(
    snapshot: &[(SensorId, SensorReading)],
    history: &SensorHistory,
    cores: &[(&SensorId, &SensorReading)],
    numa_nodes: &[(u32, Vec<usize>)],
    multi_numa: bool,
    chart_max: usize,
    num_buckets: usize,
) -> CpuAnalyzerData {
    // Find global freq range across all cores
    let mut freq_min = f64::INFINITY;
    let mut freq_max = f64::NEG_INFINITY;
    for (id, _) in cores {
        let key = format!("cpu/cpufreq/{}", id.sensor);
        if let Some(buf) = history.data.get(&key) {
            for &v in &history_samples(buf, chart_max) {
                if v < freq_min {
                    freq_min = v;
                }
                if v > freq_max {
                    freq_max = v;
                }
            }
        }
    }
    if freq_min.is_finite() && freq_max.is_finite() {
        freq_min = (freq_min / 100.0).floor() * 100.0;
        freq_max = (freq_max / 100.0).ceil() * 100.0;
    } else {
        freq_min = 0.0;
        freq_max = 5000.0;
    }

    let groups: Vec<CoreGroup> = if multi_numa {
        // Multi-NUMA: one group per NUMA node, aggregated
        let mut g: Vec<CoreGroup> = numa_nodes
            .iter()
            .filter_map(|(node_id, cpu_set)| {
                let thread_sensors: Vec<&str> = cores
                    .iter()
                    .filter(|(id, _)| {
                        let cpu_num: usize = id
                            .sensor
                            .trim_start_matches("cpu")
                            .parse()
                            .unwrap_or(usize::MAX);
                        cpu_set.contains(&cpu_num)
                    })
                    .map(|(id, _)| id.sensor.as_str())
                    .collect();
                if thread_sensors.is_empty() {
                    return None;
                }
                // Aggregate frequency across all cores in this CCD
                let all_freq: Vec<Vec<f64>> = thread_sensors
                    .iter()
                    .filter_map(|s| {
                        history
                            .data
                            .get(&format!("cpu/cpufreq/{s}"))
                            .map(|b| history_samples(b, chart_max))
                    })
                    .collect();
                let freq_samples = average_samples(&all_freq);
                let freq_bins = compute_histogram(&freq_samples, num_buckets, freq_min, freq_max);

                // Aggregate load across all cores in this CCD
                let all_util: Vec<Vec<f64>> = thread_sensors
                    .iter()
                    .filter_map(|s| {
                        history
                            .data
                            .get(&format!("cpu/utilization/{s}"))
                            .map(|b| history_samples(b, chart_max))
                    })
                    .collect();
                let util_samples = average_samples(&all_util);
                let load_bins = compute_histogram(&util_samples, num_buckets, 0.0, 100.0);
                let avg = if util_samples.is_empty() {
                    0.0
                } else {
                    util_samples.iter().sum::<f64>() / util_samples.len() as f64
                };

                Some(CoreGroup {
                    label: format!("CCD{node_id}"),
                    freq_bins,
                    threads: vec![ThreadRow {
                        label: String::new(),
                        bins: load_bins,
                        avg_load: avg,
                    }],
                })
            })
            .collect();
        g.sort_by_key(|g| g.label.clone());
        g
    } else {
        // Single NUMA: group by physical core_id
        let phys_map = read_physical_core_map();
        if phys_map.is_empty() {
            // Fallback: treat each logical CPU as its own core with 1 thread
            cores
                .iter()
                .map(|(id, _)| {
                    let cpu_num = id.sensor.trim_start_matches("cpu");
                    let sensor = id.sensor.as_str();
                    let freq_samples = history
                        .data
                        .get(&format!("cpu/cpufreq/{sensor}"))
                        .map(|b| history_samples(b, chart_max))
                        .unwrap_or_default();
                    let util_samples = history
                        .data
                        .get(&format!("cpu/utilization/{sensor}"))
                        .map(|b| history_samples(b, chart_max))
                        .unwrap_or_default();
                    let avg = if util_samples.is_empty() {
                        0.0
                    } else {
                        util_samples.iter().sum::<f64>() / util_samples.len() as f64
                    };
                    CoreGroup {
                        label: format!("C{cpu_num}"),
                        freq_bins: compute_histogram(
                            &freq_samples,
                            num_buckets,
                            freq_min,
                            freq_max,
                        ),
                        threads: vec![ThreadRow {
                            label: "Th1".into(),
                            bins: compute_histogram(&util_samples, num_buckets, 0.0, 100.0),
                            avg_load: avg,
                        }],
                    }
                })
                .collect()
        } else {
            // Group logical CPUs by physical core_id
            let mut phys_groups: std::collections::BTreeMap<u32, Vec<usize>> =
                std::collections::BTreeMap::new();
            for (id, _) in cores {
                let cpu_num: usize = id
                    .sensor
                    .trim_start_matches("cpu")
                    .parse()
                    .unwrap_or(usize::MAX);
                if let Some(&phys_id) = phys_map.get(&cpu_num) {
                    phys_groups.entry(phys_id).or_default().push(cpu_num);
                }
            }
            phys_groups
                .into_iter()
                .map(|(phys_id, mut cpus)| {
                    cpus.sort();
                    // Frequency: average across sibling threads (they share the same P-state)
                    let all_freq: Vec<Vec<f64>> = cpus
                        .iter()
                        .filter_map(|&cpu| {
                            history
                                .data
                                .get(&format!("cpu/cpufreq/cpu{cpu}"))
                                .map(|b| history_samples(b, chart_max))
                        })
                        .collect();
                    let freq_samples = average_samples(&all_freq);
                    let freq_bins =
                        compute_histogram(&freq_samples, num_buckets, freq_min, freq_max);

                    // Each thread gets its own load row
                    let threads: Vec<ThreadRow> = cpus
                        .iter()
                        .enumerate()
                        .map(|(ti, &cpu)| {
                            let util_samples = history
                                .data
                                .get(&format!("cpu/utilization/cpu{cpu}"))
                                .map(|b| history_samples(b, chart_max))
                                .unwrap_or_default();
                            let avg = if util_samples.is_empty() {
                                0.0
                            } else {
                                util_samples.iter().sum::<f64>() / util_samples.len() as f64
                            };
                            ThreadRow {
                                label: format!("Th{}", ti + 1),
                                bins: compute_histogram(&util_samples, num_buckets, 0.0, 100.0),
                                avg_load: avg,
                            }
                        })
                        .collect();

                    CoreGroup {
                        label: format!("C{phys_id}"),
                        freq_bins,
                        threads,
                    }
                })
                .collect()
        }
    };

    // Compute global max for color normalization
    let freq_global_max = groups
        .iter()
        .flat_map(|g| g.freq_bins.iter())
        .copied()
        .fold(0.0f64, f64::max);
    let load_global_max = groups
        .iter()
        .flat_map(|g| g.threads.iter().flat_map(|t| t.bins.iter()))
        .copied()
        .fold(0.0f64, f64::max);

    let freq_axis_labels = axis_labels(freq_min, freq_max, 6, "", 0);
    let load_axis_labels = axis_labels(0.0, 100.0, 6, "%", 0);

    CpuAnalyzerData {
        groups,
        freq_global_max,
        load_global_max,
        freq_axis_labels,
        load_axis_labels,
        freq_bg: Color::Rgb(15, 30, 65),
        load_bg: Color::Rgb(12, 45, 20),
        power_histogram: build_power_histogram(snapshot, history, chart_max),
        l3_histogram: build_side_histogram(
            history,
            "perf/cache/l3_hit_rate",
            chart_max,
            0.0,
            100.0,
            Color::Rgb(40, 15, 60),
        ),
        ddr_histogram: build_ddr_histogram(snapshot, history, chart_max),
    }
}

/// Read physical core_id for each logical CPU from sysfs topology.
/// Returns a map of logical_cpu_num → physical_core_id.
fn read_physical_core_map() -> std::collections::HashMap<usize, u32> {
    let mut map = std::collections::HashMap::new();
    let Ok(entries) = std::fs::read_dir("/sys/devices/system/cpu") else {
        return map;
    };
    for entry in entries.flatten() {
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if let Some(num_str) = name.strip_prefix("cpu") {
            if let Ok(cpu_num) = num_str.parse::<usize>() {
                let core_id_path = entry.path().join("topology/core_id");
                if let Ok(content) = std::fs::read_to_string(&core_id_path) {
                    if let Ok(core_id) = content.trim().parse::<u32>() {
                        map.insert(cpu_num, core_id);
                    }
                }
            }
        }
    }
    map
}

/// Average multiple sample vectors element-wise (for CCD aggregation).
fn average_samples(all: &[Vec<f64>]) -> Vec<f64> {
    if all.is_empty() {
        return Vec::new();
    }
    let max_len = all.iter().map(|v| v.len()).max().unwrap_or(0);
    (0..max_len)
        .map(|i| {
            let (sum, count) = all
                .iter()
                .filter_map(|v| v.get(i).copied())
                .filter(|v| v.is_finite())
                .fold((0.0, 0u32), |(s, c), v| (s + v, c + 1));
            if count > 0 { sum / count as f64 } else { 0.0 }
        })
        .collect()
}

/// Build power histogram from RAPL package sensors.
fn build_power_histogram(
    snapshot: &[(SensorId, SensorReading)],
    history: &SensorHistory,
    chart_max: usize,
) -> Option<HistogramData> {
    let rapl_pkgs: Vec<&(SensorId, SensorReading)> = snapshot
        .iter()
        .filter(|(id, _)| {
            id.source == "cpu" && id.chip == "rapl" && id.sensor.starts_with("package")
        })
        .collect();
    if rapl_pkgs.is_empty() {
        return None;
    }
    // Aggregate all package power samples
    let mut all_samples = Vec::new();
    for (id, _) in &rapl_pkgs {
        let key = format!("{}/{}/{}", id.source, id.chip, id.sensor);
        if let Some(buf) = history.data.get(&key) {
            all_samples.extend(history_samples(buf, chart_max));
        }
    }
    if all_samples.is_empty() {
        return None;
    }
    let pmin = (all_samples.iter().copied().fold(f64::INFINITY, f64::min) / 5.0).floor() * 5.0;
    let pmax = (all_samples
        .iter()
        .copied()
        .fold(f64::NEG_INFINITY, f64::max)
        / 5.0)
        .ceil()
        * 5.0;
    let num_buckets = 20;
    let bins = compute_histogram(&all_samples, num_buckets, pmin, pmax);
    let gmax = bins.iter().copied().fold(0.0f64, f64::max);
    let labels = axis_labels(pmin, pmax, 5, "W", 0);

    Some(HistogramData {
        rows: vec![("Power".into(), bins)],
        global_max: gmax,
        axis_labels: labels,
        bg_color: Color::Rgb(95, 33, 33),
    })
}

/// Build a side histogram from a single sensor key.
fn build_side_histogram(
    history: &SensorHistory,
    key: &str,
    chart_max: usize,
    min: f64,
    max: f64,
    bg_color: Color,
) -> Option<HistogramData> {
    let buf = history.data.get(key)?;
    let samples = history_samples(buf, chart_max);
    if samples.is_empty() {
        return None;
    }
    let num_buckets = 20;
    let bins = compute_histogram(&samples, num_buckets, min, max);
    let gmax = bins.iter().copied().fold(0.0f64, f64::max);
    Some(HistogramData {
        rows: vec![(key.into(), bins)],
        global_max: gmax,
        axis_labels: Vec::new(),
        bg_color,
    })
}

/// Build DDR bandwidth histogram from HSMP or resctrl sensors.
fn build_ddr_histogram(
    snapshot: &[(SensorId, SensorReading)],
    history: &SensorHistory,
    chart_max: usize,
) -> Option<HistogramData> {
    // Try HSMP DDR bandwidth first
    let ddr_sensor = snapshot.iter().find(|(id, _)| {
        id.source == "hsmp" && (id.sensor == "ddr_bw_used" || id.sensor == "ddr_bw_util")
    });
    if let Some((id, _)) = ddr_sensor {
        let key = format!("{}/{}/{}", id.source, id.chip, id.sensor);
        return build_side_histogram(
            history,
            &key,
            chart_max,
            0.0,
            100.0,
            Color::Rgb(70, 50, 100),
        );
    }
    // Try resctrl MBM
    build_side_histogram(
        history,
        "resctrl/L3_0/mbm_total",
        chart_max,
        0.0,
        100.0,
        Color::Rgb(70, 50, 100),
    )
}

// ---------------------------------------------------------------------------
// Thermal Panel
// ---------------------------------------------------------------------------

fn build_thermal_panel<'a>(
    snapshot: &'a [(SensorId, SensorReading)],
    history: &'a SensorHistory,
    spark_width: usize,
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

    let headline = temps
        .first()
        .map(|(_, r)| format!("{:.1}\u{00b0}C", r.current));

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

    Some(Panel {
        title: "Thermal".into(),
        headline,
        content: PanelContent::Lines(lines),

        column: Column::Right,
        truncated: false,
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
    history: &'a SensorHistory,
    chart_max: usize,
    theme: &TuiTheme,
) -> Option<Panel<'a>> {
    // Chart: track RAM and Swap utilization over time
    let mem_sensors: Vec<&(SensorId, SensorReading)> = snapshot
        .iter()
        .filter(|(id, _)| {
            id.source == "memory" && (id.sensor == "ram_util" || id.sensor == "swap_util")
        })
        .collect();

    if !mem_sensors.is_empty() {
        let series: Vec<ChartSeries> = mem_sensors
            .iter()
            .enumerate()
            .map(|(i, (id, r))| make_chart_series(id, r, history, i, chart_max))
            .collect();
        return Some(Panel {
            title: "Memory".into(),
            headline: None,
            content: PanelContent::TimeChart {
                series,
                y_unit: "%".into(),
            },

            column: Column::Left,
            truncated: false,
        });
    }

    let mut rows: Vec<PanelRow<'a>> = Vec::new();

    // RAM usage gauge (fallback when no history)
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
    chart_max: usize,
    _theme: &TuiTheme,
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
    power.truncate(CHART_COLORS.len());

    let series: Vec<ChartSeries> = power
        .iter()
        .enumerate()
        .map(|(i, (id, r))| make_chart_series(id, r, history, i, chart_max))
        .collect();

    Some(Panel {
        title: "Power".into(),
        headline: None,
        content: PanelContent::TimeChart {
            series,
            y_unit: "W".into(),
        },

        column: Column::Right,
        truncated: false,
    })
}

// ---------------------------------------------------------------------------
// Storage Panel
// ---------------------------------------------------------------------------

fn build_storage_panel<'a>(
    snapshot: &'a [(SensorId, SensorReading)],
    history: &'a SensorHistory,
    chart_max: usize,
    _theme: &TuiTheme,
) -> Option<Panel<'a>> {
    // Chart: read/write throughput per device over time
    let disk_sensors: Vec<&(SensorId, SensorReading)> = snapshot
        .iter()
        .filter(|(id, _)| {
            id.source == "disk" && (id.sensor == "read_mbps" || id.sensor == "write_mbps")
        })
        .collect();

    if disk_sensors.is_empty() {
        return None;
    }

    let series: Vec<ChartSeries> = disk_sensors
        .iter()
        .take(CHART_COLORS.len())
        .enumerate()
        .map(|(i, (id, r))| make_chart_series(id, r, history, i, chart_max))
        .collect();

    Some(Panel {
        title: "Storage".into(),
        headline: None,
        content: PanelContent::TimeChart {
            series,
            y_unit: "MB/s".into(),
        },

        column: Column::Left,
        truncated: false,
    })
}

// ---------------------------------------------------------------------------
// Network Panel
// ---------------------------------------------------------------------------

fn build_network_panel<'a>(
    snapshot: &'a [(SensorId, SensorReading)],
    history: &'a SensorHistory,
    chart_max: usize,
    _theme: &TuiTheme,
) -> Option<Panel<'a>> {
    // Chart: RX/TX throughput per interface over time
    let throughput_sensors: Vec<&(SensorId, SensorReading)> = snapshot
        .iter()
        .filter(|(id, _)| id.source == "net" && (id.sensor == "rx_mbps" || id.sensor == "tx_mbps"))
        .collect();

    if throughput_sensors.is_empty() {
        return None;
    }

    let series: Vec<ChartSeries> = throughput_sensors
        .iter()
        .take(CHART_COLORS.len())
        .enumerate()
        .map(|(i, (id, r))| make_chart_series(id, r, history, i, chart_max))
        .collect();

    Some(Panel {
        title: "Network".into(),
        headline: None,
        content: PanelContent::TimeChart {
            series,
            y_unit: "MB/s".into(),
        },

        column: Column::Right,
        truncated: false,
    })
}

// ---------------------------------------------------------------------------
// Fans Panel
// ---------------------------------------------------------------------------

fn build_fans_panel<'a>(
    snapshot: &'a [(SensorId, SensorReading)],
    history: &'a SensorHistory,
    chart_max: usize,
    _theme: &TuiTheme,
) -> Option<Panel<'a>> {
    let mut fans: Vec<&(SensorId, SensorReading)> = snapshot
        .iter()
        .filter(|(_, r)| r.category == SensorCategory::Fan)
        .collect();

    if fans.is_empty() {
        return None;
    }

    fans.sort_by(|(a, _), (b, _)| a.natural_cmp(b));
    fans.truncate(CHART_COLORS.len());

    let series: Vec<ChartSeries> = fans
        .iter()
        .enumerate()
        .map(|(i, (id, r))| make_chart_series(id, r, history, i, chart_max))
        .collect();

    Some(Panel {
        title: "Fans".into(),
        headline: None,
        content: PanelContent::TimeChart {
            series,
            y_unit: "RPM".into(),
        },

        column: Column::Left,
        truncated: false,
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
    history: &'a SensorHistory,
    chart_max: usize,
    _theme: &TuiTheme,
) -> Option<Panel<'a>> {
    let freqs: Vec<&(SensorId, SensorReading)> = snapshot
        .iter()
        .filter(|(id, _)| id.source == "cpu" && id.chip == "cpufreq")
        .collect();

    if freqs.is_empty() {
        return None;
    }

    let numa_nodes = read_numa_nodes();
    let series: Vec<ChartSeries> = if numa_nodes.len() > 1 {
        // Multi-NUMA: pick the highest-frequency core per NUMA node
        let mut node_series: Vec<ChartSeries> = numa_nodes
            .iter()
            .enumerate()
            .filter_map(|(i, (node_id, cpu_set))| {
                // Find the core with the highest current frequency in this node
                let best = freqs
                    .iter()
                    .filter(|(id, _)| {
                        let cpu_num: usize = id
                            .sensor
                            .trim_start_matches("cpu")
                            .parse()
                            .unwrap_or(usize::MAX);
                        cpu_set.contains(&cpu_num)
                    })
                    .max_by(|(_, a), (_, b)| {
                        a.current
                            .partial_cmp(&b.current)
                            .unwrap_or(std::cmp::Ordering::Equal)
                    })?;
                let (id, r) = best;
                let mut s = make_chart_series(id, r, history, i, chart_max);
                s.name = format!("NUMA {node_id} Frequency");
                Some(s)
            })
            .collect();
        node_series.truncate(CHART_COLORS.len());
        node_series
    } else {
        // Single NUMA: show individual cores (limited for readability)
        let mut sorted = freqs;
        sorted.sort_by(|(a, _), (b, _)| a.natural_cmp(b));
        sorted.truncate(CHART_COLORS.len());
        sorted
            .iter()
            .enumerate()
            .map(|(i, (id, r))| {
                let mut s = make_chart_series(id, r, history, i, chart_max);
                let core_num = id.sensor.trim_start_matches("cpu");
                s.name = format!("Core {core_num} Frequency");
                s
            })
            .collect()
    };

    Some(Panel {
        title: "CPU Freq".into(),
        headline: None,
        content: PanelContent::TimeChart {
            series,
            y_unit: "MHz".into(),
        },

        column: Column::Center,
        truncated: false,
    })
}

// ---------------------------------------------------------------------------
// Voltage Panel (3-col only)
// ---------------------------------------------------------------------------

fn build_voltage_panel<'a>(
    snapshot: &'a [(SensorId, SensorReading)],
    history: &'a SensorHistory,
    chart_max: usize,
    _theme: &TuiTheme,
) -> Option<Panel<'a>> {
    let mut volts: Vec<&(SensorId, SensorReading)> = snapshot
        .iter()
        .filter(|(_, r)| r.category == SensorCategory::Voltage)
        .collect();

    if volts.is_empty() {
        return None;
    }

    volts.sort_by(|(a, _), (b, _)| a.natural_cmp(b));
    volts.truncate(CHART_COLORS.len());

    let series: Vec<ChartSeries> = volts
        .iter()
        .enumerate()
        .map(|(i, (id, r))| make_chart_series(id, r, history, i, chart_max))
        .collect();

    Some(Panel {
        title: "Voltage".into(),
        headline: None,
        content: PanelContent::TimeChart {
            series,
            y_unit: "V".into(),
        },

        column: Column::Right,
        truncated: false,
    })
}

// ---------------------------------------------------------------------------
// GPU Panel (3-col only — groups NVML + amdgpu sensors per GPU)
// ---------------------------------------------------------------------------

fn build_gpu_panel<'a>(
    snapshot: &'a [(SensorId, SensorReading)],
    history: &'a SensorHistory,
    chart_max: usize,
    _theme: &TuiTheme,
) -> Option<Panel<'a>> {
    let gpu_sensors: Vec<&(SensorId, SensorReading)> = snapshot
        .iter()
        .filter(|(id, _)| id.source == "nvml" || id.source == "amdgpu")
        .collect();

    if gpu_sensors.is_empty() {
        return None;
    }

    let series: Vec<ChartSeries> = gpu_sensors
        .iter()
        .take(CHART_COLORS.len())
        .enumerate()
        .map(|(i, (id, r))| make_chart_series(id, r, history, i, chart_max))
        .collect();

    Some(Panel {
        title: "GPU".into(),
        headline: None,
        content: PanelContent::TimeChart {
            series,
            y_unit: String::new(),
        },

        column: Column::Left,
        truncated: false,
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
#[cfg(test)]
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

/// Read NUMA node topology from sysfs. Returns sorted (node_id, cpu_set) pairs.
fn read_numa_nodes() -> Vec<(u32, Vec<usize>)> {
    let Ok(entries) = std::fs::read_dir("/sys/devices/system/node") else {
        return Vec::new();
    };
    let mut nodes: Vec<(u32, Vec<usize>)> = entries
        .filter_map(|e| {
            let e = e.ok()?;
            let name = e.file_name();
            let name = name.to_str()?;
            let node_id: u32 = name.strip_prefix("node")?.parse().ok()?;
            let cpulist = std::fs::read_to_string(e.path().join("cpulist")).ok()?;
            let cpus = parse_cpulist(&cpulist);
            Some((node_id, cpus))
        })
        .collect();
    nodes.sort_by_key(|(id, _)| *id);
    nodes
}

/// Parse a CPU list string like "0-3,8-11" into a sorted Vec of CPU IDs.
fn parse_cpulist(s: &str) -> Vec<usize> {
    let mut result = Vec::new();
    for part in s.trim().split(',') {
        let part = part.trim();
        if let Some((start, end)) = part.split_once('-') {
            if let (Ok(s), Ok(e)) = (start.parse::<usize>(), end.parse::<usize>()) {
                result.extend(s..=e);
            }
        } else if let Ok(n) = part.parse::<usize>() {
            result.push(n);
        }
    }
    result
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

        // spark_width = 80 - 35 = 45 (fills remaining column width)
        assert_eq!(l.spark_width, 45);
        assert!(l.max_entries >= 2);
    }

    #[test]
    fn test_compute_layout_standard() {
        let l = compute_layout(160, 50, 9);
        assert_eq!(l.num_columns, 2);

        // spark_width = 80 - 35 = 45
        assert_eq!(l.spark_width, 45);
        assert!(l.max_entries > 6);
    }

    #[test]
    fn test_compute_layout_ultrawide() {
        let l = compute_layout(250, 60, 9);
        assert_eq!(l.num_columns, 3);

        // spark_width = 83 - 35 = 48
        assert_eq!(l.spark_width, 48);
    }

    #[test]
    fn test_compute_layout_tiny() {
        let l = compute_layout(60, 10, 9);
        assert_eq!(l.num_columns, 1);

        // spark_width = 60 - 35 = 25
        assert_eq!(l.spark_width, 25);
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
