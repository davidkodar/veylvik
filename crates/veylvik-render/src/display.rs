//! SDR output transforms from scene-linear Rec.709 to display-linear values, applied only at
//! SDR boundaries (viewer, PNG, video); the graph and EXR output stay scene-linear.
use veylvik_document::media::DisplayTransform;

/// ACES 1.0 RRT with the sRGB ODT, as fitted by Stephen Hill: scene-linear Rec.709 in,
/// display-linear Rec.709 out in 0..1. Mid grey 0.18 maps to about 0.11, and highlights roll
/// off smoothly instead of clipping.
pub fn filmic(rgb: [f32; 3]) -> [f32; 3] {
    const INPUT: [[f32; 3]; 3] = [
        [0.597_19, 0.354_58, 0.048_23],
        [0.076_00, 0.908_34, 0.015_66],
        [0.028_40, 0.133_83, 0.837_77],
    ];
    const OUTPUT: [[f32; 3]; 3] = [
        [1.604_75, -0.531_08, -0.073_67],
        [-0.102_08, 1.108_13, -0.006_05],
        [-0.003_27, -0.072_76, 1.076_02],
    ];
    let apply = |m: [[f32; 3]; 3], v: [f32; 3]| m.map(|r| r[0] * v[0] + r[1] * v[1] + r[2] * v[2]);
    let v = apply(INPUT, rgb.map(|c| c.max(0.0)));
    let v = v.map(|x| {
        (x * (x + 0.024_578_6) - 0.000_090_537) / (x * (0.983_729 * x + 0.432_951) + 0.238_081)
    });
    apply(OUTPUT, v).map(|c| c.clamp(0.0, 1.0))
}
/// Display-linear premultiplied pixels for an SDR output; Standard returns them unchanged
/// (the encoder clips).
pub fn apply(transform: DisplayTransform, pixels: &[[f32; 4]]) -> Vec<[f32; 4]> {
    match transform {
        DisplayTransform::Standard => pixels.to_vec(),
        DisplayTransform::Filmic => pixels
            .iter()
            .map(|p| {
                let a = p[3].clamp(0.0, 1.0);
                if a <= 0.0 {
                    return [0.0; 4];
                }
                let [r, g, b] = filmic([p[0] / a, p[1] / a, p[2] / a]);
                [r * a, g * a, b * a, a]
            })
            .collect(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn filmic_keeps_grey_neutral_rolls_highlights_off_and_stays_in_range() {
        let grey = filmic([0.18; 3]);
        assert!((grey[0] - grey[2]).abs() < 0.005, "{grey:?}");
        assert!((0.09..0.13).contains(&grey[1]), "{grey:?}");
        let bright = filmic([4.0; 3]);
        let brighter = filmic([16.0; 3]);
        assert!(bright[1] < 1.0 && brighter[1] > bright[1] && brighter[1] <= 1.0);
        assert_eq!(filmic([0.0; 3]), [0.0; 3].map(|_: f32| filmic([0.0; 3])[0]));
        let out = apply(DisplayTransform::Filmic, &[[0.0; 4], [9.0, 0.1, 0.1, 0.5]]);
        assert_eq!(out[0], [0.0; 4]);
        assert!(out[1].iter().all(|v| (0.0..=1.0).contains(v)) && out[1][3] == 0.5);
        let same = apply(DisplayTransform::Standard, &[[2.0, 0.5, 0.1, 1.0]]);
        assert_eq!(same, [[2.0, 0.5, 0.1, 1.0]]);
    }
}
