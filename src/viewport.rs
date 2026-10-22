//! Reveal a block using measured layout bounds rather than estimated line heights.

use iced::advanced::widget::operation::{Outcome, Scrollable};
use iced::advanced::widget::{self, Id, Operation};
use iced::widget::{operation, scrollable};
use iced::{Rectangle, Task, Vector};

pub fn reveal<Message: Send + 'static>(index: usize) -> Task<Message> {
    widget::operate(Locate {
        target: Id::from(format!("block-{index}")),
        origin: 0.0,
        offset: None,
    })
    .then(|y| {
        operation::scroll_to(
            "document-scroll",
            scrollable::AbsoluteOffset {
                x: None,
                y: Some(y),
            },
        )
    })
}

struct Locate {
    target: Id,
    origin: f32,
    offset: Option<f32>,
}

impl Operation<f32> for Locate {
    fn traverse(&mut self, operate: &mut dyn FnMut(&mut dyn Operation<f32>)) {
        operate(self);
    }
    fn scrollable(
        &mut self,
        id: Option<&Id>,
        _bounds: Rectangle,
        content: Rectangle,
        _translation: Vector,
        _state: &mut dyn Scrollable,
    ) {
        if id == Some(&Id::from("document-scroll")) {
            self.origin = content.y;
        }
    }
    fn container(&mut self, id: Option<&Id>, bounds: Rectangle) {
        if id == Some(&self.target) {
            self.offset = Some((bounds.y - self.origin - 24.0).max(0.0));
        }
    }
    fn finish(&self) -> Outcome<f32> {
        self.offset.map_or(Outcome::None, Outcome::Some)
    }
}
