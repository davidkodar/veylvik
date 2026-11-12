//! Wire schemas shared by command transports and authoring clients.
use serde_json::{Value, json};
pub fn position() -> Value {
    json!({"anyOf":[{"type":"null"},{"type":"object","properties":{"x":{"type":"number","minimum":-1000000,"maximum":1000000},"y":{"type":"number","minimum":-1000000,"maximum":1000000}},"required":["x","y"],"additionalProperties":false}]})
}
pub fn node() -> Value {
    json!({"type":"object","properties":{"id":{"type":"string","format":"uuid"},"version":{"const":1},"label":{"type":"string"},"position":position(),"operation":crate::registry::operation_schema(),"inputs":{"type":"object","additionalProperties":{"type":"string","format":"uuid"}}},"required":["id","version","label","operation","inputs"],"additionalProperties":false})
}
pub fn command() -> Value {
    json!({"oneOf":[
        {"type":"object","properties":{"command":{"const":"put"},"node":node()},"required":["command","node"],"additionalProperties":false},
        {"type":"object","properties":{"command":{"const":"set_parameter"},"id":{"type":"string","format":"uuid"},"parameter":{"type":"string","minLength":1},"value":{}},"required":["command","id","parameter","value"],"additionalProperties":false,"description":"Replace one registered operation parameter. Value must match its schema in the nodes catalog; node identity and wiring are preserved."},
        {"type":"object","properties":{"command":{"const":"set_key"},"id":{"type":"string","format":"uuid"},"parameter":{"type":"string","minLength":1},"time":{"type":"object","properties":{"ticks":{"type":"integer","minimum":i64::MIN,"maximum":i64::MAX},"rate":{"type":"integer","minimum":1,"maximum":u32::MAX}},"required":["ticks","rate"],"additionalProperties":false},"value":{"type":"number"}},"required":["command","id","parameter","time","value"],"additionalProperties":false,"description":"Insert or update an exact-time scalar key, preserving existing interpolation and equivalent timestamp representation."},
        {"type":"object","properties":{"command":{"enum":["remove","output"]},"id":{"type":"string","format":"uuid"}},"required":["command","id"],"additionalProperties":false},
        {"type":"object","properties":{"command":{"const":"resize"},"width":{"type":"integer","minimum":1,"maximum":8192},"height":{"type":"integer","minimum":1,"maximum":8192}},"required":["command","width","height"],"additionalProperties":false},
        {"type":"object","properties":{"command":{"const":"frame_count"},"frame_count":{"type":"integer","minimum":1,"maximum":crate::MAX_FRAME_COUNT}},"required":["command","frame_count"],"additionalProperties":false},
        {"type":"object","properties":{"command":{"const":"frame_rate"},"frame_rate":{"type":"object","properties":{"ticks":{"type":"integer","minimum":1,"maximum":i64::MAX},"rate":{"type":"integer","minimum":1,"maximum":u32::MAX}},"required":["ticks","rate"],"additionalProperties":false}},"required":["command","frame_rate"],"additionalProperties":false},
        {"type":"object","properties":{"command":{"const":"place"},"id":{"type":"string","format":"uuid"},"position":position()},"required":["command","id","position"],"additionalProperties":false}
    ]})
}
