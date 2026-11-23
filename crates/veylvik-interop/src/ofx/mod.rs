//! An OpenFX image-effect host: loads a plugin bundle, describes its plugins in a context and renders them on the CPU in float RGBA. Plugin code runs in this process, so callers run it in a helper process of its own: a crashing plugin must never take the editor with it.
mod parametric;
/// A curve setting's value at `x`, as the host evaluates it for plugins.
pub use parametric::evaluate as curve_value;
mod params;
mod props;
mod services;
mod suites;
pub use props::{Prop, PropSet};
use std::ffi::{CStr, CString, c_char, c_int, c_uint, c_void};
use std::path::{Path, PathBuf};
use suites::{Effect, Pixels};

pub type Status = c_int;
pub const OK: Status = 0;
pub const FAILED: Status = 1;
pub const ERR_UNKNOWN: Status = 3;
pub const ERR_MISSING_HOST_FEATURE: Status = 4;
pub const ERR_EXISTS: Status = 6;
pub const ERR_MEMORY: Status = 8;
pub const ERR_BAD_HANDLE: Status = 9;
pub const ERR_BAD_INDEX: Status = 10;
pub const ERR_VALUE: Status = 11;
pub const REPLY_DEFAULT: Status = 14;
pub type Result<T> = std::result::Result<T, String>;
/// Appends a line to the file named by `VEYLVIK_OFX_TRACE`, if set: what a plugin asked
/// for that the host could not give, to find what a plugin needs.
pub(crate) fn trace(line: impl FnOnce() -> String) {
    static FILE: std::sync::OnceLock<Option<std::sync::Mutex<std::fs::File>>> =
        std::sync::OnceLock::new();
    let file = FILE.get_or_init(|| {
        let path = std::env::var_os("VEYLVIK_OFX_TRACE")?;
        let file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
            .ok()?;
        Some(std::sync::Mutex::new(file))
    });
    if let Some(file) = file {
        use std::io::Write;
        let mut file = file.lock().unwrap_or_else(|e| e.into_inner());
        let _ = writeln!(file, "{}", line());
    }
}

#[repr(C)]
struct Host {
    host: *mut c_void,
    fetch_suite: extern "C" fn(*mut c_void, *const c_char, c_int) -> *const c_void,
}
type MainEntry = extern "C" fn(*const c_char, *const c_void, *mut c_void, *mut c_void) -> Status;
#[repr(C)]
struct Plugin {
    api: *const c_char,
    api_version: c_int,
    identifier: *const c_char,
    major: c_uint,
    minor: c_uint,
    set_host: Option<extern "C" fn(*mut Host)>,
    main_entry: Option<MainEntry>,
}
extern "C" fn fetch_suite(
    _host: *mut c_void,
    name: *const c_char,
    version: c_int,
) -> *const c_void {
    if name.is_null() {
        return std::ptr::null();
    }
    // SAFETY: suite names from plugins are C strings.
    let name = unsafe { CStr::from_ptr(name) }.to_bytes();
    fn table<T>(t: &'static T) -> *const c_void {
        std::ptr::from_ref(t).cast()
    }
    match (name, version) {
        (b"OfxPropertySuite", 1) => table(&props::SUITE),
        (b"OfxImageEffectSuite", 1) => table(&suites::EFFECT_SUITE),
        (b"OfxParameterSuite", 1) => table(&params::PARAM_SUITE),
        (b"OfxMemorySuite", 1) => table(&suites::MEMORY_SUITE),
        (b"OfxMultiThreadSuite", 1) => table(&services::THREAD_SUITE),
        (b"OfxMessageSuite", 1 | 2) => table(&services::MESSAGE_SUITE),
        (b"OfxProgressSuite", 1) => table(&services::PROGRESS_SUITE),
        (b"OfxTimeLineSuite", 1) => table(&services::TIMELINE_SUITE),
        (b"OfxParametricParameterSuite", 1) => table(&parametric::SUITE),
        _ => {
            trace(|| format!("no suite {} v{version}", String::from_utf8_lossy(name)));
            std::ptr::null()
        }
    }
}
/// The host's own description, as plugins read it.
fn host_props() -> PropSet {
    let p = PropSet::default();
    p.string("OfxPropType", "OfxTypeImageEffectHost");
    p.string("OfxPropName", "org.veylvik.host");
    p.string("OfxPropLabel", "Veylvik");
    p.int("OfxPropAPIVersion", &[1, 4]);
    p.int("OfxPropVersion", &[0, 1, 0]);
    p.string("OfxPropVersionLabel", env!("CARGO_PKG_VERSION"));
    p.int("OfxImageEffectHostPropIsBackground", &[1]);
    p.int("OfxImageEffectPropSupportsOverlays", &[0]);
    p.int("OfxImageEffectPropSupportsMultiResolution", &[0]);
    p.int("OfxImageEffectPropSupportsTiles", &[0]);
    // Plugins may read other frames: they name them before rendering (frames needed).
    p.int("OfxImageEffectPropTemporalClipAccess", &[1]);
    p.strings(
        "OfxImageEffectPropSupportedComponents",
        &["OfxImageComponentRGBA"],
    );
    p.strings(
        "OfxImageEffectPropSupportedContexts",
        &[
            "OfxImageEffectContextFilter",
            "OfxImageEffectContextGeneral",
            "OfxImageEffectContextGenerator",
        ],
    );
    p.strings(
        "OfxImageEffectPropSupportedPixelDepths",
        &["OfxBitDepthFloat"],
    );
    p.int("OfxImageEffectPropMultipleClipDepths", &[0]);
    p.int("OfxImageEffectPropSupportsMultipleClipPARs", &[0]);
    p.int("OfxImageEffectPropSetableFrameRate", &[0]);
    p.int("OfxImageEffectPropSetableFielding", &[0]);
    p.int("OfxImageEffectInstancePropSequentialRender", &[0]);
    p.string("OfxImageEffectPropOpenGLRenderSupported", "false");
    p.string("OfxImageEffectPropCPURenderSupported", "true");
    p.string(
        "OfxImageEffectHostPropNativeOrigin",
        "kOfxImageEffectHostPropNativeOriginBottomLeft",
    );
    p.string("OfxImageEffectPropRenderQualityDraft", "false");
    p.int("OfxParamHostPropSupportsCustomInteract", &[0]);
    p.int("OfxParamHostPropSupportsStringAnimation", &[0]);
    p.int("OfxParamHostPropSupportsChoiceAnimation", &[0]);
    p.int("OfxParamHostPropSupportsBooleanAnimation", &[0]);
    p.int("OfxParamHostPropSupportsCustomAnimation", &[0]);
    p.int("OfxParamHostPropSupportsParametricAnimation", &[0]);
    p.int("OfxParamHostPropMaxParameters", &[-1]);
    p.int("OfxParamHostPropMaxPages", &[0]);
    p.int("OfxParamHostPropPageRowColumnCount", &[0, 0]);
    p
}
/// What a plugin says of itself.
#[derive(Debug, Clone)]
pub struct PluginInfo {
    pub index: usize,
    pub identifier: String,
    pub version: [u32; 2],
    pub label: String,
    pub grouping: String,
    pub description: String,
    pub contexts: Vec<String>,
    pub depths: Vec<String>,
    /// The plugin reads other frames of its inputs (it says which before rendering).
    pub temporal: bool,
}
/// A parameter as a plugin defines it in a context.
#[derive(Debug, Clone)]
pub struct ParamInfo {
    pub name: String,
    /// The OpenFX type, e.g. `OfxParamTypeDouble`.
    pub kind: String,
    pub label: String,
    pub hint: String,
    pub parent: String,
    pub default: Vec<f64>,
    pub text: String,
    pub min: Vec<f64>,
    pub max: Vec<f64>,
    pub display_min: Vec<f64>,
    pub display_max: Vec<f64>,
    pub options: Vec<String>,
    pub secret: bool,
    /// A curve setting's default points per curve, its key range and curve names.
    pub curves: Vec<Vec<[f64; 2]>>,
    pub range: [f64; 2],
    pub curve_labels: Vec<String>,
}
#[derive(Debug, Clone)]
pub struct ClipInfo {
    pub name: String,
    pub optional: bool,
    pub mask: bool,
}
#[derive(Debug, Clone)]
pub struct Description {
    pub context: String,
    pub clips: Vec<ClipInfo>,
    pub params: Vec<ParamInfo>,
}
/// A parameter value given for a render.
#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    Numbers(Vec<f64>),
    Text(String),
    /// A curve setting's points ([key, value]) per curve.
    Curves(Vec<Vec<[f64; 2]>>),
}
/// A loaded bundle. Its plugins stay loaded until it is dropped.
pub struct Library {
    plugins: Vec<*const Plugin>,
    loaded: std::cell::RefCell<Vec<bool>>,
    host: Box<Host>,
    _host_props: Box<PropSet>,
    /// Each plugin's descriptor, once described.
    described: Vec<std::cell::OnceCell<Box<Effect>>>,
    bundle: PathBuf,
    // Last, so the plugins' code is unloaded after everything above.
    _lib: libloading::Library,
}
/// The binary inside an `.ofx.bundle` for this machine, or the path itself for a bare
/// `.ofx` file.
pub fn binary(bundle: &Path) -> Result<PathBuf> {
    if bundle.is_file() {
        return Ok(bundle.to_path_buf());
    }
    let stem = bundle
        .file_name()
        .and_then(|n| n.to_str())
        .and_then(|n| n.strip_suffix(".bundle"))
        .ok_or_else(|| format!("{} is not an .ofx.bundle", bundle.display()))?;
    let arch = if cfg!(target_arch = "aarch64") {
        ["Linux-arm64", "Linux-aarch64"]
    } else {
        ["Linux-x86-64", "Linux-x86_64"]
    };
    arch.iter()
        .map(|a| bundle.join("Contents").join(a).join(stem))
        .find(|p| p.is_file())
        .ok_or_else(|| {
            format!(
                "{} has no binary for this machine ({})",
                bundle.display(),
                arch[0]
            )
        })
}
fn c(s: &str) -> CString {
    CString::new(s).unwrap_or_default()
}
impl Library {
    pub fn open(bundle: &Path) -> Result<Self> {
        let path = binary(bundle)?;
        // SAFETY: loading a plugin runs its initialisers; that is what hosting it means,
        // and callers run this in a helper process kept apart from the editor.
        let lib = unsafe { libloading::Library::new(&path) }
            .map_err(|e| format!("{}: {e}", path.display()))?;
        let host_props = Box::new(host_props());
        let mut host = Box::new(Host {
            host: host_props.handle(),
            fetch_suite,
        });
        // SAFETY: the symbols have the standard's signatures.
        let plugins = unsafe {
            if let Ok(set) = lib.get::<extern "C" fn(*mut Host) -> Status>(b"OfxSetHost\0") {
                set(&mut *host);
            }
            let count = lib
                .get::<extern "C" fn() -> c_int>(b"OfxGetNumberOfPlugins\0")
                .map_err(|e| format!("{}: not an OpenFX plugin ({e})", path.display()))?;
            let get = lib
                .get::<extern "C" fn(c_int) -> *const Plugin>(b"OfxGetPlugin\0")
                .map_err(|e| format!("{}: not an OpenFX plugin ({e})", path.display()))?;
            (0..count().clamp(0, 4096))
                .map(|i| get(i))
                .filter(|p| !p.is_null())
                .collect::<Vec<_>>()
        };
        let n = plugins.len();
        Ok(Self {
            plugins,
            loaded: std::cell::RefCell::new(vec![false; n]),
            host,
            _host_props: host_props,
            described: (0..n).map(|_| std::cell::OnceCell::new()).collect(),
            bundle: bundle.to_path_buf(),
            _lib: lib,
        })
    }
    pub fn len(&self) -> usize {
        self.plugins.len()
    }
    pub fn is_empty(&self) -> bool {
        self.plugins.is_empty()
    }
    fn plugin(&self, index: usize) -> Result<&Plugin> {
        let p = *self.plugins.get(index).ok_or("no such plugin")?;
        // SAFETY: OfxGetPlugin returned it; it lives as long as the library.
        Ok(unsafe { &*p })
    }
    /// The plugin's identifier and whether it is an image effect this host can run.
    pub fn identifier(&self, index: usize) -> Result<(String, bool)> {
        let p = self.plugin(index)?;
        // SAFETY: the plugin's strings are C strings that live with the library.
        let (api, id) = unsafe {
            (
                CStr::from_ptr(p.api).to_string_lossy().into_owned(),
                CStr::from_ptr(p.identifier).to_string_lossy().into_owned(),
            )
        };
        Ok((
            id,
            api == "OfxImageEffectPluginAPI" && p.api_version == 1 && p.main_entry.is_some(),
        ))
    }
    /// Calls an action on a plugin.
    fn action(
        &self,
        index: usize,
        action: &str,
        handle: *mut c_void,
        in_args: Option<&PropSet>,
        out_args: Option<&PropSet>,
    ) -> Status {
        let Ok(p) = self.plugin(index) else {
            return ERR_BAD_HANDLE;
        };
        let Some(main) = p.main_entry else {
            return ERR_MISSING_HOST_FEATURE;
        };
        let action = c(action);
        let args = |a: Option<&PropSet>| a.map_or(std::ptr::null_mut(), PropSet::handle);
        main(action.as_ptr(), handle, args(in_args), args(out_args))
    }
    fn load(&self, index: usize) -> Result<()> {
        if self.loaded.borrow()[index] {
            return Ok(());
        }
        let (id, ok) = self.identifier(index)?;
        if !ok {
            return Err(format!("{id} is not an image effect this host can run"));
        }
        let p = self.plugin(index)?;
        if let Some(set) = p.set_host {
            // Plugins only read the host structure.
            set(std::ptr::from_ref(&*self.host).cast_mut());
        }
        match self.action(index, "OfxActionLoad", std::ptr::null_mut(), None, None) {
            OK | REPLY_DEFAULT => {}
            s => return Err(format!("{id} failed to load (status {s}){}", said())),
        }
        self.loaded.borrow_mut()[index] = true;
        Ok(())
    }
    fn descriptor(&self, index: usize) -> Result<&Effect> {
        self.load(index)?;
        if self.described[index].get().is_none() {
            let effect = Box::new(Effect::default());
            effect.props.string("OfxPropType", "OfxTypeImageEffect");
            effect
                .props
                .string("OfxPluginPropFilePath", &self.bundle.to_string_lossy());
            effect
                .props
                .string("OfxPropLabel", &self.identifier(index)?.0);
            effect
                .props
                .strings("OfxImageEffectPropSupportedContexts", &[]);
            effect
                .props
                .strings("OfxImageEffectPropSupportedPixelDepths", &[]);
            effect.props.string("OfxImageEffectPluginPropGrouping", "");
            effect
                .props
                .strings("OfxImageEffectPropClipPreferencesSlaveParam", &[]);
            effect.props.string("OfxPropPluginDescription", "");
            effect
                .props
                .int("OfxImageEffectPluginPropSingleInstance", &[0]);
            effect.props.string(
                "OfxImageEffectPluginRenderThreadSafety",
                "OfxImageEffectRenderInstanceSafe",
            );
            effect
                .props
                .int("OfxImageEffectPluginPropHostFrameThreading", &[0]);
            effect
                .props
                .int("OfxImageEffectPropSupportsMultiResolution", &[1]);
            effect.props.int("OfxImageEffectPropSupportsTiles", &[1]);
            effect
                .props
                .int("OfxImageEffectPropTemporalClipAccess", &[0]);
            effect
                .props
                .int("OfxImageEffectPluginPropFieldRenderTwiceAlways", &[1]);
            effect
                .props
                .int("OfxImageEffectPropMultipleClipDepths", &[0]);
            effect
                .props
                .int("OfxImageEffectPropSupportsMultipleClipPARs", &[0]);
            match self.action(index, "OfxActionDescribe", effect.handle(), None, None) {
                OK | REPLY_DEFAULT => {}
                s => return Err(format!("describing failed (status {s}){}", said())),
            }
            let _ = self.described[index].set(effect);
        }
        Ok(self.described[index]
            .get()
            .map(|e| &**e)
            .expect("described above"))
    }
    pub fn info(&self, index: usize) -> Result<PluginInfo> {
        let p = self.plugin(index)?;
        let version = [p.major, p.minor];
        let identifier = self.identifier(index)?.0;
        let d = self.descriptor(index)?;
        Ok(PluginInfo {
            index,
            label: d
                .props
                .get_string("OfxPropLabel", 0)
                .unwrap_or_else(|| identifier.clone()),
            identifier,
            version,
            grouping: d
                .props
                .get_string("OfxImageEffectPluginPropGrouping", 0)
                .unwrap_or_default(),
            description: d
                .props
                .get_string("OfxPropPluginDescription", 0)
                .unwrap_or_default(),
            contexts: d.props.get_strings("OfxImageEffectPropSupportedContexts"),
            depths: d
                .props
                .get_strings("OfxImageEffectPropSupportedPixelDepths"),
            temporal: d
                .props
                .get_numbers("OfxImageEffectPropTemporalClipAccess")
                .first()
                .is_some_and(|v| *v != 0.0),
        })
    }
    /// The plugin's clips and parameters in `context`, on a copy of its descriptor.
    fn in_context(&self, index: usize, context: &str) -> Result<Box<Effect>> {
        let effect = Box::new(self.descriptor(index)?.copied());
        let args = PropSet::default();
        args.string("OfxImageEffectPropContext", context);
        match self.action(
            index,
            "OfxImageEffectActionDescribeInContext",
            effect.handle(),
            Some(&args),
            None,
        ) {
            OK | REPLY_DEFAULT => Ok(effect),
            s => Err(format!(
                "describing in {context} failed (status {s}){}",
                said()
            )),
        }
    }
    pub fn describe(&self, index: usize, context: &str) -> Result<Description> {
        let effect = self.in_context(index, context)?;
        let clips = suites_lock(&effect.clip_order)
            .iter()
            .filter_map(|name| {
                let clips = suites_lock(&effect.clips);
                let clip = clips.get(name)?;
                let flag =
                    |n: &str| clip.props.get_numbers(n).first().copied().unwrap_or(0.0) != 0.0;
                Some(ClipInfo {
                    name: name.clone(),
                    optional: flag("OfxImageClipPropOptional"),
                    mask: flag("OfxImageClipPropIsMask"),
                })
            })
            .collect();
        let order = suites_lock(&effect.params.order).clone();
        let all = suites_lock(&effect.params.params);
        let params = order
            .iter()
            .filter_map(|name| {
                let p = all.get(name)?;
                let props = &p.props;
                Some(ParamInfo {
                    name: name.clone(),
                    kind: p.kind.clone(),
                    label: props
                        .get_string("OfxPropLabel", 0)
                        .unwrap_or_else(|| name.clone()),
                    hint: props.get_string("OfxParamPropHint", 0).unwrap_or_default(),
                    parent: props
                        .get_string("OfxParamPropParent", 0)
                        .unwrap_or_default(),
                    default: props.get_numbers("OfxParamPropDefault"),
                    text: if suites::is_text(&p.kind) {
                        props
                            .get_string("OfxParamPropDefault", 0)
                            .unwrap_or_default()
                    } else {
                        String::new()
                    },
                    min: props.get_numbers("OfxParamPropMin"),
                    max: props.get_numbers("OfxParamPropMax"),
                    display_min: props.get_numbers("OfxParamPropDisplayMin"),
                    display_max: props.get_numbers("OfxParamPropDisplayMax"),
                    options: props.get_strings("OfxParamPropChoiceOption"),
                    secret: props
                        .get_numbers("OfxParamPropSecret")
                        .first()
                        .copied()
                        .unwrap_or(0.0)
                        != 0.0,
                    curves: match &*suites_lock(&p.value) {
                        suites::Value::Curves(c) => c.clone(),
                        _ => Vec::new(),
                    },
                    range: match props.get_numbers("OfxParamPropParametricRange")[..] {
                        [low, high] if low < high => [low, high],
                        _ => [0.0, 1.0],
                    },
                    curve_labels: props.get_strings("OfxParamPropDimensionLabel"),
                })
            })
            .collect();
        Ok(Description {
            context: context.to_string(),
            clips,
            params,
        })
    }
    /// An instance of plugin `index` in `context`, rendering `width` x `height`.
    pub fn instance(
        &self,
        index: usize,
        context: &str,
        width: u32,
        height: u32,
    ) -> Result<Instance<'_>> {
        if width == 0 || height == 0 || width > 16384 || height > 16384 {
            return Err(format!(
                "{width} x {height} is not a size an effect can render"
            ));
        }
        let effect = self.in_context(index, context)?;
        let p = &effect.props;
        p.string("OfxPropType", "OfxTypeImageEffectInstance");
        p.string("OfxImageEffectPropContext", context);
        let (w, h) = (f64::from(width), f64::from(height));
        p.double("OfxImageEffectPropProjectSize", &[w, h]);
        p.double("OfxImageEffectPropProjectExtent", &[w, h]);
        p.double("OfxImageEffectPropProjectOffset", &[0.0, 0.0]);
        p.double("OfxImageEffectPropPixelAspectRatio", &[1.0]);
        p.double("OfxImageEffectInstancePropEffectDuration", &[1.0]);
        p.double("OfxImageEffectPropFrameRate", &[24.0]);
        p.double("OfxImageEffectPropFrameRange", &[0.0, 0.0]);
        p.int("OfxPropIsInteractive", &[0]);
        p.int("OfxImageEffectInstancePropSequentialRender", &[0]);
        p.pointer("OfxPropInstanceData", std::ptr::null_mut());
        p.double("VeylvikTime", &[0.0]);
        for param in suites_lock(&effect.params.params).values() {
            *suites_lock(&param.value) = params::default_value(param);
            param
                .props
                .string("OfxPropType", "OfxTypeParameterInstance");
        }
        for clip in suites_lock(&effect.clips).values() {
            let c = &clip.props;
            c.string("OfxImageEffectPropPixelDepth", "OfxBitDepthFloat");
            c.string("OfxImageEffectPropComponents", "OfxImageComponentRGBA");
            c.string("OfxImageClipPropUnmappedPixelDepth", "OfxBitDepthFloat");
            c.string(
                "OfxImageClipPropUnmappedComponents",
                "OfxImageComponentRGBA",
            );
            c.string(
                "OfxImageEffectPropPreMultiplication",
                "OfxImageAlphaPremultiplied",
            );
            c.double("OfxImagePropPixelAspectRatio", &[1.0]);
            c.double("OfxImageEffectPropFrameRate", &[24.0]);
            c.double("OfxImageEffectPropFrameRange", &[0.0, 0.0]);
            c.double("OfxImageEffectPropUnmappedFrameRate", &[24.0]);
            c.double("OfxImageEffectPropUnmappedFrameRange", &[0.0, 0.0]);
            c.string("OfxImageClipPropFieldOrder", "OfxFieldNone");
            c.int("OfxImageClipPropContinuousSamples", &[0]);
            c.int("OfxImageClipPropConnected", &[0]);
        }
        match self.action(
            index,
            "OfxActionCreateInstance",
            effect.handle(),
            None,
            None,
        ) {
            OK | REPLY_DEFAULT => {}
            s => {
                return Err(format!(
                    "creating an instance failed (status {s}){}",
                    said()
                ));
            }
        }
        Ok(Instance {
            library: self,
            index,
            effect,
            size: [width, height],
            scratch: Vec::new(),
            output: Vec::new(),
        })
    }
}
fn suites_lock<T>(m: &std::sync::Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}
/// What plugins said since last asked, as a suffix for an error.
fn said() -> String {
    let messages = std::mem::take(&mut *suites_lock(&services::MESSAGES));
    if messages.is_empty() {
        String::new()
    } else {
        format!(": {}", messages.join("; "))
    }
}
/// Plugins' messages since last asked.
pub fn messages() -> Vec<String> {
    std::mem::take(&mut *suites_lock(&services::MESSAGES))
}
impl Drop for Library {
    fn drop(&mut self) {
        for i in 0..self.plugins.len() {
            if self.loaded.borrow()[i] {
                self.action(i, "OfxActionUnload", std::ptr::null_mut(), None, None);
            }
        }
    }
}
/// A picture handed to or taken from an effect: premultiplied RGBA float, top row first.
pub struct Frame<'a> {
    pub pixels: &'a [f32],
    pub width: u32,
    pub height: u32,
    /// The frame this picture is of; None: the render's own.
    pub time: Option<f64>,
}
/// An effect instance.
pub struct Instance<'l> {
    library: &'l Library,
    index: usize,
    effect: Box<Effect>,
    size: [u32; 2],
    /// Each input's picture in the plugin's row order, and the output, kept between
    /// renders so a frame costs no fresh allocation.
    scratch: Vec<Vec<f32>>,
    output: Vec<f32>,
}
/// Copies `from` into `to` with its rows in the other order (OpenFX counts rows from
/// the bottom), on a few threads: a 4K frame is 133 MB.
fn flip_into(from: &[f32], to: &mut [f32], width: u32) {
    let row = width as usize * 4;
    let rows = to.len() / row.max(1);
    let threads = std::thread::available_parallelism()
        .map_or(1, |n| n.get())
        .clamp(1, 8);
    let band = rows.div_ceil(threads).max(1);
    std::thread::scope(|s| {
        for (k, part) in to.chunks_mut(band * row).enumerate() {
            s.spawn(move || {
                for (r, line) in part.chunks_exact_mut(row).enumerate() {
                    let source = rows - 1 - (k * band + r);
                    line.copy_from_slice(&from[source * row..(source + 1) * row]);
                }
            });
        }
    });
}
impl Instance<'_> {
    pub fn size(&self) -> [u32; 2] {
        self.size
    }
    /// The composition's length in frames, as the plugin sees its clips' frame range.
    pub fn set_duration(&self, frames: u32) {
        let last = f64::from(frames.max(1) - 1);
        let p = &self.effect.props;
        p.double(
            "OfxImageEffectInstancePropEffectDuration",
            &[f64::from(frames.max(1))],
        );
        p.double("OfxImageEffectPropFrameRange", &[0.0, last]);
        for clip in suites_lock(&self.effect.clips).values() {
            clip.props
                .double("OfxImageEffectPropFrameRange", &[0.0, last]);
            clip.props
                .double("OfxImageEffectPropUnmappedFrameRange", &[0.0, last]);
        }
    }
    /// Sets settings by name, as a render does.
    fn set_values(&self, values: &[(String, Value)]) -> Result<()> {
        let params = suites_lock(&self.effect.params.params);
        for (name, value) in values {
            let Some(p) = params.get(name) else {
                return Err(format!("the effect has no parameter {name}"));
            };
            *suites_lock(&p.value) = match value {
                Value::Numbers(n) => suites::Value::Numbers(n.clone()),
                Value::Text(t) => suites::Value::Text(c(t)),
                Value::Curves(curves) => suites::Value::Curves(curves.clone()),
            };
        }
        Ok(())
    }
    /// The frames of each input clip the plugin needs to render `time` with `values`
    /// (its own frame unless it says otherwise), at most 64 a clip.
    /// `connected` names the input clips that are wired: a plugin asks for frames only of
    /// clips that are.
    pub fn frames_needed(
        &mut self,
        time: f64,
        values: &[(String, Value)],
        connected: &[&str],
    ) -> Result<Vec<(String, Vec<f64>)>> {
        self.set_values(values)?;
        self.effect.props.double("VeylvikTime", &[time]);
        for (name, clip) in suites_lock(&self.effect.clips).iter() {
            let wired = name == "Output" || connected.contains(&name.as_str());
            clip.props
                .int("OfxImageClipPropConnected", &[i32::from(wired)]);
        }
        let names: Vec<String> = suites_lock(&self.effect.clip_order)
            .iter()
            .filter(|n| *n != "Output")
            .cloned()
            .collect();
        let args = PropSet::default();
        args.double("OfxPropTime", &[time]);
        let out = PropSet::default();
        for name in &names {
            out.double(&format!("OfxImageClipPropFrameRange_{name}"), &[time, time]);
        }
        match self.library.action(
            self.index,
            "OfxImageEffectActionGetFramesNeeded",
            self.effect.handle(),
            Some(&args),
            Some(&out),
        ) {
            OK | REPLY_DEFAULT => {}
            s => {
                return Err(format!(
                    "asking for the frames needed failed (status {s}){}",
                    said()
                ));
            }
        }
        let mut needed = Vec::new();
        for name in names {
            let ranges = out.get_numbers(&format!("OfxImageClipPropFrameRange_{name}"));
            let mut frames = Vec::new();
            for [a, b] in ranges.as_chunks::<2>().0 {
                let (first, last) = (a.min(*b), a.max(*b));
                if !first.is_finite() || !last.is_finite() || last - first > 64.0 {
                    return Err(format!("the plugin asks for more than 64 frames of {name}"));
                }
                let mut at = first;
                while at <= last + 1e-9 {
                    if !frames.iter().any(|f: &f64| (f - at).abs() < 1e-6) {
                        frames.push(at);
                    }
                    at += 1.0;
                }
            }
            if frames.len() > 64 {
                return Err(format!("the plugin asks for more than 64 frames of {name}"));
            }
            needed.push((name, frames));
        }
        Ok(needed)
    }
    /// A parameter's current value (as given, or as the plugin set it).
    pub fn value(&self, name: &str) -> Option<Value> {
        let params = suites_lock(&self.effect.params.params);
        let value = suites_lock(&params.get(name)?.value).clone();
        Some(match value {
            suites::Value::Numbers(n) => Value::Numbers(n),
            suites::Value::Text(t) => Value::Text(t.to_string_lossy().into_owned()),
            suites::Value::Curves(c) => Value::Curves(c),
        })
    }
    /// Renders at `time` with `values` (by parameter name; others keep their value), from
    /// `inputs` (by clip name) into a new frame of the instance's size.
    pub fn render(
        &mut self,
        time: f64,
        values: &[(String, Value)],
        inputs: &[(&str, Frame<'_>)],
    ) -> Result<Vec<f32>> {
        let [w, h] = self.size;
        let mut out = vec![0.0f32; w as usize * h as usize * 4];
        self.render_into(time, values, inputs, &mut out)?;
        Ok(out)
    }
    /// As [`Instance::render`], into `out` (a frame of the instance's size, top row first).
    pub fn render_into(
        &mut self,
        time: f64,
        values: &[(String, Value)],
        inputs: &[(&str, Frame<'_>)],
        out: &mut [f32],
    ) -> Result<()> {
        let [w, h] = self.size;
        let pixels = w as usize * h as usize * 4;
        if out.len() != pixels {
            return Err(format!("the output is not {w} x {h}"));
        }
        self.set_values(values)?;
        self.effect.props.double("VeylvikTime", &[time]);
        // Inputs stay in these buffers (bottom row first) for the length of the render;
        // the output starts clear, as a plugin may leave parts of it untouched.
        if self.scratch.len() < inputs.len() {
            self.scratch.resize_with(inputs.len(), Vec::new);
        }
        self.output.resize(pixels, 0.0);
        self.output.fill(0.0);
        {
            let clips = suites_lock(&self.effect.clips);
            for clip in clips.values() {
                suites_lock(&clip.pixels).clear();
                clip.props.int("OfxImageClipPropConnected", &[0]);
            }
            for (k, (name, frame)) in inputs.iter().enumerate() {
                let Some(clip) = clips.get(*name) else {
                    return Err(format!("the effect has no input {name}"));
                };
                if frame.width != w || frame.height != h || frame.pixels.len() != pixels {
                    return Err(format!(
                        "input {name} is {} x {}, the effect renders {w} x {h}",
                        frame.width, frame.height
                    ));
                }
                let scratch = &mut self.scratch[k];
                scratch.resize(pixels, 0.0);
                flip_into(frame.pixels, scratch, w);
                let data = scratch.as_ptr() as usize;
                suites_lock(&clip.pixels).push((
                    frame.time.unwrap_or(time),
                    Pixels {
                        data,
                        width: w as i32,
                        height: h as i32,
                    },
                ));
                clip.props.int("OfxImageClipPropConnected", &[1]);
            }
            let out = clips.get("Output").ok_or("the effect has no Output clip")?;
            suites_lock(&out.pixels).push((
                time,
                Pixels {
                    data: self.output.as_mut_ptr() as usize,
                    width: w as i32,
                    height: h as i32,
                },
            ));
            out.props.int("OfxImageClipPropConnected", &[1]);
        }
        let window = [0, 0, w as i32, h as i32];
        let sequence = PropSet::default();
        sequence.double("OfxImageEffectPropFrameRange", &[time, time]);
        sequence.double("OfxImageEffectPropFrameStep", &[1.0]);
        sequence.int("OfxPropIsInteractive", &[0]);
        sequence.double("OfxImageEffectPropRenderScale", &[1.0, 1.0]);
        sequence.int("OfxImageEffectPropSequentialRenderStatus", &[0]);
        sequence.int("OfxImageEffectPropInteractiveRenderStatus", &[0]);
        sequence.int("OfxImageEffectPropRenderQualityDraft", &[0]);
        let args = PropSet::default();
        args.double("OfxPropTime", &[time]);
        args.string("OfxImageEffectPropFieldToRender", "OfxFieldNone");
        args.int("OfxImageEffectPropRenderWindow", &window);
        args.double("OfxImageEffectPropRenderScale", &[1.0, 1.0]);
        args.int("OfxImageEffectPropSequentialRenderStatus", &[0]);
        args.int("OfxImageEffectPropInteractiveRenderStatus", &[0]);
        args.int("OfxImageEffectPropRenderQualityDraft", &[0]);
        let handle = self.effect.handle();
        let lib = self.library;
        let begun = lib.action(
            self.index,
            "OfxImageEffectActionBeginSequenceRender",
            handle,
            Some(&sequence),
            None,
        );
        let status = lib.action(
            self.index,
            "OfxImageEffectActionRender",
            handle,
            Some(&args),
            None,
        );
        if matches!(begun, OK | REPLY_DEFAULT) {
            lib.action(
                self.index,
                "OfxImageEffectActionEndSequenceRender",
                handle,
                Some(&sequence),
                None,
            );
        }
        // Nothing may point into these buffers once the render is over.
        for clip in suites_lock(&self.effect.clips).values() {
            suites_lock(&clip.pixels).clear();
        }
        match status {
            OK | REPLY_DEFAULT => {
                flip_into(&self.output, out, w);
                Ok(())
            }
            s => Err(format!("the render failed (status {s}){}", said())),
        }
    }
}
impl Drop for Instance<'_> {
    fn drop(&mut self) {
        self.library.action(
            self.index,
            "OfxActionDestroyInstance",
            self.effect.handle(),
            None,
            None,
        );
    }
}
