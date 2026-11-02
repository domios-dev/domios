use super::TerminalQueries;

#[test]
fn every_query_split_retains_exactly_one_reply() {
    for (query, expected) in [
        (b"\x1b[c".as_slice(), b"\x1b[?1;2c".as_slice()),
        (b"\x1b[5n", b"\x1b[0n"),
        (b"\x1b[6n", b"\x1b[1;1R"),
        (b"\x1b[005n", b"\x1b[0n"),
        (b"\x1b[006n", b"\x1b[1;1R"),
    ] {
        for split in 1..query.len() {
            let mut parser = TerminalQueries::default();
            assert!(parser.replies(&query[..split]).is_empty());
            assert_eq!(parser.replies(&query[split..]), vec![expected]);
            assert!(parser.replies(b"plain output").is_empty());
        }
    }
}

#[test]
fn repeated_interleaved_queries_each_receive_a_reply_in_stream_order() {
    let mut parser = TerminalQueries::default();
    assert_eq!(
        parser.replies(b"opaque\x1b[6n\x1b[5ntext\x1b[5n\x1b[c\x1b[c\x1b[6n"),
        vec![
            b"\x1b[1;1R".as_slice(),
            b"\x1b[0n",
            b"\x1b[0n",
            b"\x1b[?1;2c",
            b"\x1b[?1;2c",
            b"\x1b[1;1R",
        ]
    );
}

#[test]
fn numeric_variants_use_the_first_primary_parameter() {
    let mut parser = TerminalQueries::default();
    assert_eq!(
        parser.replies(b"\x1b[0005n\x1b[06;1n\x1b[5:2n\x1b[0c"),
        vec![
            b"\x1b[0n".as_slice(),
            b"\x1b[1;1R",
            b"\x1b[0n",
            b"\x1b[?1;2c",
        ]
    );
}

#[test]
fn unrelated_private_and_intermediate_queries_do_not_receive_replies() {
    let mut parser = TerminalQueries::default();
    assert!(parser
        .replies(b"[5n\x1b[4n\x1b[?6n\x1b[>5n\x1b[5$n\x1b[5 n\x1b[1c")
        .is_empty());
}

#[test]
fn control_strings_and_cancelled_sequences_do_not_forge_queries() {
    let mut parser = TerminalQueries::default();
    assert!(parser
        .replies(b"\x1b]0;[5n\x07\x1bP$q[6n\x1b\\\x1b[5\x18n\x1b[6\x1an")
        .is_empty());
    assert_eq!(parser.replies(b"\x1b[5n"), vec![b"\x1b[0n".as_slice()]);
}

#[test]
fn parser_memory_is_constant_for_unterminated_and_overflowing_sequences() {
    let mut parser = TerminalQueries::default();
    assert!(std::mem::size_of_val(&parser) <= 16);
    assert!(parser.replies(b"\x1b]0;").is_empty());
    assert!(parser.replies(&vec![b'x'; 65536]).is_empty());
    assert!(parser.replies(b"\x07\x1b[").is_empty());
    assert!(parser.replies(&vec![b'9'; 65536]).is_empty());
    assert!(parser.replies(b"5n").is_empty());
    assert_eq!(parser.replies(b"\x1b[6n"), vec![b"\x1b[1;1R".as_slice()]);
}

#[test]
fn partial_query_authority_belongs_to_one_reader() {
    let mut first = TerminalQueries::default();
    let mut second = TerminalQueries::default();
    assert!(first.replies(b"\x1b[6").is_empty());
    assert!(second.replies(b"n").is_empty());
    assert_eq!(first.replies(b"n"), vec![b"\x1b[1;1R".as_slice()]);
}
