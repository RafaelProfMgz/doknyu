//! A small line chart of the latest samples, drawn on a canvas in the accent color.

use iced::widget::canvas::{self, Canvas, Frame, Geometry, Path, Stroke};
use iced::{Element, Length, Point, Rectangle, Renderer, Theme, mouse};

use crate::ui::theme::palette;
use crate::ui::tokens::{ALPHA_SOFT, SPARKLINE_HEIGHT, SPARKLINE_STROKE};

/// Heights as fractions of the chart (0 = bottom, 1 = top), one per value. `max` is the value
/// at the top; without it (or when it is not positive) the largest value is. Nothing divides
/// by zero: all-zero values sit at the bottom. Values that are not finite count as zero.
pub fn heights(values: &[f64], max: Option<f64>) -> Vec<f32> {
    let clean = |v: f64| if v.is_finite() { v.max(0.0) } else { 0.0 };
    let top = max
        .filter(|m| m.is_finite() && *m > 0.0)
        .unwrap_or_else(|| values.iter().copied().map(clean).fold(0.0, f64::max));
    values
        .iter()
        .map(|&v| {
            if top > 0.0 {
                (clean(v) / top).clamp(0.0, 1.0) as f32
            } else {
                0.0
            }
        })
        .collect()
}

struct Sparkline {
    heights: Vec<f32>,
}

impl Sparkline {
    /// Points of the line in a chart `w` x `h`, from the left edge to the right one; a single
    /// value is a flat line.
    fn points(&self, w: f32, h: f32) -> Vec<Point> {
        let n = self.heights.len();
        let at = |i: usize, height: f32| {
            let x = if n > 1 {
                w * i as f32 / (n - 1) as f32
            } else {
                0.0
            };
            Point::new(x, h * (1.0 - height))
        };
        let mut points: Vec<Point> = self
            .heights
            .iter()
            .enumerate()
            .map(|(i, &v)| at(i, v))
            .collect();
        if let [only] = points[..] {
            points.push(Point::new(w, only.y));
        }
        points
    }
}

impl<M> canvas::Program<M> for Sparkline {
    type State = ();

    fn draw(
        &self,
        _state: &(),
        renderer: &Renderer,
        theme: &Theme,
        bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Vec<Geometry> {
        let accent = palette(theme).accent;
        let mut frame = Frame::new(renderer, bounds.size());
        let points = self.points(bounds.width, bounds.height);
        if let (Some(first), Some(last)) = (points.first(), points.last()) {
            let area = Path::new(|b| {
                b.move_to(Point::new(first.x, bounds.height));
                for p in &points {
                    b.line_to(*p);
                }
                b.line_to(Point::new(last.x, bounds.height));
                b.close();
            });
            frame.fill(&area, accent.scale_alpha(ALPHA_SOFT));
            let line = Path::new(|b| {
                b.move_to(*first);
                for p in &points[1..] {
                    b.line_to(*p);
                }
            });
            frame.stroke(
                &line,
                Stroke::default()
                    .with_color(accent)
                    .with_width(SPARKLINE_STROKE),
            );
        }
        vec![frame.into_geometry()]
    }
}

/// The chart of `values`. `max` is the value at the top; `None` scales by the largest value.
pub fn sparkline<'a, M: 'a>(values: &[f64], max: Option<f64>) -> Element<'a, M> {
    Canvas::new(Sparkline {
        heights: heights(values, max),
    })
    .width(Length::Fill)
    .height(SPARKLINE_HEIGHT)
    .into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sparkline_handles_all_zero_values() {
        assert_eq!(heights(&[0.0, 0.0, 0.0], None), [0.0, 0.0, 0.0]);
        assert_eq!(heights(&[0.0, 0.0], Some(0.0)), [0.0, 0.0]);
        let _: Element<'_, ()> = sparkline(&[0.0; 60], None);
        let _: Element<'_, ()> = sparkline(&[0.0; 60], Some(0.0));
    }

    #[test]
    fn sparkline_handles_empty_single_and_bad_values() {
        assert!(heights(&[], None).is_empty());
        let _: Element<'_, ()> = sparkline(&[], None);
        let _: Element<'_, ()> = sparkline(&[5.0], None);
        assert_eq!(heights(&[f64::NAN, f64::INFINITY, -3.0], None), [0.0; 3]);
        assert_eq!(heights(&[1.0], Some(f64::NAN)), [1.0]);
        let line = Sparkline { heights: vec![0.5] };
        let points = line.points(100.0, 40.0);
        assert_eq!(points.len(), 2, "a single value is a flat line");
        assert_eq!(points[0].y, points[1].y);
    }

    #[test]
    fn heights_scale_by_the_largest_or_the_given_max() {
        assert_eq!(heights(&[0.0, 5.0, 10.0], None), [0.0, 0.5, 1.0]);
        assert_eq!(heights(&[0.0, 25.0, 50.0], Some(100.0)), [0.0, 0.25, 0.5]);
        // Above the max it stays inside the chart.
        assert_eq!(heights(&[300.0], Some(100.0)), [1.0]);
    }

    #[test]
    fn points_span_the_chart_with_the_top_up() {
        let line = Sparkline {
            heights: vec![0.0, 1.0],
        };
        let points = line.points(100.0, 40.0);
        assert_eq!((points[0].x, points[0].y), (0.0, 40.0));
        assert_eq!((points[1].x, points[1].y), (100.0, 0.0));
    }
}
