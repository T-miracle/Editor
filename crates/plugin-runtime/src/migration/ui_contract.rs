//! Finite disk-record import never changes public package admission or runs a retired UI component.
use crate::Installed;
use serde_json::Value;
use std::collections::BTreeMap;

/// Decode stored registry bytes, reporting whether known removed fields require an immutable raw backup.
/// Unknown current manifest fields still fail; grants, scope, enablement and other metadata stay intact.
pub(crate) fn decode_registry(bytes: &[u8]) -> anyhow::Result<(BTreeMap<String, Installed>, bool)> {
    let records: BTreeMap<String, Value> = serde_json::from_slice(bytes)?;
    let mut changed = false;
    let mut installed = BTreeMap::new();
    for (id, record) in records {
        let (entry, imported) = decode_record(record)?;
        changed |= imported;
        installed.insert(id, entry);
    }
    Ok((installed, changed))
}

/// Historical backup records use the same bounded import as registries, without rewriting the evidence.
pub(crate) fn decode_installed(bytes: &[u8]) -> anyhow::Result<Installed> {
    Ok(decode_record(serde_json::from_slice(bytes)?)?.0)
}

/// Strip only three known retired fields from saved metadata and persist its prohibition on execution.
fn decode_record(mut record: Value) -> anyhow::Result<(Installed, bool)> {
    let mut changed = false;
    if let Some(manifest) = record.get_mut("manifest") {
        for (collection, fields) in [
            ("panels", &["view_modes"][..]),
            ("commands", &["toolbar", "toolbar_icon"][..]),
        ] {
            if let Some(entries) = manifest.get_mut(collection).and_then(Value::as_array_mut) {
                for entry in entries {
                    if let Some(fields_map) = entry.as_object_mut() {
                        for field in fields {
                            // A null command field is still evidence of the retired serializer and SDK.
                            changed |= fields_map.remove(*field).is_some();
                        }
                    }
                }
            }
        }
    }
    if changed {
        record["retired_ui_contract"] = Value::Bool(true);
    }
    let mut installed: Installed = serde_json::from_value(record)?;
    installed.error = installed.compatibility_error();
    Ok((installed, changed))
}
