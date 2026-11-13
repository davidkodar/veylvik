//! The live editor's tool catalogue, shared by Studio and the stdio relay so the relay can list tools while Studio is closed.
use serde_json::{Value, json};
fn tool(name: &str, description: &str, properties: Value, required: &[&str]) -> Value {
    json!({"name":name,"description":description,"inputSchema":{"type":"object","properties":properties,"required":required,"additionalProperties":false}})
}
/// Tool descriptors served by a live Studio session.
pub fn catalog() -> &'static Value {
    static CATALOG: std::sync::OnceLock<Value> = std::sync::OnceLock::new();
    CATALOG.get_or_init(build_catalog)
}
fn build_catalog() -> Value {
    let session = json!({"type":"string","format":"uuid"});
    let revision = json!({"type":"integer","minimum":0});
    json!({"tools":[
        tool("inspect","Read project state and revision. Set summary=true for compact composition metadata without node payloads; graph and get_node provide targeted inspection.",json!({"summary":{"type":"boolean"}}),&[]),
        tool("graph","Page node metadata in stable UUID order without parameter payloads. Labels over 256 characters are marked as truncated. Compare revisions between pages.",json!({"offset":{"type":"integer","minimum":0},"limit":{"type":"integer","minimum":1,"maximum":128}}),&[]),
        tool("get_node","Read one exact authored node, or just a named parameter. Does not evaluate curves or change selection.",json!({"id":{"type":"string","format":"uuid"},"parameter":{"type":"string"}}),&["id"]),
        tool("diagnostics","Read GPU buffer and cache counters without waiting for an active render. Snapshot is null while the renderer is busy. Declared buffers exclude driver and rasterizer scratch allocations.",json!({}),&[]),
        tool("nodes","Read built-in operations, ports, defaults and parameter schemas.",json!({}),&[]),
        tool("get_preset","Read a bundled starter, or capture a group with its current values and animation. Supply exactly one of starter or group. No filesystem access.",json!({"group":{"type":"string","format":"uuid"},"starter":{"type":"string","enum":veylvik_document::presets::builtin::ALL.map(|p|p.id).as_slice()}}),&[]),
        tool("insert_preset","Insert an independent instance with explicitly bound inputs. One undo step; output remains unchanged. Dry-run IDs are provisional.",json!({"session":session,"revision":revision,"preset":veylvik_document::schema::preset(),"inputs":{"type":"object","additionalProperties":{"type":"string","format":"uuid"}},"sizing":{"type":"string","enum":["fit","fill","original"],"default":"fit"},"position":veylvik_document::schema::position(),"dry_run":{"type":"boolean"}}),&["session","revision","preset","inputs"]),
        tool("apply","Validate or atomically apply commands to the open editor. Requires the session and revision from inspect. set_parameter and set_key preserve node identity and wiring. Image references must already be known to the editor; new paths require import. Unapplied inspector changes or file jobs block mutation.",json!({"session":session,"revision":revision,"dry_run":{"type":"boolean"},"commands":{"type":"array","minItems":1,"maxItems":4096,"items":veylvik_document::schema::command()}}),&["session","revision","commands"]),
        tool("add_effect","Create an editable effect group with exposed controls and ordinary internal nodes as one undoable transaction. Places the effect without overlapping existing nodes, selects/previews its root, and preserves composition output. Returns named role IDs for editing controls. Discover recipes with nodes; dry-run IDs are provisional.",json!({"session":session,"revision":revision,"input":{"type":"string","format":"uuid"},"recipe":{"type":"string","enum":veylvik_document::recipes::Recipe::ALL},"dry_run":{"type":"boolean"}}),&["session","revision","input","recipe"]),
        tool("duplicate_branch","Copy a node and all upstream dependencies as one undoable transaction, preserving shared ancestors and asset references. Places the copy outside existing graph bounds, selects/previews its root, and leaves composition output unchanged. Returns root and old-to-new ID mapping. Dry-run IDs are provisional and are not reused on commit.",json!({"session":session,"revision":revision,"node":{"type":"string","format":"uuid"},"dry_run":{"type":"boolean"}}),&["session","revision","node"]),
        tool("undo","Undo one editor transaction.",json!({"session":session,"revision":revision}),&["session","revision"]),
        tool("redo","Redo one editor transaction.",json!({"session":session,"revision":revision}),&["session","revision"]),
        tool("preview_image","Capture the current completed preview as an inline PNG without changing the editor. Use preview to pause/seek first; pending or stale frames are rejected. Captures applied pixels, excluding inspector drafts, overlays and viewer exposure/alpha settings. Encoding runs in the background; returned session/revision/time identify the captured snapshot.",json!({"session":session,"revision":revision,"max_edge":{"type":"integer","minimum":1,"maximum":1024}}),&["session","revision"]),
        tool("preview","Pause playback, seek to an absolute frame and optionally preview a node. The GPU preview is asynchronous; composition output is unchanged.",json!({"session":session,"revision":revision,"frame":{"type":"integer","minimum":0,"maximum":1000000},"node":{"type":"string","format":"uuid"}}),&["session","revision","frame"])
    ]})
}
