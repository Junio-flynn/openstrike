//! Desktop weapon presentation: the procedural rifle uploaded as a wgpu
//! model asset. All weapon *behavior* lives in `openstrike_core::weapon`.

use std::sync::Arc;

use pocket3d::model::{ModelAsset, ModelVertex};
use pocket3d::prelude::*;
use pocket3d::renderer::Renderer;

use openstrike_core::weapon::rifle_boxes;
pub use openstrike_core::weapon::{GUN_COLORS, WeaponConfig};

fn add_box(
    verts: &mut Vec<ModelVertex>,
    indices: &mut Vec<u32>,
    min: Vec3,
    max: Vec3,
    color: usize,
) {
    let u = (color as f32 + 0.5) / GUN_COLORS.len() as f32;
    let uv = [u, 0.5];
    let corners = |x: f32, y: f32, z: f32| {
        Vec3::new(
            if x > 0.0 { max.x } else { min.x },
            if y > 0.0 { max.y } else { min.y },
            if z > 0.0 { max.z } else { min.z },
        )
    };
    // (normal, four corners CCW seen from outside)
    let faces: [(Vec3, [Vec3; 4]); 6] = [
        (
            Vec3::X,
            [
                corners(1.0, -1.0, 1.0),
                corners(1.0, -1.0, -1.0),
                corners(1.0, 1.0, -1.0),
                corners(1.0, 1.0, 1.0),
            ],
        ),
        (
            -Vec3::X,
            [
                corners(-1.0, -1.0, -1.0),
                corners(-1.0, -1.0, 1.0),
                corners(-1.0, 1.0, 1.0),
                corners(-1.0, 1.0, -1.0),
            ],
        ),
        (
            Vec3::Y,
            [
                corners(-1.0, 1.0, 1.0),
                corners(1.0, 1.0, 1.0),
                corners(1.0, 1.0, -1.0),
                corners(-1.0, 1.0, -1.0),
            ],
        ),
        (
            -Vec3::Y,
            [
                corners(-1.0, -1.0, -1.0),
                corners(1.0, -1.0, -1.0),
                corners(1.0, -1.0, 1.0),
                corners(-1.0, -1.0, 1.0),
            ],
        ),
        (
            Vec3::Z,
            [
                corners(-1.0, -1.0, 1.0),
                corners(1.0, -1.0, 1.0),
                corners(1.0, 1.0, 1.0),
                corners(-1.0, 1.0, 1.0),
            ],
        ),
        (
            -Vec3::Z,
            [
                corners(1.0, -1.0, -1.0),
                corners(-1.0, -1.0, -1.0),
                corners(-1.0, 1.0, -1.0),
                corners(1.0, 1.0, -1.0),
            ],
        ),
    ];
    for (n, quad) in faces {
        let base = verts.len() as u32;
        for p in quad {
            verts.push(ModelVertex {
                pos: p.to_array(),
                normal: n.to_array(),
                uv,
                joints: [0; 4],
                weights: [1.0, 0.0, 0.0, 0.0],
            });
        }
        indices.extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
    }
}

pub fn build_rifle(gpu: &Gpu, renderer: &Renderer) -> Arc<ModelAsset> {
    let mut v = Vec::new();
    let mut i = Vec::new();
    for b in rifle_boxes() {
        add_box(&mut v, &mut i, b.min, b.max, b.color);
    }

    let mut px = Vec::new();
    for c in GUN_COLORS {
        px.extend_from_slice(&c);
    }
    ModelAsset::from_geometry(
        gpu,
        &renderer.model_material_layout,
        &renderer.samplers,
        "rifle",
        &v,
        &i,
        Some((GUN_COLORS.len() as u32, 1, &px)),
    )
}

pub fn build_bullet_hole(gpu: &Gpu, renderer: &Renderer) -> Arc<ModelAsset> {
    let segments = 12u32;
    let mut vertices = Vec::with_capacity(segments as usize + 1);
    vertices.push(ModelVertex {
        pos: [0.0, 0.0, 0.0],
        normal: [0.0, 0.0, 1.0],
        uv: [0.5, 0.5],
        joints: [0; 4],
        weights: [1.0, 0.0, 0.0, 0.0],
    });
    for index in 0..segments {
        let angle = index as f32 / segments as f32 * core::f32::consts::TAU;
        let radius = if index % 3 == 0 { 0.82 } else { 1.0 };
        let (sin, cos) = angle.sin_cos();
        vertices.push(ModelVertex {
            pos: [cos * radius, sin * radius, 0.0],
            normal: [0.0, 0.0, 1.0],
            uv: [0.5 + cos * 0.5, 0.5 + sin * 0.5],
            joints: [0; 4],
            weights: [1.0, 0.0, 0.0, 0.0],
        });
    }
    let mut indices = Vec::with_capacity(segments as usize * 3);
    for index in 0..segments {
        indices.extend_from_slice(&[0, index + 1, (index + 1) % segments + 1]);
    }
    ModelAsset::from_geometry(
        gpu,
        &renderer.model_material_layout,
        &renderer.samplers,
        "bullet hole",
        &vertices,
        &indices,
        Some((1, 1, &[24, 19, 16, 255])),
    )
}
