use ratatui::style::{Color, Modifier, Style};

use crate::model::sensor::{SensorCategory, SensorReading};

/// Terminal color capability level.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum ColorLevel {
    None = 0,
    Basic = 1,     // 16 ANSI colors
    Color256 = 2,  // 256-color palette
    TrueColor = 3, // 24-bit RGB
}

/// Detect terminal color capability from environment variables.
pub fn detect_color_level() -> ColorLevel {
    if std::env::var_os("NO_COLOR").is_some_and(|v| !v.is_empty()) {
        return ColorLevel::None;
    }
    if let Ok(ct) = std::env::var("COLORTERM") {
        if ct == "truecolor" || ct == "24bit" {
            return ColorLevel::TrueColor;
        }
    }
    if let Ok(term) = std::env::var("TERM") {
        if term == "dumb" {
            return ColorLevel::None;
        }
        if term.ends_with("direct") {
            return ColorLevel::TrueColor;
        }
        if term.contains("256color") {
            return ColorLevel::Color256;
        }
    }
    ColorLevel::Basic
}

#[derive(Clone, Debug)]
pub struct TuiTheme {
    pub name: String,
    pub color_level: ColorLevel,
    pub border: Color,
    pub muted: Color,
    pub cursor_bg: Color,
    pub accent: Color,
    pub label: Color,
    pub source: Color,
    pub chip: Color,
    pub cat: Color,
    pub power: Color,
    pub voltage: Color,
    pub info: Color,
    pub good: Color,
    pub warn: Color,
    pub crit: Color,
    pub status_fg: Color,
    pub status_bg: Color,
    pub alert_fg: Color,
    pub alert_bg: Color,
    pub search_active_fg: Color,
    pub search_active_bg: Color,
    pub search_inactive_fg: Color,
    pub search_inactive_bg: Color,
    pub header_bg: Color,
    // Per-panel accent colors
    pub panel_cpu: Color,
    pub panel_thermal: Color,
    pub panel_memory: Color,
    pub panel_power: Color,
    pub panel_storage: Color,
    pub panel_network: Color,
    pub panel_fans: Color,
    pub panel_gpu: Color,
    pub panel_voltage: Color,
    pub panel_frequency: Color,
    pub panel_platform: Color,
    pub panel_errors: Color,
}

impl Default for TuiTheme {
    fn default() -> Self {
        Self {
            name: "default".into(),
            color_level: detect_color_level(),
            border: Color::Gray,
            muted: Color::Gray,
            cursor_bg: Color::Gray,
            accent: Color::Cyan,
            label: Color::White,
            source: Color::Yellow,
            chip: Color::White,
            cat: Color::Cyan,
            power: Color::Magenta,
            voltage: Color::Blue,
            info: Color::Cyan,
            good: Color::Green,
            warn: Color::Yellow,
            crit: Color::Red,
            status_fg: Color::Gray,
            status_bg: Color::Reset,
            alert_fg: Color::Yellow,
            alert_bg: Color::Gray,
            search_active_fg: Color::Black,
            search_active_bg: Color::Yellow,
            search_inactive_fg: Color::Yellow,
            search_inactive_bg: Color::Gray,
            header_bg: Color::Gray,
            panel_cpu: Color::Cyan,
            panel_thermal: Color::LightRed,
            panel_memory: Color::Blue,
            panel_power: Color::Magenta,
            panel_storage: Color::LightYellow,
            panel_network: Color::Green,
            panel_fans: Color::LightCyan,
            panel_gpu: Color::LightGreen,
            panel_voltage: Color::LightBlue,
            panel_frequency: Color::LightMagenta,
            panel_platform: Color::Yellow,
            panel_errors: Color::Red,
        }
    }
}

impl TuiTheme {
    pub fn light() -> Self {
        Self {
            name: "light".into(),
            color_level: detect_color_level(),
            border: Color::DarkGray,
            muted: Color::DarkGray,
            cursor_bg: Color::DarkGray,
            accent: Color::Blue,
            label: Color::Black,
            source: Color::Red,
            chip: Color::Black,
            cat: Color::Blue,
            power: Color::Magenta,
            voltage: Color::Blue,
            info: Color::Cyan,
            good: Color::Green,
            warn: Color::Yellow,
            crit: Color::Red,
            status_fg: Color::DarkGray,
            status_bg: Color::Reset,
            alert_fg: Color::Red,
            alert_bg: Color::DarkGray,
            search_active_fg: Color::White,
            search_active_bg: Color::Blue,
            search_inactive_fg: Color::Blue,
            search_inactive_bg: Color::DarkGray,
            header_bg: Color::DarkGray,
            panel_cpu: Color::Blue,
            panel_thermal: Color::Red,
            panel_memory: Color::Magenta,
            panel_power: Color::DarkGray,
            panel_storage: Color::Green,
            panel_network: Color::Cyan,
            panel_fans: Color::Black,
            panel_gpu: Color::Red,
            panel_voltage: Color::Blue,
            panel_frequency: Color::Cyan,
            panel_platform: Color::DarkGray,
            panel_errors: Color::Red,
        }
    }

    pub fn high_contrast() -> Self {
        Self {
            name: "high-contrast".into(),
            color_level: detect_color_level(),
            border: Color::White,
            muted: Color::White,
            cursor_bg: Color::DarkGray,
            accent: Color::LightCyan,
            label: Color::White,
            source: Color::LightYellow,
            chip: Color::White,
            cat: Color::LightCyan,
            power: Color::LightMagenta,
            voltage: Color::LightBlue,
            info: Color::LightCyan,
            good: Color::LightGreen,
            warn: Color::LightYellow,
            crit: Color::LightRed,
            status_fg: Color::White,
            status_bg: Color::Reset,
            alert_fg: Color::LightYellow,
            alert_bg: Color::DarkGray,
            search_active_fg: Color::Black,
            search_active_bg: Color::LightYellow,
            search_inactive_fg: Color::LightYellow,
            search_inactive_bg: Color::DarkGray,
            header_bg: Color::DarkGray,
            panel_cpu: Color::LightCyan,
            panel_thermal: Color::LightYellow,
            panel_memory: Color::LightBlue,
            panel_power: Color::LightMagenta,
            panel_storage: Color::LightYellow,
            panel_network: Color::LightGreen,
            panel_fans: Color::LightCyan,
            panel_gpu: Color::LightGreen,
            panel_voltage: Color::LightBlue,
            panel_frequency: Color::LightCyan,
            panel_platform: Color::LightMagenta,
            panel_errors: Color::LightRed,
        }
    }

    pub fn monochrome() -> Self {
        Self {
            name: "monochrome".into(),
            color_level: ColorLevel::None,
            border: Color::Reset,
            muted: Color::Reset,
            cursor_bg: Color::Reset,
            accent: Color::Reset,
            label: Color::Reset,
            source: Color::Reset,
            chip: Color::Reset,
            cat: Color::Reset,
            power: Color::Reset,
            voltage: Color::Reset,
            info: Color::Reset,
            good: Color::Reset,
            warn: Color::Reset,
            crit: Color::Reset,
            status_fg: Color::Reset,
            status_bg: Color::Reset,
            alert_fg: Color::Reset,
            alert_bg: Color::Reset,
            search_active_fg: Color::Reset,
            search_active_bg: Color::Reset,
            search_inactive_fg: Color::Reset,
            search_inactive_bg: Color::Reset,
            header_bg: Color::Reset,
            panel_cpu: Color::Reset,
            panel_thermal: Color::Reset,
            panel_memory: Color::Reset,
            panel_power: Color::Reset,
            panel_storage: Color::Reset,
            panel_network: Color::Reset,
            panel_fans: Color::Reset,
            panel_gpu: Color::Reset,
            panel_voltage: Color::Reset,
            panel_frequency: Color::Reset,
            panel_platform: Color::Reset,
            panel_errors: Color::Reset,
        }
    }

    pub fn from_name(name: &str) -> Self {
        match name {
            "default" => Self::default(),
            "light" => Self::light(),
            "high-contrast" => Self::high_contrast(),
            "monochrome" => Self::monochrome(),
            other => {
                log::warn!("Unknown theme {other:?}, using default");
                Self::default()
            }
        }
    }

    pub fn resolve(theme_name: &str, color_mode: &crate::cli::ColorMode) -> Self {
        if matches!(color_mode, crate::cli::ColorMode::Never) {
            return Self::monochrome();
        }
        Self::from_name(theme_name)
    }

    // -- Style helpers -------------------------------------------------------

    pub fn accent_style(&self) -> Style {
        Style::default()
            .fg(self.accent)
            .add_modifier(Modifier::BOLD)
    }

    pub fn source_style(&self) -> Style {
        Style::default()
            .fg(self.source)
            .add_modifier(Modifier::BOLD)
    }

    pub fn chip_style(&self) -> Style {
        Style::default().fg(self.chip).add_modifier(Modifier::BOLD)
    }

    pub fn label_style(&self) -> Style {
        Style::default().fg(self.label)
    }

    pub fn cat_style(&self) -> Style {
        Style::default().fg(self.cat)
    }

    pub fn muted_style(&self) -> Style {
        Style::default().fg(self.muted)
    }

    pub fn border_style(&self) -> Style {
        Style::default().fg(self.border)
    }

    pub fn power_style(&self) -> Style {
        Style::default().fg(self.power)
    }

    pub fn info_style(&self) -> Style {
        Style::default().fg(self.info)
    }

    pub fn good_style(&self) -> Style {
        Style::default().fg(self.good)
    }

    pub fn warn_style(&self) -> Style {
        Style::default().fg(self.warn)
    }

    pub fn crit_style(&self) -> Style {
        Style::default().fg(self.crit)
    }

    pub fn voltage_style(&self) -> Style {
        Style::default().fg(self.voltage)
    }

    pub fn status_style(&self) -> Style {
        Style::default().fg(self.status_fg).bg(self.status_bg)
    }

    pub fn alert_status_style(&self) -> Style {
        Style::default()
            .fg(self.alert_fg)
            .bg(self.alert_bg)
            .add_modifier(Modifier::BOLD)
    }

    pub fn cursor_style(&self) -> Style {
        if self.cursor_bg == Color::Reset {
            Style::default().add_modifier(Modifier::REVERSED)
        } else {
            Style::default().bg(self.cursor_bg)
        }
    }

    pub fn search_active_style(&self) -> Style {
        if self.search_active_bg == Color::Reset {
            Style::default().add_modifier(Modifier::REVERSED)
        } else {
            Style::default()
                .fg(self.search_active_fg)
                .bg(self.search_active_bg)
        }
    }

    pub fn search_inactive_style(&self) -> Style {
        Style::default()
            .fg(self.search_inactive_fg)
            .bg(self.search_inactive_bg)
    }

    pub fn panel_accent(&self, title: &str) -> Color {
        match title {
            "CPU" => self.panel_cpu,
            "Thermal" => self.panel_thermal,
            "Memory" => self.panel_memory,
            "Power" => self.panel_power,
            "Storage" => self.panel_storage,
            "Network" => self.panel_network,
            "Fans" => self.panel_fans,
            "GPU" => self.panel_gpu,
            "Voltage" => self.panel_voltage,
            "CPU Freq" => self.panel_frequency,
            "Platform" => self.panel_platform,
            "Errors" => self.panel_errors,
            _ => self.accent,
        }
    }

    /// Return a color for a sparkline data point based on its normalized position
    /// (0.0 = min in window, 1.0 = max) and the sensor category.
    /// On truecolor terminals, returns smooth RGB gradients.
    /// On basic terminals, falls back to a single ANSI color per category.
    pub fn sparkline_color(&self, category: SensorCategory, fraction: f64) -> Color {
        let t = fraction.clamp(0.0, 1.0);

        match self.color_level {
            ColorLevel::None => Color::Reset,
            ColorLevel::Basic => self.sparkline_basic(category, t),
            ColorLevel::Color256 => Self::sparkline_256(category, t),
            ColorLevel::TrueColor => Self::sparkline_rgb(category, t),
        }
    }

    /// Per-category gradient as floating-point RGB (0.0–255.0).
    fn gradient_rgb(category: SensorCategory, t: f64) -> (f64, f64, f64) {
        match category {
            // Dim red-orange → bright red-orange
            SensorCategory::Temperature => (120.0 + t * 135.0, 40.0 + t * 50.0, 20.0 + t * 20.0),
            // Near-black → bright cyan (idle cores are dark)
            SensorCategory::Utilization => (5.0 + t * 85.0, 15.0 + t * 240.0, 20.0 + t * 235.0),
            // Dim magenta → bright magenta
            SensorCategory::Power | SensorCategory::Current => {
                (100.0 + t * 155.0, 40.0 + t * 40.0, 120.0 + t * 135.0)
            }
            // Dim blue → bright blue
            SensorCategory::Voltage => (60.0 + t * 80.0, 80.0 + t * 100.0, 160.0 + t * 95.0),
            // Dim cyan → bright cyan
            SensorCategory::Frequency => (40.0 + t * 80.0, 140.0 + t * 115.0, 160.0 + t * 95.0),
            // Dim gray → bright white
            _ => {
                let v = 100.0 + t * 155.0;
                (v, v, v)
            }
        }
    }

    /// 24-bit RGB gradient.
    fn sparkline_rgb(category: SensorCategory, t: f64) -> Color {
        let (r, g, b) = Self::gradient_rgb(category, t);
        Color::Rgb(r as u8, g as u8, b as u8)
    }

    /// 256-color palette gradient using the 6×6×6 color cube (indices 16-231).
    fn sparkline_256(category: SensorCategory, t: f64) -> Color {
        let (r, g, b) = Self::gradient_rgb(category, t);
        Color::Indexed(rgb_to_cube(r, g, b))
    }

    /// 16-color ANSI gradient: dim (DarkGray) for low values, theme color for high.
    fn sparkline_basic(&self, category: SensorCategory, t: f64) -> Color {
        if t < 0.5 {
            Color::DarkGray
        } else {
            match category {
                SensorCategory::Temperature => self.panel_thermal,
                SensorCategory::Utilization => self.panel_cpu,
                SensorCategory::Power | SensorCategory::Current => self.power,
                SensorCategory::Voltage => self.voltage,
                SensorCategory::Frequency => self.info,
                _ => self.muted,
            }
        }
    }

    pub fn value_style(&self, reading: &SensorReading) -> Style {
        match reading.category {
            SensorCategory::Temperature => {
                if reading.current > 80.0 {
                    Style::default().fg(self.crit).add_modifier(Modifier::BOLD)
                } else if reading.current >= 60.0 {
                    Style::default().fg(self.warn)
                } else {
                    Style::default().fg(self.good)
                }
            }
            SensorCategory::Fan => {
                if reading.current == 0.0 {
                    Style::default().fg(self.crit).add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(self.info)
                }
            }
            SensorCategory::Power => Style::default().fg(self.power),
            SensorCategory::Voltage => Style::default().fg(self.voltage),
            SensorCategory::Frequency => Style::default().fg(self.info),
            SensorCategory::Utilization => {
                if reading.current > 90.0 {
                    Style::default().fg(self.crit)
                } else if reading.current > 70.0 {
                    Style::default().fg(self.warn)
                } else {
                    Style::default().fg(self.good)
                }
            }
            _ => Style::default(),
        }
    }
}

/// Map an RGB value (0.0–255.0 per channel) to the nearest xterm 6×6×6 cube index (16-231).
/// The cube levels are non-linear: 0, 95, 135, 175, 215, 255.
fn rgb_to_cube(r: f64, g: f64, b: f64) -> u8 {
    const LEVELS: [u8; 6] = [0, 95, 135, 175, 215, 255];

    fn nearest(v: f64) -> u8 {
        let v = v.clamp(0.0, 255.0) as u8;
        let mut best = 0u8;
        let mut best_dist = u8::MAX;
        for (i, &level) in LEVELS.iter().enumerate() {
            let dist = v.abs_diff(level);
            if dist < best_dist {
                best_dist = dist;
                best = i as u8;
            }
        }
        best
    }

    16 + 36 * nearest(r) + 6 * nearest(g) + nearest(b)
}
