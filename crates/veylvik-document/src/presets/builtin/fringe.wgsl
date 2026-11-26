// slider amount 0 20 8
// slider falloff 0.5 4 2

// What a real lens does to colour away from the middle of the frame: red and blue
// focused a little apart, so bright edges toward the corners show a red fringe on one
// side and a blue one on the other. Amount is how many pixels apart they are at the
// frame's corner (in a 1920-wide frame; in proportion in others); falloff is how fast
// it grows from nothing in the middle (2: with the square of the distance).
fn shade(at: vec2<f32>) -> vec4<f32> {
    let middle = size * 0.5;
    let from_middle = (at - middle) / length(middle);
    let shift = from_middle * pow(length(from_middle), u.falloff - 1.0) * u.amount * size.x / 1920.0;
    let red = image(at + shift);
    let green = image(at);
    let blue = image(at - shift);
    return vec4<f32>(red.r, green.g, blue.b, max(green.a, max(red.a, blue.a)));
}
