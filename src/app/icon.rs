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
    Leaf,
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
            Icon::Leaf => &[
                &[(4., 14.), (9., 9.), (13., 5.)],
                &[
                    (4., 12.),
                    (3., 8.),
                    (6., 4.),
                    (15., 2.),
                    (15., 10.),
                    (11., 14.),
                    (7., 15.),
                    (4., 12.),
                ],
            ],
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
        vec![frame.into_geometry()]
    }
}
