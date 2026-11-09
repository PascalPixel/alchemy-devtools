//! The progress chart the README shows: each game's DONE code, day by day
//! from the history, drawn on whole pixels and written as an indexed PNG.
//! Each game's name at the top right, with its icon, is drawn in its line's
//! colour, which is the chart's only key.

mod letters;
mod palette;
mod raster;

use super::history::{day_number, History};
use letters::Letters;
use palette::{BLUE, FACE, GOLD, GRID, INK, MUTED, SHADOW, WELL};
use raster::{Canvas, Relief};

const WIDTH: i32 = 838;
pub(crate) const CHART: &str = "PROGRESS_CHART.png";
const LINK: &str = "github.com/PascalPixel/alchemy";
/// Below the plot: the date axis and its labels.
const FOOTER: i32 = 30;

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
    let masthead_end = masthead(&mut canvas, letters, 6, "Alchemy: Golden Sun Decompilation");
    let mut key_x = WIDTH - 8;
    for (key, name, ink) in series.iter().rev() {
        key_x -= letters.width(name) as i32;
        canvas.text(letters, key_x, 6, name, ink, Some(SHADOW));
        key_x -= letters::mark_width(key) as i32 + 4;
        canvas.mark(key_x, 6, key, ink, SHADOW);
        key_x -= 12;
    }
    debug_assert!(
        masthead_end + 12 <= key_x - 14,
        "the chart's top line overflows"
    );
    let (left, top) = (40, 32);
    let (right, bottom) = (WIDTH - 8, height - FOOTER);
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
    for (key, _, ink) in &series {
        let points = super::history::points(history, key)
            .into_iter()
            .map(|(hour, value)| (x_hour(hour), y_of(value)))
            .collect::<Vec<_>>();
        for pair in points.windows(2) {
            canvas.line(
                (pair[0].0, pair[0].1 - 1),
                (pair[1].0, pair[1].1 - 1),
                2,
                ink,
            );
        }
        if let Some(end) = points.last() {
            canvas.fill(end.0 - 1, end.1 - 2, 4, 4, ink);
        }
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
    use crate::alchemy::publish::history::{Current, Day, Measure};
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
    fn pending_audits_keep_the_historical_lines() {
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
        let canvas = chart(&fixture(), &history);
        for ink in [GOLD, BLUE] {
            let ink = rgb(ink);
            assert!((32..canvas.height - FOOTER).any(|y| canvas.get(41, y) == Some(ink)));
        }
        assert_eq!(history.days, published);
        // A last measurement says where it was taken.
        let mut last = history.clone();
        last.current = Some(Current {
            date: "2026-09-28".into(),
            tbs_pending: false,
            tla_pending: false,
            tla_last: Some("0123456789abcdef0123456789abcdef01234567".into()),
        });
        assert_ne!(chart(&fixture(), &last).rgba(1), chart(&fixture(), &history).rgba(1));
    }
    #[test]
    fn the_chart_draws_code_alone_and_keys_it_by_the_names_colours() {
        let code = |date: &str, done| Day {
            tbs: Some(Measure::bytes(done, 100)),
            ..Day::new(date)
        };
        let with_data = |date: &str, done, data| Day {
            tbs: Some(Measure {
                data: Some(data),
                data_total: Some(100),
                ..Measure::bytes(done, 100)
            }),
            ..Day::new(date)
        };
        let history = |days| History {
            began: "2026-10-01".into(),
            days,
            ..History::default()
        };
        let code_only = chart(&fixture(), &history(vec![code("2026-10-01", 90), code("2026-10-09", 100)]));
        let drawn = chart(
            &fixture(),
            &history(vec![with_data("2026-10-01", 90, 40), with_data("2026-10-09", 100, 50)]),
        );
        // Recorded data draws nothing.
        assert_eq!(code_only.rgba(1), drawn.rgba(1));
        // The names at the top right are their lines' colours, not white.
        let white = rgb(INK);
        assert!((WIDTH - 260..WIDTH - 8).all(|x| (0..28).all(|y| drawn.get(x, y) != Some(white))));
        let gold = rgb(GOLD);
        assert!((WIDTH - 260..WIDTH - 8).any(|x| (0..28).any(|y| drawn.get(x, y) == Some(gold))));
    }
}
