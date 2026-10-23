use meowndesk_lib::telemetry::bounds;
use meowndesk_lib::telemetry::model::{Bootstrap, Delivery, View};
use serde_json::{Value, json};

fn fixture() -> Value {
    serde_json::from_str(include_str!("../../tests/fixtures/contract-v1.json"))
        .expect("shared contract fixture must be valid JSON")
}

fn resolve<'a>(mut node: &'a mut Value, path: &[Value]) -> &'a mut Value {
    for part in path {
        node = match part {
            Value::String(key) => node.get_mut(key).expect("fixture path key must exist"),
            Value::Number(index) => {
                let index = index.as_u64().expect("fixture path index must be numeric") as usize;
                &mut node.as_array_mut().expect("fixture path must be an array")[index]
            }
            other => panic!("unsupported fixture path segment: {other}"),
        };
    }
    node
}

fn apply_ops(document: &mut Value, ops: &[Value]) {
    for op in ops {
        let path = op["path"].as_array().expect("patch op needs a path");
        match op["op"].as_str().expect("patch op needs a kind") {
            "set" => {
                let (last, parent) = path.split_last().expect("set path must not be empty");
                let parent_node = resolve(document, parent);
                match last {
                    Value::String(key) => {
                        parent_node
                            .as_object_mut()
                            .expect("set parent must be an object")
                            .insert(key.clone(), op["value"].clone());
                    }
                    Value::Number(index) => {
                        let slot = &mut parent_node
                            .as_array_mut()
                            .expect("set parent must be an array")
                            [index.as_u64().unwrap() as usize];
                        *slot = op["value"].clone();
                    }
                    other => panic!("unsupported set segment: {other}"),
                }
            }
            "remove" => {
                let (last, parent) = path.split_last().expect("remove path must not be empty");
                match last {
                    Value::String(key) => {
                        resolve(document, parent)
                            .as_object_mut()
                            .expect("remove parent must be an object")
                            .remove(key);
                    }
                    Value::Number(index) => {
                        resolve(document, parent)
                            .as_array_mut()
                            .expect("remove parent must be an array")
                            .remove(index.as_u64().unwrap() as usize);
                    }
                    other => panic!("unsupported remove segment: {other}"),
                }
            }
            "repeat" => {
                let count = op["count"].as_u64().unwrap() as usize;
                let array = resolve(document, path).as_array_mut().unwrap();
                let first = array[0].clone();
                while array.len() < count {
                    array.push(first.clone());
                }
            }
            other => panic!("unsupported patch op: {other}"),
        }
    }
}

fn assert_byte_strings_are_decimal(node: &Value) {
    match node {
        Value::Object(map) => {
            for (key, value) in map {
                if key.to_ascii_lowercase().ends_with("bytes")
                    && !value.is_object()
                    && !value.is_array()
                {
                    let text = value.as_str().unwrap_or_else(|| {
                        panic!("{key} must be an unsigned decimal string on the wire")
                    });
                    assert!(
                        text.parse::<u64>().is_ok() && (text == "0" || !text.starts_with('0')),
                        "{key} is not a canonical unsigned decimal string: {text}"
                    );
                }
                assert_byte_strings_are_decimal(value);
            }
        }
        Value::Array(items) => items.iter().for_each(assert_byte_strings_are_decimal),
        _ => {}
    }
}

#[test]
fn canonical_bootstrap_view_and_delivery_validate() {
    let fixture = fixture();
    let bootstrap: Bootstrap = serde_json::from_value(fixture["bootstrap"].clone())
        .expect("bootstrap must deserialize from the shared fixture");
    bootstrap
        .validate()
        .expect("canonical bootstrap must validate");

    let delivery: Delivery = serde_json::from_value(fixture["delivery"].clone())
        .expect("delivery must deserialize from the shared fixture");
    delivery
        .validate()
        .expect("canonical delivery must validate");
}

fn normalize_numbers(node: &Value) -> Value {
    match node {
        Value::Number(number) => Value::from(number.as_f64().expect("fixture numbers are finite")),
        Value::Object(map) => Value::Object(
            map.iter()
                .map(|(key, value)| (key.clone(), normalize_numbers(value)))
                .collect(),
        ),
        Value::Array(items) => Value::Array(items.iter().map(normalize_numbers).collect()),
        other => other.clone(),
    }
}

#[test]
fn canonical_view_roundtrips_the_whole_field_catalog() {
    let fixture = fixture();
    let view: View = serde_json::from_value(fixture["bootstrap"]["view"].clone())
        .expect("canonical view must deserialize");
    let reserialized = serde_json::to_value(&view).expect("view must serialize");

    assert_eq!(
        normalize_numbers(&reserialized),
        normalize_numbers(&fixture["bootstrap"]["view"]),
        "serialization must preserve every contract field, name and decimal-byte string"
    );
    assert_byte_strings_are_decimal(&reserialized);
}

#[test]
fn deliberately_invalid_contract_cases_are_rejected() {
    let fixture = fixture();
    let mut rejected = Vec::new();

    for case in fixture["invalidCases"]
        .as_array()
        .expect("invalidCases array")
    {
        let name = case["name"].as_str().unwrap();
        let mut document = match case["target"].as_str().unwrap() {
            "view" => fixture["bootstrap"]["view"].clone(),
            "bootstrap" => fixture["bootstrap"].clone(),
            "delivery" => fixture["delivery"].clone(),
            other => panic!("unknown patch target: {other}"),
        };
        apply_ops(&mut document, case["ops"].as_array().unwrap());

        let accepted = match case["target"].as_str().unwrap() {
            "view" => serde_json::from_value::<View>(document)
                .map_err(|error| error.to_string())
                .and_then(|view| view.validate().map_err(|error| error.to_string()))
                .is_ok(),
            "bootstrap" => serde_json::from_value::<Bootstrap>(document)
                .map_err(|error| error.to_string())
                .and_then(|bootstrap| bootstrap.validate().map_err(|error| error.to_string()))
                .is_ok(),
            "delivery" => serde_json::from_value::<Delivery>(document)
                .map_err(|error| error.to_string())
                .and_then(|delivery| delivery.validate().map_err(|error| error.to_string()))
                .is_ok(),
            other => panic!("unknown patch target: {other}"),
        };

        assert!(!accepted, "invalid contract case was accepted: {name}");
        rejected.push(name.to_string());
    }

    assert_eq!(
        rejected.len(),
        fixture["invalidCases"].as_array().unwrap().len()
    );
}

#[test]
fn decimal_byte_strings_accept_the_u64_domain_only() {
    let wire = json!({
        "status": "valid",
        "value": "18446744073709551615",
        "sampledAtMs": 1761091199000i64,
        "elapsedMs": 63000,
        "scopeId": "host:test",
        "reason": null
    });
    let reading: meowndesk_lib::telemetry::model::Reading<
        meowndesk_lib::telemetry::model::ByteCount,
    > = serde_json::from_value(wire).expect("u64::MAX must fit the decimal-byte field");
    assert_eq!(reading.value.unwrap().0, u64::MAX);

    for invalid in [
        "",
        "0x10",
        "-1",
        "+1",
        " 1",
        "1 ",
        "01",
        "18446744073709551616",
    ] {
        let wire = json!({
            "status": "valid",
            "value": invalid,
            "sampledAtMs": 1761091199000i64,
            "elapsedMs": 63000,
            "scopeId": "host:test",
            "reason": null
        });
        assert!(
            serde_json::from_value::<
                meowndesk_lib::telemetry::model::Reading<
                    meowndesk_lib::telemetry::model::ByteCount,
                >,
            >(wire)
            .is_err(),
            "decimal bytes accepted {invalid:?}"
        );
    }
}

#[test]
fn utf8_bounds_truncate_at_character_boundaries() {
    assert_eq!(bounds::truncate_utf8("日本語", 4), "日");
    assert_eq!(bounds::truncate_utf8("日本語", 8), "日本");
    assert_eq!(bounds::truncate_utf8("日本語", 9), "日本語");
    assert_eq!(bounds::truncate_utf8("abc", 10), "abc");
    assert_eq!(bounds::truncate_utf8("Kärntner", 4), "Kär");

    let long = "é".repeat(200);
    assert_eq!(
        bounds::truncate_utf8(&long, bounds::MESSAGE_MAX_BYTES).len(),
        160
    );
    assert_eq!(bounds::strip_control("a\u{0}b\nc\u{7f}d"), "abcd");
    assert_eq!(
        bounds::bounded_label("  GPU\nName  ", bounds::LABEL_MAX_BYTES),
        "  GPUName  "
    );
}

#[test]
fn reading_states_preserve_optional_values() {
    use meowndesk_lib::telemetry::model::{ByteCount, Reading, ReadingStatus};

    let stale = Reading::stale(
        Some(ByteCount(0)),
        Some(1761091198000),
        Some(62000),
        "host:test",
        "No newer observation",
    );
    assert_eq!(stale.status, ReadingStatus::Stale);
    assert_eq!(
        stale.value,
        Some(ByteCount(0)),
        "a genuine zero stays observable"
    );

    let unsupported = Reading::<ByteCount>::unsupported("host:test", "Not offered");
    assert_eq!(unsupported.status, ReadingStatus::Unsupported);
    assert!(unsupported.value.is_none());
    assert!(unsupported.sampled_at_ms.is_none());
}
