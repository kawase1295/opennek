use std::collections::HashMap;

use opennek_core::callbacks::{encode_uri_component, extract_downlink, render_template};

fn vars(pairs: &[(&str, &str)]) -> HashMap<String, String> {
    pairs.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect()
}

#[test]
fn render_template_replaces_known_variables_and_leaves_others() {
    let v = vars(&[("device", "0A1B2C3D"), ("data", "cafe"), ("customData#temp", "21.5")]);
    assert_eq!(
        render_template(
            r#"{"d":"{device}","x":"{data}","t":{customData#temp},"u":"{unknown}"}"#,
            &v,
            None
        ),
        r#"{"d":"0A1B2C3D","x":"cafe","t":21.5,"u":"{unknown}"}"#
    );
    assert_eq!(
        render_template(
            "https://x/?q={data}",
            &vars(&[("data", "a b")]),
            Some(encode_uri_component)
        ),
        "https://x/?q=a%20b"
    );
}

#[test]
fn uri_components_are_encoded_like_javascript() {
    assert_eq!(encode_uri_component("A-z_0.9!~*'()"), "A-z_0.9!~*'()");
    assert_eq!(encode_uri_component("a/b?c=d&e #ü"), "a%2Fb%3Fc%3Dd%26e%20%23%C3%BC");
}

#[test]
fn extract_downlink_accepts_sigfox_style_and_flat_responses() {
    assert_eq!(
        extract_downlink(r#"{"0A1B2C3D":{"downlinkData":"0102030405060708"}}"#, "0A1B2C3D").as_deref(),
        Some("0102030405060708")
    );
    assert_eq!(
        extract_downlink(r#"{"0a1b2c3d":{"downlinkData":"0102030405060708"}}"#, "0A1B2C3D").as_deref(),
        Some("0102030405060708")
    );
    assert_eq!(
        extract_downlink(r#"{"downlinkData":"AABBCCDDEEFF0011"}"#, "0A1B2C3D").as_deref(),
        Some("aabbccddeeff0011")
    );
    assert_eq!(extract_downlink(r#"{"downlinkData":"0102"}"#, "0A1B2C3D"), None);
    assert_eq!(extract_downlink("not json", "0A1B2C3D"), None);
}

#[test]
fn responses_are_cut_at_2000_utf16_units_like_javascript() {
    let long = "😀".repeat(1500);
    assert_eq!(
        opennek_core::callbacks::truncate_utf16(&long, 2000).chars().count(),
        1000
    );
    assert_eq!(opennek_core::callbacks::truncate_utf16("abc", 2000), "abc");
}
