//! Kernels told where their dispatch begins: a pass over only part of the frame (a layer's own area) runs its kernel's threads from an origin, and every pixel it computes is the pixel a full-frame pass computes there. The kernels stay written for the whole frame; the compositor's copy of their source is given the origin when it is loaded.

/// The binding the origin is read from: `vec4<u32>`, the first pixel's x and y.
pub(crate) const BINDING: u32 = 9;
const PARAMETER: &str = "@builtin(global_invocation_id) gid";

/// `source` with every kernel that takes the global id reading it from the origin on:
/// the parameter is renamed and `gid` is made again first thing in the body.
pub(crate) fn with_origin(source: &str) -> String {
    let mut out = String::with_capacity(source.len() + 16 * 1024);
    out.push_str("@group(0) @binding(9) var<uniform> origin: vec4<u32>;\n");
    let mut rest = source;
    while let Some(at) = rest.find(PARAMETER) {
        let (before, from) = rest.split_at(at);
        out.push_str(before);
        out.push_str("@builtin(global_invocation_id) dispatched");
        let from = &from[PARAMETER.len()..];
        // The body begins at the first brace after the parameter list.
        let open = from.find('{').expect("a kernel has a body");
        out.push_str(&from[..=open]);
        out.push_str(" let gid = dispatched + vec3<u32>(origin.xy, 0u);");
        rest = &from[open + 1..];
    }
    out.push_str(rest);
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn every_kernel_reads_its_pixel_from_the_origin_on() {
        let source = "fn helper(gid:vec3<u32>)->u32 {return gid.x;}\n\
            @compute @workgroup_size(8,8)\n\
            fn a(@builtin(global_invocation_id) gid:vec3<u32>) {\n  result[gid.x]=1.0;\n}\n\
            @compute @workgroup_size(64,1)\n\
            fn b(@builtin(global_invocation_id) gid: vec3<u32>,@builtin(local_invocation_id) lid:vec3<u32>) { x(gid, lid); }\n\
            @compute @workgroup_size(1)\n\
            fn c() { y(); }\n";
        let out = with_origin(source);
        assert_eq!(
            out.matches("let gid = dispatched + vec3<u32>(origin.xy, 0u);")
                .count(),
            2
        );
        assert!(
            out.contains("fn a(@builtin(global_invocation_id) dispatched:vec3<u32>) { let gid")
        );
        assert!(out.contains(
            "dispatched: vec3<u32>,@builtin(local_invocation_id) lid:vec3<u32>) { let gid"
        ));
        // A helper that merely names gid, and a kernel without it, are left as they are.
        assert!(out.contains("fn helper(gid:vec3<u32>)->u32 {return gid.x;}"));
        assert!(out.contains("fn c() { y(); }"));
        assert!(out.starts_with("@group(0) @binding(9) var<uniform> origin: vec4<u32>;"));
    }
}
