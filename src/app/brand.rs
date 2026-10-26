//! The canonical outlined signature, recolored with the active semantic tokens.

use iced::mouse;
use iced::widget::Canvas;
use iced::widget::canvas::{self, Frame, Geometry, Path};
use iced::{Element, Point, Rectangle, Renderer, Theme};

use crate::theme::Tokens;

include!("../../branding/dosty/native-paths.rs");

pub fn view<'a, Message: 'a>(tokens: Tokens) -> Element<'a, Message> {
    Canvas::new(Signature(tokens))
        .width(40.0 * SIGNATURE_WIDTH / SIGNATURE_HEIGHT)
        .height(40)
        .into()
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
        frame.scale(bounds.width / SIGNATURE_WIDTH);
        frame.fill(&pen_page_page(), self.0.brand_paper);
        frame.fill(&pen_page_curl(), self.0.brand_accent);
        frame.fill(
            &pen_page_body(),
            canvas::Fill {
                style: canvas::Style::Solid(self.0.brand_ink),
                rule: canvas::fill::Rule::EvenOdd,
            },
        );
        frame.fill(
            &wordmark(),
            canvas::Fill {
                style: canvas::Style::Solid(self.0.text),
                // Match font/SVG winding: overlapping glyph contours stay filled.
                rule: canvas::fill::Rule::NonZero,
            },
        );
        vec![frame.into_geometry()]
    }
}
