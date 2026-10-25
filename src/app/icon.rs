//! Small vector icons drawn with semantic colors; no platform glyph dependencies.

use iced::mouse;
use iced::widget::Canvas;
use iced::widget::canvas::{self, Frame, Geometry, Path, Stroke};
use iced::{Color, Element, Point, Rectangle, Renderer, Theme};

#[derive(Debug, Clone, Copy)]
pub enum Icon {
    Sidebar,
    Document,
    Plus,
    Folder,
    Search,
    Focus,
    Settings,
    Save,
    Code,
    Close,
    Book,
    Export,
    Undo,
    Redo,
}

pub fn view<'a, Message: 'a>(kind: Icon, color: Color) -> Element<'a, Message> {
    Canvas::new(Drawing { kind, color })
        .width(18)
        .height(18)
        .into()
}

struct Drawing {
    kind: Icon,
    color: Color,
}

impl<Message> canvas::Program<Message> for Drawing {
    type State = ();
    fn draw(
        &self,
        _state: &(),
        renderer: &Renderer,
        _theme: &Theme,
        bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Vec<Geometry> {
        let mut frame = Frame::new(renderer, bounds.size());
        let stroke = Stroke::default()
            .with_color(self.color)
            .with_width(1.35)
            .with_line_cap(canvas::LineCap::Round)
            .with_line_join(canvas::LineJoin::Round);
        let paths: &[&[(f32, f32)]] = match self.kind {
            Icon::Sidebar => &[
                &[(3., 3.), (15., 3.), (15., 15.), (3., 15.), (3., 3.)],
                &[(7., 3.), (7., 15.)],
            ],
            Icon::Document => &[
                &[
                    (5., 2.),
                    (11., 2.),
                    (14., 5.),
                    (14., 16.),
                    (4., 16.),
                    (4., 2.),
                    (5., 2.),
                ],
                &[(10., 2.), (10., 6.), (14., 6.)],
                &[(7., 9.), (11., 9.)],
                &[(7., 12.), (11., 12.)],
            ],
            Icon::Plus => &[&[(9., 3.), (9., 15.)], &[(3., 9.), (15., 9.)]],
            Icon::Folder => &[
                &[
                    (2., 5.),
                    (7., 5.),
                    (9., 7.),
                    (16., 7.),
                    (15., 15.),
                    (3., 15.),
                    (2., 5.),
                ],
                &[(2., 5.), (2., 3.), (7., 3.), (9., 5.), (15., 5.)],
            ],
            Icon::Search => &[&[(11., 11.), (16., 16.)]],
            Icon::Focus => &[
                &[(6., 2.), (2., 2.), (2., 6.)],
                &[(12., 2.), (16., 2.), (16., 6.)],
                &[(2., 12.), (2., 16.), (6., 16.)],
                &[(12., 16.), (16., 16.), (16., 12.)],
            ],
            Icon::Settings => &[
                &[(3., 5.), (15., 5.)],
                &[(3., 13.), (15., 13.)],
                &[(6., 3.), (6., 7.)],
                &[(12., 11.), (12., 15.)],
            ],
            Icon::Save => &[
                &[
                    (3., 2.),
                    (13., 2.),
                    (16., 5.),
                    (16., 16.),
                    (2., 16.),
                    (2., 2.),
                    (3., 2.),
                ],
                &[(6., 2.), (6., 7.), (12., 7.), (12., 2.)],
                &[(5., 16.), (5., 11.), (13., 11.), (13., 16.)],
            ],
            Icon::Code => &[
                &[(5., 5.), (1., 9.), (5., 13.)],
                &[(13., 5.), (17., 9.), (13., 13.)],
                &[(10., 3.), (8., 15.)],
            ],
            Icon::Close => &[&[(4., 4.), (14., 14.)], &[(14., 4.), (4., 14.)]],
            Icon::Book => &[],
            Icon::Export => &[
                &[(9., 11.), (9., 2.)],
                &[(5., 6.), (9., 2.), (13., 6.)],
                &[(3., 10.), (3., 16.), (15., 16.), (15., 10.)],
            ],
            Icon::Undo => &[
                &[(6., 4.), (2., 8.), (6., 12.)],
                &[(2., 8.), (11., 8.), (15., 11.), (15., 15.)],
            ],
            Icon::Redo => &[
                &[(12., 4.), (16., 8.), (12., 12.)],
                &[(16., 8.), (7., 8.), (3., 11.), (3., 15.)],
            ],
        };
        for points in paths {
            let path = Path::new(|builder| {
                if let Some(&(x, y)) = points.first() {
                    builder.move_to(Point::new(x, y));
                }
                for &(x, y) in &points[1..] {
                    builder.line_to(Point::new(x, y));
                }
            });
            frame.stroke(&path, stroke);
        }
        if matches!(self.kind, Icon::Search) {
            frame.stroke(&Path::circle(Point::new(7.5, 7.5), 5.0), stroke);
        }
        if matches!(self.kind, Icon::Book) {
            frame.with_save(|frame| {
                // Compact open-book master, in its original 256-unit coordinates.
                // The monochrome treatment follows the active semantic icon color.
                frame.scale(18.0 / 256.0);
                let left = Path::new(|builder| {
                    builder.move_to(Point::new(21.0, 12.0));
                    builder.bezier_curve_to(
                        Point::new(15.0, 8.0),
                        Point::new(10.0, 12.0),
                        Point::new(10.0, 19.0),
                    );
                    builder.line_to(Point::new(10.0, 151.0));
                    builder.bezier_curve_to(
                        Point::new(10.0, 160.0),
                        Point::new(13.0, 166.0),
                        Point::new(20.0, 171.0),
                    );
                    builder.line_to(Point::new(113.0, 244.0));
                    builder.line_to(Point::new(102.0, 65.0));
                    builder.bezier_curve_to(
                        Point::new(101.0, 55.0),
                        Point::new(99.0, 51.0),
                        Point::new(92.0, 47.0),
                    );
                    builder.close();
                });
                let right = Path::new(|builder| {
                    builder.move_to(Point::new(224.0, 59.0));
                    builder.bezier_curve_to(
                        Point::new(230.0, 55.0),
                        Point::new(233.0, 58.0),
                        Point::new(233.0, 65.0),
                    );
                    builder.line_to(Point::new(233.0, 152.0));
                    builder.bezier_curve_to(
                        Point::new(233.0, 160.0),
                        Point::new(231.0, 163.0),
                        Point::new(225.0, 168.0),
                    );
                    builder.line_to(Point::new(143.0, 244.0));
                    builder.line_to(Point::new(156.0, 102.0));
                    builder.bezier_curve_to(
                        Point::new(157.0, 95.0),
                        Point::new(159.0, 92.0),
                        Point::new(165.0, 88.0),
                    );
                    builder.close();
                });
                frame.fill(&left, self.color);
                frame.fill(&right, self.color);
            });
        }
        vec![frame.into_geometry()]
    }
}
