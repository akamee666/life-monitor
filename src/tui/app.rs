use std::path::{Path, PathBuf};

use anyhow::Result;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::tui::data::{
    load_dashboard_snapshot, ChartMetric, DashboardSnapshot, DEFAULT_BUCKET_COUNT,
    DEFAULT_BUCKET_MINUTES,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChartMode {
    Single,
    Scope,
}

impl ChartMode {
    pub fn next(self) -> Self {
        match self {
            ChartMode::Single => ChartMode::Scope,
            ChartMode::Scope => ChartMode::Single,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            ChartMode::Single => "single",
            ChartMode::Scope => "scope",
        }
    }
}

/// Time window for the activity chart and top-apps range.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TimeWindow {
    OneHour,
    SixHours,
    TwentyFourHours,
    SevenDays,
    ThirtyDays,
}

impl TimeWindow {
    pub const ALL: [TimeWindow; 5] = [
        TimeWindow::OneHour,
        TimeWindow::SixHours,
        TimeWindow::TwentyFourHours,
        TimeWindow::SevenDays,
        TimeWindow::ThirtyDays,
    ];

    pub fn label(self) -> &'static str {
        match self {
            TimeWindow::OneHour => "1h",
            TimeWindow::SixHours => "6h",
            TimeWindow::TwentyFourHours => "24h",
            TimeWindow::SevenDays => "7d",
            TimeWindow::ThirtyDays => "30d",
        }
    }

    /// Days of focus/activity history to query for top-apps and heatmap.
    pub fn range_days(self) -> u32 {
        match self {
            TimeWindow::OneHour | TimeWindow::SixHours | TimeWindow::TwentyFourHours => 1,
            TimeWindow::SevenDays => 7,
            TimeWindow::ThirtyDays => 30,
        }
    }

    /// (bucket_minutes, bucket_count) for the activity chart.
    pub fn series_params(self) -> (i64, usize) {
        match self {
            // 1 h  → 4 × 15-min buckets (SQLite resolution limit)
            TimeWindow::OneHour => (15, 4),
            // 6 h  → 24 × 15-min buckets
            TimeWindow::SixHours => (15, 24),
            // 24 h → 96 × 15-min buckets (default)
            TimeWindow::TwentyFourHours => (DEFAULT_BUCKET_MINUTES, DEFAULT_BUCKET_COUNT),
            // 7 d  → 168 × 60-min buckets
            TimeWindow::SevenDays => (60, 168),
            // 30 d → 180 × 4-hour buckets
            TimeWindow::ThirtyDays => (240, 180),
        }
    }

    fn next(self) -> Self {
        let idx = Self::ALL.iter().position(|w| *w == self).unwrap_or(0);
        Self::ALL[(idx + 1) % Self::ALL.len()]
    }

    fn previous(self) -> Self {
        let idx = Self::ALL.iter().position(|w| *w == self).unwrap_or(0);
        Self::ALL[(idx + Self::ALL.len() - 1) % Self::ALL.len()]
    }
}

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
    pub ascii: bool,
    pub snapshot: DashboardSnapshot,
    pub focused_section: FocusSection,
    pub selected_summary_index: usize,
    pub selected_app_index: usize,
    pub app_scroll_offset: usize,
    pub selected_heatmap_index: usize,
    pub heatmap_scroll_offset: usize,
    pub chart_metric: ChartMetric,
    pub chart_mode: ChartMode,
    pub time_window: TimeWindow,
    pub show_help: bool,
    pub status_message: String,
}

impl DashboardApp {
    pub fn load(db_path: &Path, range_days: u32, ascii: bool) -> Result<Self> {
        let time_window = match range_days {
            30 => TimeWindow::ThirtyDays,
            7..=29 => TimeWindow::SevenDays,
            _ => TimeWindow::TwentyFourHours,
        };
        let (bucket_minutes, bucket_count) = time_window.series_params();
        Ok(Self {
            db_path: db_path.to_path_buf(),
            ascii,
            snapshot: load_dashboard_snapshot(
                db_path,
                time_window.range_days(),
                bucket_minutes,
                bucket_count,
            )?,
            focused_section: FocusSection::Activity,
            selected_summary_index: 0,
            selected_app_index: 0,
            app_scroll_offset: 0,
            selected_heatmap_index: 0,
            heatmap_scroll_offset: 0,
            chart_metric: ChartMetric::Activity,
            chart_mode: ChartMode::Single,
            time_window,
            show_help: false,
            status_message: "dashboard opened".to_string(),
        })
    }

    pub fn refresh(&mut self) {
        let (bucket_minutes, bucket_count) = self.time_window.series_params();
        match load_dashboard_snapshot(
            &self.db_path,
            self.time_window.range_days(),
            bucket_minutes,
            bucket_count,
        ) {
            Ok(snapshot) => {
                self.snapshot = snapshot;
                self.clamp_app_selection();
                self.clamp_heatmap_selection();
                self.status_message.clear();
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
            KeyCode::Char('r') | KeyCode::F(5) => AppAction::Refresh,
            KeyCode::Char('l') | KeyCode::Right
                if self.focused_section == FocusSection::Summary =>
            {
                if self.selected_summary_index < 4 {
                    self.selected_summary_index += 1;
                } else {
                    self.focused_section = self.focused_section.next();
                }
                AppAction::None
            }
            KeyCode::Char('h') | KeyCode::Left if self.focused_section == FocusSection::Summary => {
                if self.selected_summary_index > 0 {
                    self.selected_summary_index -= 1;
                } else {
                    self.focused_section = self.focused_section.previous();
                }
                AppAction::None
            }
            KeyCode::Tab | KeyCode::Char('l') | KeyCode::Right => {
                self.focused_section = self.focused_section.next();
                AppAction::None
            }
            KeyCode::BackTab | KeyCode::Char('h') | KeyCode::Left => {
                self.focused_section = self.focused_section.previous();
                AppAction::None
            }
            KeyCode::Char('j') | KeyCode::Down if self.focused_section == FocusSection::Apps => {
                self.move_app_selection(1);
                AppAction::None
            }
            KeyCode::Char('k') | KeyCode::Up if self.focused_section == FocusSection::Apps => {
                self.move_app_selection(-1);
                AppAction::None
            }
            KeyCode::Char('j') | KeyCode::Down if self.focused_section == FocusSection::Heatmap => {
                self.move_heatmap_selection(1);
                AppAction::None
            }
            KeyCode::Char('k') | KeyCode::Up if self.focused_section == FocusSection::Heatmap => {
                self.move_heatmap_selection(-1);
                AppAction::None
            }
            KeyCode::Char('j') | KeyCode::Down
                if self.focused_section == FocusSection::Activity =>
            {
                self.chart_metric = self.chart_metric.next();
                AppAction::None
            }
            KeyCode::Char('k') | KeyCode::Up if self.focused_section == FocusSection::Activity => {
                self.chart_metric = self.chart_metric.previous();
                AppAction::None
            }
            KeyCode::Char('m') => {
                self.chart_metric = self.chart_metric.next();
                AppAction::None
            }
            KeyCode::Char('v') if self.focused_section == FocusSection::Activity => {
                self.chart_mode = self.chart_mode.next();
                self.status_message = format!("chart mode: {}", self.chart_mode.label());
                AppAction::None
            }
            KeyCode::Char('u') => {
                self.ascii = !self.ascii;
                self.status_message = if self.ascii {
                    "ascii-safe rendering".to_string()
                } else {
                    "unicode rendering".to_string()
                };
                AppAction::None
            }
            // Time-window cycling
            KeyCode::Char(']') => {
                self.time_window = self.time_window.next();
                self.status_message = format!("window: {}", self.time_window.label());
                AppAction::Refresh
            }
            KeyCode::Char('[') => {
                self.time_window = self.time_window.previous();
                self.status_message = format!("window: {}", self.time_window.label());
                AppAction::Refresh
            }
            // Focus-section shortcuts
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

    fn move_app_selection(&mut self, delta: isize) {
        let len = self.snapshot.top_apps.len();
        if len == 0 {
            self.selected_app_index = 0;
            self.app_scroll_offset = 0;
            return;
        }

        let next = self.selected_app_index.saturating_add_signed(delta);
        self.selected_app_index = next.min(len.saturating_sub(1));
    }

    fn clamp_app_selection(&mut self) {
        let len = self.snapshot.top_apps.len();
        if len == 0 {
            self.selected_app_index = 0;
            self.app_scroll_offset = 0;
            return;
        }

        self.selected_app_index = self.selected_app_index.min(len.saturating_sub(1));
        self.app_scroll_offset = self.app_scroll_offset.min(self.selected_app_index);
    }

    fn move_heatmap_selection(&mut self, delta: isize) {
        let len = self.snapshot.heatmap_rows.len();
        if len == 0 {
            self.selected_heatmap_index = 0;
            self.heatmap_scroll_offset = 0;
            return;
        }

        let next = self.selected_heatmap_index.saturating_add_signed(delta);
        self.selected_heatmap_index = next.min(len.saturating_sub(1));
    }

    fn clamp_heatmap_selection(&mut self) {
        let len = self.snapshot.heatmap_rows.len();
        if len == 0 {
            self.selected_heatmap_index = 0;
            self.heatmap_scroll_offset = 0;
            return;
        }

        self.selected_heatmap_index = self.selected_heatmap_index.min(len.saturating_sub(1));
        self.heatmap_scroll_offset = self.heatmap_scroll_offset.min(self.selected_heatmap_index);
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
            ascii: false,
            snapshot: DashboardSnapshot {
                generated_at_local: Local::now(),
                range_days: 7,
                bucket_minutes: DEFAULT_BUCKET_MINUTES,
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
                categories: Vec::new(),
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
            selected_summary_index: 0,
            selected_app_index: 0,
            app_scroll_offset: 0,
            selected_heatmap_index: 0,
            heatmap_scroll_offset: 0,
            chart_metric: ChartMetric::Activity,
            chart_mode: ChartMode::Single,
            time_window: TimeWindow::TwentyFourHours,
            show_help: false,
            status_message: String::new(),
        }
    }

    /// Proves focus switching stays in a small finite cycle.
    #[test]
    fn tab_cycles_through_focus_sections() {
        let mut app = sample_app();

        app.handle_key(key(KeyCode::Tab));
        assert_eq!(app.focused_section, FocusSection::Apps);
        app.handle_key(key(KeyCode::Tab));
        assert_eq!(app.focused_section, FocusSection::Activity);
        app.handle_key(key(KeyCode::Tab));
        assert_eq!(app.focused_section, FocusSection::Heatmap);
        app.handle_key(key(KeyCode::BackTab));
        assert_eq!(app.focused_section, FocusSection::Activity);
    }

    #[test]
    fn summary_arrow_navigation_moves_across_cards_before_leaving_section() {
        let mut app = sample_app();
        app.focused_section = FocusSection::Summary;
        assert_eq!(app.selected_summary_index, 0);

        app.handle_key(key(KeyCode::Right));
        assert_eq!(app.selected_summary_index, 1);
        assert_eq!(app.focused_section, FocusSection::Summary);

        app.selected_summary_index = 4;
        app.handle_key(key(KeyCode::Right));
        assert_eq!(app.focused_section, FocusSection::Apps);
    }

    /// Proves j advances and k reverses the chart metric cycle.
    #[test]
    fn metric_cycle_advances_and_reverses_from_activity_panel() {
        let mut app = sample_app();
        app.focused_section = FocusSection::Activity;

        app.handle_key(key(KeyCode::Char('m')));
        assert_eq!(app.chart_metric, ChartMetric::KeyPresses);
        app.handle_key(key(KeyCode::Char('m')));
        assert_eq!(app.chart_metric, ChartMetric::LeftClicks);
        // k should go back
        app.handle_key(key(KeyCode::Char('k')));
        assert_eq!(app.chart_metric, ChartMetric::KeyPresses);
    }

    /// Proves [ ] cycle through time windows.
    #[test]
    fn bracket_cycles_time_window() {
        let mut app = sample_app();
        assert_eq!(app.time_window, TimeWindow::TwentyFourHours);

        app.handle_key(key(KeyCode::Char(']')));
        assert_eq!(app.time_window, TimeWindow::SevenDays);
        app.handle_key(key(KeyCode::Char('[')));
        assert_eq!(app.time_window, TimeWindow::TwentyFourHours);
    }

    #[test]
    fn v_toggles_chart_mode_on_activity_panel() {
        let mut app = sample_app();
        app.focused_section = FocusSection::Activity;
        assert_eq!(app.chart_mode, ChartMode::Single);
        app.handle_key(key(KeyCode::Char('v')));
        assert_eq!(app.chart_mode, ChartMode::Scope);
        app.handle_key(key(KeyCode::Char('v')));
        assert_eq!(app.chart_mode, ChartMode::Single);
    }

    #[test]
    fn app_list_navigation_moves_selection_when_apps_is_focused() {
        let mut app = sample_app();
        app.focused_section = FocusSection::Apps;
        app.snapshot.top_apps = vec![
            crate::tui::data::AppShare {
                label: "Ghostty".to_string(),
                detail: None,
                focus_seconds: 10,
                share_percent: 50,
                sparkline: vec![1, 2, 3, 4, 5, 4, 3, 2],
            },
            crate::tui::data::AppShare {
                label: "Firefox".to_string(),
                detail: None,
                focus_seconds: 5,
                share_percent: 25,
                sparkline: vec![0, 0, 1, 1, 0, 0, 0, 0],
            },
        ];

        app.handle_key(key(KeyCode::Down));
        assert_eq!(app.selected_app_index, 1);
        app.handle_key(key(KeyCode::Up));
        assert_eq!(app.selected_app_index, 0);
    }

    #[test]
    fn heatmap_navigation_moves_selection_when_daily_is_focused() {
        let mut app = sample_app();
        app.focused_section = FocusSection::Heatmap;
        app.snapshot.heatmap_rows = vec![
            crate::tui::data::DailyAverageRow {
                weekday: chrono::Weekday::Mon,
                values: [0.0; 5],
            },
            crate::tui::data::DailyAverageRow {
                weekday: chrono::Weekday::Tue,
                values: [0.0; 5],
            },
        ];

        app.handle_key(key(KeyCode::Down));
        assert_eq!(app.selected_heatmap_index, 1);
        app.handle_key(key(KeyCode::Up));
        assert_eq!(app.selected_heatmap_index, 0);
    }
}
