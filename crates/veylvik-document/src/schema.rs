//! Wire schemas shared by command transports and authoring clients.
use serde_json::{Value, json};
pub fn position() -> Value {
    json!({"anyOf":[{"type":"null"},{"type":"object","properties":{"x":{"type":"number","minimum":-1000000,"maximum":1000000},"y":{"type":"number","minimum":-1000000,"maximum":1000000}},"required":["x","y"],"additionalProperties":false}]})
}
pub fn node() -> Value {
    json!({"type":"object","properties":{"id":{"type":"string","format":"uuid"},"version":{"const":1},"label":{"type":"string"},"position":position(),"operation":crate::registry::operation_schema(),"inputs":{"type":"object","additionalProperties":{"type":"string","format":"uuid"}}},"required":["id","version","label","operation","inputs"],"additionalProperties":false})
}
pub fn command() -> Value {
    let mut schema = json!({"oneOf":[
        {"type":"object","properties":{"command":{"const":"put"},"node":node()},"required":["command","node"],"additionalProperties":false},
        {"type":"object","properties":{"command":{"const":"set_parameter"},"id":{"type":"string","format":"uuid"},"parameter":{"type":"string","minLength":1},"value":{}},"required":["command","id","parameter","value"],"additionalProperties":false,"description":"Replace one registered operation parameter. Value must match its schema in the nodes catalog; node identity and wiring are preserved."},
        {"type":"object","properties":{"command":{"const":"set_key"},"id":{"type":"string","format":"uuid"},"parameter":{"type":"string","minLength":1},"time":{"type":"object","properties":{"ticks":{"type":"integer","minimum":i64::MIN,"maximum":i64::MAX},"rate":{"type":"integer","minimum":1,"maximum":u32::MAX}},"required":["ticks","rate"],"additionalProperties":false},"value":{"type":"number"}},"required":["command","id","parameter","time","value"],"additionalProperties":false,"description":"Insert or update an exact-time scalar key, preserving existing interpolation and equivalent timestamp representation."},
        {"type":"object","properties":{"command":{"const":"remove_key"},"id":{"type":"string","format":"uuid"},"parameter":{"type":"string","minLength":1},"time":{"type":"object","properties":{"ticks":{"type":"integer","minimum":i64::MIN,"maximum":i64::MAX},"rate":{"type":"integer","minimum":1,"maximum":u32::MAX}},"required":["ticks","rate"],"additionalProperties":false}},"required":["command","id","parameter","time"],"additionalProperties":false,"description":"Remove an exact-time scalar key. Missing keys are errors; the final key becomes a constant with its value."},
        {"type":"object","properties":{"command":{"enum":["remove","output"]},"id":{"type":"string","format":"uuid"}},"required":["command","id"],"additionalProperties":false},
        {"type":"object","properties":{"command":{"const":"resize"},"width":{"type":"integer","minimum":1,"maximum":8192},"height":{"type":"integer","minimum":1,"maximum":8192}},"required":["command","width","height"],"additionalProperties":false},
        {"type":"object","properties":{"command":{"const":"frame_count"},"frame_count":{"type":"integer","minimum":1,"maximum":crate::MAX_FRAME_COUNT}},"required":["command","frame_count"],"additionalProperties":false},
        {"type":"object","properties":{"command":{"const":"frame_rate"},"frame_rate":{"type":"object","properties":{"ticks":{"type":"integer","minimum":1,"maximum":i64::MAX},"rate":{"type":"integer","minimum":1,"maximum":u32::MAX}},"required":["ticks","rate"],"additionalProperties":false}},"required":["command","frame_rate"],"additionalProperties":false},
        {"type":"object","properties":{"command":{"const":"place"},"id":{"type":"string","format":"uuid"},"position":position()},"required":["command","id","position"],"additionalProperties":false}
    ]});
    schema["oneOf"].as_array_mut().unwrap().extend([
        json!({"type":"object","properties":{"command":{"const":"put_group"},"group":group()},"required":["command","group"],"additionalProperties":false}),
        json!({"type":"object","properties":{"command":{"const":"ungroup"},"id":{"type":"string","format":"uuid"}},"required":["command","id"],"additionalProperties":false}),
        json!({"type":"object","properties":{"command":{"const":"set_control"},"id":{"type":"string","format":"uuid"},"control":{"type":"string"},"value":{}},"required":["command","id","control","value"],"additionalProperties":false,"description":"Edit the real parameter referenced by a group control. Scalar values accept the same animated curves as set_parameter."}),
        json!({"type":"object","properties":{"command":{"const":"connect_group"},"id":{"type":"string","format":"uuid"},"input":{"type":"string"},"source":{"type":"string","format":"uuid"}},"required":["command","id","input","source"],"additionalProperties":false,"description":"Reconnect all internal targets of a public group input atomically."}),
    ]);
    schema
}

pub fn group() -> Value {
    let id = json!({"type":"string","format":"uuid"});
    let name = json!({"type":"string","minLength":1,"maxLength":128});
    let port = json!({"type":"object","properties":{"node":id,"port":{"type":"string"}},"required":["node","port"],"additionalProperties":false});
    let input = json!({"type":"object","properties":{"label":name,"targets":{"type":"array","minItems":1,"maxItems":8192,"items":port}},"required":["label","targets"],"additionalProperties":false});
    let control = json!({"type":"object","properties":{"label":name,"node":id,"parameter":{"type":"string"},"default":{},"range":{"anyOf":[{"type":"null"},{"type":"array","items":{"type":"number"},"minItems":2,"maxItems":2}]}},"required":["label","node","parameter","default"],"additionalProperties":false});
    json!({"type":"object","properties":{"id":id,"label":name,"members":{"type":"array","minItems":1,"maxItems":4096,"uniqueItems":true,"items":id},"output":id,"inputs":{"type":"object","maxProperties":64,"propertyNames":name,"additionalProperties":input},"controls":{"type":"object","maxProperties":64,"propertyNames":name,"additionalProperties":control}},"required":["id","label","members","output","inputs","controls"],"additionalProperties":false,"description":"Disjoint single-output node group. Every member feeds the output; all external input edges are exposed exactly once. Controls alias existing parameters; defaults and optional narrower scalar ranges are validated."})
}

/// Outgoing per-key interpolation; null inherits the scalar mode.
pub fn segment() -> Value {
    let handle = json!({"type":"object","properties":{"time":{"type":"number","minimum":0,"maximum":1},"value":{"type":"number"}},"required":["time","value"],"additionalProperties":false});
    json!({"oneOf":[{"type":"null"},
        {"type":"object","properties":{"mode":{"enum":["linear","hold","smooth","auto"]}},"required":["mode"],"additionalProperties":false},
        {"type":"object","properties":{"mode":{"const":"bezier"},"outgoing":handle,"incoming":handle},"required":["mode","outgoing","incoming"],"additionalProperties":false,"description":"Times are nonnegative fractions of the key interval and must sum to at most one. Values are endpoint-relative offsets; control values must remain within the parameter bounds."}
    ]})
}
