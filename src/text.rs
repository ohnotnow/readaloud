//! Turns a Markdown (or plain text) document into blocks of plain text worth reading aloud.

use pulldown_cmark::{Event, Options, Parser, Tag, TagEnd};
use regex::Regex;

/// One block per paragraph, heading, list item, table row or quote. Code blocks, HTML and
/// image descriptions are skipped; inline code is read as ordinary words.
pub fn blocks(markdown: &str) -> Vec<String> {
    let mut blocks = Vec::new();
    let mut current = String::new();
    let mut skipping = 0usize;

    for event in Parser::new_ext(markdown, Options::ENABLE_TABLES | Options::ENABLE_STRIKETHROUGH) {
        match event {
            Event::Start(Tag::CodeBlock(_) | Tag::Image { .. } | Tag::HtmlBlock) => skipping += 1,
            Event::End(TagEnd::CodeBlock | TagEnd::Image | TagEnd::HtmlBlock) => skipping -= 1,
            _ if skipping > 0 => {}
            Event::Text(text) | Event::Code(text) => current.push_str(&text),
            Event::SoftBreak | Event::HardBreak => current.push(' '),
            Event::End(TagEnd::TableCell) => current.push_str(", "),
            Event::Start(Tag::Item | Tag::BlockQuote(_))
            | Event::End(
                TagEnd::Paragraph
                | TagEnd::Heading(_)
                | TagEnd::Item
                | TagEnd::TableHead
                | TagEnd::TableRow
                | TagEnd::BlockQuote(_),
            ) => push_block(&mut blocks, &mut current),
            _ => {}
        }
    }
    push_block(&mut blocks, &mut current);
    blocks
}

/// Headings, list items and table rows often have no closing punctuation; adding a full
/// stop gives Kokoro its cue to pause before the next block.
fn push_block(blocks: &mut Vec<String>, current: &mut String) {
    let text = current.trim().trim_end_matches(',').trim_end();
    if text.chars().any(char::is_alphanumeric) {
        let mut text = text.to_string();
        if !text.ends_with(['.', '!', '?', ':', ';']) {
            text.push('.');
        }
        blocks.push(text);
    }
    current.clear();
}

/// The user's own fixes for words the voice gets wrong, one `word = say this instead` per line.
pub struct Pronunciations(Vec<(Regex, String)>);

impl Pronunciations {
    pub fn parse(source: &str) -> Self {
        let rules = source
            .lines()
            .map(str::trim)
            .filter(|line| !line.is_empty() && !line.starts_with('#'))
            .filter_map(|line| line.split_once('='))
            .filter_map(|(word, say)| {
                let word = word.trim();
                // \b only means something next to a word character, so ".env" matches too.
                let edge = |c: Option<char>| if c.is_some_and(char::is_alphanumeric) { r"\b" } else { "" };
                let pattern = format!(
                    "(?i){}{}{}",
                    edge(word.chars().next()),
                    regex::escape(word),
                    edge(word.chars().last())
                );
                Some((Regex::new(&pattern).ok()?, say.trim().to_string()))
            })
            .collect();
        Self(rules)
    }

    pub fn apply(&self, text: &str) -> String {
        self.0.iter().fold(text.to_string(), |text, (word, say)| {
            word.replace_all(&text, regex::NoExpand(say)).into_owned()
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn markdown_becomes_readable_blocks() {
        let doc = "# Install\n\nRun `make` then [open it](https://example.com).\n\n```sh\nmake install\n```\n\n- first item\n- second item\n\n![a robot](icon.png)\n";
        assert_eq!(
            blocks(doc),
            ["Install.", "Run make then open it.", "first item.", "second item."]
        );
    }

    #[test]
    fn table_rows_are_read_cell_by_cell() {
        let doc = "| Provider | Needs |\n|---|---|\n| Kokoro | Nothing |\n";
        assert_eq!(blocks(doc), ["Provider, Needs.", "Kokoro, Nothing."]);
    }

    #[test]
    fn plain_text_paragraphs_survive() {
        assert_eq!(blocks("One line.\nStill the same paragraph.\n\nA second one"), [
            "One line. Still the same paragraph.",
            "A second one."
        ]);
    }

    #[test]
    fn the_example_pronunciations_file_parses() {
        let fixes = Pronunciations::parse(include_str!("../pronunciations.txt"));
        assert_eq!(fixes.apply("Runs on macOS."), "Runs on mac O S.");
    }

    #[test]
    fn pronunciations_replace_whole_words_in_any_case() {
        let fixes = Pronunciations::parse("# my fixes\nmacOS = mac O S\n.env = dot env\n\nnot a rule\n");
        assert_eq!(
            fixes.apply("MacOS reads .env but not macOSX."),
            "mac O S reads dot env but not macOSX."
        );
    }
}
