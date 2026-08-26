use pulldown_cmark::{html, CowStr, Event, Options, Parser, Tag, TagEnd};

use super::sanitize::sanitize_html_for_telegram;

pub fn markdown_to_sanitized_html(markdown: &str) -> String {
    let parser = Parser::new_ext(markdown, Options::ENABLE_STRIKETHROUGH);
    let mut telegram_events = Vec::new();

    for event in parser {
        match event {
            Event::Start(Tag::Paragraph | Tag::List(_)) => {}
            // A paragraph break is a blank line, not a wrapped line: single-spacing
            // here silently reflows the shape the author wrote.
            Event::End(TagEnd::Paragraph | TagEnd::List(_)) => {
                telegram_events.push(Event::Text(CowStr::Borrowed("\n\n")));
            }
            Event::Start(Tag::Item) => {
                telegram_events.push(Event::Text(CowStr::Borrowed("• ")));
            }
            Event::End(TagEnd::Item) => {
                telegram_events.push(Event::Text(CowStr::Borrowed("\n")));
            }
            Event::Start(Tag::Heading { .. }) => telegram_events.push(Event::Start(Tag::Strong)),
            Event::End(TagEnd::Heading(_)) => {
                telegram_events.push(Event::End(TagEnd::Strong));
                telegram_events.push(Event::Text(CowStr::Borrowed("\n")));
            }
            other => telegram_events.push(other),
        }
    }

    let mut html_body = String::new();
    html::push_html(&mut html_body, telegram_events.into_iter());
    sanitize_html_for_telegram(html_body.trim_end())
}
