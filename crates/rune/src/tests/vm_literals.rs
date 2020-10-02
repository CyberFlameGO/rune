#[test]
fn test_literals() {
    assert_eq!(rune!(String => fn main() { "Hello World" }), "Hello World");
    assert_eq!(
        rune_s!(runestick::Bytes => r#"fn main() { b"Hello World" }"#),
        b"Hello World"[..]
    );

    assert_eq!(rune_s!(i64 => r#"fn main() { 0xff }"#), 0xff);
    assert_eq!(rune_s!(i64 => r#"fn main() { -0xff }"#), -0xff);

    assert_eq!(rune_s!(i64 => r#"fn main() { 0b10010001 }"#), 0b10010001);
    assert_eq!(rune_s!(i64 => r#"fn main() { -0b10010001 }"#), -0b10010001);

    assert_eq!(rune_s!(i64 => r#"fn main() { 0o77 }"#), 0o77);
    assert_eq!(rune_s!(i64 => r#"fn main() { -0o77 }"#), -0o77);

    assert_eq!(rune_s!(u8 => r#"fn main() { b'0' }"#), b'0');
    assert_eq!(rune_s!(u8 => r#"fn main() { b'\xaf' }"#), b'\xaf');

    assert_eq!(rune_s!(char => r#"fn main() { '\x60' }"#), '\x60');
    assert_eq!(rune_s!(char => r#"fn main() { '\u{1F4AF}' }"#), '\u{1F4AF}');
    assert_eq!(rune_s!(char => r#"fn main() { '💯' }"#), '💯');
}
