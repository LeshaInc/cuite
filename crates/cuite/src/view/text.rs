use super::TypedView;

pub struct Text {
    text: String,
}

impl Text {
    pub fn new(text: String) -> Text {
        Text { text }
    }
}

impl TypedView for Text {
    type Message = String;

    fn update(&mut self, message: String) {
        self.text = message;
    }
}
