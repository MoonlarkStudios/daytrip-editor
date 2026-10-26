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
    Heading,
    Bold,
    Italic,
    Link,
    Quote,
    List,
    Task,
    Fence,
    Table,
    Image,
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
            Icon::Book => &[
                &[
                    (9., 4.),
                    (6., 2.),
                    (1., 2.),
                    (1., 14.),
                    (6., 14.),
                    (9., 16.),
                ],
                &[
                    (9., 4.),
                    (12., 2.),
                    (17., 2.),
                    (17., 14.),
                    (12., 14.),
                    (9., 16.),
                    (9., 4.),
                ],
            ],
            Icon::Heading => &[
                &[(3., 3.), (3., 15.)],
                &[(11., 3.), (11., 15.)],
                &[(3., 9.), (11., 9.)],
                &[(14., 11.), (16., 10.), (16., 15.)],
            ],
            Icon::Bold => &[
                &[
                    (4., 3.),
                    (10., 3.),
                    (13., 5.),
                    (13., 7.),
                    (10., 9.),
                    (4., 9.),
                    (4., 3.),
                ],
                &[
                    (4., 9.),
                    (11., 9.),
                    (14., 11.),
                    (14., 13.),
                    (11., 15.),
                    (4., 15.),
                    (4., 9.),
                ],
            ],
            Icon::Italic => &[
                &[(8., 3.), (14., 3.)],
                &[(4., 15.), (10., 15.)],
                &[(11., 3.), (7., 15.)],
            ],
            Icon::Link => &[
                &[
                    (7., 6.),
                    (10., 3.),
                    (13., 3.),
                    (15., 5.),
                    (15., 8.),
                    (12., 11.),
                ],
                &[
                    (6., 7.),
                    (3., 10.),
                    (3., 13.),
                    (5., 15.),
                    (8., 15.),
                    (11., 12.),
                ],
                &[(6., 12.), (12., 6.)],
            ],
            Icon::Quote => &[
                &[(3., 8.), (7., 8.), (7., 12.), (3., 12.), (3., 7.), (6., 4.)],
                &[
                    (11., 8.),
                    (15., 8.),
                    (15., 12.),
                    (11., 12.),
                    (11., 7.),
                    (14., 4.),
                ],
            ],
            Icon::List => &[
                &[(3., 4.), (3.2, 4.)],
                &[(7., 4.), (15., 4.)],
                &[(3., 9.), (3.2, 9.)],
                &[(7., 9.), (15., 9.)],
                &[(3., 14.), (3.2, 14.)],
                &[(7., 14.), (15., 14.)],
            ],
            Icon::Task => &[
                &[(9., 3.), (3., 3.), (3., 15.), (15., 15.), (15., 9.)],
                &[(7., 7.), (10., 10.), (16., 3.)],
            ],
            Icon::Fence => &[
                &[(7., 6.), (4., 9.), (7., 12.)],
                &[(11., 6.), (14., 9.), (11., 12.)],
                &[
                    (3., 2.),
                    (15., 2.),
                    (16., 3.),
                    (16., 15.),
                    (15., 16.),
                    (3., 16.),
                    (2., 15.),
                    (2., 3.),
                    (3., 2.),
                ],
            ],
            Icon::Table => &[
                &[(2., 3.), (16., 3.), (16., 15.), (2., 15.), (2., 3.)],
                &[(2., 7.), (16., 7.)],
                &[(2., 11.), (16., 11.)],
                &[(7., 3.), (7., 15.)],
            ],
            Icon::Image => &[
                &[(2., 3.), (16., 3.), (16., 15.), (2., 15.), (2., 3.)],
                &[(2., 13.), (7., 8.), (11., 12.), (13., 10.), (16., 13.)],
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
        if matches!(self.kind, Icon::Image) {
            frame.fill(&Path::circle(Point::new(12.0, 6.5), 1.25), self.color);
        }
        vec![frame.into_geometry()]
    }
}
