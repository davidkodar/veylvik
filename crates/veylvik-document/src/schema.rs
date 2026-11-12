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
        {"type":"object","properties":{"command":{"enum":["remove","output"]},"id":{"type":"string","format":"uuid"}},"required":["command","id"],"additionalProperties":false},
        {"type":"object","properties":{"command":{"const":"resize"},"width":{"type":"integer","minimum":1,"maximum":8192},"height":{"type":"integer","minimum":1,"maximum":8192}},"required":["command","width","height"],"additionalProperties":false},
        {"type":"object","properties":{"command":{"const":"place"},"id":{"type":"string","format":"uuid"},"position":position()},"required":["command","id","position"],"additionalProperties":false}
    ]})
}
