//! Human figures for 3D scenes: a skeleton posed by a movement (walking, for now) over time, and a smooth body round it as a mesh. Lengths are shares of the figure's height until the mesh, which is in pixels; x is the way it walks, y down (the ground at 0), z towards its near side.
mod body;
mod gait;

pub use body::{distance, mesh};
pub use gait::{STRIDE, walk};

/// Where every joint of the figure is, in heights. Index 0 is the near side (+z), 1 the far.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Pose {
    /// The middle of the pelvis, between the hip joints.
    pub pelvis: [f32; 3],
    /// Which way the trunk points (unit, up the spine).
    pub up: [f32; 3],
    /// Where the upper body rides (it follows the pelvis a moment later).
    pub chest: [f32; 3],
    pub hip: [[f32; 3]; 2],
    pub knee: [[f32; 3]; 2],
    pub ankle: [[f32; 3]; 2],
    pub heel: [[f32; 3]; 2],
    pub ball: [[f32; 3]; 2],
    pub toe: [[f32; 3]; 2],
    pub shoulder: [[f32; 3]; 2],
    pub elbow: [[f32; 3]; 2],
    pub wrist: [[f32; 3]; 2],
    /// The tip of the hand.
    pub hand: [[f32; 3]; 2],
    /// The base and top of the neck.
    pub neck: [f32; 3],
    pub head: [f32; 3],
    /// The head's nod (degrees, chin down positive).
    pub head_pitch: f32,
}
impl Pose {
    /// Every joint, for bounds.
    pub fn points(&self) -> Vec<[f32; 3]> {
        let mut out = vec![
            self.pelvis,
            self.chest,
            self.neck,
            self.head,
            [self.head[0], self.head[1] - 0.13, self.head[2]],
        ];
        for side in 0..2 {
            out.extend([
                self.hip[side],
                self.knee[side],
                self.ankle[side],
                self.heel[side],
                self.ball[side],
                self.toe[side],
                self.shoulder[side],
                self.elbow[side],
                self.wrist[side],
                self.hand[side],
            ]);
        }
        out
    }
}
