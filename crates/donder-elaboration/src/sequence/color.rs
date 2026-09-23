use donder_language::values::Color;

pub(super) fn compose_max(target: &mut Color, source: Color) {
    target.red = target.red.max(source.red);
    target.green = target.green.max(source.green);
    target.blue = target.blue.max(source.blue);
}

pub(crate) fn black() -> Color {
    Color {
        red: 0,
        green: 0,
        blue: 0,
    }
}
