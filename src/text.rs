//! Turns a document (Markdown, HTML, plain prose or code) into blocks of plain text worth reading aloud.

use std::path::Path;
use std::sync::LazyLock;

use pulldown_cmark::{CodeBlockKind, Event, Options, Parser, Tag, TagEnd};
use regex::Regex;

/// How a document is read, decided by its file name.
pub enum Format {
    Markdown,
    /// Turned into Markdown first, without the page's menus, scripts and styles.
    Html,
    /// Paragraphs at blank lines, with nothing treated as Markdown.
    Prose,
    /// Read a line at a time after its name: "Php file.".
    Code(String),
}

impl Format {
    /// `.md` and `.markdown` are Markdown, `.html` and `.htm` are HTML, `.txt` and no
    /// extension are prose, and any other extension is code. Standard input (no path)
    /// is Markdown.
    pub fn of(path: Option<&Path>) -> Self {
        let Some(path) = path else { return Self::Markdown };
        let extension = path.extension().and_then(|e| e.to_str()).unwrap_or("").to_lowercase();
        match extension.as_str() {
            "md" | "markdown" => Self::Markdown,
            "html" | "htm" => Self::Html,
            "txt" => Self::Prose,
            other => language_name(other).map_or(Self::Prose, Self::Code),
        }
    }

    pub fn blocks(&self, document: &str) -> Vec<String> {
        match self {
            Self::Markdown => markdown_blocks(document),
            Self::Html => markdown_blocks(&html_to_markdown(document)),
            Self::Prose => prose_blocks(document),
            Self::Code(name) => {
                let mut blocks = vec![format!("{name} file.")];
                push_code_lines(&mut blocks, document);
                // A file with nothing to say gets no announcement either.
                if blocks.len() == 1 { Vec::new() } else { blocks }
            }
        }
    }
}

/// One block per paragraph, heading, list item, table row or quote. A code block is
/// announced ("Python code block."), read one line per block so each line gets its own
/// pause, and closed with "End of code block.". Inline code is read the same way as
/// code blocks (see `read_code`). HTML and image descriptions are skipped.
fn markdown_blocks(markdown: &str) -> Vec<String> {
    let mut blocks = Vec::new();
    let mut current = String::new();
    let mut code: Option<String> = None;
    let mut skipping = 0usize;

    for event in Parser::new_ext(markdown, Options::ENABLE_TABLES | Options::ENABLE_STRIKETHROUGH) {
        match event {
            Event::Start(Tag::CodeBlock(kind)) => {
                push_block(&mut blocks, &mut current);
                let language = match kind {
                    CodeBlockKind::Fenced(info) => language_name(&info),
                    CodeBlockKind::Indented => None,
                };
                blocks.push(match language {
                    Some(language) => format!("{language} code block."),
                    None => "Code block.".to_string(),
                });
                code = Some(String::new());
            }
            Event::End(TagEnd::CodeBlock) => {
                push_code_lines(&mut blocks, &code.take().unwrap_or_default());
                blocks.push("End of code block.".to_string());
            }
            Event::Text(text) if code.is_some() => code.as_mut().unwrap().push_str(&text),
            Event::Start(Tag::Image { .. } | Tag::HtmlBlock) => skipping += 1,
            Event::End(TagEnd::Image | TagEnd::HtmlBlock) => skipping -= 1,
            _ if skipping > 0 => {}
            Event::Text(text) => current.push_str(&text),
            Event::Code(code) => current.push_str(&read_code(&code)),
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

/// Page chrome is skipped without a word, on purpose: it is not part of the document
/// (ant readaloud-Ed6UZ). `<header>`, `<footer>` and `<aside>` are kept, as documentation
/// often puts real content in them.
fn html_to_markdown(html: &str) -> String {
    let converter = htmd::HtmlToMarkdown::builder()
        .skip_tags(vec!["script", "style", "noscript", "template", "nav"])
        .options(htmd::options::Options {
            // Fenced, so a code block keeps its language: "Rust code block.".
            code_block_style: htmd::options::CodeBlockStyle::Fenced,
            ..Default::default()
        })
        .build();
    // html5ever recovers from any input, and htmd 0.5 never returns an error.
    converter.convert(html).expect("htmd always converts a string")
}

/// One block per paragraph, a paragraph ending at a blank line.
fn prose_blocks(text: &str) -> Vec<String> {
    let mut blocks = Vec::new();
    let mut current = String::new();
    for line in text.lines() {
        if line.trim().is_empty() {
            push_block(&mut blocks, &mut current);
        } else {
            current.push_str(line.trim());
            current.push(' ');
        }
    }
    push_block(&mut blocks, &mut current);
    blocks
}

/// One block per line of code, so each line gets its own pause.
fn push_code_lines(blocks: &mut Vec<String>, code: &str) {
    for line in code.lines() {
        push_block(blocks, &mut read_code(line));
    }
}

/// The fence's language tag (or a file's extension) as it would be said: "python" is "Python", and the usual
/// abbreviations are spelled out.
fn language_name(info: &str) -> Option<String> {
    let tag = info.split(|c: char| c.is_whitespace() || c == ',' || c == '{').next()?.to_lowercase();
    let name = match tag.as_str() {
        "" => return None,
        "sh" => "shell",
        "js" => "JavaScript",
        "ts" => "TypeScript",
        "py" => "Python",
        "rb" => "Ruby",
        "rs" => "Rust",
        // Written so the phonemiser says them as people do: "YAML" and "JSON" get
        // spelled out letter by letter, and "Json" comes out as mush.
        "yaml" | "yml" => "Yaml",
        "json" => "Jason",
        "md" => "Markdown",
        "cpp" | "c++" => "C plus plus",
        "golang" => "Go",
        other => other,
    };
    let mut chars = name.chars();
    Some(chars.next()?.to_uppercase().chain(chars).collect())
}

/// Reads code the way a developer would say it aloud. Most symbols are passed over, but
/// the ones that carry the meaning are said:
///
/// - a dash that starts a word is "minus", one per dash: "-v" is "minus v",
///   "--release" is "minus minus release", "a - b" is "a minus b";
/// - a dot between letters or digits, or starting a name, is "dot": "some-doc.md",
///   ".bashrc", "~/.config";
/// - "|" is "pipe";
/// - "_" is "underscore", wherever it is: "bm_george" is "bm underscore george".
///
/// Hyphens inside a word ("my-app", "x86-64") are left alone, and a line with nothing
/// to say (a YAML "---", a lone "}") is returned as it is, to be dropped.
fn read_code(code: &str) -> String {
    static WORD_START_DASHES: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(^|\s)(-+)").unwrap());
    if !code.chars().any(char::is_alphanumeric) {
        return code.to_string();
    }
    let said = WORD_START_DASHES.replace_all(code, |c: &regex::Captures| {
        format!("{} {} ", &c[1], vec!["minus"; c[2].len()].join(" "))
    });
    let said = read_dots(&said).replace('|', " pipe ").replace('_', " underscore ");
    said.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// " dot " for each dot that is followed by a letter or digit and comes after one, or
/// starts a name (after a space, a slash or the start of the text).
fn read_dots(code: &str) -> String {
    let chars: Vec<char> = code.chars().collect();
    let mut out = String::with_capacity(code.len());
    for (i, &c) in chars.iter().enumerate() {
        let next = chars.get(i + 1).is_some_and(|n| n.is_alphanumeric());
        let prev = i.checked_sub(1).map(|p| chars[p]);
        let after = prev.is_none_or(|p| p.is_alphanumeric() || p.is_whitespace() || p == '/');
        if c == '.' && next && after {
            out.push_str(" dot ");
        } else {
            out.push(c);
        }
    }
    out
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

/// The lines of one of the user's files that aren't blank or a `#` comment: each
/// `key = value` line as its pair, and any other line as itself, to be warned about.
pub fn entries(source: &str) -> impl Iterator<Item = Result<(&str, &str), &str>> {
    source
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .map(|line| match line.split_once('=') {
            Some((key, value)) if !key.trim().is_empty() => Ok((key.trim(), value.trim())),
            _ => Err(line),
        })
}

/// The user's own fixes for words the voice gets wrong, one `word = say this instead` per line.
pub struct Pronunciations(Vec<(Regex, String)>);

impl Pronunciations {
    pub fn parse(source: &str) -> Self {
        let rules = entries(source)
            .filter_map(Result::ok)
            .filter_map(|(word, say)| {
                // \b only means something next to a word character, so ".env" matches too.
                let edge = |c: Option<char>| if c.is_some_and(char::is_alphanumeric) { r"\b" } else { "" };
                // A word in capitals ("IT") only matches capitals, so "it" is left alone.
                let capitals = word.chars().any(char::is_uppercase) && !word.chars().any(char::is_lowercase);
                let pattern = format!(
                    "{}{}{}{}",
                    if capitals { "" } else { "(?i)" },
                    edge(word.chars().next()),
                    regex::escape(word),
                    edge(word.chars().last())
                );
                Some((Regex::new(&pattern).ok()?, say.to_string()))
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
        let doc = "# Install\n\nRun `make` then [open it](https://example.com).\n\n- first item\n- second item\n\n![a robot](icon.png)\n";
        assert_eq!(
            markdown_blocks(doc),
            ["Install.", "Run make then open it.", "first item.", "second item."]
        );
    }

    #[test]
    fn code_blocks_are_announced_read_by_line_and_closed() {
        let doc = "Run this:\n\n```sh\ncargo build --release\ngo test -v ./...\n```\n\nThen carry on.\n";
        assert_eq!(markdown_blocks(doc), [
            "Run this:",
            "Shell code block.",
            "cargo build minus minus release.",
            "go test minus v ./...",
            "End of code block.",
            "Then carry on.",
        ]);
    }

    #[test]
    fn code_block_languages_are_named_and_untagged_blocks_are_plain() {
        assert_eq!(markdown_blocks("```python\nx = 1\n```\n")[0], "Python code block.");
        assert_eq!(markdown_blocks("```rust,ignore\nlet x = 1;\n```\n")[0], "Rust code block.");
        assert_eq!(markdown_blocks("```json\n{}\n```\n")[0], "Jason code block.");
        assert_eq!(markdown_blocks("```\nx = 1\n```\n")[0], "Code block.");
        assert_eq!(markdown_blocks("Text.\n\n    indented = 1\n")[1], "Code block.");
    }

    #[test]
    fn lines_with_nothing_to_say_are_dropped_from_code() {
        let doc = "```yaml\n---\nkey: value\n\n}\n```\n";
        assert_eq!(markdown_blocks(doc), ["Yaml code block.", "key: value.", "End of code block."]);
    }

    #[test]
    fn only_dashes_that_start_a_word_become_minus() {
        assert_eq!(read_code("x = a - b"), "x = a minus b");
        assert_eq!(read_code("-count=1 --race"), "minus count=1 minus minus race");
        assert_eq!(read_code("my-app targets x86-64"), "my-app targets x86-64");
        assert_eq!(read_code("let n = -1;"), "let n = minus 1;");
    }

    #[test]
    fn dots_pipes_and_underscores_are_said() {
        assert_eq!(read_code("readaloud some-doc.md"), "readaloud some-doc dot md");
        assert_eq!(read_code("cat some-doc.md | readaloud"), "cat some-doc dot md pipe readaloud");
        assert_eq!(read_code("cp .bashrc ~/.config/app"), "cp dot bashrc ~/ dot config/app");
        assert_eq!(read_code("af_"), "af underscore");
        assert_eq!(read_code("readaloud --voice bm_george"), "readaloud minus minus voice bm underscore george");
        assert_eq!(read_code("def __init__(self):"), "def underscore underscore init underscore underscore (self):");
        assert_eq!(read_code(r#"{"reasoning_effort": "none"}"#), r#"{"reasoning underscore effort": "none"}"#);
        assert_eq!(read_code("go test ./..."), "go test ./...");
    }

    #[test]
    fn inline_code_is_read_like_code() {
        assert_eq!(
            markdown_blocks("Any voice works: `af_` and `am_` voices, or pass `--voice`."),
            ["Any voice works: af underscore and am underscore voices, or pass minus minus voice."]
        );
    }

    #[test]
    fn table_rows_are_read_cell_by_cell() {
        let doc = "| Provider | Needs |\n|---|---|\n| Kokoro | Nothing |\n";
        assert_eq!(markdown_blocks(doc), ["Provider, Needs.", "Kokoro, Nothing."]);
    }

    #[test]
    fn plain_text_paragraphs_survive() {
        assert_eq!(markdown_blocks("One line.\nStill the same paragraph.\n\nA second one"), [
            "One line. Still the same paragraph.",
            "A second one."
        ]);
    }

    #[test]
    fn the_file_name_decides_the_format() {
        let format = |name: &str| match Format::of(Some(Path::new(name))) {
            Format::Markdown => "markdown".to_string(),
            Format::Html => "html".to_string(),
            Format::Prose => "prose".to_string(),
            Format::Code(language) => language,
        };
        assert_eq!(format("README.md"), "markdown");
        assert_eq!(format("page.HTM"), "html");
        assert_eq!(format("notes.MARKDOWN"), "markdown");
        assert_eq!(format("notes.txt"), "prose");
        assert_eq!(format("LICENSE"), "prose");
        assert_eq!(format("User.php"), "Php");
        assert_eq!(format("main.rs"), "Rust");
        assert!(matches!(Format::of(None), Format::Markdown));
    }

    #[test]
    fn code_files_are_announced_and_read_by_line() {
        // As Markdown, "<?php" opens an HTML block that never closes, and the file vanished.
        let php = "<?php\n\nnamespace App\\Models;\n\nclass User extends Model\n{\n}\n";
        assert_eq!(Format::Code("Php".into()).blocks(php), [
            "Php file.",
            "<?php.",
            "namespace App\\Models;",
            "class User extends Model.",
        ]);
        assert!(Format::Code("Php".into()).blocks("{\n}\n").is_empty());
    }

    #[test]
    fn html_is_read_without_its_menus_scripts_or_styles() {
        let page = r#"<html><head><title>Guide</title><style>p { color: red }</style></head><body>
            <nav><a href="/">Home</a></nav>
            <script>var tracking = 1;</script>
            <h1>Install</h1>
            <p>Run <code>make</code> then <a href="x">open it</a>.</p>
            <pre><code class="language-sh">cargo build --release</code></pre>
            <ul><li>first item</li></ul>
            <aside>A note box.</aside>
        </body></html>"#;
        assert_eq!(Format::Html.blocks(page), [
            "Guide.",
            "Install.",
            "Run make then open it.",
            "Shell code block.",
            "cargo build minus minus release.",
            "End of code block.",
            "first item.",
            "A note box.",
        ]);
    }

    #[test]
    fn prose_is_not_read_as_markdown() {
        let text = "  An indented paragraph,\n  not a code block.\n\n# not a heading\n";
        assert_eq!(Format::Prose.blocks(text), ["An indented paragraph, not a code block.", "# not a heading."]);
    }

    #[test]
    fn entries_hand_back_the_lines_that_are_not_key_value() {
        let source = "# a comment\n\n  macOS = mac O S \nnot a rule\n= no word\n.env = dot env\n";
        let lines: Vec<_> = entries(source).collect();
        assert_eq!(lines, [Ok(("macOS", "mac O S")), Err("not a rule"), Err("= no word"), Ok((".env", "dot env"))]);
    }

    #[test]
    fn the_example_pronunciations_file_parses() {
        let fixes = Pronunciations::parse(include_str!("../pronunciations.txt"));
        assert_eq!(fixes.apply("Runs on macOS."), "Runs on mac O S.");
        assert_eq!(fixes.apply("Is it for a small IT team?"), "Is it for a small I T team?");
    }

    #[test]
    fn a_word_in_capitals_only_matches_capitals() {
        let fixes = Pronunciations::parse("IT = I T\nmacOS = mac O S\n");
        assert_eq!(fixes.apply("Ask IT if it is on MacOS."), "Ask I T if it is on mac O S.");
        assert_eq!(fixes.apply("It is."), "It is.");
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
