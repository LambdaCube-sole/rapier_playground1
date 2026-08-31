use std::io::{BufReader, Cursor};
use wgpu::util::DeviceExt;
use crate::{model, texture};

#[cfg(target_arch="wasm32")]
fn format_url(file_name: &str) -> reqwest::Url {
    let href = web_sys::window().unwrap().location().href().unwrap();
    let dir = href.rsplit_once('/').map(|(d, _)| d).unwrap_or(&href);
    reqwest::Url::parse(&format!("{dir}/res/{file_name}")).unwrap()
}

pub async fn load_string(file_name: &str) -> anyhow::Result<String> {
    #[cfg(target_arch="wasm32")]
    let txt = {
        let url = format_url(file_name);
        reqwest::get(url).await?.text().await?
    };
    #[cfg(not(target_arch="wasm32"))]
    let txt = {
        let path = std::path::Path::new(env!("OUT_DIR"))
            .join("res")
            .join(file_name);
        std::fs::read_to_string(path)?
    };

    Ok(txt)
}

pub async fn load_binary(file_name: &str) -> anyhow::Result<Vec<u8>> {
    #[cfg(target_arch="wasm32")]
    let data = {
        let url = format_url(file_name);
        reqwest::get(url).await?.bytes().await?.to_vec()
    };
    #[cfg(not(target_arch="wasm32"))]
    let data = {
        let path = std::path::Path::new(env!("OUT_DIR"))
            .join("res")
            .join(file_name);
        std::fs::read(path)?
    };

    Ok(data)

}

pub async fn load_texture(
    file_name: &str,
    device: &wgpu::Device,
    queue: &wgpu::Queue,
) -> anyhow::Result<texture::Texture> {
    let data =load_binary(file_name).await?;
    texture::Texture::from_bytes(device, queue, &data, file_name)
}

pub async fn load_model(
    file_name: &str,
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    layout: &wgpu::BindGroupLayout,
) -> anyhow::Result<model::Model> {
    let lower = file_name.to_ascii_lowercase();
    if lower.ends_with(".glb") || lower.ends_with(".gltf") {
        return load_gltf_model(file_name, device, queue, layout).await;
    }

    let obj_text = load_string(file_name).await?;
    let obj_cursor = Cursor::new(obj_text);
    let mut obj_reader = BufReader::new(obj_cursor);

    let (models, obj_materials) = tobj::load_obj_buf_async(
        &mut obj_reader,
        &tobj::LoadOptions {
            triangulate: true,
            single_index: true,
            ..Default::default()
        },
        |p| async move {
            let mat_text = load_string(&p).await.unwrap();
            tobj::load_mtl_buf(&mut BufReader::new(Cursor::new(mat_text)))
        },
    )
    .await?;

    let mut materials = Vec::new();
    for m in obj_materials? {
        let diffuse_texture = load_texture(&m.diffuse_texture, device, queue).await?;
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&diffuse_texture.view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(&diffuse_texture.sampler),
                },
            ],
            label: None,
        });
        materials.push(model::Material {
            name: m.name,
            diffuse_texture,
            bind_group,
        })
    }
    let meshes = models
        .into_iter()
        .map(|m| {
            let vertices = (0..m.mesh.positions.len() / 3 )
            .map(|i| {
                if m.mesh.normals.is_empty() {
                    model::ModelVertex {
                        position: [
                            m.mesh.positions[i * 3],
                            m.mesh.positions[i * 3 + 1],
                            m.mesh.positions[i * 3 + 2],
                        ],
                        tex_coords: [
                            m.mesh.texcoords[i * 2],
                            1.0 - m.mesh.texcoords[i * 2 + 1],
                        ],
                        normal: [0.0, 0.0, 0.0],
                    }
                } else {
                    model::ModelVertex {
                        position: [
                                m.mesh.positions[i * 3],
                                m.mesh.positions[i * 3 + 1],
                                m.mesh.positions[i * 3 + 2],
                            ],
                            tex_coords: [m.mesh.texcoords[i * 2], 1.0 - m.mesh.texcoords[i * 2 + 1]],
                            normal: [
                                m.mesh.normals[i * 3],
                                m.mesh.normals[i * 3 + 1],
                                m.mesh.normals[i * 3 + 2],
                            ],
                    }
                }
            })
            .collect::<Vec<_>>();

            let vertex_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some(&format!("{:?} Vertex Buffer", file_name)),
                contents: bytemuck::cast_slice(&vertices),
                usage: wgpu::BufferUsages::VERTEX,
            });

            let index_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some(&format!("{:?} Index Buffer", file_name)),
                contents: bytemuck::cast_slice(&m.mesh.indices),
                usage: wgpu::BufferUsages::INDEX,
            });

            let positions = m.mesh.positions.chunks_exact(3).map(|c| [c[0], c[1], c[2]]).collect();
            let tri_indices = m.mesh.indices.chunks_exact(3).map(|t| [t[0], t[1], t[2]]).collect();
            model::Mesh {
                name: file_name.to_string(),
                vertex_buffer,
                index_buffer,
                num_element: m.mesh.indices.len() as u32,
                material: m.mesh.material_id.unwrap_or(0),
                positions,
                tri_indices,
            }
        }).collect();
    Ok(model::Model{ meshes, materials})
}

fn material_bind_group(
    device: &wgpu::Device,
    layout: &wgpu::BindGroupLayout,
    diffuse_texture: &texture::Texture,
) -> wgpu::BindGroup {
    device.create_bind_group(&wgpu::BindGroupDescriptor {
        layout,
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::TextureView(&diffuse_texture.view),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: wgpu::BindingResource::Sampler(&diffuse_texture.sampler),
            },
        ],
        label: None,
    })
}

async fn load_gltf_model(
    file_name: &str,
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    layout: &wgpu::BindGroupLayout,
) -> anyhow::Result<model::Model> {
    let bytes = load_binary(file_name).await?;
    let gltf = gltf::Gltf::from_slice(&bytes)?;

    let mut buffers = Vec::new();
    for buffer in gltf.buffers() {
        match buffer.source() {
            gltf::buffer::Source::Bin => {
                let blob = gltf
                    .blob
                    .as_ref()
                    .ok_or_else(|| anyhow::anyhow!("{file_name}: GLB has no BIN chunk"))?;
                buffers.push(blob.clone());
            }
            gltf::buffer::Source::Uri(uri) => {
                anyhow::bail!("{file_name}: external glTF buffer `{uri}` is not supported yet");
            }
        }
    }

    let diffuse_texture = texture::Texture::from_color(device, queue, [255, 255, 255, 255], "white")?;
    let bind_group = material_bind_group(device, layout, &diffuse_texture);
    let materials = vec![model::Material {
        name: "default".to_string(),
        diffuse_texture,
        bind_group,
    }];

    let mut meshes = Vec::new();
    if let Some(scene) = gltf.default_scene() {
        for node in scene.nodes() {
            load_gltf_node(
                file_name,
                node,
                glam::Mat4::IDENTITY,
                &buffers,
                device,
                &mut meshes,
            )?;
        }
    } else {
        for mesh in gltf.meshes() {
            load_gltf_mesh(
                file_name,
                mesh,
                glam::Mat4::IDENTITY,
                &buffers,
                device,
                &mut meshes,
            )?;
        }
    }

    if meshes.is_empty() {
        anyhow::bail!("{file_name}: no meshes found");
    }

    Ok(model::Model { meshes, materials })
}

fn load_gltf_node(
    file_name: &str,
    node: gltf::Node,
    parent: glam::Mat4,
    buffers: &[Vec<u8>],
    device: &wgpu::Device,
    meshes: &mut Vec<model::Mesh>,
) -> anyhow::Result<()> {
    let world = parent * glam::Mat4::from_cols_array_2d(&node.transform().matrix());
    if let Some(mesh) = node.mesh() {
        load_gltf_mesh(file_name, mesh, world, buffers, device, meshes)?;
    }
    for child in node.children() {
        load_gltf_node(file_name, child, world, buffers, device, meshes)?;
    }
    Ok(())
}

fn load_gltf_mesh(
    file_name: &str,
    mesh: gltf::Mesh,
    world: glam::Mat4,
    buffers: &[Vec<u8>],
    device: &wgpu::Device,
    meshes: &mut Vec<model::Mesh>,
) -> anyhow::Result<()> {
    let mesh_name = mesh.name().unwrap_or(file_name);
    let normal_mat = glam::Mat3::from_mat4(world.inverse().transpose());

    for primitive in mesh.primitives() {
        let reader = primitive.reader(|buffer| Some(buffers[buffer.index()].as_slice()));
        let positions: Vec<[f32; 3]> = reader
            .read_positions()
            .ok_or_else(|| anyhow::anyhow!("{file_name}: primitive has no POSITION"))?
            .collect();
        let normals: Option<Vec<[f32; 3]>> =
            reader.read_normals().map(|iter| iter.collect());
        let tex_coords: Option<Vec<[f32; 2]>> = reader
            .read_tex_coords(0)
            .map(|coords| coords.into_f32().collect());
        let vertices: Vec<model::ModelVertex> = positions
            .into_iter()
            .enumerate()
            .map(|(i, pos)| {
                let position = world.transform_point3(glam::Vec3::from(pos)).to_array();
                let normal = normals
                    .as_ref()
                    .and_then(|n| n.get(i).copied())
                    .map(|n| normal_mat.mul_vec3(glam::Vec3::from(n)).normalize_or_zero().to_array())
                    .unwrap_or([0.0, 1.0, 0.0]);
                let tex_coords = tex_coords
                    .as_ref()
                    .and_then(|uv| uv.get(i).copied())
                    .unwrap_or([0.0, 0.0]);
                model::ModelVertex {
                    position,
                    tex_coords,
                    normal,
                }
            })
            .collect();

        let indices: Vec<u32> = if let Some(indices) = reader.read_indices() {
            indices.into_u32().collect()
        } else {
            (0..vertices.len() as u32).collect()
        };

        let vertex_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some(&format!("{file_name} {mesh_name} Vertex Buffer")),
            contents: bytemuck::cast_slice(&vertices),
            usage: wgpu::BufferUsages::VERTEX,
        });
        let index_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some(&format!("{file_name} {mesh_name} Index Buffer")),
            contents: bytemuck::cast_slice(&indices),
            usage: wgpu::BufferUsages::INDEX,
        });

        
        let positions = vertices.iter().map(|m| {m.position}).collect::<Vec<_>>();
        let tri_indices = indices.chunks_exact(3).map(|t| [t[0], t[1], t[2]]).collect::<Vec<_>>();
        meshes.push(model::Mesh {
            name: mesh_name.to_string(),
            vertex_buffer,
            index_buffer,
            num_element: indices.len() as u32,
            material: 0,
            positions,   
            tri_indices,     
        });
    }

    Ok(())
}