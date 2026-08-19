use std::collections::BTreeSet;

use e::protocol::{
    ClientMessage, ServerMessage, CLIENT_MESSAGE_TYPES, SERVER_MESSAGE_TYPES,
    WIRE_MESSAGE_SHAPES_JSON, WIRE_PROTOCOL_VERSION, WIRE_RECORD_SHAPES_JSON,
};
use serde_json::{Map, Value};

fn keys(value: &Value) -> BTreeSet<String> {
    value
        .as_object()
        .expect("JSON object")
        .keys()
        .cloned()
        .collect()
}

fn assert_value_type(value: &Value, ty: &str, records: &Map<String, Value>, path: &str) {
    if let Some(element) = ty.strip_suffix("[]") {
        let values = value
            .as_array()
            .unwrap_or_else(|| panic!("{path} must be {ty}"));
        for (index, value) in values.iter().enumerate() {
            assert_value_type(value, element, records, &format!("{path}[{index}]"));
        }
        return;
    }
    match ty {
        "string" => assert!(value.is_string(), "{path} must be a string"),
        "boolean" => assert!(value.is_boolean(), "{path} must be a boolean"),
        "integer" => assert!(
            value.as_u64().is_some(),
            "{path} must be an unsigned integer"
        ),
        "host-event" => {
            let event = value
                .as_object()
                .unwrap_or_else(|| panic!("{path} must be a host-event object"));
            assert!(event.get("type").is_some_and(Value::is_string));
        }
        record => {
            let shape = records
                .get(record)
                .unwrap_or_else(|| panic!("unknown contract record {record}"));
            assert_shape(value, shape, records, path);
        }
    }
}

fn assert_shape(value: &Value, shape: &Value, records: &Map<String, Value>, path: &str) {
    let object = value
        .as_object()
        .unwrap_or_else(|| panic!("{path} must be an object"));
    let required = shape["required"].as_object().expect("required fields");
    let optional = shape["optional"].as_object().expect("optional fields");
    for (field, ty) in required {
        let field_value = object
            .get(field)
            .unwrap_or_else(|| panic!("{path}.{field} is required"));
        assert_value_type(
            field_value,
            ty.as_str().expect("field type"),
            records,
            &format!("{path}.{field}"),
        );
    }
    for (field, ty) in optional {
        if let Some(field_value) = object.get(field) {
            assert_value_type(
                field_value,
                ty.as_str().expect("field type"),
                records,
                &format!("{path}.{field}"),
            );
        }
    }
    let allowed = required
        .keys()
        .chain(optional.keys())
        .map(String::as_str)
        .chain(["type"])
        .collect::<BTreeSet<_>>();
    for field in object.keys() {
        assert!(
            allowed.contains(field.as_str()),
            "{path}.{field} is not declared in the contract"
        );
    }
}

fn fixtures() -> Value {
    serde_json::from_str(include_str!("../testdata/wire-contract-fixtures.json"))
        .expect("generated wire fixtures parse")
}

#[test]
fn generated_rosters_shapes_and_fixtures_cover_the_same_messages() {
    let fixtures = fixtures();
    let shapes: Value =
        serde_json::from_str(WIRE_MESSAGE_SHAPES_JSON).expect("generated message shapes parse");
    let client = CLIENT_MESSAGE_TYPES
        .iter()
        .map(|value| (*value).to_owned())
        .collect::<BTreeSet<_>>();
    let server = SERVER_MESSAGE_TYPES
        .iter()
        .map(|value| (*value).to_owned())
        .collect::<BTreeSet<_>>();
    assert_eq!(keys(&fixtures["client"]), client);
    assert_eq!(keys(&fixtures["server"]), server);
    assert_eq!(keys(&shapes["client"]), client);
    assert_eq!(keys(&shapes["server"]), server);
    assert_eq!(fixtures["protocolVersion"], WIRE_PROTOCOL_VERSION);
    assert_eq!(
        include_str!("../testdata/wire-contract-fixtures.json"),
        include_str!("../../../bridge/test/fixtures/wire-contract-fixtures.json"),
        "Rust and Node conformance fixtures are generated together"
    );
}

#[test]
fn every_client_fixture_deserializes_reserializes_and_matches_its_shape() {
    let fixtures = fixtures();
    let shapes: Value = serde_json::from_str(WIRE_MESSAGE_SHAPES_JSON).unwrap();
    let records: Value = serde_json::from_str(WIRE_RECORD_SHAPES_JSON).unwrap();
    let records = records.as_object().unwrap();
    for message_type in CLIENT_MESSAGE_TYPES {
        let shape = &shapes["client"][*message_type];
        for form in ["minimal", "full"] {
            let fixture = &fixtures["client"][*message_type][form];
            assert_shape(
                fixture,
                shape,
                records,
                &format!("client.{message_type}.{form}"),
            );
            let message: ClientMessage = serde_json::from_value(fixture.clone())
                .unwrap_or_else(|error| panic!("client {message_type}/{form}: {error}"));
            let serialized = serde_json::to_value(message).expect("serialize client fixture");
            assert_eq!(
                serialized, *fixture,
                "client {message_type}/{form} wire drift"
            );
        }
    }
}

#[test]
fn every_server_fixture_matches_shape_and_optional_defaults_parse() {
    let fixtures = fixtures();
    let shapes: Value = serde_json::from_str(WIRE_MESSAGE_SHAPES_JSON).unwrap();
    let records: Value = serde_json::from_str(WIRE_RECORD_SHAPES_JSON).unwrap();
    let records = records.as_object().unwrap();
    for message_type in SERVER_MESSAGE_TYPES {
        let shape = &shapes["server"][*message_type];
        for form in ["minimal", "full"] {
            let fixture = &fixtures["server"][*message_type][form];
            assert_shape(
                fixture,
                shape,
                records,
                &format!("server.{message_type}.{form}"),
            );
            let wire = serde_json::to_string(fixture).unwrap();
            ServerMessage::from_wire(&wire)
                .unwrap_or_else(|| panic!("server {message_type}/{form} did not parse"));
        }
    }
}
