//! Tests of the FeatureConfig derive: reading a feature's settings struct from its settings as stored.

use bruce_embedded_std::config::{ConfigSpec, ConfigSpecValue, FeatureConfig, TypedValue};

/// A feature's settings, with a bool of each kind, and an optional number for comparison.
#[derive(FeatureConfig)]
#[config(label = "Example")]
struct Settings {
    /// An optional bool, e.g. the Screen feature's "Keep the Screen Off".
    #[config(label = "Off")]
    off: Option<bool>,
    /// A required bool, which defaults to false.
    #[config(label = "Enabled")]
    enabled: bool,
    /// An optional number.
    #[config(label = "Count")]
    count: Option<i32>,
}

/// The settings as stored, from the struct's spec, with the values given.
fn stored(off: Option<bool>, enabled: Option<bool>) -> ConfigSpec {
    let mut spec = Settings::to_config_spec().unwrap();
    spec.map.get_mut("off").unwrap().value = TypedValue::Bool(off);
    spec.map.get_mut("enabled").unwrap().value = TypedValue::Bool(enabled);
    spec
}

#[test]
fn reads_settings_never_set_as_on_a_new_device() {
    let settings = Settings::from_config_spec(&Settings::to_config_spec().unwrap()).unwrap();
    assert_eq!(settings.off, None);
    assert!(!settings.enabled);
    assert_eq!(settings.count, None);
}

#[test]
fn reads_bools_which_are_set() {
    let settings = Settings::from_config_spec(&stored(Some(true), Some(true))).unwrap();
    assert_eq!(settings.off, Some(true));
    assert!(settings.enabled);
    let settings = Settings::from_config_spec(&stored(Some(false), Some(false))).unwrap();
    assert_eq!(settings.off, Some(false));
    assert!(!settings.enabled);
}

#[test]
fn still_refuses_a_value_of_another_type() {
    let mut spec = stored(None, None);
    spec.map.get_mut("off").unwrap().value = TypedValue::Int32(Some(1));
    assert!(Settings::from_config_spec(&spec).is_err());
}
