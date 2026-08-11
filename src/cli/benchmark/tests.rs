use super::report::json_escape;

#[test]
fn json_escape_covers_control_characters() {
    assert_eq!(
        json_escape("quote=\" slash=\\ line=\n tab=\t \u{0001}"),
        "quote=\\\" slash=\\\\ line=\\n tab=\\t \\u0001"
    );
}
