#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HlClass {
    Plain,
    Keyword,
    Property,
    Type,
    Value,
    Comment,
    Variable,
    Generic,
    Operator,
    Number,
    Literal,
    Attribute,
    Addition,
    Deletion,
    Unknown,
}

pub type HlLine = Vec<(String, HlClass)>;

const MAX_TAG_LEN: usize = 256;
const MAX_ENTITY_LEN: usize = 12;

fn class_from_name(name: &str) -> HlClass {
    match name.strip_prefix("hl-").unwrap_or("") {
        "keyword" => HlClass::Keyword,
        "property" => HlClass::Property,
        "type" => HlClass::Type,
        "value" => HlClass::Value,
        "comment" => HlClass::Comment,
        "variable" => HlClass::Variable,
        "generic" => HlClass::Generic,
        "operator" => HlClass::Operator,
        "number" => HlClass::Number,
        "literal" => HlClass::Literal,
        "attribute" => HlClass::Attribute,
        "addition" => HlClass::Addition,
        "deletion" => HlClass::Deletion,
        _ => HlClass::Unknown,
    }
}

fn decode_entity(name: &str) -> Option<char> {
    match name {
        "lt" => Some('<'),
        "gt" => Some('>'),
        "amp" => Some('&'),
        "quot" => Some('"'),
        "apos" => Some('\''),
        "nbsp" => Some('\u{a0}'),
        _ => {
            let num = name.strip_prefix('#')?;
            let code = match num.strip_prefix(['x', 'X']) {
                Some(hex) => u32::from_str_radix(hex, 16).ok()?,
                None => num.parse().ok()?,
            };
            char::from_u32(code)
        }
    }
}

fn span_class(tag: &str) -> Option<HlClass> {
    let (_, rest) = tag.split_once("class=\"")?;
    let (name, _) = rest.split_once('"')?;
    Some(class_from_name(name))
}

struct Builder {
    lines: Vec<HlLine>,
    stack: Vec<HlClass>,
}

impl Builder {
    fn push(&mut self, c: char) {
        let class = self.stack.last().copied().unwrap_or(HlClass::Plain);
        let line = self.lines.last_mut().expect("at least one line");
        match line.last_mut() {
            Some((text, last)) if *last == class => text.push(c),
            _ => line.push((c.to_string(), class)),
        }
    }

    fn tag(&mut self, tag: &str) {
        let name = tag.split_whitespace().next().unwrap_or("");
        match name {
            "span" => {
                let inherited = self.stack.last().copied().unwrap_or(HlClass::Plain);
                self.stack.push(span_class(tag).unwrap_or(inherited));
            }
            "/span" => {
                self.stack.pop();
            }
            _ => {}
        }
    }
}

pub fn highlight(html: &str) -> Vec<HlLine> {
    let mut b = Builder {
        lines: vec![Vec::new()],
        stack: Vec::new(),
    };
    let mut rest = html;
    while let Some(c) = rest.chars().next() {
        let tail = &rest[c.len_utf8()..];
        match c {
            '\n' => {
                b.lines.push(Vec::new());
                rest = tail;
            }
            // 閉じ括弧を末尾まで探すと壊れた `<` が続く入力で O(n^2) になるため、
            // タグ長の上限と、途中の `<` での打ち切りを設けている。
            '<' => match tail
                .char_indices()
                .take(MAX_TAG_LEN)
                .find(|&(_, c)| c == '>' || c == '<')
            {
                Some((i, '>')) => {
                    b.tag(&tail[..i]);
                    rest = &tail[i + 1..];
                }
                _ => {
                    b.push(c);
                    rest = tail;
                }
            },
            // `;` を末尾まで探すと `&` だらけの入力で O(n^2) になるため、先頭の数文字に限る。
            '&' => match tail
                .char_indices()
                .take(MAX_ENTITY_LEN)
                .find(|&(_, c)| c == ';')
            {
                Some((i, _)) => match decode_entity(&tail[..i]) {
                    Some(ch) => {
                        b.push(ch);
                        rest = &tail[i + 1..];
                    }
                    None => {
                        b.push(c);
                        rest = tail;
                    }
                },
                None => {
                    b.push(c);
                    rest = tail;
                }
            },
            _ => {
                b.push(c);
                rest = tail;
            }
        }
    }
    b.lines
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text_of(lines: &[Vec<(String, HlClass)>]) -> String {
        lines
            .iter()
            .map(|l| l.iter().map(|(s, _)| s.as_str()).collect::<String>())
            .collect::<Vec<_>>()
            .join("\n")
    }

    fn span(s: &str, c: HlClass) -> (String, HlClass) {
        (s.to_string(), c)
    }

    // 16. 基本

    #[test]
    fn t16_keyword_span_is_classified_and_rest_is_plain() {
        let lines = highlight("<span class=\"hl-keyword\">fn</span> main() {}");
        assert_eq!(
            lines,
            vec![vec![
                span("fn", HlClass::Keyword),
                span(" main() {}", HlClass::Plain)
            ]]
        );
    }

    #[test]
    fn t16_nested_spans_use_the_innermost_class() {
        let lines = highlight("<span class=\"hl-type\">a<span class=\"hl-value\">b</span>c</span>");
        assert_eq!(
            lines,
            vec![vec![
                span("a", HlClass::Type),
                span("b", HlClass::Value),
                span("c", HlClass::Type)
            ]]
        );
    }

    #[test]
    fn t16_each_class_name_maps_to_its_variant() {
        let cases = [
            ("keyword", HlClass::Keyword),
            ("property", HlClass::Property),
            ("type", HlClass::Type),
            ("value", HlClass::Value),
            ("comment", HlClass::Comment),
            ("variable", HlClass::Variable),
            ("generic", HlClass::Generic),
            ("operator", HlClass::Operator),
            ("number", HlClass::Number),
            ("literal", HlClass::Literal),
            ("attribute", HlClass::Attribute),
            ("addition", HlClass::Addition),
            ("deletion", HlClass::Deletion),
        ];
        for (name, class) in cases {
            let lines = highlight(&format!("<span class=\"hl-{name}\">x</span>"));
            assert_eq!(lines, vec![vec![span("x", class)]], "hl-{name}");
        }
    }

    // 17. エンティティ

    #[test]
    fn t17_known_entities_are_unescaped() {
        let lines = highlight("&lt;&gt;&amp;&quot;&#039;&#39;&#x27;&nbsp;");
        assert_eq!(text_of(&lines), "<>&\"'''\u{a0}");
    }

    #[test]
    fn t17_unknown_entity_is_kept_verbatim() {
        assert_eq!(text_of(&highlight("a &foo; b")), "a &foo; b");
    }

    #[test]
    fn t17_entity_unescape_keeps_enclosing_class() {
        let lines = highlight("<span class=\"hl-value\">&lt;a&gt;</span>");
        assert_eq!(lines, vec![vec![span("<a>", HlClass::Value)]]);
    }

    // 18. 改行

    #[test]
    fn t18_newline_splits_lines_and_span_class_carries_over() {
        let lines = highlight("<span class=\"hl-comment\">a\nb</span> c");
        assert_eq!(
            lines,
            vec![
                vec![span("a", HlClass::Comment)],
                vec![span("b", HlClass::Comment), span(" c", HlClass::Plain)],
            ]
        );
    }

    #[test]
    fn t18_blank_line_between_text_is_an_empty_line() {
        let lines = highlight("a\n\nb");
        assert_eq!(
            lines,
            vec![
                vec![span("a", HlClass::Plain)],
                vec![],
                vec![span("b", HlClass::Plain)]
            ]
        );
    }

    #[test]
    fn t18_lone_newline_gives_two_empty_lines() {
        assert_eq!(highlight("\n"), vec![vec![], vec![]]);
    }

    #[test]
    fn t19_span_without_class_inherits_parent_class() {
        let lines = highlight("<span class=\"hl-comment\">a<span>b</span>c</span>");
        assert_eq!(lines, vec![vec![span("abc", HlClass::Comment)]]);
    }

    // 19. 未知のタグ・壊れた入力

    #[test]
    fn t19_unknown_tag_is_dropped_but_its_text_is_kept() {
        assert_eq!(text_of(&highlight("a<b>x</b>c")), "axc");
    }

    #[test]
    fn t19_unknown_class_keeps_text_as_unknown() {
        let lines = highlight("<span class=\"hl-nonsense\">y</span>");
        assert_eq!(lines, vec![vec![span("y", HlClass::Unknown)]]);
    }

    #[test]
    fn t19_unclosed_span_keeps_text() {
        let lines = highlight("<span class=\"hl-keyword\">z");
        assert_eq!(lines, vec![vec![span("z", HlClass::Keyword)]]);
    }

    #[test]
    fn t19_broken_input_does_not_panic() {
        let inputs = [
            "<",
            "<span",
            "<span class=",
            "<span class=\"hl-keyword",
            "</",
            "</span>",
            "&",
            "&#",
            "&#x;",
            "&#99999999999;",
            "a < b",
            "<<>>",
            "",
            "\n",
            "</span></span>x",
        ];
        for input in inputs {
            let _ = highlight(input);
        }
    }
}
