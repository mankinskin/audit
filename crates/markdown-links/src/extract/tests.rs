use super::{LinkRef, extract};

fn dests(text: &str) -> Vec<String> {
    extract(text).into_iter().map(|link| link.destination).collect()
}

/// Source text of each link's span (`None` when unlocated).
fn span_texts(text: &str) -> Vec<Option<&str>> {
    extract(text)
        .into_iter()
        .map(|link| link.span.map(|span| &text[span]))
        .collect()
}

fn one(text: &str) -> LinkRef {
    let mut links = extract(text);
    assert_eq!(links.len(), 1, "{links:?}");
    links.remove(0)
}

#[test]
fn links_in_code_are_not_extracted() {
    let fenced = "```\n[a](x.md)\n```\n";
    let tilde = "~~~md\n[a](x.md)\n~~~\n";
    let indented = "para\n\n    [a](x.md)\n";
    let code_span = "text `[a](x.md)` and `<https://a.b>`\n";
    for text in [fenced, tilde, indented, code_span] {
        assert!(extract(text).is_empty(), "{text:?}");
    }
    assert_eq!(dests("```\n[a](x.md)\n```\n[b](y.md)\n"), ["y.md"]);
}

#[test]
fn all_link_kinds_have_spans_equal_to_destination() {
    let text = "[i](a.md) ![img](b.png) <https://c.d/e> [full][r] [coll][] [short]\n\n\
                [r]: r.md\n[coll]: c.md\n[short]: s.md\n";
    let links = extract(text);
    let got: Vec<_> = links.iter().map(|link| link.destination.as_str()).collect();
    assert_eq!(got, ["a.md", "b.png", "https://c.d/e", "r.md", "c.md", "s.md"]);
    for link in &links {
        let span = link.span.clone().expect("span located");
        assert_eq!(text[span], link.destination);
    }
}

#[test]
fn shared_definition_gives_the_same_span_and_undefined_reference_none() {
    let text = "[a][x] and [b][x] and [c][missing]\n\n[x]: shared.md\n";
    let links = extract(text);
    assert_eq!(links.len(), 2, "{links:?}");
    assert_eq!(links[0].span, links[1].span);
    assert_eq!(span_texts(text), [Some("shared.md"), Some("shared.md")]);
}

#[test]
fn line_numbers_across_lf_and_crlf() {
    let lf = "[a](a.md)\n\n[b](b.md)\n";
    assert_eq!(extract(lf).iter().map(|l| l.line).collect::<Vec<_>>(), [1, 3]);
    let crlf = lf.replace('\n', "\r\n");
    assert_eq!(extract(&crlf).iter().map(|l| l.line).collect::<Vec<_>>(), [1, 3]);
    assert_eq!(one("x\ny\n[a](a.md)").line, 3);
}

#[test]
fn multiline_separator_inline_and_image() {
    for text in [
        "[a](\ny.md)",
        "[a](\r\ny.md)",
        "![a](\ny.md)",
        "[a](  \n  y.md)",
        "[a](\r\n\ty.md)",
    ] {
        assert_eq!(span_texts(text), [Some("y.md")], "{text:?}");
    }
}

#[test]
fn multiline_separator_in_block_quote_and_list() {
    assert_eq!(span_texts("> [a](\n> y.md)"), [Some("y.md")]);
    assert_eq!(span_texts("> > [a](\n> > y.md)"), [Some("y.md")]);
    assert_eq!(span_texts("- item [a](\n  y.md)"), [Some("y.md")]);
    assert_eq!(span_texts("- item ![a](\r\n  y.md)"), [Some("y.md")]);
}

#[test]
fn multiline_reference_definitions() {
    assert_eq!(span_texts("[a]\n\n[a]:\n  y.md\n"), [Some("y.md")]);
    assert_eq!(span_texts("[a]\n\n[a]:\r\n  y.md\r\n"), [Some("y.md")]);
    assert_eq!(span_texts("> [a]\n>\n> [a]:\n> y.md\n"), [Some("y.md")]);
    assert_eq!(span_texts("- [a]\n\n  [a]:\n  y.md\n"), [Some("y.md")]);
}

#[test]
fn escapes_entities_and_angle_form_have_no_span() {
    assert_eq!(one("[a](<x y.md>)").span, None);
    assert_eq!(one("[a](<x.md>)").span, None);
    assert_eq!(one("[a](a\\_b.md)").span, None);
    assert_eq!(one("[a](a&amp;b.md)").span, None);
    assert_eq!(one("[a]\n\n[a]: <x y.md>\n").span, None);
    assert_eq!(one("[a]\n\n[a]: a\\_b.md\n").span, None);
    assert_eq!(one("[a](a\\_b.md)").destination, "a_b.md");
}

#[test]
fn definition_label_and_title_are_never_matched() {
    let label = "[foo.md]\n\n[foo.md]: foo.md\n";
    let link = one(label);
    assert_eq!(link.span, Some(label.rfind("foo.md").unwrap()..label.len() - 1));
    let title = "[a]\n\n[a]: x.md \"x.md\"\n";
    let link = one(title);
    assert_eq!(link.span, Some(title.find(": x.md").unwrap() + 2..title.find(" \"").unwrap()));
    assert_eq!(span_texts(title), [Some("x.md")]);
}

#[test]
fn inline_link_text_containing_destination_syntax() {
    let text = "[see `](x)`](y.md)";
    let link = one(text);
    assert_eq!(link.destination, "y.md");
    assert_eq!(link.span, Some(text.len() - 5..text.len() - 1));
    assert_eq!(span_texts("[a](y.md \"title\")"), [Some("y.md")]);
}

#[test]
fn autolink_span_is_the_inner_range() {
    let text = "see <https://a.b> now";
    let link = one(text);
    assert_eq!(link.destination, "https://a.b");
    assert_eq!(link.span, Some(5..16));
    assert_eq!(&text[link.span.unwrap()], "https://a.b");
}

#[test]
fn escaped_backslash_label_still_finds_the_destination() {
    let text = "[a\\\\]\n\n[a\\\\]: x.md\n";
    assert_eq!(span_texts(text), [Some("x.md")]);
    // An escaped `]` does not end the label.
    assert_eq!(span_texts("[a\\]b]\n\n[a\\]b]: y.md\n"), [Some("y.md")]);
}

#[test]
fn utf8_text_before_links_keeps_byte_spans() {
    let text = "café [a](é.md) ✓\n[b](x.md)";
    assert_eq!(span_texts(text), [Some("é.md"), Some("x.md")]);
    assert_eq!(extract(text)[1].line, 2);
}
