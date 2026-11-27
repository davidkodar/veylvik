use super::*;

const W: u32 = 192;
const H: u32 = 108;
/// A dark picture with soft grains of `radius` pixels every `spacing` pixels, in `colour`.
fn grains(spacing: usize, radius: f32, colour: [f32; 3], brightness: f32) -> Vec<[f32; 4]> {
    let mut px = vec![[0.002, 0.002, 0.002, 1.0]; (W * H) as usize];
    for cy in (spacing / 2..H as usize).step_by(spacing) {
        for cx in (spacing / 2..W as usize).step_by(spacing) {
            let r = (radius * 3.0).ceil() as isize;
            for dy in -r..=r {
                for dx in -r..=r {
                    let (x, y) = (cx as isize + dx, cy as isize + dy);
                    if x < 0 || y < 0 || x >= W as isize || y >= H as isize {
                        continue;
                    }
                    let d2 = (dx * dx + dy * dy) as f32;
                    let v = brightness * (-d2 / (2.0 * radius * radius)).exp();
                    let p = &mut px[y as usize * W as usize + x as usize];
                    for i in 0..3 {
                        p[i] += v * colour[i];
                    }
                }
            }
        }
    }
    px
}
fn pic(px: &[[f32; 4]]) -> Picture<'_> {
    Picture {
        width: W,
        height: H,
        pixels: px,
    }
}
#[test]
fn more_grains_are_counted_as_more() {
    let sparse = grains(16, 0.8, [1.0, 0.1, 0.1], 0.6);
    let dense = grains(8, 0.8, [1.0, 0.1, 0.1], 0.6);
    let (a, b) = (
        look(pic(&sparse), &Area::WHOLE),
        look(pic(&dense), &Area::WHOLE),
    );
    // A quarter the spacing squared: four times the grains.
    assert!(
        (b.grains / a.grains - 4.0).abs() < 0.6,
        "{} {}",
        a.grains,
        b.grains
    );
}
#[test]
fn soft_grains_measure_larger_and_with_less_fine_contrast() {
    let sharp = grains(16, 0.7, [1.0, 1.0, 1.0], 0.6);
    let soft = grains(16, 2.0, [1.0, 1.0, 1.0], 0.6);
    let (a, b) = (
        look(pic(&sharp), &Area::WHOLE),
        look(pic(&soft), &Area::WHOLE),
    );
    assert!(
        b.grain_size > 1.8 * a.grain_size,
        "{} {}",
        a.grain_size,
        b.grain_size
    );
    // Sizes are given as at 1920 wide: this picture is a tenth of that.
    assert!(a.grain_size > 10.0, "{}", a.grain_size);
}
#[test]
fn colour_is_sorted_into_hue_bands() {
    let mut px = grains(12, 1.5, [1.0, 0.05, 0.05], 0.8);
    let teal = grains(12, 1.5, [0.05, 0.8, 0.9], 0.8);
    // The right half teal.
    for y in 0..H as usize {
        for x in W as usize / 2..W as usize {
            px[y * W as usize + x] = teal[y * W as usize + x];
        }
    }
    let l = look(pic(&px), &Area::WHOLE);
    let share = |band: &str| {
        l.hues
            .iter()
            .find(|h| h.band == band)
            .map_or(0.0, |h| h.share)
    };
    assert!((share("red") - 0.5).abs() < 0.1, "{:?}", l.hues);
    assert!((share("teal") - 0.5).abs() < 0.1, "{:?}", l.hues);
    let red = look(
        pic(&px),
        &Area {
            x: 0.0,
            y: 0.0,
            width: 0.5,
            height: 1.0,
        },
    );
    assert!(red.hues.iter().all(|h| h.band == "red"), "{:?}", red.hues);
}
#[test]
fn grains_that_flash_on_and_off_shimmer() {
    let on = grains(12, 1.0, [1.0, 1.0, 1.0], 1.0);
    let off = vec![[0.002, 0.002, 0.002, 1.0]; (W * H) as usize];
    let still = shimmer(pic(&on), pic(&on), &Area::WHOLE).unwrap();
    let flashing = shimmer(pic(&on), pic(&off), &Area::WHOLE).unwrap();
    assert!(still.bright > 0.0 && still.changing == 0.0);
    assert!(flashing.changing > 0.99);
    assert!(
        shimmer(
            pic(&on),
            Picture {
                width: 10,
                height: 10,
                pixels: &off[..100]
            },
            &Area::WHOLE
        )
        .is_err()
    );
}
#[test]
fn a_smaller_copy_keeps_the_light_and_measures_alike() {
    let px = grains(16, 1.6, [1.0, 0.2, 0.1], 0.7);
    let half = resampled(pic(&px), W / 2);
    assert_eq!((half.width, half.height), (W / 2, H / 2));
    let mean = |p: &[[f32; 4]]| p.iter().map(|v| v[0]).sum::<f32>() / p.len() as f32;
    assert!((mean(&half.pixels) - mean(&px)).abs() < 1e-4);
    // Densities and sizes are given as at 1920 wide, so a smaller copy reads about the same.
    let (a, b) = (
        look(pic(&px), &Area::WHOLE),
        look(half.picture(), &Area::WHOLE),
    );
    assert!(
        (b.grains / a.grains - 1.0).abs() < 0.3,
        "{} {}",
        a.grains,
        b.grains
    );
    assert!(
        (b.grain_size / a.grain_size - 1.0).abs() < 0.35,
        "{} {}",
        a.grain_size,
        b.grain_size
    );
}
#[test]
fn advice_names_what_differs_and_is_quiet_when_nothing_does() {
    let reference = look(pic(&grains(16, 0.8, [1.0, 0.05, 0.05], 0.6)), &Area::WHOLE);
    let same = look(pic(&grains(16, 0.8, [1.0, 0.05, 0.05], 0.6)), &Area::WHOLE);
    assert!(
        advise(&reference, &same).is_empty(),
        "{:?}",
        advise(&reference, &same)
    );
    let crowded = look(pic(&grains(7, 0.8, [1.0, 0.05, 0.05], 0.6)), &Area::WHOLE);
    let words = advise(&reference, &crowded).join("\n");
    assert!(words.contains("fewer particles"), "{words}");
    let teal = look(pic(&grains(16, 0.8, [0.05, 0.8, 0.9], 0.6)), &Area::WHOLE);
    let words = advise(&reference, &teal).join("\n");
    assert!(
        words.contains("more red") && words.contains("teal, which the reference does not have"),
        "{words}"
    );
}
#[test]
fn areas_outside_the_picture_are_refused() {
    assert!(Area::WHOLE.check().is_ok());
    assert!(
        Area {
            x: 0.6,
            y: 0.0,
            width: 0.6,
            height: 1.0
        }
        .check()
        .is_err()
    );
    assert!(
        Area {
            x: 0.0,
            y: 0.0,
            width: 0.0,
            height: 1.0
        }
        .check()
        .is_err()
    );
    assert_eq!(Area::grid(3).len(), 9);
}
