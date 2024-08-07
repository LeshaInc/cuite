pub mod text;

use crate::AnyValue;

pub trait TypedView {
    type Message: 'static;

    fn update(&mut self, message: Self::Message);
}

pub trait View {
    fn update(&mut self, message: AnyValue);
}

impl<V: TypedView> View for V {
    fn update(&mut self, message: AnyValue) {
        self.update(message.downcast());
    }
}
