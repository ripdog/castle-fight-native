#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Wc3Color {
    pub(crate) alpha: u8,
    pub(crate) red: u8,
    pub(crate) green: u8,
    pub(crate) blue: u8,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Wc3TextRun {
    pub(crate) text: String,
    pub(crate) color: Option<Wc3Color>,
}

/// Parses the classic Warcraft III text escapes used by object-data tooltips and UI strings.
///
/// Warcraft III's classic text markup uses `|cAARRGGBB` to start a color, `|r` to reset the
/// color, `|n` to insert a line break, and `||` to escape the pipe character itself. The letter
/// tags are case-insensitive. Anything that is not a complete, valid escape is kept literally
/// rather than being discarded.
pub(crate) fn parse_wc3_text(source: &str) -> Vec<Wc3TextRun> {
    let bytes = source.as_bytes();
    let mut runs = Vec::new();
    let mut buffer = String::new();
    let mut color = None;
    let mut index = 0;

    while index < bytes.len() {
        if bytes[index] != b'|' {
            let ch = source[index..]
                .chars()
                .next()
                .expect("index must remain on a UTF-8 character boundary");
            buffer.push(ch);
            index += ch.len_utf8();
            continue;
        }

        let Some(tag) = bytes.get(index + 1).copied() else {
            buffer.push('|');
            break;
        };
        match tag.to_ascii_lowercase() {
            b'|' => {
                buffer.push('|');
                index += 2;
            }
            b'n' => {
                buffer.push('\n');
                index += 2;
            }
            b'r' => {
                flush_run(&mut runs, &mut buffer, color);
                color = None;
                index += 2;
            }
            b'c' => {
                let end = index + 10;
                if end <= bytes.len() && bytes[index + 2..end].iter().all(u8::is_ascii_hexdigit) {
                    flush_run(&mut runs, &mut buffer, color);
                    color = Some(Wc3Color {
                        alpha: hex_byte(bytes[index + 2], bytes[index + 3]),
                        red: hex_byte(bytes[index + 4], bytes[index + 5]),
                        green: hex_byte(bytes[index + 6], bytes[index + 7]),
                        blue: hex_byte(bytes[index + 8], bytes[index + 9]),
                    });
                    index = end;
                } else {
                    // Preserve malformed color tags byte-for-byte. Consuming only the pipe lets
                    // the ordinary UTF-8 path below retain every following character.
                    buffer.push('|');
                    index += 1;
                }
            }
            _ => {
                // Warcraft treats unrecognised pipe sequences as ordinary text. Do the same so
                // future map text cannot silently disappear if Blizzard adds unrelated markup.
                buffer.push('|');
                index += 1;
            }
        }
    }

    flush_run(&mut runs, &mut buffer, color);
    runs
}

fn flush_run(runs: &mut Vec<Wc3TextRun>, buffer: &mut String, color: Option<Wc3Color>) {
    if buffer.is_empty() {
        return;
    }
    let text = std::mem::take(buffer);
    if let Some(last) = runs.last_mut()
        && last.color == color
    {
        last.text.push_str(&text);
        return;
    }
    runs.push(Wc3TextRun { text, color });
}

fn hex_byte(high: u8, low: u8) -> u8 {
    (hex_nibble(high) << 4) | hex_nibble(low)
}

fn hex_nibble(value: u8) -> u8 {
    match value {
        b'0'..=b'9' => value - b'0',
        b'a'..=b'f' => value - b'a' + 10,
        b'A'..=b'F' => value - b'A' + 10,
        _ => unreachable!("caller validates hexadecimal digits"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_every_classic_wc3_escape() {
        let runs = parse_wc3_text("plain||pipe|n|c8012aBcDcolored|rnormal");
        assert_eq!(
            runs,
            vec![
                Wc3TextRun {
                    text: "plain|pipe\n".into(),
                    color: None,
                },
                Wc3TextRun {
                    text: "colored".into(),
                    color: Some(Wc3Color {
                        alpha: 0x80,
                        red: 0x12,
                        green: 0xab,
                        blue: 0xcd,
                    }),
                },
                Wc3TextRun {
                    text: "normal".into(),
                    color: None,
                },
            ]
        );
    }

    #[test]
    fn escape_letters_are_case_insensitive() {
        let runs = parse_wc3_text("|CFF010203x|R|Ny");
        assert_eq!(runs.len(), 2);
        assert_eq!(runs[0].text, "x");
        assert_eq!(
            runs[0].color,
            Some(Wc3Color {
                alpha: 0xff,
                red: 1,
                green: 2,
                blue: 3,
            })
        );
        assert_eq!(runs[1].text, "\ny");
        assert_eq!(runs[1].color, None);
    }

    #[test]
    fn malformed_and_unknown_sequences_are_preserved_literally() {
        let source = "a|qfoo|c123oops|z|";
        let runs = parse_wc3_text(source);
        assert_eq!(
            runs,
            vec![Wc3TextRun {
                text: source.into(),
                color: None
            }]
        );
    }

    #[test]
    fn utf8_text_around_escapes_keeps_character_boundaries() {
        let runs = parse_wc3_text("Frost • |cffffcc00Ångström|r");
        assert_eq!(runs[0].text, "Frost • ");
        assert_eq!(runs[1].text, "Ångström");
    }

    #[test]
    fn extracted_building_tooltips_contain_only_supported_wc3_escapes() {
        let rows = include_str!("../../../docs/original_map/extracted/resolved/buildings.tsv");
        for line in rows.lines().skip(1) {
            let columns = line.split('\t').collect::<Vec<_>>();
            for tooltip in [columns[4], columns[5]] {
                let rendered = parse_wc3_text(tooltip)
                    .into_iter()
                    .map(|run| run.text)
                    .collect::<String>();
                assert!(
                    !rendered.contains("|c"),
                    "unparsed color escape in {tooltip:?}"
                );
                assert!(
                    !rendered.contains("|C"),
                    "unparsed color escape in {tooltip:?}"
                );
                assert!(
                    !rendered.contains("|r"),
                    "unparsed reset escape in {tooltip:?}"
                );
                assert!(
                    !rendered.contains("|R"),
                    "unparsed reset escape in {tooltip:?}"
                );
                assert!(
                    !rendered.contains("|n"),
                    "unparsed newline escape in {tooltip:?}"
                );
                assert!(
                    !rendered.contains("|N"),
                    "unparsed newline escape in {tooltip:?}"
                );
            }
        }
    }
}
