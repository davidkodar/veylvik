//! The project file format and the promise kept about it: every project saved from Veylvik 0.1.0 on opens in every later Veylvik. A file says which format it is in (`schema`) and which Veylvik saved it (`saved_by`); reading turns an older format into the current one, step by step, and each node of an older version of its type into the current version, before the project is read, so old projects never meet code that does not know them. A file from a newer Veylvik is refused with words that say so.
use crate::{Composition, Error, Result, fail};
use serde_json::Value;

/// The format this Veylvik writes.
pub const FORMAT: u32 = 1;
/// The Veylvik that writes it.
pub const WRITER: &str = env!("CARGO_PKG_VERSION");

/// Turns a file of format `from` into format `from + 1`.
type Step = fn(&mut Value) -> Result<()>;
/// The steps from each older format, in order (`STEPS[k]` takes format k + 1 to k + 2).
/// A step is added whenever the format changes, and never changed afterwards.
const STEPS: &[Step] = &[];
/// Turns a node of one type from `version` to `version + 1`, its operation as JSON.
type NodeStep = fn(&mut Value) -> Result<()>;
/// The steps for nodes whose type has changed: (type, the version it takes up from).
/// A step is added whenever a node type's version goes up, and never changed afterwards.
const NODE_STEPS: &[(&str, u32, NodeStep)] = &[];

/// The project in `bytes`, from any format this Veylvik reads, validated.
pub fn read(bytes: &[u8]) -> Result<Composition> {
    let mut value: Value =
        serde_json::from_slice(bytes).map_err(|e| Error(crate::explain_parse(&e)))?;
    let saved_by = upgrade(&mut value)?;
    let composition: Composition = serde_json::from_value(value).map_err(|e| {
        let mut said = crate::explain_parse(&e);
        if let Some(by) = saved_by.as_deref().filter(|by| newer(by)) {
            said = format!("It was saved by Veylvik {by}, newer than this one ({WRITER}). {said}");
        }
        Error(said)
    })?;
    composition.validate()?;
    Ok(composition)
}
/// The project as it is saved: pretty JSON in the current format, with this Veylvik's
/// version beside it.
pub fn write(composition: &Composition) -> Result<Vec<u8>> {
    let mut value = serde_json::to_value(composition).map_err(|e| Error(e.to_string()))?;
    if let Value::Object(fields) = &mut value {
        fields.insert("saved_by".into(), Value::String(WRITER.into()));
    }
    serde_json::to_vec_pretty(&value).map_err(|e| Error(e.to_string()))
}
/// Whether version `by` is newer than this Veylvik.
fn newer(by: &str) -> bool {
    let parts = |v: &str| -> Vec<u64> {
        v.split(['.', '-', '+'])
            .map_while(|p| p.parse().ok())
            .collect()
    };
    parts(by) > parts(WRITER)
}
/// Brings a project's JSON up to the current format and node versions, and takes off
/// what is not part of the project itself (who saved it, which it returns).
pub fn upgrade(value: &mut Value) -> Result<Option<String>> {
    let Value::Object(fields) = value else {
        return fail("This is not a Veylvik project file");
    };
    let saved_by = match fields.remove("saved_by") {
        Some(Value::String(by)) => Some(by),
        _ => None,
    };
    let format = fields
        .get("schema")
        .and_then(Value::as_u64)
        .and_then(|f| u32::try_from(f).ok())
        .ok_or_else(|| {
            Error("This is not a Veylvik project file (it has no format number)".into())
        })?;
    if format > FORMAT {
        return fail(format!(
            "This project is in format {format}{}, and this Veylvik ({WRITER}) reads up to format {FORMAT}: open it in the newer Veylvik",
            saved_by
                .as_deref()
                .map(|by| format!(", saved by Veylvik {by}"))
                .unwrap_or_default()
        ));
    }
    if format == 0 {
        return fail("This is not a Veylvik project file (its format number is 0)");
    }
    for step in &STEPS[(format - 1) as usize..] {
        step(value)?;
    }
    if let Value::Object(fields) = value {
        fields.insert("schema".into(), Value::from(FORMAT));
    }
    upgrade_nodes(value)?;
    Ok(saved_by)
}
/// Every node of an older version of its type, brought to the current version.
fn upgrade_nodes(value: &mut Value) -> Result<()> {
    let Some(Value::Object(nodes)) = value.get_mut("nodes") else {
        return Ok(());
    };
    for node in nodes.values_mut() {
        let Some(kind) = node
            .get("operation")
            .and_then(|o| o.get("type"))
            .and_then(Value::as_str)
            .map(str::to_owned)
        else {
            continue;
        };
        let Some(current) = crate::registry::TYPES
            .iter()
            .find(|t| t.type_id == kind)
            .map(|t| t.version)
        else {
            // A type this Veylvik does not know is reported when the project is read.
            continue;
        };
        let mut version = node.get("version").and_then(Value::as_u64).unwrap_or(0) as u32;
        if version > current {
            return fail(format!(
                "The project has a {kind} node of version {version}; this Veylvik ({WRITER}) knows up to version {current}: open it in the newer Veylvik"
            ));
        }
        if let Some(operation) = node.get_mut("operation") {
            version = upgrade_operation(&kind, version, operation)?;
        }
        node["version"] = Value::from(version);
    }
    Ok(())
}
/// A node's operation of type `kind` at `version`, brought to its type's current version
/// (which it returns).
pub fn upgrade_operation(kind: &str, version: u32, operation: &mut Value) -> Result<u32> {
    let current = crate::registry::TYPES
        .iter()
        .find(|t| t.type_id == kind)
        .map(|t| t.version)
        .ok_or_else(|| {
            Error(format!(
                "{kind} is not a node type this Veylvik ({WRITER}) knows"
            ))
        })?;
    let mut version = version;
    while version < current {
        let Some((_, _, step)) = NODE_STEPS
            .iter()
            .find(|(t, from, _)| *t == kind && *from == version)
        else {
            return fail(format!(
                "The project has a {kind} node of version {version}, which this Veylvik cannot bring up to version {current}"
            ));
        };
        step(operation)?;
        version += 1;
    }
    Ok(version)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn a_project_from_a_newer_veylvik_is_refused_in_words_that_say_so() {
        let mut doc = serde_json::to_value(crate::demo()).unwrap();
        doc["schema"] = Value::from(FORMAT + 1);
        doc["saved_by"] = Value::from("9.0.0");
        let error = read(&serde_json::to_vec(&doc).unwrap()).unwrap_err().0;
        assert!(
            error.contains("format 2") && error.contains("9.0.0"),
            "{error}"
        );
        // The same format but a setting this one does not know: named as newer too.
        let mut doc = serde_json::to_value(crate::demo()).unwrap();
        doc["saved_by"] = Value::from("9.0.0");
        doc["some_new_setting"] = Value::from(1);
        let error = read(&serde_json::to_vec(&doc).unwrap()).unwrap_err().0;
        assert!(
            error.contains("saved by Veylvik 9.0.0, newer than this one"),
            "{error}"
        );
    }
    #[test]
    fn a_saved_project_says_who_saved_it_and_reads_back_the_same() {
        let doc = crate::demo();
        let bytes = write(&doc).unwrap();
        let value: Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(value["saved_by"], WRITER);
        let back = read(&bytes).unwrap();
        assert_eq!(
            serde_json::to_value(&back).unwrap(),
            serde_json::to_value(&doc).unwrap()
        );
    }
    #[test]
    fn versions_compare_by_their_numbers() {
        assert!(newer("99.0.0") && !newer("0.0.1") && !newer(WRITER));
    }
}
