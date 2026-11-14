//! Media sources are bound by the planner; they need no uniforms.
#[allow(unused_imports)]
use super::{Cx, Passes, RenderNode};
#[allow(unused_imports)]
use crate::{Kernel, Params, Result, error};
#[allow(unused_imports)]
use veylvik_document::{Operation, Scalar};

impl RenderNode for veylvik_document::nodes::Image {}
impl RenderNode for veylvik_document::nodes::Footage {}

impl RenderNode for veylvik_document::nodes::MotionBlur {}
impl RenderNode for veylvik_document::nodes::TimeRemap {}

impl RenderNode for veylvik_document::nodes::OpticalFlow {}
