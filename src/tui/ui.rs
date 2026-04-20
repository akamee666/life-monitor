use chrono::{DateTime, Datelike, Local, Utc, Weekday};
use ratatui::{
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    symbols,
    text::{Line, Span},
    widgets::{
        Axis, Block, Borders, Cell, Chart, Clear, Dataset, Gauge, GraphType, Paragraph, Row, Table,
        Wrap,
    },
    Frame,
};

use crate::tui::{
    app::{DashboardApp, FocusSection},
    data::{ActivityBucket, ChartMetric, DashboardSnapshot, HeatmapMetric},
};

const BG: Color = Color::Black;
const FG: Color = Color::Rgb(130, 255, 150);
const MUTED: Color = Color::Rgb(70, 120, 80);
const PANEL: Color = Color::Rgb(35, 80, 40);
const ACCENT: Color = Color::Rgb(100, 255, 120);
const DIM: Color = Color::Rgb(25, 45, 25);
const TODAY_HIGHLIGHT: Color = Color::Rgb(150, 255, 170);

pub fn render(frame: &mut Frame, app: &DashboardApp) {
    frame.render_widget(
        Block::default().style(Style::default().bg(BG)),
        frame.area(),
    );

    let area = frame.area();
    let layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Length(7),
            Constraint::Length(if area.width >= 120 { 14 } else { 16 }),
            Constraint::Min(12),
            Constraint::Length(2),
        ])
        .split(area);

    render_header(frame, layout[0], app);
    render_summary_cards(frame, layout[1], app);
    render_apps_section(frame, layout[2], app);
    render_lower_section(frame, layout[3], app);
    render_footer(frame, layout[4], app);

    if app.show_help {
        render_help_modal(frame, area, app);
    }
}

fn render_header(frame: &mut Frame, area: Rect, app: &DashboardApp) {
    let title = Line::from(vec![
        Span::styled(
            "life-monitor",
            Style::default().fg(ACCENT).add_modifier(Modifier::BOLD),
        ),
        Span::styled("  analytics dashboard", Style::default().fg(FG)),
    ]);
    let subtitle = Line::from(vec![
        Span::styled(
            format!(
                "window {}d | focus {} | chart metric {}",
                app.snapshot.range_days,
                app.focused_section.label(),
                app.chart_metric.label()
            ),
            Style::default().fg(MUTED),
        ),
        Span::raw("  "),
        Span::styled(
            format!(
                "dashboard refreshed {} ago | newest local bucket {}",
                format_relative_local(app.snapshot.generated_at_local),
                format_relative_utc(app.snapshot.status.last_activity_at_utc)
            ),
            Style::default().fg(FG),
        ),
    ]);

    let block = Block::default()
        .borders(Borders::BOTTOM)
        .border_style(Style::default().fg(PANEL))
        .style(Style::default().bg(BG));
    let paragraph = Paragraph::new(vec![title, subtitle]).block(block);
    frame.render_widget(paragraph, area);
}

fn render_summary_cards(frame: &mut Frame, area: Rect, app: &DashboardApp) {
    let layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(1), Constraint::Min(6)])
        .split(area);

    frame.render_widget(
        Paragraph::new(Line::from(Span::styled(
            app.snapshot.summary_label.as_str(),
            Style::default().fg(MUTED),
        ))),
        layout[0],
    );

    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage(20),
            Constraint::Percentage(20),
            Constraint::Percentage(20),
            Constraint::Percentage(20),
            Constraint::Percentage(20),
        ])
        .split(layout[1]);

    let summary = &app.snapshot.summary;
    let cards = [
        (
            "left clicks",
            format_compact_number(summary.left_clicks as f64),
        ),
        (
            "right clicks",
            format_compact_number(summary.right_clicks as f64),
        ),
        (
            "middle clicks",
            format_compact_number(summary.middle_clicks as f64),
        ),
        (
            "keypresses",
            format_compact_number(summary.key_presses as f64),
        ),
        (
            "mouse movement",
            format_compact_with_unit(summary.mouse_distance_cm, "cm"),
        ),
    ];

    for (index, (label, value)) in cards.into_iter().enumerate() {
        let focused = app.focused_section == FocusSection::Summary && index == 0;
        render_card(frame, chunks[index], label, &value, focused);
    }
}

fn render_apps_section(frame: &mut Frame, area: Rect, app: &DashboardApp) {
    if area.width >= 110 {
        let sections = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(62), Constraint::Percentage(38)])
            .split(area);
        render_top_apps(frame, sections[0], app);
        render_usage_mix(frame, sections[1], app);
    } else {
        let sections = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Length(10), Constraint::Min(6)])
            .split(area);
        render_top_apps(frame, sections[0], app);
        render_usage_mix(frame, sections[1], app);
    }
}

fn render_lower_section(frame: &mut Frame, area: Rect, app: &DashboardApp) {
    if area.width >= 120 {
        let sections = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(58), Constraint::Percentage(42)])
            .split(area);
        render_activity_chart(frame, sections[0], app);
        render_heatmap(frame, sections[1], app);
    } else {
        let sections = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Percentage(55), Constraint::Percentage(45)])
            .split(area);
        render_activity_chart(frame, sections[0], app);
        render_heatmap(frame, sections[1], app);
    }
}

fn render_footer(frame: &mut Frame, area: Rect, app: &DashboardApp) {
    let current_app = app.snapshot.status.current_app.as_deref().unwrap_or("none");
    let status = format!(
        "sync status: {} | tracked sources: {} | current app/context: {} | newest bucket timestamp: {} | {}",
        app.snapshot.status.sync_summary,
        app.snapshot.status.source_count,
        current_app,
        format_optional_time(app.snapshot.status.last_activity_at_utc),
        app.status_message
    );
    let footer = Paragraph::new(Line::from(vec![
        Span::styled(status, Style::default().fg(MUTED)),
        Span::raw("  "),
        Span::styled(
            "q quit  tab move  m metric  r refresh  ? help  u glyphs",
            Style::default().fg(FG),
        ),
    ]))
    .wrap(Wrap { trim: true });
    frame.render_widget(footer, area);
}

fn render_card(frame: &mut Frame, area: Rect, label: &str, value: &str, focused: bool) {
    let block = panel_block(label, focused);
    let content = Paragraph::new(vec![
        Line::from(Span::styled(
            value,
            Style::default().fg(ACCENT).add_modifier(Modifier::BOLD),
        )),
        Line::from(Span::styled(label, Style::default().fg(MUTED))),
    ])
    .alignment(Alignment::Center)
    .block(block);
    frame.render_widget(content, area);
}

fn render_top_apps(frame: &mut Frame, area: Rect, app: &DashboardApp) {
    let block = panel_block("my activity", app.focused_section == FocusSection::Apps);
    let inner = block.inner(area);
    frame.render_widget(block, area);

    if app.snapshot.top_activities.is_empty() {
        render_empty_panel(frame, inner, "No focused-app data in the selected range.");
        return;
    }

    let constraints = vec![Constraint::Length(2); app.snapshot.top_activities.len()];
    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints(constraints)
        .split(inner);

    for (row_area, app_share) in rows.iter().zip(app.snapshot.top_activities.iter()) {
        let gauge = Gauge::default()
            .ratio((app_share.share_percent / 100.0).clamp(0.0, 1.0))
            .gauge_style(Style::default().fg(ACCENT).bg(DIM))
            .label(format!(
                "{:<20} {:>5.1}%  {}",
                truncate(&app_share.label, 20),
                app_share.share_percent,
                format_duration(app_share.focus_seconds)
            ));
        frame.render_widget(gauge, *row_area);
    }
}

fn render_usage_mix(frame: &mut Frame, area: Rect, app: &DashboardApp) {
    let block = panel_block("top apps", false);
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let lines = if app.snapshot.top_apps.is_empty() {
        vec![Line::from(Span::styled(
            "No app mix yet.",
            Style::default().fg(MUTED),
        ))]
    } else {
        app.snapshot
            .top_apps
            .iter()
            .enumerate()
            .map(|(index, app_share)| {
                Line::from(vec![
                    Span::styled(format!("{:>2}. ", index + 1), Style::default().fg(MUTED)),
                    Span::styled(truncate(&app_share.label, 20), Style::default().fg(FG)),
                    if let Some(detail) = &app_share.detail {
                        Span::styled(
                            format!(" · {}", truncate(detail, 14)),
                            Style::default().fg(MUTED),
                        )
                    } else {
                        Span::raw("")
                    },
                    Span::raw(" "),
                    Span::styled(
                        format!("{:>5.1}%", app_share.share_percent),
                        Style::default().fg(ACCENT),
                    ),
                ])
            })
            .collect::<Vec<_>>()
    };

    frame.render_widget(Paragraph::new(lines).wrap(Wrap { trim: true }), inner);
}

fn render_activity_chart(frame: &mut Frame, area: Rect, app: &DashboardApp) {
    let block = panel_block(
        "past 24 hours",
        app.focused_section == FocusSection::Activity,
    );
    let inner = block.inner(area);
    frame.render_widget(block, area);

    if app.snapshot.series_buckets.is_empty() {
        render_empty_panel(frame, inner, "No recent activity buckets available.");
        return;
    }

    let points = chart_points(&app.snapshot, app.chart_metric);
    let y_max = points
        .iter()
        .map(|(_, value)| *value)
        .fold(0.0, f64::max)
        .max(1.0);
    let x_labels = chart_x_labels(&app.snapshot.series_buckets);
    let y_labels = vec![
        Span::styled("0", Style::default().fg(MUTED)),
        Span::styled(
            format_compact_chart_value(y_max / 2.0, app.chart_metric),
            Style::default().fg(MUTED),
        ),
        Span::styled(
            format_compact_chart_value(y_max, app.chart_metric),
            Style::default().fg(MUTED),
        ),
    ];

    let dataset = Dataset::default()
        .name(app.chart_metric.label())
        .marker(if app.ascii {
            symbols::Marker::Dot
        } else {
            symbols::Marker::Braille
        })
        .style(Style::default().fg(ACCENT))
        .graph_type(GraphType::Line)
        .data(&points);

    let chart = Chart::new(vec![dataset])
        .x_axis(
            Axis::default()
                .title("time")
                .style(Style::default().fg(MUTED))
                .bounds([0.0, (points.len().saturating_sub(1)) as f64])
                .labels(x_labels),
        )
        .y_axis(
            Axis::default()
                .title(app.chart_metric.label())
                .style(Style::default().fg(MUTED))
                .bounds([0.0, y_max])
                .labels(y_labels),
        );

    frame.render_widget(chart, inner);
}

fn render_heatmap(frame: &mut Frame, area: Rect, app: &DashboardApp) {
    let block = panel_block(
        "avg daily activity",
        app.focused_section == FocusSection::Heatmap,
    );
    let inner = block.inner(area);
    frame.render_widget(block, area);

    if app.snapshot.heatmap_rows.is_empty() {
        render_empty_panel(frame, inner, "No daily activity rows available.");
        return;
    }

    let today = Local::now().weekday();
    let header = Row::new(
        std::iter::once(Cell::from(Span::styled(
            "day",
            Style::default().fg(ACCENT).add_modifier(Modifier::BOLD),
        )))
        .chain(HeatmapMetric::ALL.into_iter().map(|metric| {
            Cell::from(Span::styled(
                metric.label(),
                Style::default().fg(ACCENT).add_modifier(Modifier::BOLD),
            ))
        }))
        .collect::<Vec<_>>(),
    );

    let rows = app
        .snapshot
        .heatmap_rows
        .iter()
        .map(|row| {
            let mut cells = Vec::with_capacity(1 + HeatmapMetric::ALL.len());
            let row_is_today = row.weekday == today;
            let label_style = if row_is_today {
                Style::default()
                    .fg(TODAY_HIGHLIGHT)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(FG)
            };
            cells.push(Cell::from(Span::styled(
                weekday_label(row.weekday),
                label_style,
            )));
            for (index, value) in row.values.iter().enumerate() {
                let max = app.snapshot.heatmap_maxima[index];
                let intensity = if max <= f64::EPSILON {
                    0.0
                } else {
                    value / max
                };
                let glyph = intensity_glyph(intensity, app.ascii);
                let base_color = if row_is_today {
                    brighten_color(heatmap_color(intensity))
                } else {
                    heatmap_color(intensity)
                };
                let cell_style = if row_is_today {
                    Style::default().fg(base_color).bg(DIM)
                } else {
                    Style::default().fg(base_color).bg(BG)
                };
                let value_text = if HeatmapMetric::ALL[index] == HeatmapMetric::MouseMove {
                    centered_cell_text(
                        &format!("{glyph}{}", format_compact_with_unit(*value, "cm")),
                        heatmap_width(index),
                    )
                } else {
                    centered_cell_text(
                        &format!("{glyph}{}", format_compact_number(*value)),
                        heatmap_width(index),
                    )
                };
                cells.push(Cell::from(Span::styled(value_text, cell_style)));
            }
            let row = Row::new(cells);
            if row_is_today {
                row.style(Style::default().bg(DIM))
            } else {
                row
            }
        })
        .collect::<Vec<_>>();

    let widths = [
        Constraint::Length(6),
        Constraint::Length(12),
        Constraint::Length(11),
        Constraint::Length(12),
        Constraint::Length(13),
        Constraint::Length(14),
    ];
    let table = Table::new(rows, widths)
        .header(header)
        .block(
            Block::default()
                .borders(Borders::TOP)
                .border_style(Style::default().fg(PANEL)),
        )
        .column_spacing(0)
        .style(Style::default().bg(BG));
    frame.render_widget(table, inner);
}

fn render_help_modal(frame: &mut Frame, area: Rect, app: &DashboardApp) {
    let popup = centered_rect(area, 72, 56);
    frame.render_widget(Clear, popup);
    let block = Block::default()
        .title("controls")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(ACCENT))
        .style(Style::default().bg(BG));
    let lines = vec![
        Line::from("q / Esc        quit or close help"),
        Line::from("Tab / Shift-Tab switch dashboard focus"),
        Line::from("1..4           jump to totals, apps, 24h chart, daily grid"),
        Line::from("m              cycle chart metric"),
        Line::from("h j k l        move focus / cycle chart"),
        Line::from("r              reload analytics from SQLite"),
        Line::from("u              toggle unicode or ascii rendering"),
        Line::from(""),
        Line::from("This dashboard is read-only."),
        Line::from(
            "Counters change only when another Life Monitor collector process flushes new buckets into the local SQLite database.",
        ),
        Line::from(""),
        Line::from(format!(
            "db: {}",
            truncate(
                &app.snapshot.status.db_path_display,
                popup.width.saturating_sub(8) as usize
            )
        )),
    ];
    frame.render_widget(
        Paragraph::new(lines)
            .block(block)
            .alignment(Alignment::Left)
            .wrap(Wrap { trim: true }),
        popup,
    );
}

fn render_empty_panel(frame: &mut Frame, area: Rect, message: &str) {
    frame.render_widget(
        Paragraph::new(message)
            .style(Style::default().fg(MUTED))
            .alignment(Alignment::Center)
            .wrap(Wrap { trim: true }),
        area,
    );
}

fn panel_block<'a>(title: &'a str, focused: bool) -> Block<'a> {
    Block::default()
        .title(title)
        .borders(Borders::ALL)
        .style(Style::default().bg(BG))
        .border_style(if focused {
            Style::default().fg(ACCENT)
        } else {
            Style::default().fg(PANEL)
        })
}

fn chart_points(snapshot: &DashboardSnapshot, metric: ChartMetric) -> Vec<(f64, f64)> {
    snapshot
        .series_buckets
        .iter()
        .enumerate()
        .map(|(index, bucket)| {
            let value = match metric {
                ChartMetric::Activity => bucket.activity_score,
                ChartMetric::KeyPresses => bucket.key_presses,
                ChartMetric::Clicks => bucket.clicks,
                ChartMetric::MouseMove => bucket.mouse_distance_cm,
                ChartMetric::Focus => bucket.focus_minutes,
            };
            (index as f64, value)
        })
        .collect()
}

fn chart_x_labels(buckets: &[ActivityBucket]) -> Vec<Span<'static>> {
    let labels = [0, buckets.len() / 2, buckets.len().saturating_sub(1)];
    labels
        .into_iter()
        .map(|index| {
            let time = buckets
                .get(index)
                .map(|bucket| bucket.started_at_utc.with_timezone(&Local))
                .unwrap_or_else(Local::now);
            Span::styled(time.format("%H:%M").to_string(), Style::default().fg(MUTED))
        })
        .collect()
}

fn centered_rect(area: Rect, width_percent: u16, height_percent: u16) -> Rect {
    let vertical = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage((100 - height_percent) / 2),
            Constraint::Percentage(height_percent),
            Constraint::Percentage((100 - height_percent) / 2),
        ])
        .split(area);
    Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage((100 - width_percent) / 2),
            Constraint::Percentage(width_percent),
            Constraint::Percentage((100 - width_percent) / 2),
        ])
        .split(vertical[1])[1]
}

fn weekday_label(weekday: Weekday) -> &'static str {
    match weekday {
        Weekday::Mon => "Mon",
        Weekday::Tue => "Tue",
        Weekday::Wed => "Wed",
        Weekday::Thu => "Thu",
        Weekday::Fri => "Fri",
        Weekday::Sat => "Sat",
        Weekday::Sun => "Sun",
    }
}

fn intensity_glyph(intensity: f64, ascii: bool) -> &'static str {
    if ascii {
        if intensity >= 0.8 {
            "#"
        } else if intensity >= 0.55 {
            "+"
        } else if intensity >= 0.25 {
            "-"
        } else {
            "."
        }
    } else if intensity >= 0.8 {
        "█"
    } else if intensity >= 0.55 {
        "▓"
    } else if intensity >= 0.25 {
        "▒"
    } else {
        "░"
    }
}

fn brighten_color(color: Color) -> Color {
    match color {
        Color::Rgb(r, g, b) => Color::Rgb(
            r.saturating_add(35),
            g.saturating_add(35),
            b.saturating_add(35),
        ),
        other => other,
    }
}

fn heatmap_width(index: usize) -> usize {
    match index {
        0 => 12,
        1 => 11,
        2 => 12,
        3 => 13,
        4 => 14,
        _ => 12,
    }
}

fn centered_cell_text(value: &str, width: usize) -> String {
    let visible = value.chars().count();
    if visible >= width {
        return truncate(value, width);
    }

    let left = (width - visible) / 2;
    let right = width - visible - left;
    format!("{}{}{}", " ".repeat(left), value, " ".repeat(right))
}

fn heatmap_color(intensity: f64) -> Color {
    if intensity >= 0.8 {
        ACCENT
    } else if intensity >= 0.55 {
        FG
    } else if intensity >= 0.25 {
        Color::Rgb(95, 190, 110)
    } else {
        MUTED
    }
}

fn format_optional_time(value: Option<DateTime<Utc>>) -> String {
    value
        .map(|timestamp| {
            timestamp
                .with_timezone(&Local)
                .format("%Y-%m-%d %H:%M")
                .to_string()
        })
        .unwrap_or_else(|| "never".to_string())
}

fn format_duration(seconds: u64) -> String {
    let hours = seconds / 3600;
    let minutes = (seconds % 3600) / 60;
    if hours > 0 {
        format!("{hours}h {minutes:02}m")
    } else {
        format!("{minutes}m")
    }
}

fn format_compact_number(value: f64) -> String {
    const UNITS: [(&str, f64); 4] = [
        ("B", 1_000_000_000.0),
        ("M", 1_000_000.0),
        ("K", 1_000.0),
        ("", 1.0),
    ];

    for (suffix, scale) in UNITS {
        if value.abs() >= scale || scale == 1.0 {
            let scaled = value / scale;
            return if scale == 1.0 {
                format!("{scaled:.0}")
            } else if scaled >= 100.0 {
                format!("{scaled:.0}{suffix}")
            } else {
                format!("{scaled:.1}{suffix}")
            };
        }
    }

    "0".to_string()
}

fn format_compact_with_unit(value: f64, unit: &str) -> String {
    format!("{} {}", format_compact_number(value), unit)
}

fn format_compact_chart_value(value: f64, metric: ChartMetric) -> String {
    match metric {
        ChartMetric::MouseMove => format_compact_with_unit(value, "cm"),
        ChartMetric::Focus => format_compact_with_unit(value, "min"),
        _ => format_compact_number(value),
    }
}

fn format_relative_local(value: DateTime<Local>) -> String {
    let now = Local::now();
    format_duration_delta(now.signed_duration_since(value).num_seconds())
}

fn format_relative_utc(value: Option<DateTime<Utc>>) -> String {
    value
        .map(|timestamp| {
            format_duration_delta(Utc::now().signed_duration_since(timestamp).num_seconds())
        })
        .unwrap_or_else(|| "never".to_string())
}

fn format_duration_delta(seconds: i64) -> String {
    let seconds = seconds.max(0);
    if seconds < 60 {
        format!("{seconds}s")
    } else if seconds < 3600 {
        format!("{}m", seconds / 60)
    } else if seconds < 86_400 {
        format!("{}h", seconds / 3600)
    } else {
        format!("{}d", seconds / 86_400)
    }
}

fn truncate(value: &str, max_chars: usize) -> String {
    if value.chars().count() <= max_chars {
        return value.to_string();
    }

    let mut truncated = value
        .chars()
        .take(max_chars.saturating_sub(1))
        .collect::<String>();
    truncated.push('…');
    truncated
}
