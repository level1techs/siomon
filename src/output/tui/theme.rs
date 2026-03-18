use ratatui::style::{Color, Modifier, Style};

use crate::model::sensor::{SensorCategory, SensorReading};

#[derive(Clone, Debug)]
pub struct TuiTheme {
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
}

impl Default for TuiTheme {
    fn default() -> Self {
        Self {
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
        }
    }
}

impl TuiTheme {
    pub fn light() -> Self {
        Self {
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
        }
    }

    pub fn high_contrast() -> Self {
        Self {
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
        }
    }

    pub fn monochrome() -> Self {
        Self {
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
