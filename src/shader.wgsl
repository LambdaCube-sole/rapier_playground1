//Vertex Shader

struct Camera {
    view_pos: vec4<f32>,
    view_proj: mat4x4<f32>,
};

struct InstanceInput {
    @location(5) model_matrix_0: vec4<f32>,
    @location(6) model_matrix_1: vec4<f32>,
    @location(7) model_matrix_2: vec4<f32>,
    @location(8) model_matrix_3: vec4<f32>,
    @location(9) normal_matrix_0: vec3<f32>,
    @location(10) normal_matrix_1: vec3<f32>,
    @location(11) normal_matrix_2: vec3<f32>,
    @location(12) color_1 : vec4<f32>,
    @location(13) color_2 : vec4<f32>,
    @location(14) pattern : u32,
};

struct Light {
    position: vec3<f32>,
    apply_contact: u32,
    color: vec3<f32>,
    ball_count: u32,
    balls: array<vec4<f32>, 3000>,
}
@group(2) @binding(0)
var<uniform> light: Light;


@group(1) @binding(0)
var<uniform> camera: Camera;

struct VertexInput {
    @location(0) position : vec3<f32>,
    @location(1) tex_coords : vec2<f32>,
    @location(2) normal: vec3<f32>,
};

struct VertexOutput {
    @builtin(position) clip_position : vec4<f32>,
    @location(0) tex_coords : vec2<f32>,
    @location(1) world_normal: vec3<f32>,
    @location(2) world_position: vec3<f32>,
    @location(3) color_1: vec4<f32>,
    @location(4) color_2: vec4<f32>,
    @location(5) @interpolate(flat) pattern: u32,
    @location(6) center: vec3<f32>,
    @location(7) local_pos: vec3<f32>,
};

@vertex
fn vs_main(
    model: VertexInput,
    instance: InstanceInput,
) -> VertexOutput {
    let model_matrix = mat4x4<f32>(
        instance.model_matrix_0,
        instance.model_matrix_1,
        instance.model_matrix_2,
        instance.model_matrix_3,
    );
    let normal_matrix = mat3x3<f32>(
        instance.normal_matrix_0,
        instance.normal_matrix_1,
        instance.normal_matrix_2,
    );
    var out: VertexOutput;
    out.tex_coords = model.tex_coords;
    out.world_normal = normal_matrix * model.normal;
    var world_position: vec4<f32> = model_matrix * vec4<f32>(model.position, 1.0);
    out.world_position = world_position.xyz;
    out.clip_position = camera.view_proj * world_position;
    out.color_1 = instance.color_1;
    out.color_2 = instance.color_2;
    out.pattern = instance.pattern;
    out.center = model_matrix[3].xyz;
    out.local_pos = model.position;
    return out;
}



//Fragment Shader
@group(0) @binding(0) 
var t_diffuse: texture_2d<f32>;
@group(0) @binding(1)
var s_diffuse: sampler;
override apply_gamma: bool = false;
@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    var object_color: vec4<f32> = textureSample(t_diffuse, s_diffuse, in.tex_coords);
    if (light.apply_contact == 0u) {
        object_color = vec4<f32>(pattern_color(in), object_color.a);
    } else {
        object_color = vec4<f32>(screw_color(in.local_pos, in.color_1.xyz, in.color_2.xyz), 1.0);
    }
    // We don't need (or want) much ambient light, so 0.1 is fine
    let ambient_strength = 0.1;
    let ambient_color = light.color * ambient_strength;
    let light_dir = normalize(light.position - in.world_position);
    let view_dir = normalize(camera.view_pos.xyz - in.world_position);
    let n = normalize(in.world_normal);
    let reflect_dir = reflect(-light_dir, n);
    let rim = pow(1.0 - max(dot(n, view_dir), 0.0), 3.0);
    let diffuse_strength = max(dot(n, light_dir), 0.0);
    let diffuse_color = light.color * diffuse_strength;

    let specular_strength = pow(max(dot(view_dir, reflect_dir), 0.0), 32.0);
    let specular_color = specular_strength * light.color;
    
    var result = (ambient_color + diffuse_color + specular_color) * object_color.xyz;
    // proximity / spherical AO
    if (light.apply_contact == 1u) {
        var shade = 1.0;
        let radius = 1.0;
        let reach = 0.45; // 이 거리 밖이면 AO 없음
        for (var i = 0u; i < light.ball_count; i++) {
            let gap = distance(in.world_position, light.balls[i].xyz) - radius;
            shade = min(shade, smoothstep(0.0, reach, gap));
        }
        result = result * (0.2 + 0.8 * shade);
    } 
    result = result + rim * light.color * 0.4;
    if (apply_gamma == true) {
        result = pow(result, vec3<f32>(1.0 / 2.2));
    }

    return vec4<f32>(result, object_color.a);
}
 
fn pick(a: vec3<f32>, b: vec3<f32>, t: bool) -> vec3<f32> {
    if (t) { return a; } else { return b; }
}

fn pattern_color(in: VertexOutput) -> vec3<f32> {
    let c1 = in.color_1.xyz;
    let c2 = in.color_2.xyz;
    if (in.pattern == 0u) {
        return c1;
    }

    let offset = in.world_position - in.center;
    let radius = max(length(offset), 1e-4);
    //let q = offset / radius; // 단위 구 위의 점. 크기와 무관
    let q = normalize(in.local_pos);
    // 지름에 몇 칸인지. 4면 공 하나에 체크/줄이 적당히 보임
    let freq = 4.0;

    switch in.pattern {
        case 1u: { // Checker
            let s = floor(q * freq);
            let odd = fract((s.x + s.y + s.z) * 0.5) > 0.25;
            return pick(c1, c2, odd);
        }
        case 2u: { // Stripe1 세로줄 (로컬 Y에 평행)
            let bands = 8.0;
            let odd = fract(q.x * bands * 0.5 + 0.5) > 0.5;
            return pick(c1, c2, odd);
        }
        case 3u: { // Stripe2 가로줄 (로컬 Y에 수직)
            let bands = 8.0;
            let odd = fract(q.y * bands * 0.5 + 0.5) > 0.5;
            return pick(c1, c2, odd);
        }
        case 4u: { // Bubble 물방울
            let p = q * freq;
            let cell = round(p);
            let d = length(p - cell);
            let dot = d < 0.32;
            return pick(c2, c1, dot); // 바탕 c1, 점 c2
        }
        default: {
            return c1;
        }
    }
}

const TAU: f32 = 6.283185;
fn screw_color(p: vec3<f32>, c1: vec3<f32>, c2: vec3<f32>) -> vec3<f32> {
    let angle = atan2(p.x, p.z);       // 축 둘레 각도
    let turns = 4.0;                   // 높이 1만큼 내려갈 때 몇 바퀴
    let u = angle / TAU + p.y * turns;
    let stripe = fract(u);
    // 띠 두께. 0.5면 반반, 0.2면 가는 나사골
    return pick(c1, c2, stripe < 0.22);
}
