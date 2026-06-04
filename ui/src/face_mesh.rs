//! A small synthetic 3D face mesh for the preview point cloud.
//!
//! There is no real landmark data available, so we build a stylised face out of
//! rings (outline, eyes, brows, nose, mouth), deform it with the live output
//! parameters, rotate it by the head angles, and project it to 2D. The result
//! is a set of screen-space points plus a wireframe path (line segments), both
//! handed to Slint for rendering.

use std::fmt::Write as _;

/// Normalised face parameters driving the mesh. Angles are in radians; the
/// openness/smile/brow values are unit-ish (0..1 or -1..1).
#[derive(Default, Clone, Copy)]
pub struct FaceParams {
    pub yaw: f32,
    pub pitch: f32,
    pub roll: f32,
    pub mouth_open: f32,
    pub mouth_smile: f32,
    pub mouth_x: f32,
    pub tongue_out: f32,
    pub eye_open_l: f32,
    pub eye_open_r: f32,
    pub brow_l: f32,
    pub brow_r: f32,
}

/// Projected mesh ready for rendering: points are `(x, y, depth)` with x/y in
/// 0..1 (screen space within a square) and depth in -1..1 (back..front).
pub struct ProjectedMesh {
    pub points: Vec<(f32, f32, f32)>,
    pub wireframe: String,
}

struct Mesh {
    points: Vec<[f32; 3]>,
    edges: Vec<(usize, usize)>,
}

impl Mesh {
    fn new() -> Self {
        Self {
            points: Vec::new(),
            edges: Vec::new(),
        }
    }

    /// Add a ring (or arc) of points and chain them with edges. `sweep` is the
    /// angle range in radians; `closed` connects the last point back to the
    /// first.
    fn ring(&mut self, center: [f32; 3], radii: [f32; 2], sweep: [f32; 2], n: usize, closed: bool) {
        let [cx, cy, cz] = center;
        let [rx, ry] = radii;
        let [t0, t1] = sweep;
        let start = self.points.len();
        for i in 0..n {
            let t = t0 + (t1 - t0) * (i as f32) / ((n - 1).max(1) as f32);
            self.points.push([cx + rx * t.cos(), cy + ry * t.sin(), cz]);
        }
        for i in 0..n - 1 {
            self.edges.push((start + i, start + i + 1));
        }
        if closed {
            self.edges.push((start + n - 1, start));
        }
    }
}

const TAU: f32 = std::f32::consts::TAU;

fn base_mesh(p: &FaceParams) -> Mesh {
    let mut m = Mesh::new();

    // Face outline — pushed back in z so rotation reads as a head.
    m.ring([0.0, 0.0, -0.18], [0.62, 0.82], [0.0, TAU], 28, true);

    // Eyes. Vertical radius collapses as the eye closes.
    let eye_ry = |open: f32| 0.03 + open.clamp(0.0, 1.0) * 0.08;
    m.ring(
        [-0.28, -0.14, 0.10],
        [0.16, eye_ry(p.eye_open_l)],
        [0.0, TAU],
        12,
        true,
    );
    m.ring(
        [0.28, -0.14, 0.10],
        [0.16, eye_ry(p.eye_open_r)],
        [0.0, TAU],
        12,
        true,
    );

    // Brows — top arcs that rise with the brow value.
    let mut brow = |x: f32, raise: f32| {
        m.ring(
            [x, -0.34 - raise.clamp(-1.0, 1.0) * 0.07, 0.16],
            [0.18, 0.10],
            [TAU * 0.55, TAU * 0.95],
            6,
            false,
        );
    };
    brow(-0.28, p.brow_l);
    brow(0.28, p.brow_r);

    // Nose — bridge to tip, tip pushed forward in z.
    let nose_start = m.points.len();
    m.points.push([0.0, -0.10, 0.22]);
    m.points.push([0.0, 0.02, 0.34]);
    m.points.push([0.0, 0.14, 0.46]);
    m.points.push([-0.08, 0.20, 0.30]);
    m.points.push([0.08, 0.20, 0.30]);
    m.edges.push((nose_start, nose_start + 1));
    m.edges.push((nose_start + 1, nose_start + 2));
    m.edges.push((nose_start + 2, nose_start + 3));
    m.edges.push((nose_start + 2, nose_start + 4));

    // Mouth — ring that opens vertically; corners lift with smile.
    let mouth_start = m.points.len();
    let mrx = 0.22 + p.mouth_smile.clamp(0.0, 1.0) * 0.04;
    let mry = 0.04 + p.mouth_open.clamp(0.0, 1.0) * 0.14;
    let n = 14;
    for i in 0..n {
        let t = TAU * (i as f32) / (n as f32);
        let x = mrx * t.cos();
        // Lift corners (|cos| near 1) with smile.
        let corner = t.cos().abs();
        let y = 0.42 + mry * t.sin() - p.mouth_smile.clamp(-1.0, 1.0) * 0.06 * corner;
        m.points.push([x + p.mouth_x * 0.08, y, 0.20]);
    }
    for i in 0..n {
        m.edges.push((mouth_start + i, mouth_start + (i + 1) % n));
    }

    // Tongue — a small triangle below the mouth that extends with TongueOut.
    // Always present (constant topology); near-flat and hidden when retracted.
    let t = p.tongue_out.clamp(0.0, 1.0);
    let mx = p.mouth_x * 0.08;
    let half_w = 0.03 + t * 0.05;
    let base_y = 0.42 + mry;
    let tip_y = base_y + 0.02 + t * 0.20;
    let tz = 0.22 + t * 0.08;
    let tongue_start = m.points.len();
    m.points.push([mx - half_w, base_y, tz]);
    m.points.push([mx + half_w, base_y, tz]);
    m.points.push([mx, tip_y, tz]);
    m.edges.push((tongue_start, tongue_start + 1));
    m.edges.push((tongue_start + 1, tongue_start + 2));
    m.edges.push((tongue_start + 2, tongue_start));

    m
}

/// Rotate, project, and screen-map the mesh.
#[must_use]
pub fn compute(p: &FaceParams) -> ProjectedMesh {
    let mesh = base_mesh(p);

    let (sr, cr) = p.roll.sin_cos();
    let (sy, cy) = p.yaw.sin_cos();
    let (sp, cp) = p.pitch.sin_cos();

    let project = |v: &[f32; 3]| -> (f32, f32, f32) {
        let [mut x, mut y, mut z] = *v;
        // Roll (about Z)
        let (rx, ry) = (x * cr - y * sr, x * sr + y * cr);
        x = rx;
        y = ry;
        // Yaw (about Y)
        let (yx, yz) = (x * cy + z * sy, -x * sy + z * cy);
        x = yx;
        z = yz;
        // Pitch (about X)
        let (py, pz) = (y * cp - z * sp, y * sp + z * cp);
        y = py;
        z = pz;
        // Weak perspective: nearer points spread out slightly.
        let scale = 1.0 / (1.0 - z * 0.22);
        (x * scale, y * scale, z)
    };

    let projected: Vec<(f32, f32, f32)> = mesh.points.iter().map(project).collect();

    // Screen-map to 0..1 within a centred square.
    const FIT: f32 = 0.46;
    let to_screen = |(x, y): (f32, f32)| (0.5 + x * FIT, 0.5 + y * FIT);

    let points: Vec<(f32, f32, f32)> = projected
        .iter()
        .map(|&(x, y, z)| {
            let (sx, sy) = to_screen((x, y));
            (sx, sy, (z * 1.6).clamp(-1.0, 1.0))
        })
        .collect();

    let mut wireframe = String::with_capacity(mesh.edges.len() * 24);
    for &(a, b) in &mesh.edges {
        let (ax, ay) = to_screen((projected[a].0, projected[a].1));
        let (bx, by) = to_screen((projected[b].0, projected[b].1));
        let _ = write!(wireframe, "M {ax:.4} {ay:.4} L {bx:.4} {by:.4} ");
    }

    ProjectedMesh { points, wireframe }
}
