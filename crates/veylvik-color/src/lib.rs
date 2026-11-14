//! OpenColorIO configuration and GPU shader extraction. Unsafe code is confined to
//! this ownership-checked C ABI; processing pixels in the application stays on Vulkan.
use std::ffi::{CStr, CString, c_char, c_void};
use std::ptr::NonNull;
pub type Result<T> = std::result::Result<T, String>;
#[repr(C)]
struct RawTexture {
    sampler: *const c_char,
    values: *const f32,
    width: u32,
    height: u32,
    depth: u32,
    channels: u32,
    dimension: u32,
    linear: u32,
    binding: u32,
}
unsafe extern "C" {
    fn vv_ocio_error() -> *const c_char;
    fn vv_ocio_config(path: *const c_char) -> *mut c_void;
    fn vv_ocio_config_free(config: *mut c_void);
    fn vv_ocio_count(config: *mut c_void, kind: i32, display: *const c_char) -> i32;
    fn vv_ocio_name(
        config: *mut c_void,
        kind: i32,
        display: *const c_char,
        index: i32,
    ) -> *const c_char;
    fn vv_ocio_program(
        config: *mut c_void,
        source: *const c_char,
        destination: *const c_char,
        display: *const c_char,
        view: *const c_char,
    ) -> *mut c_void;
    fn vv_ocio_program_free(program: *mut c_void);
    fn vv_ocio_shader(program: *mut c_void) -> *const c_char;
    fn vv_ocio_files(program: *mut c_void) -> i32;
    fn vv_ocio_file(program: *mut c_void, index: i32) -> *const c_char;
    fn vv_ocio_textures(program: *mut c_void) -> u32;
    fn vv_ocio_texture(program: *mut c_void, index: u32, output: *mut RawTexture) -> i32;
    fn vv_ocio_reference(program: *mut c_void, rgba: *mut f32) -> i32;
}
fn cstring(s: &str) -> Result<CString> {
    CString::new(s).map_err(|_| "OCIO names cannot contain NUL".into())
}
// All pointers below are supplied by our C++ wrapper and remain valid until their
// owning Config/Program is dropped, or (for the error) the next failing call.
unsafe fn string(ptr: *const c_char) -> String {
    if ptr.is_null() {
        return String::new();
    }
    unsafe { CStr::from_ptr(ptr) }
        .to_string_lossy()
        .into_owned()
}
fn last_error() -> String {
    unsafe { string(vv_ocio_error()) }
}
pub struct Config(NonNull<c_void>);
impl Drop for Config {
    fn drop(&mut self) {
        unsafe { vv_ocio_config_free(self.0.as_ptr()) }
    }
}
pub struct Program(NonNull<c_void>);
impl Drop for Program {
    fn drop(&mut self) {
        unsafe { vv_ocio_program_free(self.0.as_ptr()) }
    }
}
#[derive(Debug)]
pub struct Texture {
    pub sampler: String,
    pub width: u32,
    pub height: u32,
    pub depth: u32,
    pub dimension: u32,
    pub linear: bool,
    pub binding: u32,
    /// RGBA32F in OCIO's native texture order. Alpha is always one.
    pub pixels: Vec<[f32; 4]>,
}
pub struct Shader {
    pub wgsl: String,
    pub textures: Vec<Texture>,
}
impl Config {
    pub fn load(path: &str) -> Result<Self> {
        let path = cstring(path)?;
        NonNull::new(unsafe { vv_ocio_config(path.as_ptr()) })
            .map(Self)
            .ok_or_else(last_error)
    }
    fn names(&self, kind: i32, display: &str) -> Result<Vec<String>> {
        let display = cstring(display)?;
        let count = unsafe { vv_ocio_count(self.0.as_ptr(), kind, display.as_ptr()) };
        if !(0..=65536).contains(&count) {
            return Err(last_error());
        }
        Ok((0..count)
            .map(|i| unsafe { string(vv_ocio_name(self.0.as_ptr(), kind, display.as_ptr(), i)) })
            .collect())
    }
    pub fn spaces(&self) -> Result<Vec<String>> {
        self.names(0, "")
    }
    pub fn displays(&self) -> Result<Vec<String>> {
        self.names(1, "")
    }
    pub fn views(&self, display: &str) -> Result<Vec<String>> {
        self.names(2, display)
    }
    pub fn processor(
        &self,
        source: &str,
        destination: &str,
        display: &str,
        view: &str,
    ) -> Result<Program> {
        let (s, d, p, v) = (
            cstring(source)?,
            cstring(destination)?,
            cstring(display)?,
            cstring(view)?,
        );
        NonNull::new(unsafe {
            vv_ocio_program(
                self.0.as_ptr(),
                s.as_ptr(),
                d.as_ptr(),
                p.as_ptr(),
                v.as_ptr(),
            )
        })
        .map(Program)
        .ok_or_else(last_error)
    }
}
impl Program {
    pub fn files(&self) -> Vec<String> {
        let count = unsafe { vv_ocio_files(self.0.as_ptr()) };
        (0..count)
            .map(|i| unsafe { string(vv_ocio_file(self.0.as_ptr(), i)) })
            .collect()
    }
    /// Reference for validation, not an application pixel-processing fallback.
    pub fn reference(&self, mut rgba: [f32; 4]) -> Result<[f32; 4]> {
        if unsafe { vv_ocio_reference(self.0.as_ptr(), rgba.as_mut_ptr()) } == 0 {
            return Err(last_error());
        }
        Ok(rgba)
    }
    pub fn shader(&self) -> Result<Shader> {
        let mut source = unsafe { string(vv_ocio_shader(self.0.as_ptr())) };
        let count = unsafe { vv_ocio_textures(self.0.as_ptr()) };
        if count > 16 {
            return Err("OCIO processor exceeds 16 texture limit".into());
        }
        let mut textures = Vec::new();
        for i in 0..count {
            let mut raw = RawTexture {
                sampler: std::ptr::null(),
                values: std::ptr::null(),
                width: 0,
                height: 0,
                depth: 0,
                channels: 0,
                dimension: 0,
                linear: 0,
                binding: 0,
            };
            if unsafe { vv_ocio_texture(self.0.as_ptr(), i, &mut raw) } == 0 {
                return Err(last_error());
            }
            let texels = u64::from(raw.width) * u64::from(raw.height) * u64::from(raw.depth);
            if texels == 0
                || texels > 16_777_216
                || ![1, 3].contains(&raw.channels)
                || raw.values.is_null()
                || !(1..=3).contains(&raw.dimension)
            {
                return Err("Invalid or oversized OCIO texture".into());
            }
            let sampler = unsafe { string(raw.sampler) };
            let values = unsafe {
                std::slice::from_raw_parts(raw.values, texels as usize * raw.channels as usize)
            };
            let pixels = values
                .chunks_exact(raw.channels as usize)
                .map(|v| {
                    if v.len() == 1 {
                        [v[0], v[0], v[0], 1.]
                    } else {
                        [v[0], v[1], v[2], 1.]
                    }
                })
                .collect();
            // Full-float LUT buffers with explicit filtering avoid optional float32
            // texture-filtering support and half-float quantisation on different GPUs.
            let declaration = format!(
                "layout(set=0, binding = {}) uniform sampler{}D {};",
                raw.binding, raw.dimension, sampler
            );
            if !source.contains(&declaration) {
                return Err("Unrecognised OCIO texture declaration".into());
            }
            let binding = 3 + i;
            let w = raw.width;
            let h = raw.height;
            let d = raw.depth;
            let coordinate = match raw.dimension {
                1 => "vec3(uv,0.5,0.5)",
                2 => "vec3(uv,0.5)",
                _ => "uv",
            };
            let ty = match raw.dimension {
                1 => "float",
                2 => "vec2",
                _ => "vec3",
            };
            let sample = if raw.linear != 0 {
                format!(
                    "vec3 p={coordinate}*vec3({w},{h},{d})-vec3(0.5);ivec3 a=ivec3(floor(p));vec3 f=fract(p);vec4 c=vec4(0.0);for(int z=0;z<2;z++)for(int y=0;y<2;y++)for(int x=0;x<2;x++){{vec3 w=mix(vec3(1.0)-f,f,vec3(x,y,z));c+=vv_fetch_{i}(a+ivec3(x,y,z))*w.x*w.y*w.z;}}return c;"
                )
            } else {
                format!("return vv_fetch_{i}(ivec3(floor({coordinate}*vec3({w},{h},{d}))));")
            };
            let helper = format!(
                "layout(set=0,binding={binding},std430) readonly buffer Lut{i} {{vec4 vv_lut_{i}[];}};\nvec4 vv_fetch_{i}(ivec3 p){{p=clamp(p,ivec3(0),ivec3({w}-1,{h}-1,{d}-1));return vv_lut_{i}[p.x+{w}*(p.y+{h}*p.z)];}}\nvec4 vv_sample_{i}({ty} uv){{{sample}}}"
            );
            source = source.replace(&declaration, &helper);
            source = source.replace(&format!("texture({sampler},"), &format!("vv_sample_{i}("));
            textures.push(Texture {
                sampler,
                width: raw.width,
                height: raw.height,
                depth: raw.depth,
                dimension: raw.dimension,
                linear: raw.linear != 0,
                binding,
                pixels,
            });
        }
        let source = format!(
            "#version 450\n#define texture(t,c) textureLod(t,c,0.0)\nlayout(local_size_x=8,local_size_y=8) in;\nlayout(set=0,binding=0,std140) uniform Size {{uvec4 size;}};\nlayout(set=0,binding=1,std430) readonly buffer Input {{vec4 src[];}};\nlayout(set=0,binding=2,std430) buffer Output {{vec4 dst[];}};\n{source}\nvoid main() {{uvec2 pxy=gl_GlobalInvocationID.xy;if(pxy.x>=size.y||pxy.y>=size.z)return;uint i=pxy.y*size.y+pxy.x;vec4 p=src[i];if(p.a<=0.0){{dst[i]=vec4(0.0);return;}}vec4 c=veylvik_color(vec4(p.rgb/p.a,1.0));dst[i]=vec4(c.rgb*p.a,p.a);}}"
        );
        let module = naga::front::glsl::Frontend::default()
            .parse(
                &naga::front::glsl::Options::from(naga::ShaderStage::Compute),
                &source,
            )
            .map_err(|e| e.emit_to_string(&source))?;
        let info = naga::valid::Validator::new(
            naga::valid::ValidationFlags::all(),
            naga::valid::Capabilities::all(),
        )
        .validate(&module)
        .map_err(|e| e.to_string())?;
        let wgsl =
            naga::back::wgsl::write_string(&module, &info, naga::back::wgsl::WriterFlags::empty())
                .map_err(|e| e.to_string())?;
        Ok(Shader { wgsl, textures })
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn built_in_config_generates_valid_matrix_and_view_shaders() {
        let c = Config::load("ocio://default").unwrap();
        assert!(c.spaces().unwrap().iter().any(|s| s == "ACEScg"));
        let matrix = c.processor("ACEScg", "sRGB - Display", "", "").unwrap();
        assert!(matrix.shader().unwrap().textures.is_empty());
        let display = c.displays().unwrap().remove(0);
        let view = c.views(&display).unwrap().remove(0);
        let view = c.processor("ACEScg", "", &display, &view).unwrap();
        assert!(!view.shader().unwrap().wgsl.is_empty());
        assert!(c.processor("missing", "also missing", "", "").is_err());
        assert!(Config::load("/tmp/veylvik-nonexistent-ocio-config").is_err());
    }
}
