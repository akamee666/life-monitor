use anyhow::{Context, Result};
use chrono::{DateTime, Datelike, Duration, Local, NaiveDate, TimeZone, Utc, Weekday};
use rusqlite::{Connection, OptionalExtension};
use std::path::Path;

use crate::storage::localdb::{
    daily_activity_report, open_con_at, setup_database, DailyActivityRow,
};

const SERIES_BUCKET_MINUTES: i64 = 15;
const SERIES_BUCKET_COUNT: usize = 96;

/// Default series parameters for the 24 h window.
pub const DEFAULT_BUCKET_MINUTES: i64 = SERIES_BUCKET_MINUTES;
pub const DEFAULT_BUCKET_COUNT: usize = SERIES_BUCKET_COUNT;
const CATEGORY_LIMIT: usize = 7;
const CATEGORY_MEMBER_LIMIT: usize = 3;
const HEATMAP_METRIC_COUNT: usize = 5;
const APP_SPARKLINE_SAMPLES: usize = 48;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChartMetric {
    Activity,
    KeyPresses,
    LeftClicks,
    RightClicks,
    MiddleClicks,
    MouseMove,
}

impl ChartMetric {
    pub const ALL: [ChartMetric; 6] = [
        ChartMetric::Activity,
        ChartMetric::KeyPresses,
        ChartMetric::LeftClicks,
        ChartMetric::RightClicks,
        ChartMetric::MiddleClicks,
        ChartMetric::MouseMove,
    ];

    pub fn label(self) -> &'static str {
        match self {
            ChartMetric::Activity => "activity score",
            ChartMetric::KeyPresses => "key presses",
            ChartMetric::LeftClicks => "left clicks",
            ChartMetric::RightClicks => "right clicks",
            ChartMetric::MiddleClicks => "middle clicks",
            ChartMetric::MouseMove => "mouse movement",
        }
    }

    pub fn next(self) -> Self {
        let index = Self::ALL
            .iter()
            .position(|metric| *metric == self)
            .unwrap_or(0);
        Self::ALL[(index + 1) % Self::ALL.len()]
    }

    pub fn previous(self) -> Self {
        let index = Self::ALL
            .iter()
            .position(|metric| *metric == self)
            .unwrap_or(0);
        Self::ALL[(index + Self::ALL.len() - 1) % Self::ALL.len()]
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HeatmapMetric {
    KeyPresses,
    LeftClicks,
    RightClicks,
    MiddleClicks,
    MouseMove,
}

impl HeatmapMetric {
    pub const ALL: [HeatmapMetric; 5] = [
        HeatmapMetric::KeyPresses,
        HeatmapMetric::LeftClicks,
        HeatmapMetric::RightClicks,
        HeatmapMetric::MiddleClicks,
        HeatmapMetric::MouseMove,
    ];
}

#[derive(Debug, Clone, PartialEq)]
pub struct SummaryTotals {
    pub left_clicks: u64,
    pub right_clicks: u64,
    pub middle_clicks: u64,
    pub key_presses: u64,
    pub mouse_distance_cm: f64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct AppShare {
    pub label: String,
    pub detail: Option<String>,
    pub focus_seconds: u64,
    pub share_percent: u64,
    pub sparkline: Vec<u64>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct CategoryShare {
    pub label: String,
    pub focus_seconds: u64,
    pub share_percent: u64,
    pub top_members: Vec<AppShare>,
}

#[derive(Debug, Clone, PartialEq)]
struct FocusUsageRow {
    app_identifier: String,
    window_title: String,
    focus_seconds: u64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ActivityBucket {
    pub started_at_utc: DateTime<Utc>,
    pub activity_score: f64,
    pub key_presses: f64,
    pub clicks: f64,
    pub left_clicks: f64,
    pub right_clicks: f64,
    pub middle_clicks: f64,
    pub mouse_distance_cm: f64,
    pub focus_minutes: f64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct DailyAverageRow {
    pub weekday: Weekday,
    pub values: [f64; HEATMAP_METRIC_COUNT],
}

#[derive(Debug, Clone, PartialEq)]
pub struct DashboardStatus {
    pub source_count: usize,
    pub source_names: Vec<String>,
    pub current_app: Option<String>,
    pub last_activity_at_utc: Option<DateTime<Utc>>,
    pub sync_summary: String,
    pub db_path_display: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct DashboardSnapshot {
    pub generated_at_local: DateTime<Local>,
    pub range_days: u32,
    /// Width of each chart bucket in minutes (varies by time window).
    pub bucket_minutes: i64,
    pub summary: SummaryTotals,
    pub summary_label: String,
    pub top_activities: Vec<AppShare>,
    pub top_apps: Vec<AppShare>,
    pub categories: Vec<CategoryShare>,
    pub series_start_utc: DateTime<Utc>,
    pub series_buckets: Vec<ActivityBucket>,
    pub heatmap_rows: Vec<DailyAverageRow>,
    pub heatmap_maxima: [f64; HEATMAP_METRIC_COUNT],
    pub status: DashboardStatus,
}

pub fn load_dashboard_snapshot(
    db_path: &Path,
    range_days: u32,
    bucket_minutes: i64,
    bucket_count: usize,
) -> Result<DashboardSnapshot> {
    let conn = open_con_at(db_path)?;
    setup_database(&conn)?;

    let effective_days = range_days.max(1);
    // Heatmap always shows the last 7 calendar days so it reflects the current week
    // regardless of which time window is selected for the chart.
    let heatmap_days = 7;
    let daily_rows = daily_activity_report(&conn, heatmap_days)?;
    let focus_rows = load_focus_usage_rows(&conn, effective_days)?;
    let series_start = aligned_series_start(Utc::now(), bucket_minutes, bucket_count)?;

    let summary = load_summary_totals(&conn)?;
    let top_activities = aggregate_top_activities(&focus_rows, 5);
    let mut top_apps = aggregate_top_apps(&focus_rows, usize::MAX);
    let categories = aggregate_categories(&focus_rows, CATEGORY_LIMIT, CATEGORY_MEMBER_LIMIT);
    let series_buckets = load_activity_series(&conn, series_start, bucket_minutes, bucket_count)?;
    let (heatmap_rows, heatmap_maxima) = build_daily_average_heatmap(&daily_rows, heatmap_days)?;
    let status = load_dashboard_status(&conn, db_path)?;
    attach_app_sparklines(&conn, &mut top_apps, effective_days, APP_SPARKLINE_SAMPLES)?;

    Ok(DashboardSnapshot {
        generated_at_local: Local::now(),
        range_days: effective_days,
        bucket_minutes,
        summary,
        summary_label: "overall activity".to_string(),
        top_activities,
        top_apps,
        categories,
        series_start_utc: series_start,
        series_buckets,
        heatmap_rows,
        heatmap_maxima,
        status,
    })
}

fn load_summary_totals(conn: &Connection) -> Result<SummaryTotals> {
    conn.query_row(
        "
        SELECT
            COALESCE(SUM(left_clicks), 0),
            COALESCE(SUM(right_clicks), 0),
            COALESCE(SUM(middle_clicks), 0),
            COALESCE(SUM(key_presses), 0),
            COALESCE(SUM(mouse_distance_cm), 0.0)
        FROM input_buckets
        ",
        [],
        |row| {
            Ok(SummaryTotals {
                left_clicks: row.get(0)?,
                right_clicks: row.get(1)?,
                middle_clicks: row.get(2)?,
                key_presses: row.get(3)?,
                mouse_distance_cm: row.get(4)?,
            })
        },
    )
    .with_context(|| "Failed to load all-time summary totals for dashboard")
}

fn load_focus_usage_rows(conn: &Connection, days: u32) -> Result<Vec<FocusUsageRow>> {
    let since = (Utc::now() - Duration::days(days.max(1) as i64)).to_rfc3339();
    let mut stmt = conn.prepare(
        "
        SELECT app_identifier, window_title, SUM(focus_seconds) AS total_focus_seconds
        FROM focus_buckets
        WHERE bucket_start_utc >= ?1
        GROUP BY app_identifier, window_title
        ORDER BY total_focus_seconds DESC, app_identifier ASC, window_title ASC
        ",
    )?;

    let rows = stmt.query_map([since], |row| {
        Ok(FocusUsageRow {
            app_identifier: row.get(0)?,
            window_title: row.get(1)?,
            focus_seconds: row.get::<_, Option<u64>>(2)?.unwrap_or(0),
        })
    })?;

    rows.collect::<rusqlite::Result<Vec<_>>>()
        .with_context(|| "Failed to load focus usage rows for dashboard")
}

fn aggregate_top_activities(rows: &[FocusUsageRow], top_n: usize) -> Vec<AppShare> {
    let mut aggregated = std::collections::BTreeMap::<String, u64>::new();
    for row in rows {
        *aggregated
            .entry(classify_activity_label(
                &row.app_identifier,
                &row.window_title,
            ))
            .or_default() += row.focus_seconds;
    }
    ranked_shares(
        aggregated
            .into_iter()
            .map(|(label, focus_seconds)| AppShare {
                label,
                detail: None,
                focus_seconds,
                share_percent: 0,
                sparkline: Vec::new(),
            })
            .collect(),
        top_n,
    )
}

fn aggregate_top_apps(rows: &[FocusUsageRow], top_n: usize) -> Vec<AppShare> {
    let mut aggregated =
        std::collections::BTreeMap::<String, (u64, std::collections::BTreeMap<String, u64>)>::new();
    for row in rows {
        let label = friendly_app_name(&row.app_identifier);
        let detail = activity_context(&row.app_identifier, &row.window_title);
        let entry = aggregated
            .entry(label)
            .or_insert_with(|| (0, std::collections::BTreeMap::new()));
        entry.0 += row.focus_seconds;
        if let Some(detail) = detail {
            *entry.1.entry(detail).or_default() += row.focus_seconds;
        }
    }

    ranked_shares(
        aggregated
            .into_iter()
            .map(|(label, (focus_seconds, details))| {
                let detail = details
                    .into_iter()
                    .max_by(|left, right| left.1.cmp(&right.1).then_with(|| right.0.cmp(&left.0)))
                    .map(|(detail, _)| detail);
                AppShare {
                    label,
                    detail,
                    focus_seconds,
                    share_percent: 0,
                    sparkline: Vec::new(),
                }
            })
            .collect(),
        top_n,
    )
}

fn aggregate_categories(
    rows: &[FocusUsageRow],
    top_n: usize,
    member_limit: usize,
) -> Vec<CategoryShare> {
    let total_focus = rows.iter().map(|row| row.focus_seconds).sum::<u64>().max(1);
    let mut categories =
        std::collections::BTreeMap::<String, (u64, std::collections::BTreeMap<String, u64>)>::new();

    for row in rows {
        let category = classify_category(&row.app_identifier, &row.window_title).to_string();
        let member = category_member_label(&row.app_identifier, &row.window_title);
        let entry = categories
            .entry(category)
            .or_insert_with(|| (0, std::collections::BTreeMap::new()));
        entry.0 += row.focus_seconds;
        *entry.1.entry(member).or_default() += row.focus_seconds;
    }

    let mut ranked = categories
        .into_iter()
        .filter(|(_, (focus_seconds, _))| *focus_seconds > 0)
        .map(|(label, (focus_seconds, members))| {
            let mut top_members = members
                .into_iter()
                .map(|(label, focus_seconds)| AppShare {
                    label,
                    detail: None,
                    focus_seconds,
                    share_percent: percent_of(focus_seconds, total_focus),
                    sparkline: Vec::new(),
                })
                .collect::<Vec<_>>();
            top_members.sort_by(|left, right| {
                right
                    .focus_seconds
                    .cmp(&left.focus_seconds)
                    .then_with(|| left.label.cmp(&right.label))
            });
            top_members.truncate(member_limit);

            CategoryShare {
                label,
                focus_seconds,
                share_percent: percent_of(focus_seconds, total_focus),
                top_members,
            }
        })
        .collect::<Vec<_>>();

    ranked.sort_by(|left, right| {
        right
            .focus_seconds
            .cmp(&left.focus_seconds)
            .then_with(|| left.label.cmp(&right.label))
    });
    ranked.truncate(top_n);
    ranked
}

fn ranked_shares(mut shares: Vec<AppShare>, top_n: usize) -> Vec<AppShare> {
    shares.sort_by(|left, right| {
        right
            .focus_seconds
            .cmp(&left.focus_seconds)
            .then_with(|| left.label.cmp(&right.label))
    });

    let total_focus = shares
        .iter()
        .map(|entry| entry.focus_seconds)
        .sum::<u64>()
        .max(1);
    let mut ranked = shares
        .into_iter()
        .take(top_n)
        .map(|mut entry| {
            entry.share_percent = percent_of(entry.focus_seconds, total_focus);
            entry
        })
        .collect::<Vec<_>>();

    let captured_focus = ranked.iter().map(|entry| entry.focus_seconds).sum::<u64>();
    let other_focus = total_focus.saturating_sub(captured_focus);
    if other_focus > 0 {
        ranked.push(AppShare {
            label: "Other".to_string(),
            detail: None,
            focus_seconds: other_focus,
            share_percent: percent_of(other_focus, total_focus),
            sparkline: Vec::new(),
        });
    }

    ranked
}

fn percent_of(value: u64, total: u64) -> u64 {
    if total == 0 {
        return 0;
    }

    ((value * 100) + (total / 2)) / total
}

fn attach_app_sparklines(
    conn: &Connection,
    apps: &mut [AppShare],
    days: u32,
    sample_count: usize,
) -> Result<()> {
    if apps.is_empty() || sample_count == 0 {
        return Ok(());
    }

    let since = Utc::now() - Duration::days(days.max(1) as i64);
    let until = Utc::now();
    let total_seconds = (until - since).num_seconds().max(1) as u64;
    let mut series_by_label = apps
        .iter()
        .map(|app| (app.label.clone(), vec![0_u64; sample_count]))
        .collect::<std::collections::BTreeMap<_, _>>();

    let mut stmt = conn.prepare(
        "
        SELECT app_identifier, bucket_start_utc, COALESCE(SUM(focus_seconds), 0)
        FROM focus_buckets
        WHERE bucket_start_utc >= ?1
        GROUP BY app_identifier, bucket_start_utc
        ORDER BY bucket_start_utc ASC
        ",
    )?;
    let rows = stmt.query_map([since.to_rfc3339()], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, Option<u64>>(2)?.unwrap_or(0),
        ))
    })?;

    for row in rows {
        let (app_identifier, bucket_start_raw, focus_seconds) = row?;
        let label = friendly_app_name(&app_identifier);
        let Some(series) = series_by_label.get_mut(&label) else {
            continue;
        };
        let Ok(bucket_start) = DateTime::parse_from_rfc3339(&bucket_start_raw) else {
            continue;
        };
        let bucket_start = bucket_start.with_timezone(&Utc);
        let elapsed = bucket_start
            .signed_duration_since(since)
            .num_seconds()
            .clamp(0, total_seconds as i64) as u64;
        let index = ((elapsed * sample_count as u64) / total_seconds)
            .min(sample_count.saturating_sub(1) as u64) as usize;
        series[index] = series[index].saturating_add(focus_seconds);
    }

    for app in apps {
        app.sparkline = series_by_label
            .remove(&app.label)
            .unwrap_or_else(|| vec![0; sample_count]);
    }

    Ok(())
}

fn normalize_app_id(app_identifier: &str) -> String {
    let trimmed = app_identifier.trim();
    if trimmed.is_empty() {
        return "unknown".to_string();
    }

    trimmed
        .rsplit_once(['/', '\\'])
        .map(|(_, suffix)| suffix)
        .unwrap_or(trimmed)
        .to_ascii_lowercase()
}

fn friendly_app_name(app_identifier: &str) -> String {
    match normalize_app_id(app_identifier).as_str() {
        "brave-browser" | "brave.exe" => "Brave Browser".to_string(),
        "firefox" | "firefox.exe" => "Firefox".to_string(),
        "chromium" | "chromium-browser" | "chrome" | "chrome.exe" => "Chromium".to_string(),
        "com.mitchellh.ghostty" | "ghostty" | "ghostty.exe" => "Ghostty".to_string(),
        "kitty" | "kitty.exe" => "Kitty".to_string(),
        "wezterm-gui" | "wezterm" | "wezterm.exe" => "WezTerm".to_string(),
        "alacritty" | "alacritty.exe" => "Alacritty".to_string(),
        "code" | "code.exe" => "VS Code".to_string(),
        "codium" | "codium.exe" => "VSCodium".to_string(),
        "nvim" | "neovim" => "Neovim".to_string(),
        "spotify" | "spotify.exe" => "Spotify".to_string(),
        "obsidian" | "obsidian.exe" => "Obsidian".to_string(),
        "slack" | "slack.exe" => "Slack".to_string(),
        "discord" | "discord.exe" => "Discord".to_string(),
        "steam" | "steam.exe" => "Steam".to_string(),
        other => title_case_identifier(other),
    }
}

fn classify_activity_label(app_identifier: &str, window_title: &str) -> String {
    let normalized = normalize_app_id(app_identifier);
    let title = window_title.trim();

    if is_browser_app(&normalized) {
        if let Some(site) = browser_context_label(title) {
            return site;
        }
        return "web browsing".to_string();
    }

    match normalized.as_str() {
        "spotify" | "spotify.exe" => "music".to_string(),
        "obsidian" | "obsidian.exe" => "writing".to_string(),
        "nvim" | "neovim" | "code" | "code.exe" | "codium" | "codium.exe" => "coding".to_string(),
        "com.mitchellh.ghostty"
        | "ghostty"
        | "ghostty.exe"
        | "kitty"
        | "kitty.exe"
        | "wezterm"
        | "wezterm-gui"
        | "wezterm.exe"
        | "alacritty"
        | "alacritty.exe" => "terminal".to_string(),
        "discord" | "discord.exe" | "slack" | "slack.exe" => "chat".to_string(),
        "steam" | "steam.exe" => "gaming".to_string(),
        other => {
            if !title.is_empty() && title != friendly_app_name(other) {
                friendly_window_title(title)
            } else {
                title_case_identifier(other)
            }
        }
    }
}

fn activity_context(app_identifier: &str, window_title: &str) -> Option<String> {
    let normalized = normalize_app_id(app_identifier);
    if is_browser_app(&normalized) {
        return browser_context_label(window_title);
    }

    let title = friendly_window_title(window_title);
    if title.is_empty() || title == friendly_app_name(app_identifier) {
        None
    } else {
        Some(title)
    }
}

fn is_browser_app(normalized: &str) -> bool {
    matches!(
        normalized,
        "brave-browser"
            | "brave.exe"
            | "firefox"
            | "firefox.exe"
            | "chromium"
            | "chromium-browser"
            | "chrome"
            | "chrome.exe"
    )
}

fn browser_context_label(window_title: &str) -> Option<String> {
    let title = window_title.trim();
    if title.is_empty() {
        return None;
    }

    let fragments = title
        .split(" - ")
        .map(str::trim)
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>();
    for fragment in fragments.iter().rev() {
        let lowered = fragment.to_ascii_lowercase();
        if lowered.contains("brave") || lowered.contains("firefox") || lowered.contains("chrome") {
            continue;
        }
        if lowered.contains("youtube") {
            return Some("YouTube".to_string());
        }
        if lowered.contains("tiktok") {
            return Some("TikTok".to_string());
        }
        if lowered.contains("twitter") || lowered == "x" || lowered.contains("x.com") {
            return Some("X".to_string());
        }
        if lowered.contains("github") {
            return Some("GitHub".to_string());
        }
        if lowered.contains("reddit") {
            return Some("Reddit".to_string());
        }
        if lowered.contains("gmail") || lowered.contains("mail") {
            return Some("Mail".to_string());
        }
        return Some(friendly_window_title(fragment));
    }

    None
}

fn classify_category(app_identifier: &str, window_title: &str) -> &'static str {
    let normalized = normalize_app_id(app_identifier);
    if let Some(site) = browser_category_site(window_title) {
        return match site {
            "github.com" | "docs.rs" | "stackoverflow.com" => "Coding",
            "youtube.com" | "tiktok.com" | "instagram.com" | "x.com" | "reddit.com"
            | "facebook.com" | "twitch.tv" => "Social Media",
            "gmail.com" | "mail" | "calendar" => "Productivity",
            _ => "Other",
        };
    }

    match normalized.as_str() {
        "nvim" | "neovim" | "code" | "code.exe" | "codium" | "codium.exe" | "cursor"
        | "cursor.exe" | "zed" | "zed.exe" | "rider" | "rider64.exe" | "clion" | "clion64.exe" => {
            "Coding"
        }
        "com.mitchellh.ghostty"
        | "ghostty"
        | "ghostty.exe"
        | "kitty"
        | "kitty.exe"
        | "wezterm"
        | "wezterm-gui"
        | "wezterm.exe"
        | "alacritty"
        | "alacritty.exe"
        | "konsole"
        | "konsole.exe"
        | "xterm"
        | "gnome-terminal-server"
        | "gnome-terminal"
        | "windows terminal"
        | "windowsterminal.exe"
        | "wt.exe" => "Terminal",
        "discord" | "discord.exe" | "slack" | "slack.exe" | "telegram-desktop" | "telegram"
        | "telegram.exe" | "whatsapp" | "whatsapp.exe" | "teams" | "teams.exe" | "ms-teams.exe"
        | "element" | "element.exe" => "Communication",
        "spotify" | "spotify.exe" | "mpv" | "mpv.exe" | "vlc" | "vlc.exe" | "steam"
        | "steam.exe" | "lutris" | "lutris.exe" | "heroic" | "heroic.exe" => "Entertainment",
        "obsidian" | "obsidian.exe" | "libreoffice" | "soffice.bin" | "soffice" | "thunderbird"
        | "thunderbird.exe" | "calendar" => "Productivity",
        _ => "Other",
    }
}

fn category_member_label(app_identifier: &str, window_title: &str) -> String {
    browser_category_site(window_title)
        .map(String::from)
        .unwrap_or_else(|| friendly_app_name(app_identifier))
}

fn browser_category_site(window_title: &str) -> Option<&'static str> {
    let title = window_title.trim();
    if title.is_empty() {
        return None;
    }

    let lowered = title.to_ascii_lowercase();
    if lowered.contains("github") {
        Some("github.com")
    } else if lowered.contains("docs.rs") {
        Some("docs.rs")
    } else if lowered.contains("stackoverflow") || lowered.contains("stack overflow") {
        Some("stackoverflow.com")
    } else if lowered.contains("youtube") {
        Some("youtube.com")
    } else if lowered.contains("tiktok") {
        Some("tiktok.com")
    } else if lowered.contains("instagram") {
        Some("instagram.com")
    } else if lowered.contains("twitter") || lowered.contains("x.com") || lowered == "x" {
        Some("x.com")
    } else if lowered.contains("reddit") {
        Some("reddit.com")
    } else if lowered.contains("facebook") {
        Some("facebook.com")
    } else if lowered.contains("twitch") {
        Some("twitch.tv")
    } else if lowered.contains("gmail") {
        Some("gmail.com")
    } else if lowered.contains("calendar") {
        Some("calendar")
    } else if lowered.contains("mail") {
        Some("mail")
    } else {
        None
    }
}

fn friendly_window_title(window_title: &str) -> String {
    let title = window_title.trim();
    if title.is_empty() {
        return String::new();
    }
    title
        .split(" - ")
        .next()
        .map(str::trim)
        .filter(|part| !part.is_empty())
        .unwrap_or(title)
        .to_string()
}

fn title_case_identifier(value: &str) -> String {
    value
        .split(['.', '-', '_', ' '])
        .filter(|part| !part.is_empty())
        .map(|part| {
            let mut chars = part.chars();
            match chars.next() {
                Some(first) => {
                    format!(
                        "{}{}",
                        first.to_ascii_uppercase(),
                        chars.as_str().to_ascii_lowercase()
                    )
                }
                None => String::new(),
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn load_activity_series(
    conn: &Connection,
    series_start: DateTime<Utc>,
    bucket_minutes: i64,
    bucket_count: usize,
) -> Result<Vec<ActivityBucket>> {
    let mut buckets = empty_series(series_start, bucket_minutes, bucket_count);

    let mut input_stmt = conn.prepare(
        "
        SELECT bucket_start_utc, key_presses, left_clicks, right_clicks, middle_clicks, mouse_distance_cm
        FROM input_buckets
        WHERE bucket_start_utc >= ?1
        ORDER BY bucket_start_utc ASC
        ",
    )?;
    let input_rows = input_stmt.query_map([series_start.to_rfc3339()], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, u64>(1)?,
            row.get::<_, u64>(2)?,
            row.get::<_, u64>(3)?,
            row.get::<_, u64>(4)?,
            row.get::<_, f64>(5)?,
        ))
    })?;
    for row in input_rows {
        let (started_at_utc, key_presses, left_clicks, right_clicks, middle_clicks, mouse_cm) =
            row?;
        let started_at_utc = parse_rfc3339(&started_at_utc)?;
        if let Some(bucket) = bucket_mut(&mut buckets, series_start, started_at_utc, bucket_minutes)
        {
            let clicks = (left_clicks + right_clicks + middle_clicks) as f64;
            bucket.key_presses += key_presses as f64;
            bucket.clicks += clicks;
            bucket.left_clicks += left_clicks as f64;
            bucket.right_clicks += right_clicks as f64;
            bucket.middle_clicks += middle_clicks as f64;
            bucket.mouse_distance_cm += mouse_cm;
            bucket.activity_score += key_presses as f64 + clicks * 6.0 + mouse_cm * 8.0;
        }
    }

    let mut focus_stmt = conn.prepare(
        "
        SELECT bucket_start_utc, focus_seconds
        FROM focus_buckets
        WHERE bucket_start_utc >= ?1
        ORDER BY bucket_start_utc ASC
        ",
    )?;
    let focus_rows = focus_stmt.query_map([series_start.to_rfc3339()], |row| {
        Ok((row.get::<_, String>(0)?, row.get::<_, u64>(1)?))
    })?;
    for row in focus_rows {
        let (started_at_utc, focus_seconds) = row?;
        let started_at_utc = parse_rfc3339(&started_at_utc)?;
        if let Some(bucket) = bucket_mut(&mut buckets, series_start, started_at_utc, bucket_minutes)
        {
            let focus_minutes = focus_seconds as f64 / 60.0;
            bucket.focus_minutes += focus_minutes;
            bucket.activity_score += focus_minutes * 2.5;
        }
    }

    Ok(buckets)
}

fn aligned_series_start(
    now: DateTime<Utc>,
    bucket_minutes: i64,
    bucket_count: usize,
) -> Result<DateTime<Utc>> {
    let bucket_seconds = bucket_minutes * 60;
    let aligned_end_seconds = now.timestamp().div_euclid(bucket_seconds) * bucket_seconds;
    let aligned_end = Utc
        .timestamp_opt(aligned_end_seconds, 0)
        .single()
        .context("Failed to align dashboard series window to bucket boundary")?;
    Ok(aligned_end - Duration::minutes(bucket_minutes * bucket_count as i64))
}

fn bucket_mut(
    buckets: &mut [ActivityBucket],
    series_start: DateTime<Utc>,
    bucket_start: DateTime<Utc>,
    bucket_minutes: i64,
) -> Option<&mut ActivityBucket> {
    let delta = bucket_start
        .signed_duration_since(series_start)
        .num_minutes();
    let slot = delta.div_euclid(bucket_minutes);
    if slot < 0 || slot as usize >= buckets.len() {
        return None;
    }

    buckets.get_mut(slot as usize)
}

fn empty_series(
    series_start: DateTime<Utc>,
    bucket_minutes: i64,
    bucket_count: usize,
) -> Vec<ActivityBucket> {
    (0..bucket_count)
        .map(|index| ActivityBucket {
            started_at_utc: series_start + Duration::minutes(index as i64 * bucket_minutes),
            activity_score: 0.0,
            key_presses: 0.0,
            clicks: 0.0,
            left_clicks: 0.0,
            right_clicks: 0.0,
            middle_clicks: 0.0,
            mouse_distance_cm: 0.0,
            focus_minutes: 0.0,
        })
        .collect()
}

fn build_daily_average_heatmap(
    rows: &[DailyActivityRow],
    range_days: u32,
) -> Result<(Vec<DailyAverageRow>, [f64; HEATMAP_METRIC_COUNT])> {
    let mut totals = [[0.0_f64; HEATMAP_METRIC_COUNT]; 7];
    for row in rows {
        let weekday = NaiveDate::parse_from_str(&row.local_date, "%Y-%m-%d")
            .with_context(|| format!("Failed to parse local_date {}", row.local_date))?
            .weekday();
        let entry = &mut totals[weekday_index(weekday)];
        entry[0] += row.key_presses as f64;
        entry[1] += row.left_clicks as f64;
        entry[2] += row.right_clicks as f64;
        entry[3] += row.middle_clicks as f64;
        entry[4] += row.mouse_distance_cm;
    }

    let weekday_counts = weekday_occurrences(range_days);
    let mut maxima = [0.0; HEATMAP_METRIC_COUNT];
    let mut averaged_rows = Vec::with_capacity(7);

    for weekday in ordered_weekdays() {
        let totals_for_day = totals[weekday_index(weekday)];
        let divisor = weekday_counts[weekday_index(weekday)].max(1) as f64;
        let mut averages = [0.0; HEATMAP_METRIC_COUNT];
        for (index, total) in totals_for_day.iter().enumerate() {
            averages[index] = *total / divisor;
            if maxima[index] < averages[index] {
                maxima[index] = averages[index];
            }
        }
        averaged_rows.push(DailyAverageRow {
            weekday,
            values: averages,
        });
    }

    Ok((averaged_rows, maxima))
}

fn weekday_occurrences(range_days: u32) -> [u32; 7] {
    let mut counts = [0_u32; 7];
    let today = Local::now().date_naive();
    for offset in 0..range_days.max(1) {
        let day = today - Duration::days(offset as i64);
        counts[weekday_index(day.weekday())] += 1;
    }
    counts
}

fn weekday_index(weekday: Weekday) -> usize {
    weekday.num_days_from_sunday() as usize
}

fn ordered_weekdays() -> [Weekday; 7] {
    [
        Weekday::Sun,
        Weekday::Mon,
        Weekday::Tue,
        Weekday::Wed,
        Weekday::Thu,
        Weekday::Fri,
        Weekday::Sat,
    ]
}

fn load_dashboard_status(conn: &Connection, db_path: &Path) -> Result<DashboardStatus> {
    let source_names = load_source_names(conn)?;
    let current_app = conn
        .query_row(
            "
            SELECT app_identifier, window_title
            FROM focus_buckets
            ORDER BY bucket_start_utc DESC
            LIMIT 1
            ",
            [],
            |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
        )
        .optional()?
        .map(|(app_identifier, window_title)| {
            activity_context(&app_identifier, &window_title)
                .map(|context| format!("{} · {}", friendly_app_name(&app_identifier), context))
                .unwrap_or_else(|| friendly_app_name(&app_identifier))
        });

    let last_input_activity = conn
        .query_row("SELECT MAX(bucket_end_utc) FROM input_buckets", [], |row| {
            row.get::<_, Option<String>>(0)
        })
        .optional()?
        .flatten();
    let last_focus_activity = conn
        .query_row("SELECT MAX(bucket_end_utc) FROM focus_buckets", [], |row| {
            row.get::<_, Option<String>>(0)
        })
        .optional()?
        .flatten();

    let last_activity_at_utc = [last_input_activity, last_focus_activity]
        .into_iter()
        .flatten()
        .map(|value| parse_rfc3339(&value))
        .collect::<Result<Vec<_>>>()?
        .into_iter()
        .max();

    Ok(DashboardStatus {
        source_count: source_names.len(),
        source_names,
        current_app,
        last_activity_at_utc,
        sync_summary: load_sync_summary(conn)?,
        db_path_display: db_path.display().to_string(),
    })
}

fn load_source_names(conn: &Connection) -> Result<Vec<String>> {
    let mut stmt = conn.prepare("SELECT source_name FROM sources ORDER BY source_name ASC")?;
    let rows = stmt.query_map([], |row| row.get::<_, String>(0))?;
    rows.collect::<rusqlite::Result<Vec<_>>>()
        .with_context(|| "Failed to load source names")
}

fn load_sync_summary(conn: &Connection) -> Result<String> {
    let row = conn
        .query_row(
            "
            SELECT sync_enabled, last_push_at_utc, last_pull_at_utc
            FROM sync_state
            ORDER BY sync_enabled DESC, rowid ASC
            LIMIT 1
            ",
            [],
            |row| {
                Ok((
                    row.get::<_, i64>(0)? != 0,
                    row.get::<_, Option<String>>(1)?,
                    row.get::<_, Option<String>>(2)?,
                ))
            },
        )
        .optional()?;

    Ok(match row {
        Some((true, last_push, last_pull)) => format!(
            "sync on | push {} | pull {}",
            short_timestamp(last_push.as_deref()),
            short_timestamp(last_pull.as_deref())
        ),
        Some((false, _, _)) => "sync configured: off".to_string(),
        None => "local-only".to_string(),
    })
}

fn short_timestamp(value: Option<&str>) -> String {
    value
        .and_then(|timestamp| parse_rfc3339(timestamp).ok())
        .map(|timestamp| {
            timestamp
                .with_timezone(&Local)
                .format("%m-%d %H:%M")
                .to_string()
        })
        .unwrap_or_else(|| "never".to_string())
}

fn parse_rfc3339(value: &str) -> Result<DateTime<Utc>> {
    Ok(DateTime::parse_from_rfc3339(value)
        .with_context(|| format!("Failed to parse RFC3339 timestamp {value}"))?
        .with_timezone(&Utc))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::localdb::DailyActivityRow;

    fn sample_daily_row(local_date: &str, weekday: Weekday) -> DailyActivityRow {
        let date = match weekday {
            Weekday::Sun => "2026-04-19",
            Weekday::Mon => "2026-04-20",
            Weekday::Tue => "2026-04-21",
            Weekday::Wed => "2026-04-22",
            Weekday::Thu => "2026-04-23",
            Weekday::Fri => "2026-04-24",
            Weekday::Sat => "2026-04-25",
        };
        DailyActivityRow {
            local_date: if local_date.is_empty() {
                date.to_string()
            } else {
                local_date.to_string()
            },
            source_uuid: "source-a".to_string(),
            source_name: "desktop".to_string(),
            platform: "linux".to_string(),
            key_presses: 10,
            left_clicks: 2,
            right_clicks: 1,
            middle_clicks: 0,
            mouse_distance_cm: 5.0,
            scroll_vertical_cm: 0.0,
            scroll_horizontal_cm: 0.0,
            focus_seconds: 3600,
        }
    }

    /// Proves activity aggregation keeps the dominant user-facing categories explicit and rolls
    /// the rest into a stable `Other` bucket, which makes the dashboard readable on wide and
    /// narrow terminals.
    #[test]
    fn aggregate_top_activities_groups_long_tail_into_other() {
        let rows = vec![
            FocusUsageRow {
                app_identifier: "firefox".to_string(),
                window_title: "YouTube - Mozilla Firefox".to_string(),
                focus_seconds: 100,
            },
            FocusUsageRow {
                app_identifier: "kitty".to_string(),
                window_title: "shell".to_string(),
                focus_seconds: 80,
            },
            FocusUsageRow {
                app_identifier: "nvim".to_string(),
                window_title: "main.rs".to_string(),
                focus_seconds: 70,
            },
            FocusUsageRow {
                app_identifier: "discord".to_string(),
                window_title: "general".to_string(),
                focus_seconds: 50,
            },
        ];

        let apps = aggregate_top_activities(&rows, 2);

        assert_eq!(apps[0].label, "YouTube");
        assert_eq!(apps[1].label, "terminal");
        assert_eq!(apps[2].label, "Other");
        assert_eq!(apps[2].focus_seconds, 120);
    }

    /// Proves browser-heavy rows are converted into friendlier app and context labels, which
    /// avoids raw package identifiers such as `brave-browser` leaking into the dashboard.
    #[test]
    fn aggregate_top_apps_prefers_friendly_names_and_dominant_context() {
        let rows = vec![
            FocusUsageRow {
                app_identifier: "brave-browser".to_string(),
                window_title: "TikTok - Brave".to_string(),
                focus_seconds: 120,
            },
            FocusUsageRow {
                app_identifier: "brave-browser".to_string(),
                window_title: "GitHub - Brave".to_string(),
                focus_seconds: 30,
            },
        ];

        let apps = aggregate_top_apps(&rows, 5);

        assert_eq!(apps[0].label, "Brave Browser");
        assert_eq!(apps[0].detail.as_deref(), Some("TikTok"));
    }

    #[test]
    fn aggregate_categories_groups_focus_into_static_product_buckets() {
        let rows = vec![
            FocusUsageRow {
                app_identifier: "nvim".to_string(),
                window_title: "main.rs".to_string(),
                focus_seconds: 200,
            },
            FocusUsageRow {
                app_identifier: "firefox".to_string(),
                window_title: "GitHub - Mozilla Firefox".to_string(),
                focus_seconds: 120,
            },
            FocusUsageRow {
                app_identifier: "ghostty".to_string(),
                window_title: "shell".to_string(),
                focus_seconds: 100,
            },
            FocusUsageRow {
                app_identifier: "discord".to_string(),
                window_title: "general".to_string(),
                focus_seconds: 80,
            },
        ];

        let categories = aggregate_categories(&rows, 7, 3);

        assert_eq!(categories[0].label, "Coding");
        assert_eq!(categories[0].focus_seconds, 320);
        assert_eq!(categories[0].top_members[0].label, "Neovim");
        assert_eq!(categories[0].top_members[1].label, "github.com");
        assert_eq!(categories[1].label, "Terminal");
        assert_eq!(categories[2].label, "Communication");
    }

    /// Proves the 24-hour series builder preserves quarter-hour slot alignment and fills missing
    /// slots with zero-valued buckets so the chart stays stable even on sparse datasets.
    #[test]
    fn empty_series_creates_full_zero_filled_window() {
        let start = Utc::now() - Duration::hours(24);
        let buckets = empty_series(start, SERIES_BUCKET_MINUTES, SERIES_BUCKET_COUNT);

        assert_eq!(buckets.len(), SERIES_BUCKET_COUNT);
        assert_eq!(buckets[0].started_at_utc, start);
        assert!(buckets.iter().all(|bucket| bucket.activity_score == 0.0));
    }

    /// Proves weekday averages divide totals by the number of matching weekdays in the selected
    /// range instead of by the number of activity rows, which keeps quiet weekdays honest.
    #[test]
    fn build_daily_average_heatmap_uses_calendar_occurrences() -> Result<()> {
        let rows = vec![
            sample_daily_row("2026-04-20", Weekday::Mon),
            DailyActivityRow {
                local_date: "2026-04-27".to_string(),
                ..sample_daily_row("", Weekday::Mon)
            },
        ];

        let (heatmap_rows, maxima) = build_daily_average_heatmap(&rows, 14)?;
        let monday = heatmap_rows
            .iter()
            .find(|row| row.weekday == Weekday::Mon)
            .expect("monday row should exist");

        assert_eq!(monday.values[0], 10.0);
        assert!(maxima[0] >= 10.0);
        Ok(())
    }
}
