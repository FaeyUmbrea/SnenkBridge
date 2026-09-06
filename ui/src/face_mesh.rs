//! A 3D face mesh asset and deformation pipeline powered by `gltf` and `glam`.
//!
//! Loads the 3D ARKit face mesh model via the standard `gltf` crate and applies
//! live blendshape morph targets (jaw drop, smiles, eye blinks, brows, mouth shapes)
//! and quaternion-based head rotation (yaw, pitch, roll) before passing view-space
//! geometry to the GPU renderer.

use std::sync::OnceLock;

use glam::{Quat, Vec3};
use slint::Image;

use crate::renderer::{render_view_space_vertices, RENDER_HEIGHT, RENDER_WIDTH};

const GLB_BYTES: &[u8] = include_bytes!("../resources/ARKitBlendshapeFaceMesh.glb");

/// A sparse blendshape morph target containing vertex delta offsets.
#[derive(Clone, Debug)]
pub struct MorphTarget {
    pub name: String,
    pub sparse_indices: Vec<u16>,
    pub sparse_deltas: Vec<[f32; 3]>,
}

/// In-memory parsed GLB face mesh asset with base geometry and morph targets.
pub struct GlbMesh {
    pub base_positions: Vec<[f32; 3]>,
    pub triangles: Vec<[u16; 3]>,
    pub targets: Vec<MorphTarget>,
    pub center: [f32; 3],
}

static GLB_MESH: OnceLock<GlbMesh> = OnceLock::new();

/// Retrieve the cached, static [`GlbMesh`] instance parsed on first access.
pub fn get_glb_mesh() -> &'static GlbMesh {
    GLB_MESH.get_or_init(|| GlbMesh::parse(GLB_BYTES))
}

impl GlbMesh {
    /// Parse a binary GLTF (.glb) buffer into vertex positions, triangle indices,
    /// and sparse morph target deltas using the standard `gltf` crate and `glam` quaternions.
    pub fn parse(bytes: &[u8]) -> Self {
        let (doc, buffers, _) = gltf::import_slice(bytes).expect("Failed to import GLB buffer");
        let mesh = doc.meshes().next().expect("Mesh missing from GLB asset");

        // Extract root node orientation quaternion from GLTF scene node if present.
        let node_rotation = doc
            .scenes()
            .find_map(|scene| {
                scene.nodes().find_map(|node| {
                    if node.mesh().is_some() {
                        let (_, rotation, _) = node.transform().decomposed();
                        Some(Quat::from_array(rotation))
                    } else {
                        None
                    }
                })
            })
            .unwrap_or(Quat::IDENTITY);

        // Parse blendshape target names from mesh extras if present
        let target_names: Vec<String> = mesh
            .extras()
            .as_ref()
            .and_then(|extras| serde_json::from_str::<serde_json::Value>(extras.get()).ok())
            .and_then(|val| val.get("targetNames").cloned())
            .and_then(|val| serde_json::from_value::<Vec<String>>(val).ok())
            .unwrap_or_default();

        let primitive = mesh
            .primitives()
            .next()
            .expect("Primitive missing from GLB asset");
        let reader = primitive.reader(|buffer| Some(&buffers[buffer.index()]));

        // Parse base vertex positions, applying the node orientation quaternion.
        let base_positions: Vec<[f32; 3]> = reader
            .read_positions()
            .expect("Base positions missing from GLB asset")
            .map(|pos| node_rotation.mul_vec3(Vec3::from_array(pos)).to_array())
            .collect();

        // Parse triangle indices.
        let raw_indices: Vec<u32> = reader
            .read_indices()
            .expect("Indices missing from GLB asset")
            .into_u32()
            .collect();

        let mut triangles = Vec::with_capacity(raw_indices.len() / 3);
        for chunk in raw_indices.chunks_exact(3) {
            triangles.push([chunk[0] as u16, chunk[1] as u16, chunk[2] as u16]);
        }

        // Parse morph target deltas transformed consistently by node quaternion.
        let mut targets = Vec::new();
        for (target_idx, (pos_iter, _, _)) in reader.read_morph_targets().enumerate() {
            let name = target_names
                .get(target_idx)
                .cloned()
                .unwrap_or_else(|| format!("target_{target_idx}"));

            let mut sparse_indices = Vec::new();
            let mut sparse_deltas = Vec::new();

            if let Some(pos_iter) = pos_iter {
                for (v_idx, delta) in pos_iter.enumerate() {
                    let rotated_delta = node_rotation.mul_vec3(Vec3::from_array(delta)).to_array();
                    let delta_vec = Vec3::from_array(delta);
                    if delta_vec.length_squared() > 1e-10 {
                        sparse_indices.push(v_idx as u16);
                        sparse_deltas.push(rotated_delta);
                    }
                }
            }

            targets.push(MorphTarget {
                name,
                sparse_indices,
                sparse_deltas,
            });
        }

        // Compute bounding center for head rotation pivot alignment.
        let center = if base_positions.is_empty() {
            [0.0, 0.0, 0.0]
        } else {
            let sum = base_positions
                .iter()
                .fold(Vec3::ZERO, |acc, p| acc + Vec3::from_array(*p));
            (sum / (base_positions.len() as f32)).to_array()
        };

        Self {
            base_positions,
            triangles,
            targets,
            center,
        }
    }
}

/// Project and render the 3D GLB mesh using live tracking values (blendshapes + head rotation).
pub fn compute_input_preview(get_tracking_value: impl Fn(&str) -> Option<f32>) -> Image {
    let mesh = get_glb_mesh();

    // 1. Initialize deformed vertices from base positions.
    let mut vertices = mesh.base_positions.clone();

    // 2. Accumulate active morph target deltas (blendshapes).
    for target in &mesh.targets {
        let weight = get_tracking_value(&target.name)
            .or_else(|| get_tracking_value(&target.name.to_lowercase()))
            .unwrap_or(0.0)
            .clamp(0.0, 1.0);

        if weight > 0.0001 {
            for (&vertex_idx, &delta) in target
                .sparse_indices
                .iter()
                .zip(target.sparse_deltas.iter())
            {
                let vertex = &mut vertices[vertex_idx as usize];
                let deformed = Vec3::from_array(*vertex) + Vec3::from_array(delta) * weight;
                *vertex = deformed.to_array();
            }
        }
    }

    // 3. Compute head rotation angles (yaw, pitch, roll) from tracking inputs.
    let normalize_rotation_angle = |degrees: f32| (degrees / 30.0).clamp(-1.0, 1.0) * 0.95;
    let yaw = get_tracking_value("FaceAngleY")
        .or_else(|| get_tracking_value("HeadRotY"))
        .or_else(|| get_tracking_value("headYaw"))
        .map_or(0.0, normalize_rotation_angle);
    let pitch = get_tracking_value("FaceAngleX")
        .or_else(|| get_tracking_value("HeadRotX"))
        .or_else(|| get_tracking_value("headPitch"))
        .map_or(0.0, normalize_rotation_angle);
    let roll = get_tracking_value("FaceAngleZ")
        .or_else(|| get_tracking_value("HeadRotZ"))
        .or_else(|| get_tracking_value("headRoll"))
        .map_or(0.0, normalize_rotation_angle);

    let head_rotation =
        Quat::from_rotation_x(pitch) * Quat::from_rotation_y(yaw) * Quat::from_rotation_z(roll);

    let center_vec = Vec3::from_array(mesh.center);

    // 4. Transform vertices into view space using quaternions.
    let mut view_space_vertices: Vec<[f32; 3]> = Vec::with_capacity(vertices.len());

    for &vertex in &vertices {
        let centered = Vec3::from_array(vertex) - center_vec;
        let rotated = head_rotation.mul_vec3(centered);
        view_space_vertices.push(rotated.to_array());
    }

    // 5. Render 3D model with wgpu offscreen GPU renderer.
    render_view_space_vertices(&view_space_vertices)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn glb_mesh_loads_and_parses_with_gltf_crate() {
        let mesh = get_glb_mesh();
        assert_eq!(mesh.base_positions.len(), 6912);
        assert_eq!(mesh.triangles.len(), 2304);
        assert_eq!(mesh.targets.len(), 51);
        assert!(mesh.center[1].abs() < 0.1);
    }

    #[test]
    fn quaternion_rotation_matches_euler_transforms() {
        let pitch = 0.2_f32;
        let yaw = 0.3_f32;
        let roll = 0.1_f32;

        let head_rotation =
            Quat::from_rotation_x(pitch) * Quat::from_rotation_y(yaw) * Quat::from_rotation_z(roll);

        let test_pt = Vec3::new(0.05, 0.08, -0.02);
        let result = head_rotation.mul_vec3(test_pt);

        let qx = Quat::from_rotation_x(pitch);
        let qy = Quat::from_rotation_y(yaw);
        let qz = Quat::from_rotation_z(roll);

        let expected = qx.mul_vec3(qy.mul_vec3(qz.mul_vec3(test_pt)));

        assert!((result.x - expected.x).abs() < 1e-6);
        assert!((result.y - expected.y).abs() < 1e-6);
        assert!((result.z - expected.z).abs() < 1e-6);
    }

    #[test]
    fn compute_input_preview_renders_valid_image() {
        let image = compute_input_preview(|name| match name {
            "jawOpen" => Some(0.8),
            "FaceAngleY" => Some(15.0),
            _ => None,
        });

        assert_eq!(image.size().width, RENDER_WIDTH);
        assert_eq!(image.size().height, RENDER_HEIGHT);
    }
}
