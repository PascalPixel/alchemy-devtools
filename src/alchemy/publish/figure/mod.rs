//! The progress chart the README shows: each game's DONE code and data, day
//! by day from the history, drawn on whole pixels and written as an indexed
//! PNG.

mod letters;
mod palette;
mod raster;

use super::history::{day_number, History, Measure};
use letters::{Letters, LINE};
use palette::{BLUE, DARK_BLUE, DARK_GOLD, FACE, GOLD, GRID, INK, MUTED, SHADOW, WELL};
use raster::{Canvas, Relief};

const WIDTH: i32 = 838;
pub(crate) const CHART: &str = "PROGRESS_CHART.png";
const LINK: &str = "github.com/PascalPixel/alchemy";
/// Below the plot: the date axis and its labels.
const FOOTER: i32 = 30;

fn latest_label(history: &History, game: &str) -> Option<String> {
    latest(history, game, Measure::percent, "")
}

/// DONE data's label, once a row has recorded it.
fn latest_data_label(history: &History, game: &str) -> Option<String> {
    let found = history
        .days
        .iter()
        .any(|row| row.game(game).and_then(Measure::data_percent).is_some());
    found
        .then(|| latest(history, game, Measure::data_percent, "data "))
        .flatten()
}

fn latest(
    history: &History,
    game: &str,
    value: fn(&Measure) -> Option<f64>,
    prefix: &str,
) -> Option<String> {
    if history.pending(game) {
        return Some(format!("{prefix}?%"));
    }
    history
        .days
        .iter()
        .rev()
        .find_map(|row| row.game(game).and_then(value))
        .map(|value| format!("{prefix}{:.2}%", ((value * 100.0) + 1e-6).floor() / 100.0))
}

fn masthead(canvas: &mut Canvas, letters: &Letters, y: i32, heading: &str) -> i32 {
    let mut x = 8 + canvas.mark(8, y, "alchemy", INK, SHADOW) + 4;
    x += canvas.text(letters, x, y, heading, INK, Some(SHADOW)) + 12;
    x + canvas.text(letters, x, y, LINK, MUTED, Some(SHADOW))
}

pub(crate) fn chart(letters: &Letters, history: &History) -> Canvas {
    let days = &history.days;
    let height = WIDTH * 9 / 16;
    let mut canvas = Canvas::new(WIDTH, height, FACE);
    canvas.bevel(0, 0, WIDTH, height, Relief::Raised);
    canvas.clear_corners(0, 0, WIDTH, height);
    let began = day_number(&history.began).unwrap_or_default();
    let last_hour = days
        .iter()
        .chain(&history.hours)
        .filter_map(|row| super::history::hour_number(&row.date))
        .max()
        .unwrap_or(began * 24)
        .max((began + 1) * 24);
    let last = last_hour / 24;
    let series = [
        ("tbs", "The Broken Seal", GOLD),
        ("tla", "The Lost Age", BLUE),
    ];
    let pending = series.map(|(key, _, _)| history.pending(key));
    let latest_data = series.map(|(key, _, _)| latest_data_label(history, key));
    // Beside a data label, the code label says which it is.
    let latest = series.map(|(key, _, _)| {
        let code = latest_label(history, key);
        match latest_data_label(history, key) {
            Some(_) => code.map(|label| format!("code {label}")),
            None => code,
        }
    });
    let masthead_end = masthead(&mut canvas, letters, 6, "Alchemy: Golden Sun Decompilation");
    let mut key_x = WIDTH - 8;
    for (key, name, ink) in series.iter().rev() {
        key_x -= letters.width(name) as i32;
        canvas.text(letters, key_x, 6, name, INK, Some(SHADOW));
        key_x -= letters::mark_width(key) as i32 + 4;
        canvas.mark(key_x, 6, key, ink, SHADOW);
        key_x -= 12;
    }
    debug_assert!(
        masthead_end + 12 <= key_x - 14,
        "the chart's top line overflows"
    );
    let label_room = latest
        .iter()
        .chain(&latest_data)
        .flatten()
        .map(|label| letters.width(label) as i32)
        .max()
        .unwrap_or(0)
        + 12;
    let (left, top) = (40, 32);
    let (right, bottom) = (WIDTH - 8 - label_room, height - FOOTER);
    let (plot_w, plot_h) = (right - left, bottom - top);
    let waiting = series
        .iter()
        .zip(pending)
        .filter(|(_, pending)| *pending)
        .map(|((_, name, _), _)| *name)
        .collect::<Vec<_>>();
    let last_measured = series.iter().find_map(|(key, name, _)| {
        history.last_measured(key).map(|commit| format!("{name}: last measured at {}", &commit[..7]))
    });
    let note = match waiting.as_slice() {
        [] => last_measured,
        all if all.len() == series.len() => {
            Some("Historical measurements; current DONE pending matching builds".to_string())
        }
        [name, ..] => Some(format!("{name}: historical measurement; pending a matching build")),
    };
    if let Some(note) = note {
        canvas.text(letters, left, top - 16, &note, MUTED, Some(SHADOW));
    }
    let span = last_hour - began * 24;
    let x_hour =
        |hour: i64| left + (((hour - began * 24) * i64::from(plot_w - 1) + span / 2) / span) as i32;
    let x_of = |day: i64| x_hour(day * 24);
    let y_of = |value: f64| {
        bottom - 1 - ((value.clamp(0.0, 100.0) / 100.0) * (plot_h - 2) as f64).round() as i32
    };
    canvas.fill(left, top, plot_w, plot_h, WELL);
    for quarter in 0..=4 {
        let value = quarter as f64 * 25.0;
        let y = y_of(value);
        if quarter > 0 && quarter < 4 {
            canvas.fill(left + 1, y, plot_w - 2, 1, GRID);
        }
        let label = format!("{}%", quarter * 25);
        let width = letters.width(&label) as i32;
        canvas.text(
            letters,
            left - 6 - width,
            y - 8,
            &label,
            MUTED,
            Some(SHADOW),
        );
    }
    canvas.bevel(left - 1, top - 1, plot_w + 2, plot_h + 2, Relief::Sunken);
    let mut labelled_until = i32::MIN;
    for day in began..=last {
        let (name, first) = super::history::day_label(day);
        if !(first || day == began) {
            continue;
        }
        let x = x_of(day);
        canvas.fill(x, bottom + 1, 1, 3, MUTED);
        let width = letters.width(&name) as i32;
        let at = (x - width / 2).clamp(left - 8, WIDTH - 8 - width);
        if at > labelled_until + 8 {
            canvas.text(letters, at, bottom + 6, &name, MUTED, Some(SHADOW));
            labelled_until = at + width;
        }
    }
    let mut ends = Vec::new();
    // DONE data, from 2026-10-08, is a line of its own in the game's darker
    // colour, drawn first so that the code line lies over it where they meet.
    let data_inks = [DARK_GOLD, DARK_BLUE];
    let lines = series
        .iter()
        .zip(&latest_data)
        .zip(&data_inks)
        .map(|(((key, _, _), label), ink)| (super::history::data_points(history, key), 2, ink, label))
        .chain(
            series
                .iter()
                .zip(&latest)
                .map(|((key, _, ink), label)| (super::history::points(history, key), 2, ink, label)),
        );
    for (points, width, ink, label) in lines {
        let points = points
            .into_iter()
            .map(|(hour, value)| (x_hour(hour), y_of(value)))
            .collect::<Vec<_>>();
        for pair in points.windows(2) {
            canvas.line(
                (pair[0].0, pair[0].1 - 1),
                (pair[1].0, pair[1].1 - 1),
                width,
                ink,
            );
        }
        if let (Some(end), Some(label)) = (points.last(), label) {
            canvas.fill(end.0 - 1, end.1 - 2, 4, 4, ink);
            ends.push((end.1, label.clone(), *ink));
        }
    }
    ends.sort_by_key(|(y, _, _)| *y);
    let mut floor = i32::MIN;
    for (y, label, ink) in ends {
        let at = (y - 8).max(floor).min(bottom - 16);
        canvas.text(letters, right + 8, at, &label, ink, Some(SHADOW));
        floor = at + LINE as i32;
    }
    canvas
}

/// The chart of `history` as PNG bytes, dated `date` (`2026-10-20`).
pub(crate) fn png(history: &History, date: &str) -> Result<Vec<u8>, String> {
    chart(&Letters::face(), history).png(letters::FIGURE_SCALE, date)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::alchemy::publish::history::{Current, Day};
    use letters::fixture;
    use raster::{blend, rgb};
    fn row(date: &str, tbs: Option<f64>, tla: Option<f64>) -> Day {
        Day {
            tbs: tbs.map(Measure::published),
            tla: tla.map(Measure::published),
            ..Day::new(date)
        }
    }
    #[test]
    fn the_chart_steps_its_corners_on_whole_pixels() {
        let history = History {
            began: "2026-07-16".into(),
            days: vec![row("2026-07-16", Some(1.0), None)],
            ..History::default()
        };
        let canvas = chart(&fixture(), &history);
        let light = blend(rgb(palette::LIGHT), rgb(FACE), palette::LIGHT_OPACITY);
        let step = (0..).find(|&a| !palette::cut(a, 0, palette::CORNER)).unwrap();
        assert_eq!(canvas.get(0, 0), None);
        assert_eq!(canvas.get(step - 1, 0), None);
        assert_eq!(canvas.get(step, 0), Some(light));
        assert_eq!(canvas.rgba(2), chart(&fixture(), &history).rgba(2));
    }
    #[test]
    fn the_chart_starts_each_line_at_its_first_measurement() {
        let letters = fixture();
        let history = History {
            began: "2026-07-16".into(),
            days: vec![
                row("2026-07-16", Some(1.0), None),
                Day {
                    correction: Some("stricter".into()),
                    ..row("2026-09-09", Some(44.0), None)
                },
                row("2026-09-18", Some(56.0), Some(0.2)),
                Day {
                    tbs: Some(Measure::bytes(6486, 10000)),
                    tla: Some(Measure::bytes(213, 10000)),
                    ..Day::new("2026-09-24")
                },
            ],
            ..History::default()
        };
        let canvas = chart(&letters, &history);
        assert_eq!((canvas.width, canvas.height), (838, 471));
        let bottom = canvas.height - FOOTER;
        let gold = rgb(GOLD);
        let blue = rgb(BLUE);
        let column = |x: i32, ink| (0..bottom).any(|y| canvas.get(x, y) == Some(ink));
        assert!(column(41, gold));
        assert!(!column(41, blue) && !column(300, blue));
        let right = (0..WIDTH).rev().find(|x| column(*x, blue)).unwrap();
        assert!(right > 600);
        // The plot reaches down to the date axis.
        let well = rgb(WELL);
        assert_eq!(canvas.get(WIDTH / 2, bottom - 2), Some(well));
    }
    #[test]
    fn hourly_points_change_the_line_and_leave_the_date_axis_alone() {
        let mut history = History {
            began: "2026-07-16".into(),
            days: vec![
                row("2026-07-16", Some(1.0), None),
                row("2026-09-23", Some(60.0), None),
                row("2026-09-24", Some(65.0), None),
            ],
            ..History::default()
        };
        let daily = chart(&fixture(), &history);
        history.hours = vec![
            row("2026-09-23T08", Some(40.0), None),
            row("2026-09-23T20", Some(60.0), None),
        ];
        let hourly = chart(&fixture(), &history);
        assert_ne!(daily.rgba(1), hourly.rgba(1));
        for y in (daily.height - FOOTER + 2)..daily.height {
            for x in 0..daily.width {
                assert_eq!(daily.get(x, y), hourly.get(x, y), "{x},{y}");
            }
        }
    }
    #[test]
    fn pending_audits_label_unknown_and_keep_the_historical_lines() {
        let history = History {
            began: "2026-09-27".into(),
            current: Some(Current {
                date: "2026-09-28".into(),
                tbs_pending: true,
                tla_pending: true,
                tla_last: None,
            }),
            days: vec![
                row("2026-09-27", Some(70.0), Some(2.0)),
                row("2026-09-28", Some(73.0), Some(2.1)),
            ],
            ..History::default()
        };
        let published = history.days.clone();
        assert_eq!(latest_label(&history, "tbs").as_deref(), Some("?%"));
        assert_eq!(latest_label(&history, "tla").as_deref(), Some("?%"));
        let canvas = chart(&fixture(), &history);
        for ink in [GOLD, BLUE] {
            let ink = rgb(ink);
            assert!((32..canvas.height - FOOTER).any(|y| canvas.get(41, y) == Some(ink)));
        }
        assert_eq!(history.days, published);
        // A last measurement keeps its figure and says where it was taken.
        let mut last = history.clone();
        last.current = Some(Current {
            date: "2026-09-28".into(),
            tbs_pending: false,
            tla_pending: false,
            tla_last: Some("0123456789abcdef0123456789abcdef01234567".into()),
        });
        assert_eq!(latest_label(&last, "tla").as_deref(), Some("2.10%"));
        assert_ne!(chart(&fixture(), &last).rgba(1), chart(&fixture(), &history).rgba(1));
    }
    #[test]
    fn data_draws_its_own_line_and_label_once_recorded() {
        let mut history = History {
            began: "2026-10-01".into(),
            days: vec![
                row("2026-10-01", Some(90.0), None),
                row("2026-10-05", Some(100.0), None),
            ],
            ..History::default()
        };
        assert_eq!(latest_data_label(&history, "tbs"), None);
        let code_only = chart(&fixture(), &history);
        let with_data = |date: &str, data| Day {
            tbs: Some(Measure {
                data: Some(data),
                data_total: Some(100),
                ..Measure::bytes(100, 100)
            }),
            ..Day::new(date)
        };
        history.days.push(with_data("2026-10-08", 40));
        history.days.push(with_data("2026-10-09", 50));
        assert_eq!(latest_label(&history, "tbs").as_deref(), Some("100.00%"));
        assert_eq!(latest_data_label(&history, "tbs").as_deref(), Some("data 50.00%"));
        let drawn = chart(&fixture(), &history);
        assert_ne!(code_only.rgba(1), drawn.rgba(1));
        // The data line has the game's darker colour, which code alone never draws.
        let dark = rgb(DARK_GOLD);
        let has = |canvas: &Canvas| (0..WIDTH).any(|x| (32..canvas.height - FOOTER).any(|y| canvas.get(x, y) == Some(dark)));
        assert!(has(&drawn) && !has(&code_only));
    }
}
