use veylvik_document::{
    Operation,
    registry::{self, ParameterKind},
};
#[test]
fn every_registered_default_round_trips_and_validates() {
    let mut names = std::collections::BTreeSet::new();
    for descriptor in registry::TYPES {
        assert!(names.insert(descriptor.type_id));
        let value = descriptor.default_operation();
        let operation: Operation = serde_json::from_value(value.clone()).unwrap();
        operation.validate().unwrap();
        assert_eq!(operation.descriptor().type_id, descriptor.type_id);
        assert_eq!(operation.inputs(), descriptor.inputs);
        assert_eq!(serde_json::to_value(operation).unwrap(), value);
        let schema = descriptor.schema();
        assert_eq!(schema["properties"]["type"]["const"], descriptor.type_id);
        assert_eq!(
            schema["required"].as_array().unwrap().len(),
            descriptor.parameters.len() + 1
        );
    }
    assert_eq!(
        registry::operation_schema()["oneOf"]
            .as_array()
            .unwrap()
            .len(),
        registry::TYPES.len()
    );
}
#[test]
fn scalar_registry_bounds_drive_document_validation() {
    for descriptor in registry::TYPES {
        for parameter in descriptor.parameters {
            if !matches!(parameter.kind, ParameterKind::Scalar) {
                continue;
            }
            for bound in [parameter.min.unwrap(), parameter.max.unwrap()] {
                let mut value = descriptor.default_operation();
                value[parameter.name]["value"] = serde_json::json!(bound);
                serde_json::from_value::<Operation>(value)
                    .unwrap()
                    .validate()
                    .unwrap();
            }
            for invalid in [parameter.min.unwrap() - 1.0, parameter.max.unwrap() + 1.0] {
                let mut value = descriptor.default_operation();
                value[parameter.name]["value"] = serde_json::json!(invalid);
                assert!(
                    serde_json::from_value::<Operation>(value)
                        .unwrap()
                        .validate()
                        .is_err()
                );
            }
        }
    }
}
#[test]
fn empty_and_nul_assets_are_rejected() {
    for asset in ["", "   ", "file\0.png"] {
        assert!(
            Operation::Image {
                asset: asset.into()
            }
            .validate()
            .is_err()
        );
    }
}
