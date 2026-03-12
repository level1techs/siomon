use std::collections::{HashMap, HashSet, VecDeque};
use std::io::{self, Stdout, Write};
use std::sync::{Arc, RwLock};
use std::time::{Duration, Instant};

use crossterm::event::{self, Event, KeyCode, KeyEventKind, MouseEventKind};
use crossterm::execute;
use crossterm::terminal::{
    EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
};
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;
use ratatui::layout::{Constraint, Direction, Layout};
use ratatui::style::{Color, Modifier, Style};
use ratatui::widgets::{Block, Borders, Cell, Paragraph, Row, Table};

use crate::model::sensor::{self, SensorCategory, SensorId, SensorReading, SensorUnit};
use crate::sensors::poller::PollStatsState;

mod dashboard;

#[derive(Clone, Copy, PartialEq)]
enum ViewMode {
    Tree,
    Dashboard,
}

/// Maximum number of data points to retain per sensor for sparklines.
const HISTORY_LEN: usize = 300;

/// Unicode block characters for sparkline rendering, from lowest to highest.
const SPARK_CHARS: &[char] = &['▁', '▂', '▃', '▄', '▅', '▆', '▇', '█'];

/// Render a `VecDeque<f64>` as a sparkline string of `width` characters.
/// Values are normalized to the min/max range of the visible window.
pub(crate) fn sparkline_str(data: &VecDeque<f64>, width: usize) -> String {
    if data.is_empty() || width == 0 {
        return String::new();
    }
    let start = data.len().saturating_sub(width);

    // First pass: find min/max over finite values in the window.
    let mut min = f64::INFINITY;
    let mut max = f64::NEG_INFINITY;
    let mut has_finite = false;
    for &v in data.iter().skip(start) {
        if v.is_finite() {
            has_finite = true;
            if v < min {
                min = v;
            }
            if v > max {
                max = v;
            }
        }
    }
    if !has_finite {
        return String::new();
    }
    let range = max - min;

    // Second pass: build sparkline directly into a pre-sized String.
    let mut out = String::with_capacity(width * 3); // UTF-8 block chars are 3 bytes
    for &v in data.iter().skip(start) {
        if !v.is_finite() {
            continue;
        }
        let idx = if range < f64::EPSILON {
            3 // flat line -> mid height
        } else {
            ((v - min) / range * 7.0).round() as usize
        };
        out.push(SPARK_CHARS[idx.min(7)]);
    }
    out
}

/// Per-sensor history ring buffer for sparklines.
pub(crate) struct SensorHistory {
    pub(crate) data: HashMap<String, VecDeque<f64>>,
}

impl SensorHistory {
    fn new() -> Self {
        Self {
            data: HashMap::new(),
        }
    }

    fn push(&mut self, snapshot: &[(SensorId, SensorReading)]) {
        for (id, reading) in snapshot {
            let key = format!("{}/{}/{}", id.source, id.chip, id.sensor);
            let buf = self
                .data
                .entry(key)
                .or_insert_with(|| VecDeque::with_capacity(HISTORY_LEN));
            if buf.len() >= HISTORY_LEN {
                buf.pop_front();
            }
            buf.push_back(reading.current);
        }
    }
}

/// Run the interactive TUI sensor dashboard.
///
/// Blocks until the user presses 'q' or Esc. Reads sensor data from the
/// shared `state` map on each tick (every `poll_interval_ms` milliseconds).
pub fn run(
    state: Arc<RwLock<HashMap<SensorId, SensorReading>>>,
    poll_stats: PollStatsState,
    poll_interval_ms: u64,
    alert_rules: Vec<crate::sensors::alerts::AlertRule>,
) -> io::Result<()> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    // Enter alternate screen, then enable button-event mouse mode (1002) + SGR
    // encoding (1006). Mode 1002 captures clicks and scroll wheel but NOT plain
    // mouse movement, avoiding unnecessary redraws on hover.
    let setup_result = (|| -> io::Result<Terminal<CrosstermBackend<Stdout>>> {
        execute!(stdout, EnterAlternateScreen)?;
        stdout.write_all(b"\x1b[?1002h\x1b[?1006h")?;
        stdout.flush()?;
        let backend = CrosstermBackend::new(stdout);
        Terminal::new(backend)
    })();

    let mut terminal = match setup_result {
        Ok(t) => t,
        Err(e) => {
            // Setup failed — best-effort cleanup before propagating error
            let _ = disable_raw_mode();
            return Err(e);
        }
    };

    let result = run_loop(
        &mut terminal,
        &state,
        &poll_stats,
        poll_interval_ms,
        alert_rules,
    );

    // Best-effort cleanup: disable mouse modes before leaving alternate screen
    let _ = disable_raw_mode();
    let _ = terminal.backend_mut().write_all(b"\x1b[?1006l\x1b[?1002l");
    let _ = terminal.backend_mut().flush();
    let _ = execute!(terminal.backend_mut(), LeaveAlternateScreen);
    let _ = terminal.show_cursor();

    result
}

fn run_loop(
    terminal: &mut Terminal<CrosstermBackend<Stdout>>,
    state: &Arc<RwLock<HashMap<SensorId, SensorReading>>>,
    poll_stats: &PollStatsState,
    poll_interval_ms: u64,
    alert_rules: Vec<crate::sensors::alerts::AlertRule>,
) -> io::Result<()> {
    let start = Instant::now();
    let mut scroll_offset: usize = 0;
    let mut collapsed: HashSet<String> = HashSet::new();
    let mut cursor: usize = 0;
    let mut last_total_rows: usize = 0;
    let mut group_indices: Vec<usize> = Vec::new();
    let mut collapse_key_vec: Vec<String> = Vec::new();
    let mut alert_engine = crate::sensors::alerts::AlertEngine::new(alert_rules);
    let mut active_alerts: Vec<String> = Vec::new();

    // View mode
    let mut view_mode = ViewMode::Tree;

    // Search/filter state
    let mut filter_mode = false;
    let mut filter_query = String::new();

    // Sparkline state
    let mut history = SensorHistory::new();

    // Auto-collapse high-count groups on first render
    let mut auto_collapsed = false;

    loop {
        let elapsed = start.elapsed();

        // Snapshot sensor state and check alerts
        let snapshot = snapshot_sorted(state);
        {
            let readings_map: HashMap<SensorId, SensorReading> = snapshot.iter().cloned().collect();
            let new_alerts = alert_engine.check(&readings_map);
            if !new_alerts.is_empty() {
                active_alerts = new_alerts;
            }
        }

        // On first render, auto-collapse categories with > 32 entries
        if !auto_collapsed && !snapshot.is_empty() {
            auto_collapsed = true;
            let mut cat_counts: HashMap<String, usize> = HashMap::new();
            for (id, reading) in &snapshot {
                let ck = chip_key(id);
                let catk = cat_key(&ck, reading.category);
                *cat_counts.entry(catk).or_default() += 1;
            }
            for (key, count) in &cat_counts {
                if *count > 32 {
                    collapsed.insert(key.clone());
                }
            }
        }

        // Record sensor values for sparklines
        history.push(&snapshot);

        let sensor_count = snapshot.len();
        let elapsed_str = format_elapsed(elapsed);

        match view_mode {
            ViewMode::Tree => {
                let filter_lc = filter_query.to_ascii_lowercase();
                let (display_rows, group_indices_new, collapse_key_vec_new) =
                    build_rows(&snapshot, &collapsed, &filter_lc, &history);
                group_indices = group_indices_new;
                collapse_key_vec = collapse_key_vec_new;
                last_total_rows = display_rows.len();

                // Clamp scroll and cursor
                scroll_offset = scroll_offset.min(last_total_rows.saturating_sub(1));
                if !group_indices.is_empty() {
                    cursor = cursor.min(group_indices.len() - 1);
                }

                let max_samples = snapshot
                    .iter()
                    .map(|(_, r)| r.sample_count)
                    .max()
                    .unwrap_or(0);
                let collapsed_count = collapsed.len();

                // Build poll timing warning string
                let poll_warning = {
                    let stats = poll_stats.read().unwrap_or_else(|e| e.into_inner());
                    if stats.cycle_duration_ms > 500 {
                        let slow: Vec<String> = stats
                            .source_durations
                            .iter()
                            .filter(|&(_, &ms)| ms > 100)
                            .map(|(name, ms)| format!("{name}: {ms}ms"))
                            .collect();
                        format!(
                            " | poll: {}ms [{}]",
                            stats.cycle_duration_ms,
                            if slow.is_empty() {
                                "aggregate".into()
                            } else {
                                slow.join(", ")
                            }
                        )
                    } else {
                        String::new()
                    }
                };

                let ctx = DrawContext {
                    group_indices: &group_indices,
                    cursor,
                    scroll_offset,
                    sensor_count,
                    max_samples,
                    collapsed_count,
                    elapsed_str: &elapsed_str,
                    active_alerts: &active_alerts,
                    poll_warning: &poll_warning,
                    filter_mode,
                    filter_query: &filter_query,
                };
                draw(terminal, display_rows, &ctx)?;
            }
            ViewMode::Dashboard => {
                dashboard::render(terminal, &snapshot, &history, &elapsed_str, sensor_count)?;
            }
        }

        // Wait for next tick or meaningful input event.
        let timeout = Duration::from_millis(poll_interval_ms);
        if event::poll(timeout)? {
            // Event available — read and handle it. If it's just mouse movement,
            // drain the queue without triggering a redraw.
            let mut needs_redraw = false;
            loop {
                match event::read()? {
                    Event::Key(key) if key.kind == KeyEventKind::Press => {
                        needs_redraw = true;
                        if filter_mode {
                            match key.code {
                                KeyCode::Esc => {
                                    filter_mode = false;
                                    filter_query.clear();
                                    scroll_offset = 0;
                                    cursor = 0;
                                }
                                KeyCode::Enter => {
                                    filter_mode = false;
                                }
                                KeyCode::Backspace => {
                                    filter_query.pop();
                                    scroll_offset = 0;
                                    cursor = 0;
                                }
                                KeyCode::Char(c) => {
                                    filter_query.push(c);
                                    scroll_offset = 0;
                                    cursor = 0;
                                }
                                _ => {}
                            }
                        } else {
                            match key.code {
                                // Universal keys (both views)
                                KeyCode::Char('q') => return Ok(()),
                                KeyCode::Esc => {
                                    if !filter_query.is_empty() {
                                        filter_query.clear();
                                        scroll_offset = 0;
                                        cursor = 0;
                                    } else {
                                        return Ok(());
                                    }
                                }
                                KeyCode::Char('d') => {
                                    view_mode = match view_mode {
                                        ViewMode::Tree => ViewMode::Dashboard,
                                        ViewMode::Dashboard => ViewMode::Tree,
                                    };
                                }
                                KeyCode::Char('/') => {
                                    // Switch to tree view if in dashboard
                                    view_mode = ViewMode::Tree;
                                    filter_mode = true;
                                }
                                // Tree-only keys
                                KeyCode::Up | KeyCode::Char('k') if view_mode == ViewMode::Tree => {
                                    if cursor > 0 {
                                        cursor -= 1;
                                        if let Some(&row_idx) = group_indices.get(cursor) {
                                            if row_idx < scroll_offset {
                                                scroll_offset = row_idx;
                                            }
                                        }
                                    }
                                }
                                KeyCode::Down | KeyCode::Char('j')
                                    if view_mode == ViewMode::Tree =>
                                {
                                    if cursor + 1 < group_indices.len() {
                                        cursor += 1;
                                        if let Some(&row_idx) = group_indices.get(cursor) {
                                            let term_height = terminal.size()?.height as usize;
                                            let visible = term_height.saturating_sub(6);
                                            if row_idx >= scroll_offset + visible {
                                                scroll_offset = row_idx.saturating_sub(visible / 2);
                                            }
                                        }
                                    }
                                }
                                KeyCode::Enter | KeyCode::Char(' ')
                                    if view_mode == ViewMode::Tree =>
                                {
                                    if let Some(key) = collapse_key_vec.get(cursor) {
                                        if collapsed.contains(key) {
                                            collapsed.remove(key);
                                        } else {
                                            collapsed.insert(key.clone());
                                        }
                                    }
                                }
                                KeyCode::Char('c') if view_mode == ViewMode::Tree => {
                                    for key in all_collapse_keys(&snapshot) {
                                        collapsed.insert(key);
                                    }
                                }
                                KeyCode::Char('e') if view_mode == ViewMode::Tree => {
                                    collapsed.clear();
                                }
                                KeyCode::PageUp if view_mode == ViewMode::Tree => {
                                    scroll_offset = scroll_offset.saturating_sub(20);
                                    while cursor > 0 {
                                        if let Some(&ri) = group_indices.get(cursor) {
                                            if ri >= scroll_offset {
                                                break;
                                            }
                                        }
                                        cursor -= 1;
                                    }
                                }
                                KeyCode::PageDown if view_mode == ViewMode::Tree => {
                                    scroll_offset = scroll_offset.saturating_add(20);
                                    while cursor + 1 < group_indices.len() {
                                        if let Some(&ri) = group_indices.get(cursor) {
                                            if ri >= scroll_offset {
                                                break;
                                            }
                                        }
                                        cursor += 1;
                                    }
                                }
                                KeyCode::Home if view_mode == ViewMode::Tree => {
                                    scroll_offset = 0;
                                    cursor = 0;
                                }
                                KeyCode::End if view_mode == ViewMode::Tree => {
                                    scroll_offset = last_total_rows.saturating_sub(1);
                                    cursor = group_indices.len().saturating_sub(1);
                                }
                                _ => {}
                            }
                        }
                        break;
                    }
                    Event::Mouse(mouse) => match mouse.kind {
                        MouseEventKind::ScrollUp => {
                            scroll_offset = scroll_offset.saturating_sub(3);
                            needs_redraw = true;
                            break;
                        }
                        MouseEventKind::ScrollDown => {
                            scroll_offset = scroll_offset.saturating_add(3);
                            needs_redraw = true;
                            break;
                        }
                        _ => {} // Mouse movement — drain silently
                    },
                    Event::Resize(_, _) => {
                        needs_redraw = true;
                        break;
                    }
                    _ => {} // Unknown event — drain silently
                }
                // No meaningful event yet — if no more events queued, stop draining
                if !event::poll(Duration::ZERO)? {
                    break;
                }
            }
            if !needs_redraw {
                continue; // Only mouse movement — skip redraw, wait for next tick
            }
        }
    }
}

fn snapshot_sorted(
    state: &Arc<RwLock<HashMap<SensorId, SensorReading>>>,
) -> Vec<(SensorId, SensorReading)> {
    let map = state.read().unwrap_or_else(|e| e.into_inner());
    let mut entries: Vec<_> = map.iter().map(|(k, v)| (k.clone(), v.clone())).collect();
    entries.sort_by(|(a, _), (b, _)| a.natural_cmp(b));
    entries
}

/// Collect all collapse keys (source + chip + category) for "collapse all".
fn all_collapse_keys(snapshot: &[(SensorId, SensorReading)]) -> Vec<String> {
    let mut keys = Vec::new();
    let mut seen = HashSet::new();
    for (id, reading) in snapshot {
        let sk = &id.source;
        if seen.insert(sk.clone()) {
            keys.push(sk.clone());
        }
        let ck = chip_key(id);
        if seen.insert(ck.clone()) {
            keys.push(ck.clone());
        }
        let catk = cat_key(&ck, reading.category);
        if seen.insert(catk.clone()) {
            keys.push(catk);
        }
    }
    keys
}

/// Chip-level key: "source/chip"
fn chip_key(id: &SensorId) -> String {
    format!("{}/{}", id.source, id.chip)
}

/// Category-level collapse key: "source/chip/CategoryName"
fn cat_key(chip: &str, cat: SensorCategory) -> String {
    format!("{}/{}", chip, cat)
}

struct Summary {
    count: usize,
    current_min: f64,
    current_max: f64,
    global_min: f64,
    global_max: f64,
    avg: f64,
    unit: String,
    precision: usize,
}

fn accumulate(entry: &mut Summary, reading: &SensorReading) {
    entry.count += 1;
    entry.current_min = entry.current_min.min(reading.current);
    entry.current_max = entry.current_max.max(reading.current);
    entry.global_min = entry.global_min.min(reading.min);
    entry.global_max = entry.global_max.max(reading.max);
    entry.avg += (reading.avg - entry.avg) / entry.count as f64;
}

fn new_summary(reading: &SensorReading) -> Summary {
    Summary {
        count: 0,
        current_min: f64::MAX,
        current_max: f64::MIN,
        global_min: f64::MAX,
        global_max: f64::MIN,
        avg: 0.0,
        unit: format!("{}", reading.unit),
        precision: format_precision(&reading.unit),
    }
}

/// Compute summaries at source, chip, and category levels.
fn compute_summaries(snapshot: &[(SensorId, SensorReading)]) -> HashMap<String, Summary> {
    let mut summaries: HashMap<String, Summary> = HashMap::new();

    for (id, reading) in snapshot {
        let sk = id.source.clone();
        let ck = chip_key(id);
        let catk = cat_key(&ck, reading.category);

        for key in [&sk, &ck, &catk] {
            let entry = summaries
                .entry(key.clone())
                .or_insert_with(|| new_summary(reading));
            accumulate(entry, reading);
        }
    }

    summaries
}

/// Build a collapsed summary row with min-max range in the value columns.
fn summary_row(header_text: String, style: Style, summary: Option<&Summary>) -> Row<'static> {
    let summary_style = Style::default().fg(Color::DarkGray);
    let cells = if let Some(s) = summary {
        let p = s.precision;
        vec![
            Cell::from(header_text).style(style),
            Cell::from(format!(
                "{:.prec$}\u{2013}{:.prec$}",
                s.current_min,
                s.current_max,
                prec = p
            ))
            .style(summary_style),
            Cell::from(format!("{:.prec$}", s.global_min, prec = p)).style(summary_style),
            Cell::from(format!("{:.prec$}", s.global_max, prec = p)).style(summary_style),
            Cell::from(format!("{:.prec$}", s.avg, prec = p)).style(summary_style),
            Cell::from(s.unit.clone()).style(summary_style),
            Cell::from(""),
        ]
    } else {
        vec![
            Cell::from(header_text).style(style),
            Cell::from(""),
            Cell::from(""),
            Cell::from(""),
            Cell::from(""),
            Cell::from(""),
            Cell::from(""),
        ]
    };
    Row::new(cells)
}

/// Build an expanded header row (no summary values).
fn header_row(header_text: String, style: Style) -> Row<'static> {
    Row::new(vec![
        Cell::from(header_text).style(style),
        Cell::from(""),
        Cell::from(""),
        Cell::from(""),
        Cell::from(""),
        Cell::from(""),
        Cell::from(""),
    ])
}

/// Sort snapshot indices: source (natural), chip (natural), category sort_key, sensor (natural).
fn sorted_indices(snapshot: &[(SensorId, SensorReading)]) -> Vec<usize> {
    let mut indices: Vec<usize> = (0..snapshot.len()).collect();
    indices.sort_by(|&a, &b| {
        let (a_id, a_r) = &snapshot[a];
        let (b_id, b_r) = &snapshot[b];
        sensor::natural_cmp_str(&a_id.source, &b_id.source)
            .then_with(|| sensor::natural_cmp_str(&a_id.chip, &b_id.chip))
            .then_with(|| a_r.category.sort_key().cmp(&b_r.category.sort_key()))
            .then_with(|| a_id.natural_cmp(b_id))
    });
    indices
}

/// Check whether a sensor matches the active filter (case-insensitive).
/// Matches against sensor label, sensor key, chip name, and source name.
///
/// `filter_lc` must already be ASCII-lowercased by the caller so that this
/// function can perform an allocation-free search on each field.
fn sensor_matches_filter(id: &SensorId, reading: &SensorReading, filter_lc: &str) -> bool {
    if filter_lc.is_empty() {
        return true;
    }
    ascii_contains_ignore_case(&reading.label, filter_lc)
        || ascii_contains_ignore_case(&id.chip, filter_lc)
        || ascii_contains_ignore_case(&id.source, filter_lc)
        || ascii_contains_ignore_case(&id.sensor, filter_lc)
}

/// ASCII-only case-insensitive substring search without heap allocation.
///
/// `needle_lc` must be pre-lowercased (ASCII). Matches by comparing one
/// ASCII byte at a time, leaving non-ASCII bytes unchanged so that their
/// original byte offsets are preserved — a property `highlight_match` relies on.
fn ascii_contains_ignore_case(haystack: &str, needle_lc: &str) -> bool {
    let needle = needle_lc.as_bytes();
    let hay = haystack.as_bytes();
    if needle.is_empty() {
        return true;
    }
    if needle.len() > hay.len() {
        return false;
    }
    for i in 0..=(hay.len() - needle.len()) {
        let mut j = 0;
        while j < needle.len() {
            let h = if hay[i + j].is_ascii() {
                hay[i + j].to_ascii_lowercase()
            } else {
                hay[i + j]
            };
            if h != needle[j] {
                break;
            }
            j += 1;
        }
        if j == needle.len() {
            return true;
        }
    }
    false
}

/// Build display rows with 4-level collapsible tree:
///   Level 1: source (yellow bold)        — "hwmon", "cpu", "ipmi", etc.
///   Level 2: chip (white bold)           — "nct6798d", "nvme0", "bmc", etc.
///   Level 3: category (cyan)             — "Temperature", "Voltage", etc.
///   Level 4: individual sensor readings
///
/// When `filter_lc` is non-empty, only groups containing matching sensors are
/// shown and those groups are always expanded regardless of `collapsed`.
///
/// Returns (rows, header_row_indices, collapse_keys).
fn build_rows(
    snapshot: &[(SensorId, SensorReading)],
    collapsed: &HashSet<String>,
    filter_lc: &str,
    history: &SensorHistory,
) -> (Vec<Row<'static>>, Vec<usize>, Vec<String>) {
    let order = sorted_indices(snapshot);
    let summaries = compute_summaries(snapshot);

    let mut rows: Vec<Row<'static>> = Vec::new();
    let mut header_indices = Vec::new();
    let mut collapse_keys = Vec::new();

    let mut cur_source: Option<String> = None;
    let mut cur_chip: Option<String> = None;
    let mut cur_cat: Option<String> = None;

    let source_style = Style::default()
        .fg(Color::Yellow)
        .add_modifier(Modifier::BOLD);
    let chip_style = Style::default()
        .fg(Color::White)
        .add_modifier(Modifier::BOLD);
    let cat_style = Style::default().fg(Color::Cyan);

    for &idx in &order {
        let (id, reading) = &snapshot[idx];

        // Skip sensors that don't match the active filter
        if !sensor_matches_filter(id, reading, filter_lc) {
            continue;
        }

        let sk = &id.source;
        let ck = chip_key(id);
        let catk = cat_key(&ck, reading.category);

        // When a filter is active, override collapse state so matching sensors
        // are always visible.
        let effective_collapsed = |key: &str| -> bool {
            if !filter_lc.is_empty() {
                false
            } else {
                collapsed.contains(key)
            }
        };

        // Level 1: Source header
        if cur_source.as_ref() != Some(sk) {
            cur_source = Some(sk.clone());
            cur_chip = None;
            cur_cat = None;

            let is_collapsed = effective_collapsed(sk);
            let count = summaries.get(sk).map(|s| s.count).unwrap_or(0);
            let arrow = if is_collapsed { "\u{25b6}" } else { "\u{25bc}" };
            let text = format!(
                " {arrow} {sk} ({count} sensor{})",
                if count == 1 { "" } else { "s" }
            );

            header_indices.push(rows.len());
            collapse_keys.push(sk.clone());

            if is_collapsed {
                rows.push(summary_row(text, source_style, summaries.get(sk)));
                continue;
            } else {
                rows.push(header_row(text, source_style));
            }
        }
        if effective_collapsed(sk) {
            continue;
        }

        // Level 2: Chip header
        if cur_chip.as_ref() != Some(&ck) {
            cur_chip = Some(ck.clone());
            cur_cat = None;

            let is_collapsed = effective_collapsed(&ck);
            let count = summaries.get(&ck).map(|s| s.count).unwrap_or(0);
            let arrow = if is_collapsed { "\u{25b6}" } else { "\u{25bc}" };
            let text = format!("   {arrow} {} ({count})", id.chip);

            header_indices.push(rows.len());
            collapse_keys.push(ck.clone());

            if is_collapsed {
                rows.push(summary_row(text, chip_style, summaries.get(&ck)));
                continue;
            } else {
                rows.push(header_row(text, chip_style));
            }
        }
        if effective_collapsed(&ck) {
            continue;
        }

        // Level 3: Category header
        if cur_cat.as_ref() != Some(&catk) {
            cur_cat = Some(catk.clone());

            let is_collapsed = effective_collapsed(&catk);
            let count = summaries.get(&catk).map(|s| s.count).unwrap_or(0);
            let arrow = if is_collapsed { "\u{25b6}" } else { "\u{25bc}" };
            let text = format!("     {arrow} {} ({count})", reading.category);

            header_indices.push(rows.len());
            collapse_keys.push(catk.clone());

            if is_collapsed {
                rows.push(summary_row(text, cat_style, summaries.get(&catk)));
                continue;
            } else {
                rows.push(header_row(text, cat_style));
            }
        }
        if effective_collapsed(&catk) {
            continue;
        }

        // Level 4: Sensor row — highlight matched characters when filter is active
        let precision = format_precision(&reading.unit);
        let style = value_style(reading);
        let label_text = if !filter_lc.is_empty() {
            // Mark matches visually with surrounding brackets
            highlight_match(&reading.label, filter_lc)
        } else {
            format!("       {}", reading.label)
        };

        let key = format!("{}/{}/{}", id.source, id.chip, id.sensor);
        let spark = history
            .data
            .get(&key)
            .map(|buf| sparkline_str(buf, 20))
            .unwrap_or_default();

        let row = Row::new(vec![
            Cell::from(label_text),
            Cell::from(format!("{:.prec$}", reading.current, prec = precision)).style(style),
            Cell::from(format!("{:.prec$}", reading.min, prec = precision)),
            Cell::from(format!("{:.prec$}", reading.max, prec = precision)),
            Cell::from(format!("{:.prec$}", reading.avg, prec = precision)),
            Cell::from(format!("{}", reading.unit)),
            Cell::from(spark).style(Style::default().fg(Color::DarkGray)),
        ]);
        rows.push(row);
    }

    (rows, header_indices, collapse_keys)
}

/// Return the label with the matched substring wrapped in [brackets] for visual
/// emphasis, keeping the 7-space indent used for sensor rows.
///
/// Uses `to_ascii_lowercase` to find the match position so that the resulting
/// byte offset is valid for slicing the original string even when it contains
/// non-ASCII characters (ASCII lowercasing never changes byte length).
fn highlight_match(label: &str, filter_lc: &str) -> String {
    let label_ascii_lc = label.to_ascii_lowercase();
    if let Some(pos) = label_ascii_lc.find(filter_lc) {
        let end = pos + filter_lc.len();
        format!(
            "       {}[{}]{}",
            &label[..pos],
            &label[pos..end],
            &label[end..]
        )
    } else {
        format!("       {label}")
    }
}

/// Bundled display state passed to the draw function.
struct DrawContext<'a> {
    group_indices: &'a [usize],
    cursor: usize,
    scroll_offset: usize,
    sensor_count: usize,
    max_samples: u64,
    collapsed_count: usize,
    elapsed_str: &'a str,
    active_alerts: &'a [String],
    poll_warning: &'a str,
    filter_mode: bool,
    filter_query: &'a str,
}

fn draw(
    terminal: &mut Terminal<CrosstermBackend<Stdout>>,
    rows: Vec<Row<'static>>,
    ctx: &DrawContext<'_>,
) -> io::Result<()> {
    let total_groups = ctx.group_indices.len();

    // Prepare cursor highlight index before moving rows into the closure
    let cursor_row_idx = ctx.group_indices.get(ctx.cursor).copied();

    terminal.draw(|frame| {
        let size = frame.area();

        // When a filter is active (or being typed), show a filter bar above the status bar.
        let show_filter_bar = ctx.filter_mode || !ctx.filter_query.is_empty();
        let filter_bar_height = if show_filter_bar { 1 } else { 0 };

        let constraints = vec![
            Constraint::Length(3),
            Constraint::Min(1),
            Constraint::Length(filter_bar_height),
            Constraint::Length(1),
        ];

        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints(constraints)
            .split(size);

        // Top bar
        let is_root = unsafe { libc::geteuid() } == 0;
        let priv_hint = if is_root {
            ""
        } else {
            " | \u{26a0} run as root for SMART, DMI serials, MSR"
        };
        let title = format!(
            " sio \u{2014} Sensor Monitor | {} sensors | {} groups ({} collapsed) | {}{}",
            ctx.sensor_count, total_groups, ctx.collapsed_count, ctx.elapsed_str, priv_hint
        );
        let header_block = Paragraph::new(title)
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
        frame.render_widget(header_block, chunks[0]);

        // Main table
        let hdr_style = Style::default()
            .fg(Color::White)
            .add_modifier(Modifier::BOLD);
        let table_header = Row::new(vec![
            Cell::from("Label").style(hdr_style),
            Cell::from("Current").style(hdr_style),
            Cell::from("Min").style(hdr_style),
            Cell::from("Max").style(hdr_style),
            Cell::from("Avg").style(hdr_style),
            Cell::from("Unit").style(hdr_style),
            Cell::from("Trend").style(hdr_style),
        ])
        .height(1)
        .bottom_margin(1)
        .style(Style::default().bg(Color::DarkGray));

        // Apply cursor highlight and scrolling
        let visible_rows: Vec<Row> = rows
            .into_iter()
            .enumerate()
            .skip(ctx.scroll_offset)
            .map(|(idx, row)| {
                if Some(idx) == cursor_row_idx {
                    row.style(Style::default().bg(Color::DarkGray))
                } else {
                    row
                }
            })
            .collect();

        let table = Table::new(
            visible_rows,
            [
                Constraint::Min(28),
                Constraint::Length(10),
                Constraint::Length(10),
                Constraint::Length(10),
                Constraint::Length(10),
                Constraint::Length(8),
                Constraint::Length(22),
            ],
        )
        .header(table_header)
        .block(Block::default().borders(Borders::NONE));

        frame.render_widget(table, chunks[1]);

        // Determine status and filter chunk positions
        let last = chunks.len() - 1;
        let (status_chunk, filter_chunk_opt) = if show_filter_bar && last > 0 {
            (chunks[last], Some(chunks[last - 1]))
        } else {
            (chunks[last], None)
        };

        // Filter bar
        if let Some(fc) = filter_chunk_opt {
            render_filter_bar(frame, fc, ctx.filter_mode, ctx.filter_query);
        }

        // Status bar
        render_status_bar(frame, status_chunk, ctx);
    })?;

    Ok(())
}

/// Render the filter input bar.
fn render_filter_bar(
    frame: &mut ratatui::Frame,
    area: ratatui::layout::Rect,
    filter_mode: bool,
    filter_query: &str,
) {
    let filter_text = if filter_mode {
        format!(" / {}\u{2588}", filter_query)
    } else {
        format!(" / {} (Esc to clear)", filter_query)
    };
    let filter_style = if filter_mode {
        Style::default().fg(Color::Black).bg(Color::Yellow)
    } else {
        Style::default().fg(Color::Yellow).bg(Color::DarkGray)
    };
    frame.render_widget(Paragraph::new(filter_text).style(filter_style), area);
}

/// Render the bottom status bar with keybindings, sensor count, and alerts.
fn render_status_bar(
    frame: &mut ratatui::Frame,
    area: ratatui::layout::Rect,
    ctx: &DrawContext<'_>,
) {
    let filter_hint = if ctx.filter_query.is_empty() && !ctx.filter_mode {
        " | /: search"
    } else {
        ""
    };
    let status = if ctx.active_alerts.is_empty() {
        format!(
            " q: quit | \u{2191}\u{2193}: navigate | Enter: toggle | c/e: collapse/expand{} | Sensors: {} | Samples: {}{}",
            filter_hint, ctx.sensor_count, ctx.max_samples, ctx.poll_warning
        )
    } else {
        format!(
            " \u{26a0} {} | {}{}",
            ctx.active_alerts.join(" | "),
            {
                format!(
                    "Sensors: {} | Samples: {}",
                    ctx.sensor_count, ctx.max_samples
                )
            },
            ctx.poll_warning
        )
    };
    let status_style = if ctx.active_alerts.is_empty() {
        Style::default().fg(Color::DarkGray).bg(Color::Black)
    } else {
        Style::default()
            .fg(Color::Yellow)
            .bg(Color::DarkGray)
            .add_modifier(Modifier::BOLD)
    };
    frame.render_widget(Paragraph::new(status).style(status_style), area);
}

pub(crate) fn format_precision(unit: &SensorUnit) -> usize {
    match unit {
        SensorUnit::Celsius
        | SensorUnit::Volts
        | SensorUnit::Millivolts
        | SensorUnit::Watts
        | SensorUnit::Milliwatts
        | SensorUnit::Amps
        | SensorUnit::Milliamps => 1,
        SensorUnit::Rpm | SensorUnit::Mhz | SensorUnit::Percent => 0,
        SensorUnit::BytesPerSec
        | SensorUnit::MegabytesPerSec
        | SensorUnit::Bytes
        | SensorUnit::Megabytes => 1,
        SensorUnit::Unitless => 1,
    }
}

pub(crate) fn value_style(reading: &SensorReading) -> Style {
    match reading.category {
        SensorCategory::Temperature => {
            if reading.current > 80.0 {
                Style::default().fg(Color::Red).add_modifier(Modifier::BOLD)
            } else if reading.current >= 60.0 {
                Style::default().fg(Color::Yellow)
            } else {
                Style::default().fg(Color::Green)
            }
        }
        SensorCategory::Fan => {
            if reading.current == 0.0 {
                Style::default().fg(Color::Red).add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::Cyan)
            }
        }
        SensorCategory::Power => Style::default().fg(Color::Magenta),
        SensorCategory::Voltage => Style::default().fg(Color::Blue),
        SensorCategory::Frequency => Style::default().fg(Color::Cyan),
        SensorCategory::Utilization => {
            if reading.current > 90.0 {
                Style::default().fg(Color::Red)
            } else if reading.current > 70.0 {
                Style::default().fg(Color::Yellow)
            } else {
                Style::default().fg(Color::Green)
            }
        }
        _ => Style::default(),
    }
}

fn format_elapsed(d: Duration) -> String {
    let total_secs = d.as_secs();
    let hours = total_secs / 3600;
    let minutes = (total_secs % 3600) / 60;
    let seconds = total_secs % 60;

    if hours > 0 {
        format!("{hours}h {minutes:02}m {seconds:02}s")
    } else if minutes > 0 {
        format!("{minutes}m {seconds:02}s")
    } else {
        format!("{seconds}s")
    }
}
