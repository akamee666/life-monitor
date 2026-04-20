use std::path::{Path, PathBuf};

use anyhow::Result;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::tui::data::{load_dashboard_snapshot, ChartMetric, DashboardSnapshot};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FocusSection {
    Summary,
    Apps,
    Activity,
    Heatmap,
}

impl FocusSection {
    const ALL: [FocusSection; 4] = [
        FocusSection::Summary,
        FocusSection::Apps,
        FocusSection::Activity,
        FocusSection::Heatmap,
    ];

    pub fn label(self) -> &'static str {
        match self {
            FocusSection::Summary => "totals",
            FocusSection::Apps => "apps",
            FocusSection::Activity => "24h",
            FocusSection::Heatmap => "daily",
        }
    }

    fn next(self) -> Self {
        let index = Self::ALL.iter().position(|item| *item == self).unwrap_or(0);
        Self::ALL[(index + 1) % Self::ALL.len()]
    }

    fn previous(self) -> Self {
        let index = Self::ALL.iter().position(|item| *item == self).unwrap_or(0);
        Self::ALL[(index + Self::ALL.len() - 1) % Self::ALL.len()]
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AppAction {
    None,
    Quit,
    Refresh,
}

pub struct DashboardApp {
    db_path: PathBuf,
    range_days: u32,
    pub ascii: bool,
    pub snapshot: DashboardSnapshot,
    pub focused_section: FocusSection,
    pub chart_metric: ChartMetric,
    pub show_help: bool,
    pub status_message: String,
}

impl DashboardApp {
    pub fn load(db_path: &Path, range_days: u32, ascii: bool) -> Result<Self> {
        Ok(Self {
            db_path: db_path.to_path_buf(),
            range_days,
            ascii,
            snapshot: load_dashboard_snapshot(db_path, range_days)?,
            focused_section: FocusSection::Activity,
            chart_metric: ChartMetric::Activity,
            show_help: false,
            status_message: "dashboard opened in read-only mode".to_string(),
        })
    }

    pub fn refresh(&mut self) {
        match load_dashboard_snapshot(&self.db_path, self.range_days) {
            Ok(snapshot) => {
                self.snapshot = snapshot;
                self.status_message = "dashboard reloaded from local SQLite".to_string();
            }
            Err(err) => {
                self.status_message = format!("refresh failed: {err}");
            }
        }
    }

    pub fn handle_key(&mut self, key: KeyEvent) -> AppAction {
        if key.kind != crossterm::event::KeyEventKind::Press {
            return AppAction::None;
        }

        match key.code {
            KeyCode::Char('q') | KeyCode::Esc if !self.show_help => AppAction::Quit,
            KeyCode::Esc => {
                self.show_help = false;
                AppAction::None
            }
            KeyCode::Char('?') => {
                self.show_help = !self.show_help;
                AppAction::None
            }
            KeyCode::Char('r') => AppAction::Refresh,
            KeyCode::Tab | KeyCode::Char('l') | KeyCode::Right => {
                self.focused_section = self.focused_section.next();
                AppAction::None
            }
            KeyCode::BackTab | KeyCode::Char('h') | KeyCode::Left => {
                self.focused_section = self.focused_section.previous();
                AppAction::None
            }
            KeyCode::Char('j') | KeyCode::Down
                if self.focused_section == FocusSection::Activity =>
            {
                self.chart_metric = self.chart_metric.next();
                AppAction::None
            }
            KeyCode::Char('k') | KeyCode::Up if self.focused_section == FocusSection::Activity => {
                self.chart_metric = self.chart_metric.next();
                AppAction::None
            }
            KeyCode::Char('m') => {
                self.chart_metric = self.chart_metric.next();
                AppAction::None
            }
            KeyCode::Char('u') => {
                self.ascii = !self.ascii;
                self.status_message = if self.ascii {
                    "ascii-safe rendering enabled".to_string()
                } else {
                    "unicode-rich rendering enabled".to_string()
                };
                AppAction::None
            }
            KeyCode::Char('1') => {
                self.focused_section = FocusSection::Summary;
                AppAction::None
            }
            KeyCode::Char('2') => {
                self.focused_section = FocusSection::Apps;
                AppAction::None
            }
            KeyCode::Char('3') => {
                self.focused_section = FocusSection::Activity;
                AppAction::None
            }
            KeyCode::Char('4') => {
                self.focused_section = FocusSection::Heatmap;
                AppAction::None
            }
            KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => AppAction::Quit,
            _ => AppAction::None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{Local, Utc};
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    fn sample_app() -> DashboardApp {
        DashboardApp {
            db_path: PathBuf::from("/tmp/life-monitor.db"),
            range_days: 7,
            ascii: false,
            snapshot: DashboardSnapshot {
                generated_at_local: Local::now(),
                range_days: 7,
                summary: crate::tui::data::SummaryTotals {
                    left_clicks: 0,
                    right_clicks: 0,
                    middle_clicks: 0,
                    key_presses: 0,
                    mouse_distance_cm: 0.0,
                },
                summary_label: "all-time totals from local SQLite".to_string(),
                top_activities: Vec::new(),
                top_apps: Vec::new(),
                series_start_utc: Utc::now(),
                series_buckets: Vec::new(),
                heatmap_rows: Vec::new(),
                heatmap_maxima: [0.0; 5],
                status: crate::tui::data::DashboardStatus {
                    source_count: 0,
                    source_names: Vec::new(),
                    current_app: None,
                    last_activity_at_utc: None,
                    sync_summary: "local-only".to_string(),
                    db_path_display: "/tmp/life-monitor.db".to_string(),
                },
            },
            focused_section: FocusSection::Summary,
            chart_metric: ChartMetric::Activity,
            show_help: false,
            status_message: String::new(),
        }
    }

    /// Proves focus switching stays in a small finite cycle, which keeps keyboard navigation
    /// predictable across layouts without depending on terminal rendering.
    #[test]
    fn tab_cycles_through_focus_sections() {
        let mut app = sample_app();

        app.handle_key(key(KeyCode::Tab));
        assert_eq!(app.focused_section, FocusSection::Apps);
        app.handle_key(key(KeyCode::Tab));
        assert_eq!(app.focused_section, FocusSection::Activity);
        app.handle_key(key(KeyCode::BackTab));
        assert_eq!(app.focused_section, FocusSection::Apps);
    }

    /// Proves the chart metric cycle is driven by keyboard state alone, which catches regressions
    /// in interaction logic without needing a live terminal backend.
    #[test]
    fn metric_cycle_advances_from_activity_panel() {
        let mut app = sample_app();
        app.focused_section = FocusSection::Activity;

        app.handle_key(key(KeyCode::Char('m')));
        assert_eq!(app.chart_metric, ChartMetric::KeyPresses);
        app.handle_key(key(KeyCode::Char('m')));
        assert_eq!(app.chart_metric, ChartMetric::Clicks);
    }
}
