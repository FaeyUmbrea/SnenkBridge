use glam::Vec3;

use crate::face_mesh::get_glb_mesh;

use super::{render_view_space_vertices, RENDER_HEIGHT, RENDER_WIDTH};

#[test]
fn wgpu_renderer_renders_mesh_without_panics() {
    let mesh = get_glb_mesh();
    let image = render_view_space_vertices(&mesh.base_positions);
    assert_eq!(image.size().width, RENDER_WIDTH);
    assert_eq!(image.size().height, RENDER_HEIGHT);
}

#[test]
fn vertex_smooth_normals_are_computed() {
    let mesh = get_glb_mesh();
    let mut normal_accum = vec![Vec3::ZERO; mesh.base_positions.len()];
    for &[i0, i1, i2] in &mesh.triangles {
        let p0 = Vec3::from_array(mesh.base_positions[i0 as usize]);
        let p1 = Vec3::from_array(mesh.base_positions[i1 as usize]);
        let p2 = Vec3::from_array(mesh.base_positions[i2 as usize]);
        let face_normal = (p1 - p0).cross(p2 - p0);
        normal_accum[i0 as usize] += face_normal;
        normal_accum[i1 as usize] += face_normal;
        normal_accum[i2 as usize] += face_normal;
    }

    let non_zero_count = normal_accum.iter().filter(|n| n.length() > 1e-8).count();
    assert!(non_zero_count > 0);
    assert_eq!(non_zero_count, mesh.base_positions.len());
}
