use std::fmt::Display;

use ohm::Encoder;

pub trait View {
    fn draw(&mut self, encoder: &mut Encoder);
}

pub trait IntoView {
    type View: View;

    fn into_view(self) -> Self::View;
}

impl<V: View> IntoView for V {
    type View = V;

    fn into_view(self) -> V {
        self
    }
}

pub trait ViewTuple {}

impl<V: View> ViewTuple for V {}

impl<A: ViewTuple> ViewTuple for (A,) {}

impl<A: ViewTuple, B: ViewTuple> ViewTuple for (A, B) {}

pub fn container(views: impl ViewTuple) -> impl IntoView {
    Container { views }
}

pub struct Container<VT> {
    views: VT,
}

impl<VT: ViewTuple> View for Container<VT> {
    fn draw(&mut self, encoder: &mut Encoder) {
        //
    }
}

pub fn label<T: Display>(text: impl Fn() -> T + 'static) -> impl IntoView {
    Label {}
}

pub struct Label {}

impl View for Label {
    fn draw(&mut self, encoder: &mut Encoder) {
        //
    }
}
