use super::*;

fn input_text_layout(window: &mut Window, text: &str, font_family: &'static str) -> ShapedLine {
    window.text_system().shape_line(
        text.to_owned().into(),
        px(13.),
        &[TextRun {
            len: text.len(),
            font: font(font_family),
            color: Hsla::default(),
            background_color: None,
            underline: None,
            strikethrough: None,
        }],
        None,
    )
}

pub(crate) fn input_byte_at_x(
    window: &mut Window,
    text: &str,
    x: Pixels,
    origin_x: Pixels,
    font_family: &'static str,
) -> usize {
    let layout = input_text_layout(window, text, font_family);
    layout.closest_index_for_x((x - origin_x).max(px(0.)))
}

pub(crate) fn input_x_for_byte(
    window: &mut Window,
    text: &str,
    byte: usize,
    origin_x: Pixels,
    font_family: &'static str,
) -> Pixels {
    let layout = input_text_layout(window, text, font_family);
    origin_x + layout.x_for_index(byte.min(text.len()))
}

pub(crate) fn byte_at_utf16(text: &str, offset: usize) -> usize {
    let mut utf16 = 0;
    for (byte, character) in text.char_indices() {
        if utf16 + character.len_utf16() > offset {
            return byte;
        }
        utf16 += character.len_utf16();
    }
    text.len()
}

pub(crate) fn utf16_at_byte(text: &str, offset: usize) -> usize {
    text[..offset].encode_utf16().count()
}
