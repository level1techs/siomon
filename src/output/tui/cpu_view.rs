//! Dedicated CPU detail view for the TUI.
//!
//! A per-core table (effective clock, active clock and time, ratio,
//! requested VID, temperature, thermal headroom, throttling) with
//! P-core/E-core min/avg/max rows, next to panels for clocks, voltages,
//! power, thermals, memory and throttling. Built from the `intel_msr`,
//! `intel_uncore`, `intel_mchbar`, `intel_gpu`, RAPL and hwmon readings plus
//! the trained memory-controller info.
//!
//! VIDs are shown as requests and board-sensed rails as measurements, in
//! separate groups, because they are different quantities.

use std::collections::HashMap;
use std::io::{self, Stdout};
use std::path::Path;

use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Cell, Paragraph, Row, Table};

use crate::model::memory::MemoryInfo;
use crate::model::sensor::{SensorCategory, SensorId, SensorReading};

use super::SystemSummary;
use super::theme::TuiTheme;

/// Hybrid core type, taken from the `P-Core`/`E-Core` label prefix the
/// MSR source emits.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum CoreKind {
    Performance,
    Efficient,
    Uniform,
}

impl CoreKind {
    fn from_label(label: &str) -> Self {
        if label.starts_with("P-Core") {
            Self::Performance
        } else if label.starts_with("E-Core") {
            Self::Efficient
        } else {
            Self::Uniform
        }
    }

    fn short(self) -> &'static str {
        match self {
            Self::Performance => "P",
            Self::Efficient => "E",
            Self::Uniform => "",
        }
    }
}

/// One logical CPU's row.
#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) struct CoreRow {
    pub cpu: u32,
    pub kind: Option<CoreKind>,
    pub eff_mhz: Option<f64>,
    pub active_mhz: Option<f64>,
    pub active_pct: Option<f64>,
    pub ratio: Option<f64>,
    pub vid: Option<f64>,
    pub temp: Option<f64>,
    pub headroom: Option<f64>,
    pub throttling: Option<bool>,
}

/// Readings keyed by `source/chip/sensor` for direct lookup.
pub(crate) struct Index<'a> {
    map: HashMap<String, &'a SensorReading>,
}

impl<'a> Index<'a> {
    pub(crate) fn new(snapshot: &'a [(SensorId, SensorReading)]) -> Self {
        Self {
            map: snapshot.iter().map(|(id, r)| (id.to_string(), r)).collect(),
        }
    }

    fn get(&self, key: &str) -> Option<&'a SensorReading> {
        self.map.get(key).copied()
    }

    fn value(&self, key: &str) -> Option<f64> {
        self.get(key).map(|r| r.current)
    }

    fn has_prefix(&self, prefix: &str) -> bool {
        self.map.keys().any(|k| k.starts_with(prefix))
    }
}

/// Per-CPU chips emitted by the MSR source.
const CORE_CHIPS: &[&str] = &[
    "eff_clock",
    "active_clock",
    "active",
    "ratio",
    "vid",
    "headroom",
    "throttling",
];

/// Build per-core rows from the snapshot, sorted by CPU number.
pub(crate) fn core_rows(snapshot: &[(SensorId, SensorReading)]) -> Vec<CoreRow> {
    let idx = Index::new(snapshot);
    let throttle_temp = idx.value("cpu/package0/throttle_temp");
    let mut cpus: Vec<(u32, Option<CoreKind>)> = Vec::new();
    for (id, r) in snapshot {
        if id.source != "cpu" || !CORE_CHIPS.contains(&id.chip.as_str()) {
            continue;
        }
        let Some(cpu) = id.sensor.strip_prefix("cpu").and_then(|n| n.parse().ok()) else {
            continue;
        };
        if !cpus.iter().any(|(c, _)| *c == cpu) {
            cpus.push((cpu, Some(CoreKind::from_label(&r.label))));
        }
    }
    cpus.sort_unstable();

    cpus.into_iter()
        .map(|(cpu, kind)| {
            let v = |chip: &str| idx.value(&format!("cpu/{chip}/cpu{cpu}"));
            let headroom = v("headroom");
            CoreRow {
                cpu,
                kind,
                eff_mhz: v("eff_clock"),
                active_mhz: v("active_clock"),
                active_pct: v("active"),
                ratio: v("ratio"),
                vid: v("vid"),
                // Throttle temperature minus headroom is the core temperature.
                temp: throttle_temp.zip(headroom).map(|(t, h)| t - h),
                headroom,
                throttling: v("throttling").map(|t| t != 0.0),
            }
        })
        .collect()
}

/// Min / average / max of the present values.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Stats {
    pub min: f64,
    pub avg: f64,
    pub max: f64,
}

pub(crate) fn stats(values: impl Iterator<Item = Option<f64>>) -> Option<Stats> {
    let vals: Vec<f64> = values.flatten().filter(|v| v.is_finite()).collect();
    if vals.is_empty() {
        return None;
    }
    let min = vals.iter().copied().fold(f64::INFINITY, f64::min);
    let max = vals.iter().copied().fold(f64::NEG_INFINITY, f64::max);
    let avg = vals.iter().sum::<f64>() / vals.len() as f64;
    Some(Stats { min, avg, max })
}

/// Throttling summary lines for a package: `(reason, cores)` for every
/// reason with at least one core affected.
pub(crate) fn throttle_summary(idx: &Index) -> Option<Vec<(&'static str, u32)>> {
    let mut any = false;
    let mut out = Vec::new();
    for (sensor, reason) in [
        ("throttle_thermal", "Thermal"),
        ("throttle_power", "Power limit"),
        ("throttle_current", "Current limit"),
    ] {
        if let Some(cores) = idx.value(&format!("cpu/package0/{sensor}")) {
            any = true;
            if cores > 0.0 {
                out.push((reason, cores as u32));
            }
        }
    }
    any.then_some(out)
}

fn fmt(v: Option<f64>, prec: usize) -> String {
    v.map(|v| format!("{v:.prec$}"))
        .unwrap_or_else(|| "-".into())
}

fn temp_style(theme: &TuiTheme, t: Option<f64>) -> Style {
    match t {
        Some(t) if t > 80.0 => Style::default().fg(theme.crit).add_modifier(Modifier::BOLD),
        Some(t) if t >= 60.0 => Style::default().fg(theme.warn),
        Some(_) => Style::default().fg(theme.good),
        None => theme.muted_style(),
    }
}

/// A labelled value line for the side panels.
fn kv<'a>(theme: &TuiTheme, label: &str, value: String, style: Style) -> Line<'a> {
    Line::from(vec![
        Span::styled(format!("{label:<20}"), theme.label_style()),
        Span::styled(value, style),
    ])
}

fn heading<'a>(theme: &TuiTheme, text: &str) -> Line<'a> {
    Line::from(Span::styled(
        text.to_string(),
        theme.muted_style().add_modifier(Modifier::BOLD),
    ))
}

fn note<'a>(theme: &TuiTheme, text: &str) -> Line<'a> {
    Line::from(Span::styled(text.to_string(), theme.muted_style()))
}

struct Panel<'a> {
    title: &'static str,
    lines: Vec<Line<'a>>,
}

/// Explain missing uncore perf readings when the PMU exists: no permission,
/// or another perf session is time-sharing the counter.
fn uncore_hint<'a>(
    idx: &Index,
    what: &str,
    prefix: &str,
    pmu: &str,
    theme: &TuiTheme,
) -> Option<Line<'a>> {
    let pmu_exists = Path::new("/sys/bus/event_source/devices")
        .join(pmu)
        .exists();
    (!idx.has_prefix(prefix) && pmu_exists)
        .then(|| note(theme, &format!("{what}: needs root or counters busy")))
}

fn clocks_panel<'a>(idx: &Index, rows: &[CoreRow], theme: &TuiTheme) -> Panel<'a> {
    let mut lines = Vec::new();
    let freq = theme.info_style();
    for (kind, name) in [
        (CoreKind::Performance, "P-core eff. clock"),
        (CoreKind::Efficient, "E-core eff. clock"),
    ] {
        if let Some(s) = stats(
            rows.iter()
                .filter(|r| r.kind == Some(kind))
                .map(|r| r.eff_mhz),
        ) {
            lines.push(kv(theme, name, format!("{:.0} MHz", s.avg), freq));
        }
    }
    if let Some(v) = idx.value("uncore/clocks/ring_clock") {
        lines.push(kv(theme, "Ring/LLC clock", format!("{v:.0} MHz"), freq));
    }
    if let (Some(lo), Some(hi)) = (
        idx.value("uncore/ring_limits/ring_clock_min"),
        idx.value("uncore/ring_limits/ring_clock_max"),
    ) {
        lines.push(kv(
            theme,
            "Ring clock range",
            format!("{lo:.0}-{hi:.0} MHz"),
            freq,
        ));
    }
    lines.extend(uncore_hint(
        idx,
        "Ring clock",
        "uncore/clocks/",
        "uncore_cncu",
        theme,
    ));
    for (key, label) in [
        ("mchbar/arl/bclk", "CPU BCLK"),
        ("mchbar/arl/soc_bclk", "SoC BCLK"),
    ] {
        if let Some(v) = idx.value(key) {
            lines.push(kv(theme, label, format!("{v:.3} MHz"), freq));
        }
    }
    let mut gts: Vec<&str> = idx
        .map
        .keys()
        .filter_map(|k| k.strip_prefix("intel_gpu/")?.strip_suffix("/act_freq"))
        .collect();
    gts.sort_unstable();
    for gt in gts {
        let Some(clock) = idx.get(&format!("intel_gpu/{gt}/act_freq")) else {
            continue;
        };
        let name = clock.label.trim_end_matches(" Clock");
        let value = format!("{:.0} MHz", clock.current);
        let idle = idx
            .value(&format!("intel_gpu/{gt}/idle"))
            .map(|p| format!(", {p:.0}% idle"))
            .unwrap_or_default();
        lines.push(kv(theme, name, format!("{value}{idle}"), freq));
    }
    Panel {
        title: "Clocks",
        lines,
    }
}

fn voltage_panel<'a>(
    idx: &Index,
    snapshot: &'a [(SensorId, SensorReading)],
    rows: &[CoreRow],
    theme: &TuiTheme,
) -> Panel<'a> {
    let mut lines = Vec::new();
    let volt = theme.voltage_style();

    let mut rails: Vec<&(SensorId, SensorReading)> = snapshot
        .iter()
        .filter(|(id, r)| id.source == "hwmon" && r.category == SensorCategory::Voltage)
        .collect();
    rails.sort_by(|(a, _), (b, _)| a.natural_cmp(b));
    if !rails.is_empty() {
        lines.push(heading(theme, "Board sense (measured)"));
        for (_, r) in rails {
            lines.push(kv(theme, &r.label, format!("{:.3} V", r.current), volt));
        }
    }

    let mut requested = Vec::new();
    for (kind, name) in [
        (CoreKind::Performance, "P-core VID max"),
        (CoreKind::Efficient, "E-core VID max"),
        (CoreKind::Uniform, "Core VID max"),
    ] {
        if let Some(s) = stats(rows.iter().filter(|r| r.kind == Some(kind)).map(|r| r.vid)) {
            requested.push(kv(theme, name, format!("{:.3} V", s.max), volt));
        }
    }
    for (key, label) in [
        ("cpu/package0/uncore_vid", "Uncore VID"),
        ("mchbar/arl/sa_vid", "SA VID"),
    ] {
        if let Some(v) = idx.value(key) {
            requested.push(kv(theme, label, format!("{v:.3} V"), volt));
        }
    }
    if !requested.is_empty() {
        lines.push(heading(theme, "Requested by the CPU (VID)"));
        lines.extend(requested);
    }
    Panel {
        title: "Voltage",
        lines,
    }
}

fn power_panel<'a>(idx: &Index, theme: &TuiTheme) -> Panel<'a> {
    let mut lines = Vec::new();
    let power = theme.power_style();
    for (key, label) in [
        ("cpu/rapl/package-0", "Package"),
        ("cpu/rapl/core-0", "CPU cores (IA)"),
        ("cpu/rapl/uncore-0", "iGPU (GT)"),
    ] {
        if let Some(v) = idx.value(key) {
            lines.push(kv(theme, label, format!("{v:.1} W"), power));
        }
    }
    let mut limits = Vec::new();
    for (key, label) in [
        ("cpu/rapl_limits/package-0-pl1", "PL1 (long term)"),
        ("cpu/rapl_limits/package-0-pl2", "PL2 (short term)"),
        ("cpu/rapl_limits/package-0-pl4", "PL4 (powercap)"),
        ("cpu/rapl_limits/package-0-tdp", "Base TDP"),
    ] {
        if let Some(v) = idx.value(key) {
            limits.push(kv(theme, label, format!("{v:.0} W"), theme.info_style()));
        }
    }
    if let Some(w) = idx.value("cpu/package0/pl4") {
        // Read from MSR 0x601 (Arrow Lake-S): the enforced PL4. Powercap's
        // "PL4 (powercap)" row, when present, keeps its own label.
        limits.push(kv(
            theme,
            "PL4 (MSR, enforced)",
            format!("{w:.0} W"),
            theme.info_style(),
        ));
    }
    if !limits.is_empty() {
        lines.push(heading(theme, "Configured limits"));
        lines.extend(limits);
    }
    Panel {
        title: "Power",
        lines,
    }
}

fn thermal_panel<'a>(
    idx: &Index,
    snapshot: &[(SensorId, SensorReading)],
    rows: &[CoreRow],
    theme: &TuiTheme,
) -> Panel<'a> {
    let mut lines = Vec::new();
    let mut push = |label: &str, t: Option<f64>| {
        if t.is_some() {
            lines.push(kv(
                theme,
                label,
                format!("{} °C", fmt(t, 1)),
                temp_style(theme, t),
            ));
        }
    };
    let pkg = snapshot
        .iter()
        .find(|(id, r)| {
            id.source == "hwmon"
                && id.chip.starts_with("coretemp")
                && r.label.starts_with("Package")
        })
        .map(|(_, r)| r.current);
    push("Package", pkg);
    push(
        "Hottest core",
        stats(rows.iter().map(|r| r.temp)).map(|s| s.max),
    );
    push("CPU cores (IA)", idx.value("mchbar/arl/ia_temp"));
    push("iGPU (GT)", idx.value("mchbar/arl/gt_temp"));
    let peci = snapshot
        .iter()
        .find(|(id, r)| {
            id.source == "hwmon"
                && r.category == SensorCategory::Temperature
                && r.label.contains("PECI")
        })
        .map(|(_, r)| r.current);
    push("Board CPU (PECI)", peci);

    if let Some(t) = idx.value("cpu/package0/throttle_temp") {
        lines.push(kv(
            theme,
            "Throttles at",
            format!("{t:.0} °C"),
            theme.info_style(),
        ));
    }
    if let Some(h) = idx.value("cpu/package0/headroom") {
        lines.push(kv(
            theme,
            "Package headroom",
            format!("{h:.0} °C"),
            theme.info_style(),
        ));
    }
    Panel {
        title: "Thermal",
        lines,
    }
}

fn memory_panel<'a>(idx: &Index, memory: &MemoryInfo, theme: &TuiTheme) -> Panel<'a> {
    let mut lines = Vec::new();
    if let Some(mc) = &memory.controller {
        let t = &mc.timings;
        let rate = mc
            .data_rate_mts
            .map(|r| format!("DDR5-{r}"))
            .unwrap_or_else(|| "DDR5".into());
        let info = theme.info_style();
        lines.push(kv(
            theme,
            "Mode",
            format!("{rate} Gear {} {}N", mc.gear, mc.command_rate),
            info,
        ));
        lines.push(kv(
            theme,
            "tCL-tRCD-tRP-tRAS",
            format!("{}-{}-{}-{}", t.tcl, t.trcd, t.trp, t.tras),
            info,
        ));
        lines.push(kv(
            theme,
            "tRC tRFC tREFI",
            format!("{} {} {}", t.trc, t.trfc, t.trefi),
            info,
        ));
        lines.push(kv(
            theme,
            "tRRD L/S tFAW",
            format!("{}/{} {}", t.trrd_l, t.trrd_s, t.tfaw),
            info,
        ));
        lines.push(kv(
            theme,
            "tCWL tWR tRTP",
            format!("{} {} {}", t.tcwl, t.twr, t.trtp),
            info,
        ));
    }
    for (key, label) in [
        ("uncore/imc/data_read", "DRAM read"),
        ("uncore/imc/data_write", "DRAM write"),
        ("uncore/imc/data_total", "DRAM total"),
    ] {
        if let Some(v) = idx.value(key) {
            lines.push(kv(theme, label, format!("{v:.0} MB/s"), theme.good_style()));
        }
    }
    lines.extend(uncore_hint(
        idx,
        "DRAM bandwidth",
        "uncore/imc/",
        "uncore_imc_free_running_0",
        theme,
    ));
    Panel {
        title: "Memory",
        lines,
    }
}

fn throttling_panel<'a>(idx: &Index, theme: &TuiTheme) -> Panel<'a> {
    let mut lines = Vec::new();
    match throttle_summary(idx) {
        Some(reasons) if reasons.is_empty() => {
            lines.push(Line::from(Span::styled("None", theme.good_style())));
        }
        Some(reasons) => {
            let power_limited = reasons.iter().any(|(r, _)| *r == "Power limit");
            let current_limited = reasons.iter().any(|(r, _)| *r == "Current limit");
            for (reason, cores) in reasons {
                let plural = if cores == 1 { "core" } else { "cores" };
                lines.push(kv(
                    theme,
                    reason,
                    format!("{cores} {plural}"),
                    theme.warn_style(),
                ));
            }
            if power_limited {
                lines.push(note(
                    theme,
                    "Power limit: below the requested clock (a power limit)",
                ));
            }
            if current_limited {
                lines.push(note(
                    theme,
                    "Current limit: an electrical limit, or on Arrow Lake-S also PL4",
                ));
            }
        }
        None => {}
    }
    Panel {
        title: "Throttling now",
        lines,
    }
}

fn kind_counts(rows: &[CoreRow]) -> String {
    let p = rows
        .iter()
        .filter(|r| r.kind == Some(CoreKind::Performance))
        .count();
    let e = rows
        .iter()
        .filter(|r| r.kind == Some(CoreKind::Efficient))
        .count();
    if p + e > 0 {
        format!("{p}P + {e}E")
    } else {
        format!("{} CPUs", rows.len())
    }
}

fn core_table_rows<'a>(rows: &[CoreRow], theme: &TuiTheme) -> Vec<Row<'a>> {
    let mut out = Vec::new();
    let freq = theme.info_style();
    let volt = theme.voltage_style();

    // P/E summary rows first, as in turbostat's summary line.
    for kind in [
        CoreKind::Performance,
        CoreKind::Efficient,
        CoreKind::Uniform,
    ] {
        let group: Vec<&CoreRow> = rows.iter().filter(|r| r.kind == Some(kind)).collect();
        if group.is_empty() {
            continue;
        }
        for (which, pick) in [("min", 0usize), ("avg", 1usize), ("max", 2usize)] {
            let col = |f: fn(&CoreRow) -> Option<f64>| {
                stats(group.iter().map(|r| f(r))).map(|s| [s.min, s.avg, s.max][pick])
            };
            let name = match kind {
                CoreKind::Uniform => format!("all {which}"),
                _ => format!("{}-core {which}", kind.short()),
            };
            let temp = col(|r| r.temp);
            out.push(
                Row::new(vec![
                    Cell::from(name).style(theme.accent_style()),
                    Cell::from(fmt(col(|r| r.eff_mhz), 0)).style(freq),
                    Cell::from(fmt(col(|r| r.active_mhz), 0)).style(freq),
                    Cell::from(fmt(col(|r| r.active_pct), 1)),
                    Cell::from(fmt(col(|r| r.ratio), 0)),
                    Cell::from(fmt(col(|r| r.vid), 3)).style(volt),
                    Cell::from(fmt(temp, 0)).style(temp_style(theme, temp)),
                    Cell::from(fmt(col(|r| r.headroom), 0)),
                    Cell::from(""),
                ])
                .style(Style::default().add_modifier(Modifier::BOLD)),
            );
        }
    }

    for r in rows {
        let kind = r.kind.map(CoreKind::short).unwrap_or("");
        let throttling = match r.throttling {
            Some(true) => Cell::from("yes").style(theme.warn_style()),
            Some(false) => Cell::from("no"),
            None => Cell::from(""),
        };
        out.push(Row::new(vec![
            Cell::from(format!("{kind:<1} cpu{}", r.cpu)).style(theme.label_style()),
            Cell::from(fmt(r.eff_mhz, 0)).style(freq),
            Cell::from(fmt(r.active_mhz, 0)).style(freq),
            Cell::from(fmt(r.active_pct, 1)),
            Cell::from(fmt(r.ratio, 0)),
            Cell::from(fmt(r.vid, 3)).style(volt),
            Cell::from(fmt(r.temp, 0)).style(temp_style(theme, r.temp)),
            Cell::from(fmt(r.headroom, 0)),
            throttling,
        ]));
    }
    out
}

fn render_panels(frame: &mut ratatui::Frame, area: Rect, panels: &[Panel<'_>], theme: &TuiTheme) {
    let panels: Vec<&Panel<'_>> = panels.iter().filter(|p| !p.lines.is_empty()).collect();
    if panels.is_empty() {
        return;
    }
    let constraints: Vec<Constraint> = panels
        .iter()
        .map(|p| Constraint::Length(p.lines.len() as u16 + 2))
        .chain(std::iter::once(Constraint::Min(0)))
        .collect();
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints(constraints)
        .split(area);
    for (panel, chunk) in panels.iter().zip(chunks.iter()) {
        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(theme.border_style())
            .title(Span::styled(
                format!(" {} ", panel.title),
                theme.accent_style(),
            ));
        frame.render_widget(Paragraph::new(panel.lines.clone()).block(block), *chunk);
    }
}

fn core_table<'a>(rows: &[CoreRow], theme: &TuiTheme) -> Table<'a> {
    let header = Row::new(vec![
        "CPU",
        "Eff MHz",
        "Act MHz",
        "Active%",
        "Ratio",
        "VID",
        "°C",
        "Headroom",
        "Throttling",
    ])
    .style(theme.label_style().add_modifier(Modifier::BOLD));
    let widths = [
        Constraint::Length(10),
        Constraint::Length(8),
        Constraint::Length(8),
        Constraint::Length(8),
        Constraint::Length(6),
        Constraint::Length(7),
        Constraint::Length(5),
        Constraint::Length(9),
        Constraint::Min(10),
    ];
    Table::new(core_table_rows(rows, theme), widths)
        .header(header)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(theme.border_style())
                .title(Span::styled(
                    " Cores (Eff = average incl. idle, Act = while active, VID = requested) ",
                    theme.accent_style(),
                )),
        )
}

/// Rows the core table needs: summary rows, one per CPU, header, borders.
fn core_table_height(rows: &[CoreRow]) -> u16 {
    let kinds = [
        CoreKind::Performance,
        CoreKind::Efficient,
        CoreKind::Uniform,
    ]
    .iter()
    .filter(|k| rows.iter().any(|r| r.kind == Some(**k)))
    .count();
    (kinds * 3 + rows.len() + 3) as u16
}

/// Render panels alternately into two side-by-side columns.
fn render_two_columns<'a>(
    frame: &mut ratatui::Frame,
    area: Rect,
    panels: Vec<Panel<'a>>,
    theme: &TuiTheme,
) {
    let cols = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
        .split(area);
    let (left, right): (Vec<_>, Vec<_>) = panels
        .into_iter()
        .filter(|p| !p.lines.is_empty())
        .enumerate()
        .partition(|(i, _)| i % 2 == 0);
    let strip = |v: Vec<(usize, Panel<'a>)>| v.into_iter().map(|(_, p)| p).collect::<Vec<_>>();
    render_panels(frame, cols[0], &strip(left), theme);
    render_panels(frame, cols[1], &strip(right), theme);
}

pub fn render(
    terminal: &mut Terminal<CrosstermBackend<Stdout>>,
    snapshot: &[(SensorId, SensorReading)],
    memory_info: &MemoryInfo,
    sys: &SystemSummary,
    elapsed_str: &str,
    theme: &TuiTheme,
) -> io::Result<()> {
    let rows = core_rows(snapshot);
    let idx = Index::new(snapshot);

    terminal.draw(|frame| {
        let size = frame.area();
        let outer = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(3),
                Constraint::Min(1),
                Constraint::Length(1),
            ])
            .split(size);

        let header = Paragraph::new(format!(
            " CPU Detail | {} | {} | {elapsed_str}",
            sys.cpu_model,
            kind_counts(&rows)
        ))
        .style(theme.accent_style())
        .block(
            Block::default()
                .borders(Borders::BOTTOM)
                .border_style(theme.border_style()),
        );
        frame.render_widget(header, outer[0]);

        let status = Paragraph::new(
            " q: quit | d: dashboard | m: DIMM | p: this view | t: theme | /: search",
        )
        .style(theme.status_style());
        frame.render_widget(status, outer[2]);

        let main = outer[1];
        if rows.is_empty() {
            let msg = Paragraph::new(
                " No per-core MSR data: run as root (or with CAP_SYS_RAWIO) with the msr module loaded",
            )
            .style(theme.label_style());
            frame.render_widget(msg, main);
            return;
        }

        let mut panels = vec![
            clocks_panel(&idx, &rows, theme),
            voltage_panel(&idx, snapshot, &rows, theme),
            power_panel(&idx, theme),
            thermal_panel(&idx, snapshot, &rows, theme),
            memory_panel(&idx, memory_info, theme),
            throttling_panel(&idx, theme),
        ];

        // Wide: table with Thermal/Memory/Throttling under it, and Clocks,
        // Voltage and Power in a side column. Narrow: everything stacked.
        let (left_col, side) = if main.width >= 130 {
            let cols = Layout::default()
                .direction(Direction::Horizontal)
                .constraints([Constraint::Min(80), Constraint::Length(50)])
                .split(main);
            (cols[0], Some(cols[1]))
        } else {
            (main, None)
        };
        let split = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(core_table_height(&rows)),
                Constraint::Min(0),
            ])
            .split(left_col);
        frame.render_widget(core_table(&rows, theme), split[0]);

        if let Some(side) = side {
            let below = panels.split_off(3);
            render_panels(frame, side, &panels, theme);
            render_two_columns(frame, split[1], below, theme);
        } else {
            render_two_columns(frame, split[1], panels, theme);
        }
    })?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::sensor::SensorUnit;

    fn reading(
        source: &str,
        chip: &str,
        sensor: &str,
        label: &str,
        value: f64,
    ) -> (SensorId, SensorReading) {
        (
            SensorId {
                source: source.into(),
                chip: chip.into(),
                sensor: sensor.into(),
            },
            SensorReading::new(
                label.into(),
                value,
                SensorUnit::Unitless,
                SensorCategory::Other,
            ),
        )
    }

    fn sample() -> Vec<(SensorId, SensorReading)> {
        vec![
            reading(
                "cpu",
                "package0",
                "throttle_temp",
                "Throttle Temperature",
                105.0,
            ),
            reading("cpu", "vid", "cpu0", "P-Core 0 VID", 1.386),
            reading("cpu", "vid", "cpu1", "P-Core 1 VID", 1.271),
            reading("cpu", "vid", "cpu8", "E-Core 8 VID", 0.775),
            reading("cpu", "headroom", "cpu0", "P-Core 0 Thermal Headroom", 71.0),
            reading("cpu", "eff_clock", "cpu8", "E-Core 8 Effective Clock", 59.2),
            reading("cpu", "throttling", "cpu1", "P-Core 1 Throttling", 1.0),
            reading("cpu", "package0", "throttle_thermal", "Cores Thermal", 0.0),
            reading("cpu", "package0", "throttle_power", "Cores Power", 8.0),
            reading("cpu", "package0", "throttle_current", "Cores Current", 1.0),
        ]
    }

    #[test]
    fn test_core_rows_sorted_with_kinds() {
        let rows = core_rows(&sample());
        assert_eq!(rows.iter().map(|r| r.cpu).collect::<Vec<_>>(), [0, 1, 8]);
        assert_eq!(rows[0].kind, Some(CoreKind::Performance));
        assert_eq!(rows[2].kind, Some(CoreKind::Efficient));
        // Temperature is the throttle temperature minus the headroom
        assert_eq!(rows[0].temp, Some(34.0));
        assert_eq!(rows[1].temp, None);
        assert_eq!(rows[1].throttling, Some(true));
        assert_eq!(rows[0].throttling, None);
        assert_eq!(rows[2].eff_mhz, Some(59.2));
    }

    #[test]
    fn test_throttle_summary_names_reasons_with_cores() {
        let snap = sample();
        let idx = Index::new(&snap);
        assert_eq!(
            throttle_summary(&idx),
            Some(vec![("Power limit", 8), ("Current limit", 1)])
        );
        let empty: Vec<(SensorId, SensorReading)> = Vec::new();
        assert_eq!(throttle_summary(&Index::new(&empty)), None);
    }

    #[test]
    fn test_stats_skip_missing() {
        let s = stats([Some(1.0), None, Some(3.0)].into_iter()).unwrap();
        assert_eq!((s.min, s.avg, s.max), (1.0, 2.0, 3.0));
        assert!(stats([None, None].into_iter()).is_none());
    }

    #[test]
    fn test_kind_counts() {
        let rows = core_rows(&sample());
        assert_eq!(kind_counts(&rows), "2P + 1E");
    }
}
