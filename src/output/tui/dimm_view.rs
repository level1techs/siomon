//! Dedicated DIMM information view for the TUI.
//!
//! Shows per-DIMM SPD data (timings, manufacturer, density) alongside
//! live temperatures from Hub/TS0/TS1 sensors with sparklines, and the
//! memory controller's trained configuration when it was read
//! (`--direct-io` on supported Intel platforms).

use std::io::{self, Stdout};

use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;
use ratatui::layout::{Constraint, Direction, Layout};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph, Row, Table};

use crate::model::memory::{MemoryControllerInfo, MemoryInfo};
use crate::model::sensor::{SensorId, SensorReading};

use super::theme::TuiTheme;
use super::{SensorHistory, sparkline_str};

pub fn render(
    terminal: &mut Terminal<CrosstermBackend<Stdout>>,
    memory_info: &MemoryInfo,
    snapshot: &[(SensorId, SensorReading)],
    history: &SensorHistory,
    elapsed_str: &str,
    theme: &TuiTheme,
) -> io::Result<()> {
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

        // Header
        let dimm_count = memory_info.dimms.len();
        let total_gb = memory_info.total_bytes / (1024 * 1024 * 1024);
        let trained = memory_info
            .controller
            .as_ref()
            .map(|mc| format!(" | {}", mode_summary(mc)))
            .unwrap_or_default();
        let header = Paragraph::new(format!(
            " DDR5 DIMM Details | {dimm_count} DIMMs | {total_gb} GB{trained} | {elapsed_str}"
        ))
        .style(theme.accent_style())
        .block(
            Block::default()
                .borders(Borders::BOTTOM)
                .border_style(theme.border_style()),
        );
        frame.render_widget(header, outer[0]);

        // Status bar
        let status = Paragraph::new(" q: quit | d: dashboard | p: CPU | /: search | m: this view")
            .style(theme.status_style());
        frame.render_widget(status, outer[2]);

        // Main content area: DIMM table, then the trained controller panel
        let controller = controller_lines(memory_info.controller.as_ref(), theme);
        // Size the table to its rows (header + borders) so the controller
        // panel sits right under it.
        let table_height = if memory_info.dimms.is_empty() {
            1
        } else {
            memory_info.dimms.len() as u16 + 3
        };
        let split = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(table_height),
                Constraint::Length(controller.len() as u16 + 2),
                Constraint::Min(0),
            ])
            .split(outer[1]);
        let main_area = split[0];
        if !controller.is_empty() {
            let title = memory_info
                .controller
                .as_ref()
                .map(|mc| format!(" Memory Controller, trained ({}) ", mc.source))
                .unwrap_or_else(|| " Memory Controller ".into());
            let panel = Paragraph::new(controller).block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_style(theme.border_style())
                    .title(title),
            );
            frame.render_widget(panel, split[1]);
        }

        if memory_info.dimms.is_empty() {
            let msg = Paragraph::new(" No DIMM data available").style(theme.label_style());
            frame.render_widget(msg, main_area);
            return;
        }

        // Build DIMM table rows
        let spark_width = 12;
        let rows: Vec<Row<'_>> = memory_info
            .dimms
            .iter()
            .map(|dimm| {
                let channel = dimm.bank_locator.as_deref().unwrap_or("?");

                let size_gb = dimm.size_bytes / (1024 * 1024 * 1024);

                // SPD timing string
                let timing_str = dimm
                    .spd
                    .as_ref()
                    .map(|spd| {
                        let taa = spd.t_aa_ns.map(|v| format!("{v:.1}")).unwrap_or_default();
                        let trcd = spd.t_rcd_ns.map(|v| format!("{v:.1}")).unwrap_or_default();
                        let trp = spd.t_rp_ns.map(|v| format!("{v:.1}")).unwrap_or_default();
                        let tras = spd.t_ras_ns.map(|v| format!("{v:.1}")).unwrap_or_default();
                        format!("{taa}/{trcd}/{trp}/{tras}")
                    })
                    .unwrap_or_else(|| "-".into());

                // CAS latencies
                let cas_str = dimm
                    .spd
                    .as_ref()
                    .map(|spd| {
                        if spd.cas_latencies.is_empty() {
                            "-".into()
                        } else {
                            let first = spd.cas_latencies.first().unwrap();
                            let last = spd.cas_latencies.last().unwrap();
                            format!("CL{first}-{last}")
                        }
                    })
                    .unwrap_or_else(|| "-".into());

                // Die info
                let die_str = dimm
                    .spd
                    .as_ref()
                    .map(|spd| format!("{}Gb x{}", spd.die_density_gb, spd.device_width))
                    .unwrap_or_else(|| "-".into());

                // Manufacturer
                let mfg = dimm
                    .spd
                    .as_ref()
                    .and_then(|s| s.spd_manufacturer.as_deref())
                    .or(dimm.manufacturer.as_deref())
                    .unwrap_or("?");

                // Part number
                let part = dimm
                    .spd
                    .as_ref()
                    .and_then(|s| s.spd_part_number.as_deref())
                    .or(dimm.part_number.as_deref())
                    .unwrap_or("?");

                // Live temperatures — match by I2C bus/addr from SPD enrichment.
                let i2c_key = dimm
                    .spd
                    .as_ref()
                    .and_then(|spd| Some((spd.i2c_bus?, spd.i2c_addr?)));
                let hub_temp = find_dimm_temp(snapshot, i2c_key, "hub");
                let ts0_temp = find_dimm_temp(snapshot, i2c_key, "ts0");
                let ts1_temp = find_dimm_temp(snapshot, i2c_key, "ts1");

                let hub_str = format_temp_with_spark(
                    hub_temp,
                    snapshot,
                    history,
                    i2c_key,
                    "hub",
                    spark_width,
                );
                let ts0_str = format_temp_with_spark(
                    ts0_temp,
                    snapshot,
                    history,
                    i2c_key,
                    "ts0",
                    spark_width,
                );
                let ts1_str = format_temp_with_spark(
                    ts1_temp,
                    snapshot,
                    history,
                    i2c_key,
                    "ts1",
                    spark_width,
                );

                let temp_style = |t: Option<f64>| match t {
                    Some(v) if v >= 80.0 => Style::default().fg(Color::Red),
                    Some(v) if v >= 60.0 => Style::default().fg(Color::Yellow),
                    Some(_) => Style::default().fg(Color::Green),
                    None => theme.muted_style(),
                };

                Row::new(vec![
                    ratatui::widgets::Cell::from(Line::from(Span::styled(
                        format!("{:<18}", truncate(channel, 18)),
                        theme.label_style(),
                    ))),
                    ratatui::widgets::Cell::from(Line::from(Span::styled(
                        format!("{:>4}GB", size_gb),
                        Style::default().fg(Color::White),
                    ))),
                    ratatui::widgets::Cell::from(Line::from(Span::styled(
                        format!("{:<10}", truncate(mfg, 10)),
                        Style::default().fg(Color::White),
                    ))),
                    ratatui::widgets::Cell::from(Line::from(Span::styled(
                        format!("{:<18}", truncate(part, 18)),
                        Style::default().fg(Color::White),
                    ))),
                    ratatui::widgets::Cell::from(Line::from(Span::styled(
                        format!("{:<8}", die_str),
                        theme.muted_style(),
                    ))),
                    ratatui::widgets::Cell::from(Line::from(Span::styled(
                        format!("{:<16}", timing_str),
                        Style::default().fg(Color::Cyan),
                    ))),
                    ratatui::widgets::Cell::from(Line::from(Span::styled(
                        format!("{:<9}", cas_str),
                        theme.muted_style(),
                    ))),
                    ratatui::widgets::Cell::from(Line::from(Span::styled(
                        hub_str,
                        temp_style(hub_temp),
                    ))),
                    ratatui::widgets::Cell::from(Line::from(Span::styled(
                        ts0_str,
                        temp_style(ts0_temp),
                    ))),
                    ratatui::widgets::Cell::from(Line::from(Span::styled(
                        ts1_str,
                        temp_style(ts1_temp),
                    ))),
                ])
                .height(1)
            })
            .collect();

        let header_row = Row::new(vec![
            "Channel",
            "Size",
            "Mfg",
            "Part Number",
            "Die",
            "tAA/RCD/RP/RAS",
            "CAS",
            "Hub",
            "TS0",
            "TS1",
        ])
        .style(
            Style::default()
                .fg(Color::White)
                .add_modifier(Modifier::BOLD),
        )
        .height(1);

        let widths = [
            Constraint::Length(19), // Channel
            Constraint::Length(7),  // Size
            Constraint::Length(11), // Mfg
            Constraint::Length(19), // Part
            Constraint::Length(9),  // Die
            Constraint::Length(17), // Timings
            Constraint::Length(10), // CAS
            Constraint::Min(18),    // Hub temp + spark
            Constraint::Min(18),    // TS0 temp + spark
            Constraint::Min(18),    // TS1 temp + spark
        ];

        let table = Table::new(rows, widths)
            .header(header_row)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_style(theme.border_style())
                    .title(" DIMM Details (SPD5118 EEPROM) "),
            )
            .row_highlight_style(Style::default().add_modifier(Modifier::BOLD));

        frame.render_widget(table, main_area);
    })?;

    Ok(())
}

/// One-line mode summary, e.g. "DDR5-7200 Gear 2 2N 36-44-44-96".
fn mode_summary(mc: &MemoryControllerInfo) -> String {
    let t = &mc.timings;
    let rate = mc
        .data_rate_mts
        .map(|r| format!("DDR5-{r} "))
        .unwrap_or_default();
    format!(
        "{rate}Gear {} {}N {}-{}-{}-{}",
        mc.gear, mc.command_rate, t.tcl, t.trcd, t.trp, t.tras
    )
}

/// Turnaround timings as "sg/dg/sd/dd".
fn turnaround(v: [u32; 4]) -> String {
    format!("{}/{}/{}/{}", v[0], v[1], v[2], v[3])
}

/// Lines for the trained controller panel. Without controller data, a hint
/// is shown only where the platform supports reading it.
fn controller_lines<'a>(mc: Option<&MemoryControllerInfo>, theme: &TuiTheme) -> Vec<Line<'a>> {
    let Some(mc) = mc else {
        if crate::platform::mchbar::host_bridge_supported().is_some() {
            return vec![Line::from(Span::styled(
                " Trained timings: run as root with --direct-io",
                theme.muted_style(),
            ))];
        }
        return Vec::new();
    };
    let t = &mc.timings;
    let row = |name: &str, fields: Vec<(&str, String)>| {
        let mut spans = vec![Span::styled(format!(" {name:<11}"), theme.label_style())];
        for (label, value) in fields {
            spans.push(Span::styled(format!("{label} "), theme.muted_style()));
            spans.push(Span::styled(
                format!("{value:<5}  "),
                Style::default().fg(Color::Cyan),
            ));
        }
        Line::from(spans)
    };
    let mut mode = vec![
        ("Gear", mc.gear.to_string()),
        ("Cmd", format!("{}N", mc.command_rate)),
    ];
    if let Some(rate) = mc.data_rate_mts {
        mode.insert(0, ("DDR5", rate.to_string()));
    }
    if let Some(qclk) = mc.qclk_mhz {
        mode.push(("QCLK", format!("{qclk:.0}MHz")));
    }
    if let Some(bw) = mc.max_bandwidth_mbs {
        mode.push(("Max BW", format!("{bw}MB/s")));
    }
    vec![
        row("Mode", mode),
        row(
            "Primary",
            vec![
                ("tCL", t.tcl.to_string()),
                ("tRCD", t.trcd.to_string()),
                ("tRP", t.trp.to_string()),
                ("tRAS", t.tras.to_string()),
                ("tRC", t.trc.to_string()),
            ],
        ),
        row(
            "Write",
            vec![
                ("tCWL", t.tcwl.to_string()),
                ("tWR", t.twr.to_string()),
                ("tWTP", t.twtp.to_string()),
                ("tRTP", t.trtp.to_string()),
                ("tRCDW", t.trcdw.to_string()),
            ],
        ),
        row(
            "Refresh",
            vec![
                ("tRFC", t.trfc.to_string()),
                ("tRFCpb", t.trfcpb.to_string()),
                ("tREFI", t.trefi.to_string()),
                ("tREFSBRD", t.trefsbrd.to_string()),
                ("tRPab", t.trpab.to_string()),
            ],
        ),
        row(
            "Activate",
            vec![
                ("tRRD_L", t.trrd_l.to_string()),
                ("tRRD_S", t.trrd_s.to_string()),
                ("tFAW", t.tfaw.to_string()),
            ],
        ),
        row(
            "Turnaround",
            vec![
                ("tRDRD", turnaround(t.trdrd)),
                ("tRDWR", turnaround(t.trdwr)),
                ("tWRRD", turnaround(t.twrrd)),
                ("tWRWR", turnaround(t.twrwr)),
            ],
        ),
        Line::from(Span::styled(
            " Turnarounds are same group / diff group / same DIMM / diff DIMM; tRC and tWR are derived",
            theme.muted_style(),
        )),
    ]
}

/// Find a DIMM temperature reading by I2C bus/addr and sensor suffix (hub/ts0/ts1).
fn find_dimm_temp(
    snapshot: &[(SensorId, SensorReading)],
    i2c_key: Option<(u32, u16)>,
    suffix: &str,
) -> Option<f64> {
    let (bus, addr) = i2c_key?;
    let sensor_name = crate::sensors::i2c::ddr5_temp::sensor_name(bus, addr, suffix);
    snapshot
        .iter()
        .find(|(id, _)| id.sensor == sensor_name)
        .map(|(_, r)| r.current)
}

/// Format a temperature value with sparkline.
fn format_temp_with_spark(
    temp: Option<f64>,
    snapshot: &[(SensorId, SensorReading)],
    history: &SensorHistory,
    i2c_key: Option<(u32, u16)>,
    suffix: &str,
    spark_width: usize,
) -> String {
    let temp_str = temp
        .map(|t| format!("{t:5.1}°C"))
        .unwrap_or_else(|| "    -  ".into());

    let sensor_name =
        i2c_key.map(|(bus, addr)| crate::sensors::i2c::ddr5_temp::sensor_name(bus, addr, suffix));
    let key = sensor_name.and_then(|name| {
        snapshot
            .iter()
            .find(|(id, _)| id.sensor == name)
            .map(|(id, _)| format!("{}/{}/{}", id.source, id.chip, id.sensor))
    });

    let spark = key
        .and_then(|k| history.data.get(&k))
        .map(|buf| sparkline_str(buf, spark_width))
        .unwrap_or_default();

    if spark.is_empty() {
        temp_str
    } else {
        format!("{temp_str} {spark}")
    }
}

fn truncate(s: &str, max: usize) -> &str {
    if s.len() <= max {
        return s;
    }
    // Find the last char boundary at or before max to avoid panicking on multi-byte UTF-8.
    let mut end = max;
    while end > 0 && !s.is_char_boundary(end) {
        end -= 1;
    }
    &s[..end]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::memory::TrainedTimings;

    #[test]
    fn test_mode_summary_and_turnaround() {
        let mc = MemoryControllerInfo {
            source: "test".into(),
            qclk_mhz: Some(3600.0),
            data_rate_mts: Some(7200),
            gear: 2,
            command_rate: 2,
            max_bandwidth_mbs: Some(115_200),
            timings: TrainedTimings {
                tcl: 36,
                trcd: 44,
                trp: 44,
                tras: 96,
                ..Default::default()
            },
        };
        assert_eq!(mode_summary(&mc), "DDR5-7200 Gear 2 2N 36-44-44-96");
        assert_eq!(turnaround([18, 8, 16, 16]), "18/8/16/16");
    }
}
