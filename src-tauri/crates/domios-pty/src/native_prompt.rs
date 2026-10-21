use crate::attributed_screen::AttributedScreen;

pub struct NativePromptInput {
    pub screen: AttributedScreen,
    pub cursor_hidden: bool,
    pub alt_screen: bool,
}
