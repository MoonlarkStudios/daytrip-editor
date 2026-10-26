//! The canonical outlined signature, recolored with the active semantic tokens.

use iced::mouse;
use iced::widget::Canvas;
use iced::widget::canvas::{self, Frame, Geometry, Path};
use iced::{Color, Element, Point, Rectangle, Renderer, Theme};

use crate::theme::Tokens;

include!("../../branding/daytrip/native-paths.rs");

pub fn view<'a, Message: 'a>(tokens: Tokens) -> Element<'a, Message> {
    Canvas::new(Signature(tokens)).width(160).height(40).into()
}

pub(super) fn draw_book(frame: &mut Frame, left: Color, right: Color) {
    frame.fill(&open_book_compact_left(), left);
    frame.fill(&open_book_compact_right(), right);
}

struct Signature(Tokens);

impl<Message> canvas::Program<Message> for Signature {
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
        frame.scale(bounds.width / 1023.0);
        frame.fill(&open_book_left(), self.0.text);
        frame.fill(&open_book_right(), self.0.brand_accent);
        frame.fill(
            &wordmark(),
            canvas::Fill {
                style: canvas::Style::Solid(self.0.text),
                rule: canvas::fill::Rule::EvenOdd,
            },
        );
        vec![frame.into_geometry()]
    }
}
