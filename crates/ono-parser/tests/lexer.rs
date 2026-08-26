//! Quoting, escaping and token-shape corpus (spec §35.1, ADR-0009 "Lexical rules").

#![allow(
    clippy::panic,
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "a failed precondition in a test should abort the test loudly"
)]

use ono_parser::{
    ArgMode, Argument, CurrentSelector, Expr, NumberValue, Statement, StrPart, TokenKind, Unit,
    parse, tokens,
};

fn kinds(source: &str) -> Vec<TokenKind> {
    tokens(source).into_iter().map(|token| token.kind).collect()
}

fn texts(source: &str) -> Vec<&str> {
    tokens(source)
        .into_iter()
        .map(|token| token.span.of(source))
        .collect()
}

fn arguments(source: &str) -> Vec<Argument> {
    let parsed = parse(source);
    assert!(
        parsed.diagnostics().is_empty(),
        "unexpected diagnostics for {source:?}: {:?}",
        parsed.diagnostics()
    );
    let Some(Statement::Pipeline(pipeline)) = parsed.program().statements.first().cloned() else {
        panic!("expected a pipeline statement in {source:?}");
    };
    pipeline
        .head
        .stages
        .into_iter()
        .next()
        .expect("expected one stage")
        .arguments
}

fn single_string(source: &str) -> Vec<StrPart> {
    match arguments(source).into_iter().next() {
        Some(Argument::Value(Expr::Str(literal))) => literal.parts,
        other => panic!("expected a string argument in {source:?}, got {other:?}"),
    }
}

fn text_of(parts: &[StrPart]) -> String {
    parts
        .iter()
        .map(|part| match part {
            StrPart::Text { text, .. } => text.clone(),
            StrPart::Expr(_) => "<expr>".to_owned(),
        })
        .collect()
}

#[test]
fn should_lex_a_bare_command_line_as_words_when_the_head_is_a_words_mode_command() {
    assert_eq!(kinds("ls -la"), vec![TokenKind::Word, TokenKind::Word]);
    assert_eq!(texts("ls -la"), vec!["ls", "-la"]);
}

#[test]
fn should_keep_punctuation_inside_a_word_when_it_is_not_structural() {
    let source = "run ./src a-b --recursive /usr/bin 1.2.3 user@host *.tmp a#b";
    assert_eq!(
        texts(source),
        vec![
            "run",
            "./src",
            "a-b",
            "--recursive",
            "/usr/bin",
            "1.2.3",
            "user@host",
            "*.tmp",
            "a#b",
        ]
    );
    assert!(kinds(source).iter().all(|kind| *kind == TokenKind::Word));
}

#[test]
fn should_lex_expression_arguments_as_identifiers_when_the_head_is_expression_mode() {
    assert_eq!(
        kinds("where cpu > 20"),
        vec![
            TokenKind::Word,
            TokenKind::Ident,
            TokenKind::Gt,
            TokenKind::Int
        ]
    );
}

#[test]
fn should_treat_a_hash_as_a_comment_when_it_begins_a_token() {
    assert_eq!(
        kinds("ls # a trailing note"),
        vec![TokenKind::Word, TokenKind::Comment]
    );
    assert_eq!(texts("ls # a trailing note")[1], "# a trailing note");
    assert!(parse("# only a comment").diagnostics().is_empty());
}

#[test]
fn should_decode_every_escape_when_the_string_is_double_quoted() {
    let parts = single_string(r#"echo "a\tb\nc\rd\0e\ef\x41g\u{1F600}h\\i\"j""#);
    assert_eq!(
        text_of(&parts),
        "a\tb\nc\rd\0e\u{1b}f\u{41}g\u{1F600}h\\i\"j"
    );
}

#[test]
fn should_keep_backslashes_verbatim_when_the_string_is_raw() {
    let parts = single_string(r#"echo 'a\tb\n$name'"#);
    assert_eq!(text_of(&parts), r"a\tb\n$name");
    assert_eq!(parts.len(), 1, "a raw string never interpolates");
}

#[test]
fn should_split_a_double_quoted_string_into_parts_when_it_interpolates() {
    let parts = single_string(r#"echo "home is $env.HOME ok""#);
    assert_eq!(parts.len(), 3);
    assert_eq!(text_of(&parts), "home is <expr> ok");
    let StrPart::Expr(Expr::Field(access)) = &parts[1] else {
        panic!("expected a field access, got {:?}", parts[1]);
    };
    assert_eq!(access.field, "HOME");
}

#[test]
fn should_embed_a_pipeline_when_a_string_interpolates_a_subshell() {
    let parts = single_string(r#"echo "count is $(get process | count)""#);
    assert_eq!(text_of(&parts), "count is <expr>");
    let StrPart::Expr(Expr::Paren(paren)) = &parts[1] else {
        panic!("expected a parenthesised value, got {:?}", parts[1]);
    };
    assert!(paren.pipeline().is_some(), "$( … ) holds a pipeline");
}

#[test]
fn should_lex_a_unit_literal_as_one_token_when_the_suffix_is_adjacent() {
    for (source, unit) in [
        ("where a > 512MiB", Unit::MiB),
        ("where a > 250ms", Unit::Ms),
        ("where a > 7d", Unit::D),
        ("where a > 95%", Unit::Percent),
        ("where a > 1.5GB", Unit::GB),
    ] {
        assert_eq!(
            kinds(source),
            vec![
                TokenKind::Word,
                TokenKind::Ident,
                TokenKind::Gt,
                TokenKind::Unit
            ],
            "{source:?} must lex the unit literal as a single token"
        );
        let Some(Argument::Value(Expr::Binary(binary))) = arguments(source).into_iter().next()
        else {
            panic!("expected a comparison in {source:?}");
        };
        let Expr::Unit(literal) = binary.rhs else {
            panic!(
                "expected a unit literal in {source:?}, got {:?}",
                binary.rhs
            );
        };
        assert_eq!(literal.unit, unit);
    }
}

#[test]
fn should_lex_a_number_and_an_identifier_when_the_unit_suffix_is_separated_by_space() {
    assert_eq!(
        kinds("where a > 512 MiB"),
        vec![
            TokenKind::Word,
            TokenKind::Ident,
            TokenKind::Gt,
            TokenKind::Int,
            TokenKind::Ident
        ]
    );
}

#[test]
fn should_lex_a_number_and_an_identifier_when_the_suffix_is_not_a_unit() {
    assert_eq!(
        kinds("where a > 7days"),
        vec![
            TokenKind::Word,
            TokenKind::Ident,
            TokenKind::Gt,
            TokenKind::Int,
            TokenKind::Ident
        ]
    );
}

#[test]
fn should_read_every_numeric_base_and_separator_when_a_number_is_expected() {
    for (source, expected) in [
        ("take 42", NumberValue::Int(42)),
        ("take 0x1f", NumberValue::Int(31)),
        ("take 0b1010", NumberValue::Int(10)),
        ("take 1_000_000", NumberValue::Int(1_000_000)),
        ("take 1.5", NumberValue::Float(1.5)),
        ("take 1.5e3", NumberValue::Float(1500.0)),
    ] {
        let Some(Argument::Value(Expr::Number(number))) = arguments(source).into_iter().next()
        else {
            panic!("expected a number in {source:?}");
        };
        assert_eq!(number.value, expected, "{source:?}");
    }
}

#[test]
fn should_lex_a_regex_when_it_stands_at_an_operand_position_in_expression_mode() {
    assert_eq!(
        kinds("where name ~= /postgres|redis/i"),
        vec![
            TokenKind::Word,
            TokenKind::Ident,
            TokenKind::Match,
            TokenKind::Regex
        ]
    );
    let Some(Argument::Value(Expr::Binary(binary))) = arguments("where name ~= /postgres|redis/i")
        .into_iter()
        .next()
    else {
        panic!("expected a match expression");
    };
    let Expr::Regex(regex) = binary.rhs else {
        panic!("expected a regex literal");
    };
    assert_eq!(regex.pattern, "postgres|redis");
    assert_eq!(regex.flags, "i");
}

#[test]
fn should_lex_a_path_as_a_word_when_the_head_is_words_mode() {
    assert_eq!(kinds("cat /etc/passwd"), vec![TokenKind::Word; 2]);
    assert_eq!(texts("cat /etc/passwd")[1], "/etc/passwd");
}

#[test]
fn should_lex_a_variable_with_its_field_path_when_the_head_is_words_mode() {
    assert_eq!(
        kinds("echo $env.PATH"),
        vec![TokenKind::Word, TokenKind::Variable]
    );
    assert_eq!(texts("echo $env.PATH")[1], "$env.PATH");
}

#[test]
fn should_lex_the_current_value_forms_when_they_are_written() {
    for (source, expected) in [
        ("echo @", CurrentSelector::Current),
        ("echo @-1", CurrentSelector::Previous(1)),
        ("echo @3", CurrentSelector::Item(3)),
    ] {
        assert_eq!(
            kinds(source),
            vec![TokenKind::Word, TokenKind::CurrentValue],
            "{source:?}"
        );
        let Some(Argument::Value(Expr::CurrentValue(current))) =
            arguments(source).into_iter().next()
        else {
            panic!("expected a current value in {source:?}");
        };
        assert_eq!(current.selector, expected, "{source:?}");
    }
}

#[test]
fn should_split_a_namespace_from_a_name_when_the_head_is_qualified() {
    for (source, namespace, name) in [
        ("ono:get process", "ono", "get"),
        ("exec:ls -la", "exec", "ls"),
    ] {
        let parsed = parse(source);
        assert!(parsed.diagnostics().is_empty(), "{source:?}");
        let Some(Statement::Pipeline(pipeline)) = parsed.program().statements.first() else {
            panic!("expected a pipeline in {source:?}");
        };
        let head = pipeline.head.stages[0]
            .head
            .command()
            .expect("a command head");
        assert_eq!(head.namespace.as_deref(), Some(namespace), "{source:?}");
        assert_eq!(head.name, name, "{source:?}");
    }
}

#[test]
fn should_lex_a_file_descriptor_prefixed_redirection_when_no_space_precedes_the_operator() {
    assert_eq!(
        kinds("cmd 2> err.log"),
        vec![TokenKind::Word, TokenKind::Gt, TokenKind::Word]
    );
    assert_eq!(texts("cmd 2> err.log")[1], "2>");
    assert_eq!(
        kinds("cmd 2>> err.log"),
        vec![TokenKind::Word, TokenKind::GtGt, TokenKind::Word]
    );
    assert_eq!(
        kinds("cmd 2>&1"),
        vec![TokenKind::Word, TokenKind::GtAmp, TokenKind::Word]
    );
    assert_eq!(
        kinds("cmd <&0"),
        vec![TokenKind::Word, TokenKind::LtAmp, TokenKind::Word]
    );
}

#[test]
fn should_report_the_exact_source_span_of_every_token() {
    let source = "git commit -m \"wip\"";
    let spans: Vec<(u32, u32)> = tokens(source)
        .into_iter()
        .map(|token| (token.span.start(), token.span.end()))
        .collect();
    assert_eq!(spans, vec![(0, 3), (4, 10), (11, 13), (14, 19)]);
}

#[test]
fn should_select_the_argument_mode_from_the_head_word() {
    for head in [
        "where", "select", "sort", "group", "take", "skip", "each", "reduce", "count", "measure",
        "join", "diff",
    ] {
        assert_eq!(ArgMode::for_head(head), ArgMode::Expression, "{head}");
    }
    for head in ["ls", "git", "get", "to", "from", "format", "cat"] {
        assert_eq!(ArgMode::for_head(head), ArgMode::Words, "{head}");
    }
}

#[test]
fn should_keep_an_escaped_space_inside_the_word_when_a_path_contains_one() {
    // ADR-0019: `cd My\ Documents` is muscle memory for every user this shell replaces Bash for.
    // The backslash keeps the word going; unescaping is the evaluator's job, so the token still
    // carries what was typed.
    let parsed = ono_parser::parse("cd My\\ Documents");
    let statement = &parsed.program().statements[0];
    let pipeline = statement.as_pipeline().expect("a pipeline");
    let stage = &pipeline.head.stages[0];
    assert_eq!(stage.arguments.len(), 1, "got {:?}", stage.arguments);
    assert_eq!(stage.arguments[0].as_word(), Some("My\\ Documents"));
}

#[test]
fn should_keep_an_escaped_structural_character_inside_the_word_when_it_is_escaped() {
    for (source, expected) in [
        ("echo a\\|b", "a\\|b"),
        ("echo a\\;b", "a\\;b"),
        ("echo a\\\"b", "a\\\"b"),
        ("echo a\\(b", "a\\(b"),
        ("echo \\*", "\\*"),
        ("echo \\\\", "\\\\"),
    ] {
        let parsed = ono_parser::parse(source);
        let stage = &parsed.program().statements[0]
            .as_pipeline()
            .expect("a pipeline")
            .head
            .stages[0];
        assert_eq!(
            stage
                .arguments
                .first()
                .and_then(ono_parser::Argument::as_word),
            Some(expected),
            "for {source:?}, got {:?}",
            stage.arguments
        );
    }
}

#[test]
fn should_end_the_word_at_a_trailing_backslash_rather_than_reading_past_the_input() {
    // A line ending in a lone backslash is being typed, not broken.
    let parsed = ono_parser::parse("echo a\\");
    assert!(!parsed.has_errors(), "{:?}", parsed.diagnostics());
}

#[test]
fn should_not_treat_a_backslash_as_an_escape_in_expression_mode_when_lexing() {
    // Expression mode has strings for that; a backslash there would collide with the escapes
    // ADR-0009 already gives a quoted string.
    let parsed = ono_parser::parse("where name == \"a\\\\b\"");
    assert!(!parsed.has_errors(), "{:?}", parsed.diagnostics());
}

#[test]
fn should_accept_an_escaped_dollar_sign_inside_an_interpolating_string() {
    // Without it there is no way at all to write a literal `$` in a string that interpolates,
    // and the first shell command anyone writes that mentions `$$` needs one.
    let parsed = ono_parser::parse("echo \"pid is \\$\\$\"");
    assert!(!parsed.has_errors(), "{:?}", parsed.diagnostics());
    let stage = &parsed.program().statements[0]
        .as_pipeline()
        .expect("a pipeline")
        .head
        .stages[0];
    let ono_parser::Argument::Value(ono_parser::Expr::Str(literal)) = &stage.arguments[0] else {
        panic!("expected a string, got {:?}", stage.arguments[0]);
    };
    assert_eq!(literal.literal_text(), Some("pid is $$"));
}
