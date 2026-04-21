use chrono::{DateTime, Datelike, Local, Utc, Weekday};
use ratatui::{
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    symbols,
    text::{Line, Span},
    widgets::{
        Axis, Block, Borders, Cell, Chart, Clear, Dataset, GraphType, Paragraph, Row, Table, Wrap,
    },
    Frame,
};

use crate::tui::{
    app::{ChartMode, DashboardApp, FocusSection, TimeWindow},
    data::{ChartMetric, DashboardSnapshot, HeatmapMetric},
};

const BG: Color = Color::Black;
const FG: Color = Color::Rgb(130, 255, 150);
const MUTED: Color = Color::Rgb(70, 120, 80);
const PANEL: Color = Color::Rgb(35, 80, 40);
const ACCENT: Color = Color::Rgb(100, 255, 120);
const DIM: Color = Color::Rgb(25, 45, 25);
const TODAY_HIGHLIGHT: Color = Color::Rgb(150, 255, 170);
const WARN: Color = Color::Rgb(255, 200, 60);
const SINGLE_CHART: Color = Color::Rgb(110, 210, 240);
const SCOPE_BLUE: Color = Color::Rgb(90, 160, 255);
const SCOPE_YELLOW: Color = Color::Rgb(255, 235, 120);
const SCOPE_ORANGE: Color = Color::Rgb(255, 190, 110);
const SCOPE_PURPLE: Color = Color::Rgb(210, 170, 255);
const SCOPE_GREEN: Color = Color::Rgb(140, 255, 150);
const HEADER_HEIGHT: u16 = 3;
const SUMMARY_HEIGHT: u16 = 5;
const FOOTER_HEIGHT: u16 = 1;
const APPS_MIN_HEIGHT: u16 = 8;
const HEATMAP_PANEL_CHROME_HEIGHT: u16 = 2;
const HEATMAP_HEADER_HEIGHT: u16 = 2;
const HEATMAP_MAX_DATA_ROWS: u16 = 7;
const CHART_MIN_HEIGHT: u16 = 8;

pub fn render(frame: &mut Frame, app: &DashboardApp) {
    let area = frame.area();

    frame.render_widget(Block::default().style(Style::default().bg(BG)), area);

    if area.width < 80 || area.height < 24 {
        let msg = format!(
            "Terminal too small ({} × {})\nMinimum required: 80 × 24",
            area.width, area.height
        );
        frame.render_widget(
            Paragraph::new(msg)
                .style(Style::default().fg(FG).bg(BG))
                .alignment(Alignment::Center)
                .wrap(Wrap { trim: true }),
            area,
        );
        return;
    }

    let layout = if area.width >= 140 {
        let heatmap_height = heatmap_panel_height(app.snapshot.heatmap_rows.len());
        Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(HEADER_HEIGHT),
                Constraint::Length(SUMMARY_HEIGHT),
                Constraint::Length(
                    heatmap_height.min(
                        area.height
                            .saturating_sub(HEADER_HEIGHT + SUMMARY_HEIGHT + FOOTER_HEIGHT + 1),
                    ),
                ),
                Constraint::Min(1),
                Constraint::Length(FOOTER_HEIGHT),
            ])
            .split(area)
    } else {
        Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(HEADER_HEIGHT),
                Constraint::Length(SUMMARY_HEIGHT),
                Constraint::Length(
                    APPS_MIN_HEIGHT.min(
                        area.height
                            .saturating_sub(HEADER_HEIGHT + SUMMARY_HEIGHT + FOOTER_HEIGHT + 1),
                    ),
                ),
                Constraint::Min(1),
                Constraint::Length(FOOTER_HEIGHT),
            ])
            .split(area)
    };

    render_header(frame, layout[0], app);
    render_summary_cards(frame, layout[1], app);
    render_activity_overview(frame, layout[2], app);
    render_lower_section(frame, layout[3], app);
    render_footer(frame, layout[4], app);

    if app.show_help {
        render_help_modal(frame, area, app);
    }
}

// ── Header ────────────────────────────────────────────────────────────────────

fn render_header(frame: &mut Frame, area: Rect, app: &DashboardApp) {
    let (collector_label, collector_color) =
        collector_state(app.snapshot.status.last_activity_at_utc);
    let title = Line::from(vec![
        Span::styled("analytics dashboard", Style::default().fg(FG)),
        Span::styled("  •  ", Style::default().fg(PANEL)),
        Span::styled(
            format!(
                "refreshed {} ago",
                format_relative_local(app.snapshot.generated_at_local),
            ),
            Style::default().fg(MUTED),
        ),
        Span::styled("  •  ", Style::default().fg(PANEL)),
        Span::styled(collector_label, Style::default().fg(collector_color)),
    ]);

    let mut tab_spans: Vec<Span> = Vec::new();
    for window in TimeWindow::ALL {
        let active = window == app.time_window;
        let label = format!("[{}]", window.label());
        let style = if active {
            Style::default().fg(ACCENT).add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(MUTED)
        };
        tab_spans.push(Span::styled(label, style));
        tab_spans.push(Span::raw(" "));
    }
    let tabs_line = Line::from(tab_spans);

    let block = Block::default()
        .borders(Borders::BOTTOM)
        .border_style(Style::default().fg(PANEL))
        .style(Style::default().bg(BG));
    frame.render_widget(Paragraph::new(vec![title, tabs_line]).block(block), area);
}

// ── Summary cards ─────────────────────────────────────────────────────────────

fn render_summary_cards(frame: &mut Frame, area: Rect, app: &DashboardApp) {
    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage(20),
            Constraint::Percentage(20),
            Constraint::Percentage(20),
            Constraint::Percentage(20),
            Constraint::Percentage(20),
        ])
        .split(area);

    let summary = &app.snapshot.summary;
    let raw_values = [
        summary.key_presses as f64,
        summary.left_clicks as f64,
        summary.right_clicks as f64,
        summary.middle_clicks as f64,
        summary.mouse_distance_cm,
    ];
    let cards = [
        ("key presses", format_compact_number(raw_values[0])),
        ("left clicks", format_compact_number(raw_values[1])),
        ("right clicks", format_compact_number(raw_values[2])),
        ("middle clicks", format_compact_number(raw_values[3])),
        (
            "mouse movement",
            format_compact_with_unit(raw_values[4], "cm"),
        ),
    ];

    let peak_idx = raw_values
        .iter()
        .enumerate()
        .filter(|(_, v)| **v > 0.0)
        .max_by(|a, b| a.1.partial_cmp(b.1).unwrap_or(std::cmp::Ordering::Equal))
        .map(|(i, _)| i);

    for (index, (label, value)) in cards.into_iter().enumerate() {
        let focused =
            app.focused_section == FocusSection::Summary && index == app.selected_summary_index;
        let is_zero = raw_values[index] == 0.0;
        let is_peak = peak_idx == Some(index);
        render_card(
            frame,
            chunks[index],
            label,
            &value,
            focused,
            is_zero,
            is_peak,
        );
    }
}

fn render_card(
    frame: &mut Frame,
    area: Rect,
    label: &str,
    value: &str,
    focused: bool,
    is_zero: bool,
    is_peak: bool,
) {
    let block = Block::default()
        .title(label)
        .title_alignment(Alignment::Center)
        .borders(Borders::ALL)
        .style(Style::default().bg(BG))
        .border_style(if focused {
            Style::default().fg(ACCENT)
        } else {
            Style::default().fg(PANEL)
        });
    let value_style = if is_zero {
        Style::default().fg(MUTED)
    } else if is_peak {
        Style::default()
            .fg(TODAY_HIGHLIGHT)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(ACCENT).add_modifier(Modifier::BOLD)
    };
    let inner = block.inner(area);
    frame.render_widget(Paragraph::new("").block(block), area);
    let value_area = centered_rect_in(inner, 100, 1);
    frame.render_widget(
        Paragraph::new(Line::from(Span::styled(value, value_style))).alignment(Alignment::Center),
        value_area,
    );
}

// ── Activity overview ─────────────────────────────────────────────────────────

fn render_activity_overview(frame: &mut Frame, area: Rect, app: &DashboardApp) {
    if area.width >= 140 {
        let sections = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(60), Constraint::Percentage(40)])
            .split(area);
        render_activity_bars(frame, sections[0], app);
        render_heatmap(frame, sections[1], app);
    } else {
        render_activity_bars(frame, area, app);
    }
}

fn render_activity_bars(frame: &mut Frame, area: Rect, app: &DashboardApp) {
    let title = format!("apps activity—{}", time_window_phrase(app.time_window));
    let block = panel_block(&title, app.focused_section == FocusSection::Apps);
    let inner = block.inner(area);
    frame.render_widget(block, area);

    if app.snapshot.top_apps.is_empty() {
        render_empty_panel(frame, inner, "No focused-app data in the selected range.");
        return;
    }

    let metrics = app
        .snapshot
        .top_apps
        .iter()
        .map(|share| RowMetrics {
            label: share.label.as_str(),
            duration: Some(format_duration(share.focus_seconds)),
        })
        .collect::<Vec<_>>();
    let needs_scrollbar = app.snapshot.top_apps.len() > inner.height as usize;
    let sections = if needs_scrollbar && inner.width > 9 {
        Layout::default()
            .direction(Direction::Horizontal)
            .constraints([
                Constraint::Min(1),
                Constraint::Length(2),
                Constraint::Length(1),
            ])
            .split(inner)
    } else {
        Layout::default()
            .direction(Direction::Horizontal)
            .constraints([
                Constraint::Min(1),
                Constraint::Length(0),
                Constraint::Length(0),
            ])
            .split(inner)
    };
    let list_area = sections[0];
    let columns = list_columns(list_area.width as usize, &metrics, 18, 22, 14);
    let visible_rows = list_area.height as usize;
    let scroll = visible_app_window(
        app.selected_app_index,
        app.app_scroll_offset,
        app.snapshot.top_apps.len(),
        visible_rows,
    );
    let visible_apps = app
        .snapshot
        .top_apps
        .iter()
        .skip(scroll.offset)
        .take(scroll.visible_rows)
        .collect::<Vec<_>>();
    let histograms = build_app_histograms(&visible_apps, columns.spark_width.max(1), app.ascii);

    let lines: Vec<Line> = visible_apps
        .iter()
        .enumerate()
        .map(|share| {
            let actual_index = scroll.offset + share.0;
            let row = *share.1;
            let selected =
                app.focused_section == FocusSection::Apps && actual_index == app.selected_app_index;
            activity_row_line(
                &row.label,
                &histograms[share.0],
                row.share_percent,
                Some(format_duration(row.focus_seconds)),
                &columns,
                selected,
            )
        })
        .collect();

    frame.render_widget(Paragraph::new(lines), list_area);
    if needs_scrollbar && sections[1].width > 0 {
        frame.render_widget(Block::default().style(Style::default().bg(BG)), sections[1]);
    }
    if needs_scrollbar && sections[2].width > 0 {
        render_scrollbar(
            frame,
            sections[2],
            scroll.offset,
            visible_rows,
            app.snapshot.top_apps.len(),
        );
    }
}

// ── Lower section ─────────────────────────────────────────────────────────────

fn render_lower_section(frame: &mut Frame, area: Rect, app: &DashboardApp) {
    if area.width >= 140 {
        render_activity_chart(frame, area, app);
    } else {
        let preferred_heatmap_height = heatmap_panel_height(app.snapshot.heatmap_rows.len());
        let heatmap_height = preferred_heatmap_height.min(
            area.height
                .saturating_sub(CHART_MIN_HEIGHT)
                .max(HEATMAP_PANEL_CHROME_HEIGHT + HEATMAP_HEADER_HEIGHT + 1),
        );
        let sections = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Min(CHART_MIN_HEIGHT),
                Constraint::Length(heatmap_height),
            ])
            .split(area);
        render_activity_chart(frame, sections[0], app);
        render_heatmap(frame, sections[1], app);
    }
}

// ── Activity chart ────────────────────────────────────────────────────────────

fn render_activity_chart(frame: &mut Frame, area: Rect, app: &DashboardApp) {
    let title = format!(
        "activity graph — {} — {}",
        chart_mode_label(app),
        time_window_phrase(app.time_window)
    );
    let block = panel_block(&title, app.focused_section == FocusSection::Activity);
    let inner = block.inner(area);
    frame.render_widget(block, area);

    if app.snapshot.series_buckets.is_empty() {
        render_empty_panel(frame, inner, "No recent activity buckets available.");
        return;
    }

    let chart_layout = if app.chart_mode == ChartMode::Scope {
        Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Length(1), Constraint::Min(1)])
            .split(inner)
    } else {
        Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Length(0), Constraint::Min(1)])
            .split(inner)
    };
    if app.chart_mode == ChartMode::Scope {
        render_scope_legend(frame, chart_layout[0]);
    }

    let datasets = chart_datasets(app);
    let x_labels = chart_x_labels(&app.snapshot, app.time_window);
    let (y_bounds, y_labels) = match app.chart_mode {
        ChartMode::Single => {
            let y_max = datasets
                .iter()
                .flat_map(|dataset| dataset.points.iter().map(|(_, v)| *v))
                .fold(0.0_f64, f64::max)
                .max(1.0);
            (
                [0.0, y_max],
                vec![
                    Span::styled("0", Style::default().fg(MUTED)),
                    Span::styled(
                        format_compact_chart_value(y_max / 2.0, chart_value_metric(app)),
                        Style::default().fg(MUTED),
                    ),
                    Span::styled(
                        format_compact_chart_value(y_max, chart_value_metric(app)),
                        Style::default().fg(MUTED),
                    ),
                ],
            )
        }
        ChartMode::Scope => {
            let y_max = scope_axis_max(&app.snapshot);
            (
                [0.0, y_max],
                vec![
                    Span::styled("0", Style::default().fg(MUTED)),
                    Span::styled(
                        format_compact_number(scope_axis_label_value(y_max * 0.5)),
                        Style::default().fg(MUTED),
                    ),
                    Span::styled(
                        format_compact_number(scope_axis_label_value(y_max)),
                        Style::default().fg(MUTED),
                    ),
                ],
            )
        }
    };

    let ratatui_datasets = datasets
        .iter()
        .map(|dataset| {
            Dataset::default()
                .marker(if app.ascii {
                    symbols::Marker::Dot
                } else {
                    symbols::Marker::Braille
                })
                .style(Style::default().fg(dataset.color))
                .graph_type(GraphType::Line)
                .data(&dataset.points)
        })
        .collect::<Vec<_>>();

    let chart = Chart::new(ratatui_datasets)
        .x_axis(
            Axis::default()
                .style(Style::default().fg(MUTED))
                .bounds([
                    0.0,
                    (app.snapshot.series_buckets.len().saturating_sub(1)) as f64,
                ])
                .labels(x_labels),
        )
        .y_axis(
            Axis::default()
                .style(Style::default().fg(MUTED))
                .bounds(y_bounds)
                .labels(y_labels),
        );

    frame.render_widget(chart, chart_layout[1]);
}

// ── Heatmap (avg daily activity) ──────────────────────────────────────────────

fn render_heatmap(frame: &mut Frame, area: Rect, app: &DashboardApp) {
    let block = panel_block(
        "week activity",
        app.focused_section == FocusSection::Heatmap,
    );
    let inner = block.inner(area);
    frame.render_widget(block, area);

    if app.snapshot.heatmap_rows.is_empty() {
        render_empty_panel(frame, inner, "No daily activity rows available.");
        return;
    }

    let today = Local::now().weekday();

    // Dynamic column widths so they fill the available panel width.
    // Structure: day(5) │ col │ col │ col │ col │ col
    //            5 + 5 separators + 5 data columns
    const DAY_W: usize = 5;
    const NUM_COLS: usize = 5;
    const NUM_SEPS: usize = 5; // one │ before each data column
    let needs_scrollbar = app.snapshot.heatmap_rows.len() > inner.height.saturating_sub(2) as usize;
    let sections = if needs_scrollbar && inner.width > 10 {
        Layout::default()
            .direction(Direction::Horizontal)
            .constraints([
                Constraint::Min(1),
                Constraint::Length(2),
                Constraint::Length(1),
            ])
            .split(inner)
    } else {
        Layout::default()
            .direction(Direction::Horizontal)
            .constraints([
                Constraint::Min(1),
                Constraint::Length(0),
                Constraint::Length(0),
            ])
            .split(inner)
    };
    let table_area = sections[0];
    let row_slots = table_area.height.saturating_sub(2) as usize;
    let visible_rows = app.snapshot.heatmap_rows.len().min(row_slots);
    let table_width = table_area.width as usize;
    let available_width = table_width.saturating_sub(DAY_W + NUM_SEPS);
    let column_widths = distribute_widths(available_width, NUM_COLS, 7);
    let min_col_w = column_widths.iter().copied().min().unwrap_or(7);
    let extra_row_space = row_slots.saturating_sub(visible_rows);

    // Choose header label length based on available column width.
    let col_labels: [&str; 5] = if min_col_w >= 13 {
        [
            "key presses",
            "left clicks",
            "right clicks",
            "middle clicks",
            "mouse mov",
        ]
    } else if min_col_w >= 10 {
        [
            "key press",
            "left clk",
            "right clk",
            "mid click",
            "mouse mov",
        ]
    } else if min_col_w >= 8 {
        ["key prs", "l.clicks", "r.clicks", "m.clicks", "mouse mov"]
    } else {
        ["keys", "l.clk", "r.clk", "m.clk", "m. mov"]
    };

    // Build ratatui constraints: day + (sep + col) × 5
    let constraints: Vec<Constraint> = std::iter::once(Constraint::Length(DAY_W as u16))
        .chain((0..NUM_COLS).flat_map(|index| {
            [
                Constraint::Length(1),
                Constraint::Length(column_widths[index] as u16),
            ]
        }))
        .collect();

    // Header row
    let mut hdr_cells: Vec<Cell> = vec![Cell::from(Span::styled(
        right_pad("day", DAY_W),
        Style::default().fg(ACCENT).add_modifier(Modifier::BOLD),
    ))];
    for (index, label) in col_labels.iter().enumerate() {
        hdr_cells.push(sep_cell());
        hdr_cells.push(Cell::from(Span::styled(
            center_align_str(label, column_widths[index]),
            Style::default().fg(ACCENT).add_modifier(Modifier::BOLD),
        )));
    }
    let header_row = Row::new(hdr_cells).style(Style::default().bg(BG));

    // Separator row (─ across every column, ┼ at junctions)
    let mut sep_cells: Vec<Cell> =
        vec![Cell::from("─".repeat(DAY_W)).style(Style::default().fg(PANEL))];
    for width in &column_widths {
        sep_cells.push(Cell::from("┼").style(Style::default().fg(PANEL)));
        sep_cells.push(Cell::from("─".repeat(*width)).style(Style::default().fg(PANEL)));
    }
    let separator_row = Row::new(sep_cells);
    let scroll = visible_window(
        app.selected_heatmap_index,
        app.heatmap_scroll_offset,
        app.snapshot.heatmap_rows.len(),
        visible_rows,
    );

    // Data rows
    let data_rows: Vec<Row> = app
        .snapshot
        .heatmap_rows
        .iter()
        .skip(scroll.offset)
        .take(scroll.visible_rows)
        .enumerate()
        .map(|row| {
            let row_index = row.0;
            let actual_index = scroll.offset + row_index;
            let row = row.1;
            let row_is_today = row.weekday == today;
            let is_future_this_week =
                row.weekday.num_days_from_monday() > today.num_days_from_monday();
            let heatmap_focused = app.focused_section == FocusSection::Heatmap;
            let row_is_selected = heatmap_focused && actual_index == app.selected_heatmap_index;
            let row_has_highlight = row_is_selected || (row_is_today && !heatmap_focused);
            let row_bg = if row_has_highlight { DIM } else { BG };
            let day_style = if is_future_this_week {
                Style::default().fg(MUTED).bg(row_bg)
            } else if row_has_highlight {
                Style::default()
                    .fg(TODAY_HIGHLIGHT)
                    .bg(row_bg)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(FG)
            };

            let mut cells: Vec<Cell> = vec![Cell::from(Span::styled(
                right_pad(weekday_label(row.weekday), DAY_W),
                day_style,
            ))];

            for (mi, value) in row.values.iter().enumerate() {
                let max = app.snapshot.heatmap_maxima[mi];
                let intensity = if max <= f64::EPSILON {
                    0.0
                } else {
                    value / max
                };
                let is_zero = *value < f64::EPSILON;
                let col_width = column_widths[mi];

                let (text, color) = if is_future_this_week {
                    (center_align_str("–", col_width), MUTED)
                } else if is_zero {
                    (center_align_str("0", col_width), MUTED)
                } else {
                    let num = if HeatmapMetric::ALL[mi] == HeatmapMetric::MouseMove {
                        format_compact_with_unit(*value, "cm")
                    } else {
                        format_compact_number(*value)
                    };
                    let t = center_align_str(&num, col_width);
                    let c = if row_has_highlight {
                        brighten_color(heatmap_color(intensity))
                    } else {
                        heatmap_color(intensity)
                    };
                    (t, c)
                };

                let cell_style = if row_has_highlight {
                    Style::default().fg(color).bg(row_bg)
                } else {
                    Style::default().fg(color)
                };
                cells.push(sep_cell_styled(row_bg));
                cells.push(Cell::from(Span::styled(text, cell_style)));
            }

            let row_height = 1
                + (extra_row_space / scroll.visible_rows.max(1))
                + usize::from(row_index < (extra_row_space % scroll.visible_rows.max(1)));
            let r = Row::new(cells).height(row_height as u16);
            if row_has_highlight {
                r.style(Style::default().bg(row_bg))
            } else {
                r
            }
        })
        .collect();

    let mut all_rows = vec![separator_row];
    all_rows.extend(data_rows);

    let table = Table::new(all_rows, constraints)
        .header(header_row)
        .column_spacing(0)
        .style(Style::default().bg(BG));
    frame.render_widget(table, table_area);
    if needs_scrollbar && sections[2].width > 0 {
        render_scrollbar(
            frame,
            sections[2],
            scroll.offset,
            visible_rows,
            app.snapshot.heatmap_rows.len(),
        );
    }

    // Fill any residual vertical slack below the rendered table with the panel background so
    // the compact table doesn't leave a dark dead zone inside the panel.
    let used_rows = 2usize + scroll.visible_rows + extra_row_space;
    if table_area.height as usize > used_rows {
        let filler = Rect {
            x: table_area.x,
            y: table_area.y + used_rows as u16,
            width: table_area.width,
            height: table_area.height.saturating_sub(used_rows as u16),
        };
        frame.render_widget(Block::default().style(Style::default().bg(BG)), filler);
    }
}

fn sep_cell<'a>() -> Cell<'a> {
    Cell::from(Span::styled("│", Style::default().fg(PANEL)))
}

fn sep_cell_styled<'a>(bg: Color) -> Cell<'a> {
    Cell::from(Span::styled("│", Style::default().fg(PANEL).bg(bg)))
}

fn heatmap_panel_height(row_count: usize) -> u16 {
    let data_rows = row_count.min(HEATMAP_MAX_DATA_ROWS as usize) as u16;
    HEATMAP_PANEL_CHROME_HEIGHT + HEATMAP_HEADER_HEIGHT + data_rows
}

// ── Footer ────────────────────────────────────────────────────────────────────

fn render_footer(frame: &mut Frame, area: Rect, app: &DashboardApp) {
    let sections = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(55), Constraint::Percentage(45)])
        .split(area);

    let stale = is_collector_stale(app.snapshot.status.last_activity_at_utc);
    let dot_color = if app.snapshot.status.sync_summary.starts_with("sync on") {
        Color::LightGreen
    } else {
        Color::Yellow
    };
    let sync_label = if app.snapshot.status.sync_summary == "local-only" {
        "local-only"
    } else {
        &app.snapshot.status.sync_summary
    };
    let mut left_spans: Vec<Span> = vec![
        Span::styled("● ", Style::default().fg(dot_color)),
        Span::styled(
            format!(
                "{}  |  {} src",
                sync_label, app.snapshot.status.source_count
            ),
            Style::default().fg(MUTED),
        ),
    ];
    if stale {
        left_spans.push(Span::styled(
            "  |  collector inactive",
            Style::default().fg(WARN),
        ));
    }
    if !app.status_message.is_empty() {
        left_spans.push(Span::styled("  |  ", Style::default().fg(PANEL)));
        left_spans.push(Span::styled(
            truncate(
                &app.status_message,
                sections[0].width.saturating_sub(24) as usize,
            ),
            Style::default().fg(MUTED),
        ));
    }

    let right = Paragraph::new(Line::from(vec![
        Span::styled(focused_panel_hint(app), Style::default().fg(MUTED)),
        Span::styled("  |  [?]help", Style::default().fg(MUTED)),
    ]))
    .alignment(Alignment::Right)
    .wrap(Wrap { trim: true });

    frame.render_widget(
        Paragraph::new(Line::from(left_spans)).wrap(Wrap { trim: true }),
        sections[0],
    );
    frame.render_widget(right, sections[1]);
}

// ── Help modal ────────────────────────────────────────────────────────────────

fn render_help_modal(frame: &mut Frame, area: Rect, app: &DashboardApp) {
    let popup = centered_rect(area, 72, 62);
    frame.render_widget(Clear, popup);
    let block = Block::default()
        .title("controls")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(ACCENT))
        .style(Style::default().bg(BG));
    let lines = vec![
        Line::from("q / Esc        quit or close help"),
        Line::from("Tab / Shift-Tab  cycle focus sections"),
        Line::from("1..4           jump: totals, apps, chart, daily"),
        Line::from("m / j / k      next / advance / reverse chart metric"),
        Line::from("v              toggle chart mode"),
        Line::from("[ / ]          previous / next time window"),
        Line::from("r / F5         reload data from SQLite"),
        Line::from("u              toggle unicode / ascii glyphs"),
        Line::from(""),
        Line::from("Dashboard is read-only. Run `vigil collector` as a separate"),
        Line::from("process (or configure autostart) to collect live data."),
        Line::from("The ⚠ indicator in the status bar means the collector"),
        Line::from("has not written new data in the last 20 minutes."),
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

// ── Chart helpers ─────────────────────────────────────────────────────────────

fn chart_points(snapshot: &DashboardSnapshot, metric: ChartMetric) -> Vec<(f64, f64)> {
    snapshot
        .series_buckets
        .iter()
        .enumerate()
        .map(|(i, b)| {
            let v = match metric {
                ChartMetric::Activity => b.activity_score,
                ChartMetric::KeyPresses => b.key_presses,
                ChartMetric::LeftClicks => b.left_clicks,
                ChartMetric::RightClicks => b.right_clicks,
                ChartMetric::MiddleClicks => b.middle_clicks,
                ChartMetric::MouseMove => b.mouse_distance_cm,
            };
            (i as f64, v)
        })
        .collect()
}

#[derive(Clone)]
struct ChartSeries {
    color: Color,
    points: Vec<(f64, f64)>,
}

const SCOPE_OVERLAY_OFFSETS: [f64; 5] = [-0.18, -0.09, 0.0, 0.09, 0.18];

fn chart_datasets(app: &DashboardApp) -> Vec<ChartSeries> {
    match app.chart_mode {
        ChartMode::Single => vec![ChartSeries {
            color: SINGLE_CHART,
            points: chart_points(&app.snapshot, app.chart_metric),
        }],
        ChartMode::Scope => vec![
            ChartSeries {
                color: SCOPE_BLUE,
                points: scope_overlay_series(
                    &app.snapshot,
                    0,
                    scope_axis_max(&app.snapshot),
                    |bucket| bucket.key_presses,
                ),
            },
            ChartSeries {
                color: SCOPE_GREEN,
                points: scope_overlay_series(
                    &app.snapshot,
                    1,
                    scope_axis_max(&app.snapshot),
                    |bucket| bucket.left_clicks,
                ),
            },
            ChartSeries {
                color: SCOPE_PURPLE,
                points: scope_overlay_series(
                    &app.snapshot,
                    2,
                    scope_axis_max(&app.snapshot),
                    |bucket| bucket.right_clicks,
                ),
            },
            ChartSeries {
                color: SCOPE_ORANGE,
                points: scope_overlay_series(
                    &app.snapshot,
                    3,
                    scope_axis_max(&app.snapshot),
                    |bucket| bucket.middle_clicks,
                ),
            },
            ChartSeries {
                color: SCOPE_YELLOW,
                points: scope_overlay_series(
                    &app.snapshot,
                    4,
                    scope_axis_max(&app.snapshot),
                    |bucket| bucket.mouse_distance_cm,
                ),
            },
        ],
    }
}

fn chart_series_from_buckets<F>(snapshot: &DashboardSnapshot, value: F) -> Vec<(f64, f64)>
where
    F: Fn(&crate::tui::data::ActivityBucket) -> f64,
{
    snapshot
        .series_buckets
        .iter()
        .enumerate()
        .map(|(index, bucket)| (index as f64, value(bucket)))
        .collect()
}

fn scope_overlay_series<F>(
    snapshot: &DashboardSnapshot,
    offset_index: usize,
    axis_max: f64,
    value: F,
) -> Vec<(f64, f64)>
where
    F: Fn(&crate::tui::data::ActivityBucket) -> f64,
{
    let raw = chart_series_from_buckets(snapshot, value);
    let offset = SCOPE_OVERLAY_OFFSETS
        .get(offset_index)
        .copied()
        .unwrap_or(0.0);

    raw.into_iter()
        .map(|(x, y)| {
            let transformed = if y <= f64::EPSILON { 0.0 } else { y.sqrt() };
            (x, (transformed + offset).clamp(0.0, axis_max))
        })
        .collect()
}

fn scope_axis_max(snapshot: &DashboardSnapshot) -> f64 {
    snapshot
        .series_buckets
        .iter()
        .flat_map(|bucket| {
            [
                bucket.key_presses,
                bucket.left_clicks,
                bucket.right_clicks,
                bucket.middle_clicks,
                bucket.mouse_distance_cm,
            ]
        })
        .fold(0.0_f64, f64::max)
        .sqrt()
        .max(1.0)
}

fn scope_axis_label_value(transformed: f64) -> f64 {
    transformed * transformed
}

fn chart_mode_label(app: &DashboardApp) -> &'static str {
    match app.chart_mode {
        ChartMode::Single => app.chart_metric.label(),
        ChartMode::Scope => "scope",
    }
}

fn chart_value_metric(app: &DashboardApp) -> ChartMetric {
    match app.chart_mode {
        ChartMode::Single => app.chart_metric,
        ChartMode::Scope => ChartMetric::Activity,
    }
}

fn render_scope_legend(frame: &mut Frame, area: Rect) {
    let line = Line::from(vec![
        Span::styled("Keypresses ", Style::default().fg(MUTED)),
        legend_square(SCOPE_BLUE),
        Span::raw("  "),
        Span::styled("Mouse Movement ", Style::default().fg(MUTED)),
        legend_square(SCOPE_YELLOW),
        Span::raw("  "),
        Span::styled("Middle Clicks ", Style::default().fg(MUTED)),
        legend_square(SCOPE_ORANGE),
        Span::raw("  "),
        Span::styled("Right Clicks ", Style::default().fg(MUTED)),
        legend_square(SCOPE_PURPLE),
        Span::raw("  "),
        Span::styled("Left Clicks ", Style::default().fg(MUTED)),
        legend_square(SCOPE_GREEN),
    ]);
    frame.render_widget(
        Paragraph::new(line)
            .alignment(Alignment::Center)
            .wrap(Wrap { trim: true }),
        area,
    );
}

fn legend_square(color: Color) -> Span<'static> {
    Span::styled("■", Style::default().fg(color))
}

/// Generates fixed clock/date labels so the chart reads as a stable time window.
fn chart_x_labels(snapshot: &DashboardSnapshot, window: TimeWindow) -> Vec<Span<'static>> {
    if snapshot.series_buckets.is_empty() {
        return vec![];
    }

    let end = snapshot
        .series_buckets
        .last()
        .map(|bucket| bucket.started_at_utc + chrono::Duration::minutes(snapshot.bucket_minutes))
        .unwrap_or_else(Utc::now)
        .with_timezone(&Local);

    let (label_count, gap_minutes, format): (usize, i64, &str) = match window {
        TimeWindow::All => {
            let total_minutes = snapshot.bucket_minutes * snapshot.series_buckets.len() as i64;
            let gap = (total_minutes / 5).max(snapshot.bucket_minutes);
            let format = if total_minutes >= 60 * 24 * 365 {
                "%m/%y"
            } else if total_minutes >= 60 * 24 * 30 {
                "%m/%d"
            } else {
                "%m/%d"
            };
            (6, gap, format)
        }
        TimeWindow::OneHour => (5, 15, "%H:%M"),
        TimeWindow::SixHours => (7, 60, "%H:%M"),
        TimeWindow::TwentyFourHours => (8, 180, "%H:%M"),
        TimeWindow::SevenDays => (7, 1440, "%m/%d"),
        TimeWindow::ThirtyDays => (6, 5 * 1440, "%m/%d"),
    };
    let first =
        end - chrono::Duration::minutes(gap_minutes * (label_count.saturating_sub(1) as i64));

    (0..label_count)
        .map(|i| {
            let label_time = first + chrono::Duration::minutes(gap_minutes * i as i64);
            let style = if i == label_count - 1 {
                Style::default().fg(FG)
            } else {
                Style::default().fg(MUTED)
            };
            Span::styled(label_time.format(format).to_string(), style)
        })
        .collect()
}

fn activity_row_line(
    label: &str,
    histogram: &str,
    percent: u64,
    duration: Option<String>,
    columns: &ListColumns,
    selected: bool,
) -> Line<'static> {
    let duration_text = duration.unwrap_or_default();
    let row_bg = if selected { DIM } else { BG };
    let label_style = if selected {
        Style::default()
            .fg(TODAY_HIGHLIGHT)
            .bg(row_bg)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(FG).bg(row_bg)
    };
    let meta_style = if selected {
        Style::default().fg(TODAY_HIGHLIGHT).bg(row_bg)
    } else {
        Style::default().fg(MUTED).bg(row_bg)
    };
    let spark_style = if selected {
        Style::default()
            .fg(ACCENT)
            .bg(row_bg)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(Color::Rgb(120, 225, 190)).bg(row_bg)
    };
    let marker = if selected { ">" } else { " " };

    Line::from(vec![
        Span::styled(
            format!(
                "{}{:<width$}  ",
                marker,
                truncate(label, columns.label_width),
                width = columns.label_width
            ),
            label_style,
        ),
        Span::styled(
            format!("{:<width$}", histogram, width = columns.spark_width),
            spark_style,
        ),
        Span::styled(
            format!(
                "  {:>3}%  {:>width$}",
                percent,
                duration_text,
                width = columns.duration_width
            ),
            meta_style,
        ),
    ])
}

fn render_scrollbar(
    frame: &mut Frame,
    area: Rect,
    offset: usize,
    visible_rows: usize,
    total_rows: usize,
) {
    if total_rows == 0 || visible_rows == 0 || area.height == 0 {
        return;
    }

    let track_height = area.height as usize;
    let thumb_height = ((visible_rows * track_height) / total_rows)
        .max(1)
        .min(track_height);
    let max_thumb_top = track_height.saturating_sub(thumb_height);
    let thumb_top = if total_rows <= visible_rows {
        0
    } else {
        (offset * max_thumb_top) / (total_rows - visible_rows)
    };

    let lines = (0..track_height)
        .map(|index| {
            let (ch, style) = if index >= thumb_top && index < thumb_top + thumb_height {
                ("█", Style::default().fg(Color::Rgb(70, 150, 95)))
            } else {
                ("│", Style::default().fg(Color::Rgb(35, 70, 45)))
            };
            Line::from(Span::styled(ch, style))
        })
        .collect::<Vec<_>>();
    frame.render_widget(Paragraph::new(lines).alignment(Alignment::Center), area);
}

// ── Layout helpers ────────────────────────────────────────────────────────────

fn centered_rect(area: Rect, width_percent: u16, height_percent: u16) -> Rect {
    let v = Layout::default()
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
        .split(v[1])[1]
}

fn centered_rect_in(area: Rect, width_percent: u16, height: u16) -> Rect {
    let v = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Fill(1),
            Constraint::Length(height.min(area.height)),
            Constraint::Fill(1),
        ])
        .split(area);
    let h = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage((100 - width_percent) / 2),
            Constraint::Percentage(width_percent),
            Constraint::Percentage((100 - width_percent) / 2),
        ])
        .split(v[1]);
    h[1]
}

// ── String/alignment helpers ──────────────────────────────────────────────────

/// Right-pads `s` with spaces to exactly `width` chars (left-aligned).
fn right_pad(s: &str, width: usize) -> String {
    let n = s.chars().count();
    if n >= width {
        truncate(s, width)
    } else {
        format!("{}{}", s, " ".repeat(width - n))
    }
}

fn center_align_str(s: &str, width: usize) -> String {
    let n = s.chars().count();
    if n >= width {
        truncate(s, width)
    } else {
        let left = (width - n) / 2;
        let right = width - n - left;
        format!("{}{}{}", " ".repeat(left), s, " ".repeat(right))
    }
}

fn distribute_widths(total: usize, count: usize, min_each: usize) -> Vec<usize> {
    if count == 0 {
        return Vec::new();
    }

    let usable = total.max(count * min_each);
    let base = usable / count;
    let extra = usable % count;

    (0..count)
        .map(|index| base + usize::from(index < extra))
        .collect()
}

#[derive(Clone, Debug)]
struct RowMetrics<'a> {
    label: &'a str,
    duration: Option<String>,
}

#[derive(Clone, Debug)]
struct ListColumns {
    label_width: usize,
    spark_width: usize,
    duration_width: usize,
}

#[derive(Clone, Copy, Debug)]
struct VisibleWindow {
    offset: usize,
    visible_rows: usize,
}

fn list_columns(
    total_width: usize,
    rows: &[RowMetrics<'_>],
    min_label: usize,
    max_label: usize,
    min_spark: usize,
) -> ListColumns {
    let percent_width = 4;
    let duration_width = rows
        .iter()
        .filter_map(|row| row.duration.as_ref())
        .map(|duration| duration.chars().count())
        .max()
        .unwrap_or(0)
        .max(6);
    let preferred_label = rows
        .iter()
        .map(|row| row.label.chars().count())
        .max()
        .unwrap_or(min_label)
        .clamp(min_label, max_label);
    let marker_width = 1;
    let spacing_after_label = 2;
    let meta_width = 2 + percent_width + 2 + duration_width;
    let fixed = marker_width + spacing_after_label + meta_width;
    let available = total_width.saturating_sub(fixed);
    let min_visible_label = min_label.min(available.saturating_sub(2)).max(1);
    let reserve_for_spark = min_spark.min(available.saturating_sub(min_visible_label));
    let label_width = preferred_label
        .min(available.saturating_sub(reserve_for_spark))
        .max(min_visible_label);
    let spark_width = available.saturating_sub(label_width).max(2);

    ListColumns {
        label_width,
        spark_width,
        duration_width,
    }
}

fn build_app_histograms(
    apps: &[&crate::tui::data::AppShare],
    width: usize,
    ascii: bool,
) -> Vec<String> {
    if apps.is_empty() {
        return Vec::new();
    }

    let base_bin_count = (width / 2).clamp(18, 36);
    let smoothed = apps
        .iter()
        .map(|app| {
            let coarse = resample_u64_series(&app.sparkline, base_bin_count);
            let smoothed = smooth_bins(&coarse);
            let smoothed = smooth_bins(&smoothed);
            resample_f64_series(&smoothed, width)
        })
        .collect::<Vec<_>>();

    let mut non_zero = smoothed
        .iter()
        .flat_map(|series| series.iter().copied())
        .filter(|value| *value > 0.0)
        .collect::<Vec<_>>();
    non_zero.sort_by(|left, right| left.partial_cmp(right).unwrap_or(std::cmp::Ordering::Equal));
    let global_peak = if non_zero.is_empty() {
        0.0
    } else {
        let idx = ((non_zero.len() - 1) * 17) / 20;
        non_zero[idx].max(*non_zero.last().unwrap_or(&0.0) * 0.55)
    };

    smoothed
        .iter()
        .map(|series| histogram_text(series, global_peak, ascii))
        .collect()
}

fn resample_u64_series(values: &[u64], width: usize) -> Vec<f64> {
    let width = width.max(1);
    if values.is_empty() {
        return vec![0.0; width];
    }

    (0..width)
        .map(|index| {
            let start = index * values.len() / width;
            let end = ((index + 1) * values.len()) / width;
            let span = &values[start..end.max(start + 1)];
            span.iter().copied().sum::<u64>() as f64 / span.len() as f64
        })
        .collect()
}

fn smooth_bins(values: &[f64]) -> Vec<f64> {
    if values.is_empty() {
        return Vec::new();
    }

    (0..values.len())
        .map(|index| {
            let left2 = values[index.saturating_sub(2)];
            let left1 = values[index.saturating_sub(1)];
            let mid = values[index];
            let right1 = values[(index + 1).min(values.len() - 1)];
            let right2 = values[(index + 2).min(values.len() - 1)];
            (left2 + (left1 * 2.0) + (mid * 3.0) + (right1 * 2.0) + right2) / 9.0
        })
        .collect()
}

fn resample_f64_series(values: &[f64], width: usize) -> Vec<f64> {
    let width = width.max(1);
    if values.is_empty() {
        return vec![0.0; width];
    }
    if values.len() == width {
        return values.to_vec();
    }

    (0..width)
        .map(|index| {
            let position = if width == 1 {
                0.0
            } else {
                index as f64 * (values.len().saturating_sub(1)) as f64 / (width - 1) as f64
            };
            let left = position.floor() as usize;
            let right = position.ceil() as usize;
            if left == right {
                values[left]
            } else {
                let t = position - left as f64;
                values[left] * (1.0 - t) + values[right] * t
            }
        })
        .collect()
}

fn histogram_text(values: &[f64], global_peak: f64, ascii: bool) -> String {
    let glyphs = if ascii {
        ['.', ':', '-', '=', '+', '*', '#', '@']
    } else {
        ['▁', '▂', '▃', '▄', '▅', '▆', '▇', '█']
    };
    if values.is_empty() || global_peak <= f64::EPSILON {
        return std::iter::repeat(glyphs[0])
            .take(values.len().max(1))
            .collect();
    }

    let noise_floor = (global_peak * 0.025).max(0.5);
    values
        .iter()
        .map(|value| {
            if *value <= noise_floor {
                return glyphs[0];
            }

            let normalized = (value / global_peak).clamp(0.0, 1.0).powf(0.65);
            if normalized <= 0.08 {
                glyphs[0]
            } else {
                let level = (normalized * (glyphs.len() as f64 - 1.0)).round() as usize;
                glyphs[level.min(glyphs.len() - 1)]
            }
        })
        .collect()
}

fn visible_app_window(
    selected_index: usize,
    current_offset: usize,
    total_rows: usize,
    visible_rows: usize,
) -> VisibleWindow {
    visible_window(selected_index, current_offset, total_rows, visible_rows)
}

fn visible_window(
    selected_index: usize,
    current_offset: usize,
    total_rows: usize,
    visible_rows: usize,
) -> VisibleWindow {
    if total_rows == 0 || visible_rows == 0 {
        return VisibleWindow {
            offset: 0,
            visible_rows,
        };
    }

    let max_offset = total_rows.saturating_sub(visible_rows);
    let mut offset = current_offset.min(max_offset);
    let selected_index = selected_index.min(total_rows.saturating_sub(1));

    if selected_index < offset {
        offset = selected_index;
    } else if selected_index >= offset + visible_rows {
        offset = selected_index + 1 - visible_rows;
    }

    VisibleWindow {
        offset,
        visible_rows,
    }
}

fn time_window_phrase(window: TimeWindow) -> String {
    match window {
        TimeWindow::All => "all time".to_string(),
        TimeWindow::OneHour => "last 1 hour".to_string(),
        TimeWindow::SixHours => "last 6 hours".to_string(),
        TimeWindow::TwentyFourHours => "last 24 hours".to_string(),
        TimeWindow::SevenDays => "last 7 days".to_string(),
        TimeWindow::ThirtyDays => "last 30 days".to_string(),
    }
}

fn is_collector_stale(last_activity_at_utc: Option<DateTime<Utc>>) -> bool {
    last_activity_at_utc
        .map(|timestamp| (Utc::now() - timestamp).num_minutes() > 20)
        .unwrap_or(true)
}

fn collector_state(last_activity_at_utc: Option<DateTime<Utc>>) -> (&'static str, Color) {
    match last_activity_at_utc.map(|timestamp| (Utc::now() - timestamp).num_minutes()) {
        Some(minutes) if minutes <= 5 => ("collecting", ACCENT),
        Some(minutes) if minutes <= 20 => ("idle", WARN),
        Some(_) => ("stale", WARN),
        None => ("stale", WARN),
    }
}

fn focused_panel_hint(app: &DashboardApp) -> String {
    match app.focused_section {
        FocusSection::Summary => {
            "summary \u{2190}\u{2192} cards  u ascii/unicode  tab focus".to_string()
        }
        FocusSection::Apps => {
            "\u{2191}\u{2193} scroll apps  u ascii/unicode  tab focus".to_string()
        }
        FocusSection::Activity => {
            format!(
                "m/k metric  v mode  u ascii/unicode  [{}] window",
                app.time_window.label()
            )
        }
        FocusSection::Heatmap => {
            "\u{2191}\u{2193} select day  u ascii/unicode  tab focus".to_string()
        }
    }
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

fn format_duration(seconds: u64) -> String {
    if seconds < 60 {
        return format!("{seconds}s");
    }

    let days = seconds / 86_400;
    let hours = seconds / 3600;
    let minutes = (seconds % 3600) / 60;

    if days > 0 {
        format!("{days}d {hours_mod:02}h", hours_mod = hours % 24)
    } else if hours > 0 {
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
        _ => format_compact_number(value),
    }
}

fn format_relative_local(value: DateTime<Local>) -> String {
    format_duration_delta(Local::now().signed_duration_since(value).num_seconds())
}

fn format_duration_delta(seconds: i64) -> String {
    let s = seconds.max(0);
    if s < 60 {
        format!("{s}s")
    } else if s < 3600 {
        format!("{}m", s / 60)
    } else if s < 86_400 {
        format!("{}h", s / 3600)
    } else {
        format!("{}d", s / 86_400)
    }
}

fn truncate(value: &str, max_chars: usize) -> String {
    if value.chars().count() <= max_chars {
        return value.to_string();
    }
    let mut s: String = value.chars().take(max_chars.saturating_sub(1)).collect();
    s.push('…');
    s
}
